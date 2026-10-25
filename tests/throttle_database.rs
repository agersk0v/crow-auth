//! Verifies rolling shared budgets, lock races, cleanup and safe HTTP failures with disposable PostgreSQL.

use axum::{
    Form, Router,
    body::{Body, to_bytes},
    extract::{ConnectInfo, State},
    http::{HeaderMap, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use crow_auth::{
    browser::{BrowserSecurity, FormAction, message_page},
    policy::Email,
    throttle::{RegistrationThrottle, ThrottleError, cleanup_expired},
};
use serde::Deserialize;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::net::SocketAddr;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

fn peer(n: u8) -> SocketAddr {
    SocketAddr::from(([192, 0, 2, n], 40000))
}
fn limited(result: Result<(), ThrottleError>) -> u64 {
    match result {
        Err(ThrottleError::Limited {
            retry_after_seconds,
        }) => {
            assert!((1..=3600).contains(&retry_after_seconds));
            retry_after_seconds
        }
        other => panic!("expected safe throttle denial: {other:?}"),
    }
}
async fn pool(url: &str, schema: &str) -> PgPool {
    let path = format!("SET search_path TO {schema}");
    PgPoolOptions::new()
        .max_connections(12)
        .after_connect(move |conn, _| {
            let path = path.clone();
            Box::pin(async move {
                sqlx::query(&path).execute(conn).await?;
                Ok(())
            })
        })
        .connect(url)
        .await
        .unwrap()
}
async fn reset(db: &PgPool) {
    sqlx::query("TRUNCATE registration_limits")
        .execute(db)
        .await
        .unwrap();
}
async fn count(db: &PgPool, kind: i16) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM registration_limits WHERE kind=$1")
        .bind(kind)
        .fetch_one(db)
        .await
        .unwrap()
}
async fn ip_snapshot(db: &PgPool) -> String {
    sqlx::query_scalar(
        "SELECT attempts::text||expires_at::text FROM registration_limits WHERE kind=0",
    )
    .fetch_one(db)
    .await
    .unwrap()
}

#[derive(Clone)]
struct Harness {
    security: BrowserSecurity,
    throttle: RegistrationThrottle,
}
#[derive(Deserialize)]
struct Fields {
    email: String,
    csrf: String,
}
async fn submit(
    State(h): State<Harness>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(fields): Form<Fields>,
) -> Response {
    if let Err(error) = h
        .security
        .authorize_post(&headers, &fields.csrf, FormAction::Register)
    {
        return error.into_response();
    }
    let email = Email::parse(&fields.email).ok();
    match h.throttle.check(peer, email.as_ref()).await {
        Ok(()) => message_page("Accepted", "Admission only; no account was created"),
        Err(error) => error.into_response(),
    }
}
async fn request(app: &Router, cookie: &str, csrf: &str, email: &str, htmx: bool) -> Response {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/submit")
        .header("origin", "https://crow.localhost:3000")
        .header("cookie", cookie)
        .header("content-type", "application/x-www-form-urlencoded")
        .header(
            "x-forwarded-for",
            if htmx {
                "203.0.113.99"
            } else {
                "203.0.113.100"
            },
        )
        .header(
            "forwarded",
            if htmx {
                "for=203.0.113.101"
            } else {
                "for=203.0.113.102"
            },
        );
    if htmx {
        builder = builder.header("hx-request", "true");
    }
    let mut req = builder
        .body(Body::from(format!("email={email}&csrf={csrf}")))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(peer(200)));
    app.clone().oneshot(req).await.unwrap()
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL; CI invokes explicitly"]
async fn shared_rolling_limits_concurrency_cleanup_and_fail_closed() {
    let url = std::env::var("TEST_DATABASE_URL").expect("disposable database required");
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("throttle_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let db = pool(&url, &schema).await;
    let second_db = pool(&url, &schema).await;
    crow_auth::migrate(&db).await.unwrap();
    crow_auth::migrate(&db).await.unwrap();
    let a = RegistrationThrottle::new(db.clone());
    let b = RegistrationThrottle::new(second_db.clone());

    // Ports and IPv4-mapped IPv6 do not split one source IP's rolling budget.
    for port in 1..=20 {
        a.check(SocketAddr::new(peer(1).ip(), port), None)
            .await
            .unwrap();
    }
    let before = ip_snapshot(&db).await;
    limited(b.check(peer(1), None).await);
    limited(
        b.check("[::ffff:192.0.2.1]:1234".parse().unwrap(), None)
            .await,
    );
    assert!(before == ip_snapshot(&db).await);
    let email = Email::parse("owner@example.test").unwrap();
    limited(a.check(peer(1), Some(&email)).await);
    assert_eq!(count(&db, 1).await, 0); // Full IP budget cannot allocate an email row.
    a.check(peer(2), None).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT max(cardinality(attempts)) FROM registration_limits")
            .fetch_one(&db)
            .await
            .unwrap(),
        20
    );
    reset(&db).await;
    for _ in 0..20 {
        a.check("[2001:db8::1]:1234".parse().unwrap(), None)
            .await
            .unwrap();
    }
    limited(
        b.check("[2001:0db8:0:0:0:0:0:1]:4567".parse().unwrap(), None)
            .await,
    );

    // Canonical email limits are independent of IP and shared by every caller
    // (registration and resend use this same admission operation).
    reset(&db).await;
    let canonical = Email::parse(" OWNER@EXAMPLE.TEST ").unwrap();
    for n in 1..=5 {
        a.check(peer(n), Some(&canonical)).await.unwrap();
    }
    limited(b.check(peer(6), Some(&email)).await);
    assert_eq!(count(&db, 1).await, 1);
    // Email-denied requests still debit IP. Twenty such attempts block other emails too.
    for _ in 0..20 {
        limited(a.check(peer(20), Some(&email)).await);
    }
    let fresh = Email::parse("fresh@example.test").unwrap();
    limited(a.check(peer(20), Some(&fresh)).await);
    assert_eq!(count(&db, 1).await, 1);

    // A rolling window releases only old attempts, never the entire active budget.
    reset(&db).await;
    for _ in 0..5 {
        a.check(peer(1), Some(&email)).await.unwrap();
    }
    sqlx::query("UPDATE registration_limits SET attempts=ARRAY[statement_timestamp()-interval '61 minutes']||array_fill(statement_timestamp()-interval '30 minutes',ARRAY[4]),expires_at=statement_timestamp()+interval '30 minutes' WHERE kind=1").execute(&db).await.unwrap();
    b.check(peer(1), Some(&email)).await.unwrap();
    let retry = limited(a.check(peer(1), Some(&email)).await);
    assert!((1795..=1800).contains(&retry));
    // Equality at an hour expires, and admission works without a cleanup run.
    sqlx::query("UPDATE registration_limits SET attempts=array_fill(statement_timestamp()-interval '1 hour',ARRAY[cardinality(attempts)]),expires_at=statement_timestamp()").execute(&db).await.unwrap();
    assert_eq!(count(&db, 0).await, 1);
    a.check(peer(1), Some(&email)).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT max(cardinality(attempts)) FROM registration_limits")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );

    // Concurrent independent instances cannot overshoot either budget.
    reset(&db).await;
    let mut workers = Vec::new();
    for n in 0..40 {
        let limiter = if n % 2 == 0 { a.clone() } else { b.clone() };
        workers.push(tokio::spawn(
            async move { limiter.check(peer(1), None).await },
        ));
    }
    let mut allowed = 0;
    for worker in workers {
        match worker.await.unwrap() {
            Ok(()) => allowed += 1,
            denied => {
                limited(denied);
            }
        }
    }
    assert_eq!(allowed, 20);
    reset(&db).await;
    let mut workers = Vec::new();
    for n in 1..=25 {
        let limiter = if n % 2 == 0 { a.clone() } else { b.clone() };
        workers.push(tokio::spawn(async move {
            limiter
                .check(peer(n), Some(&Email::parse("shared@example.test").unwrap()))
                .await
        }));
    }
    let mut allowed = 0;
    for worker in workers {
        match worker.await.unwrap() {
            Ok(()) => allowed += 1,
            denied => {
                limited(denied);
            }
        }
    }
    assert_eq!(allowed, 5);
    assert_eq!(count(&db, 1).await, 1);
    assert_eq!(count(&db, 0).await, 25);

    // An email storage failure rolls back the IP debit and never allows work.
    reset(&db).await;
    sqlx::query("CREATE FUNCTION fail_email_limit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.kind=1 THEN RAISE EXCEPTION 'private diagnostic marker'; END IF; RETURN NEW; END; $$").execute(&db).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_email_limit BEFORE INSERT OR UPDATE ON registration_limits FOR EACH ROW EXECUTE FUNCTION fail_email_limit()").execute(&db).await.unwrap();
    assert_eq!(
        a.check(peer(1), Some(&email)).await,
        Err(ThrottleError::Unavailable)
    );
    assert_eq!(count(&db, 0).await, 0);
    sqlx::query("DROP TRIGGER fail_email_limit ON registration_limits")
        .execute(&db)
        .await
        .unwrap();
    a.check(peer(1), Some(&email)).await.unwrap();

    // Fresh clock checks after row-lock waits see expiry committed by the lock holder.
    reset(&db).await;
    for _ in 0..20 {
        a.check(peer(1), None).await.unwrap();
    }
    let mut holder = db.begin().await.unwrap();
    sqlx::query("SELECT subject FROM registration_limits WHERE kind=0 FOR UPDATE")
        .execute(&mut *holder)
        .await
        .unwrap();
    let other = b.clone();
    let mut waiter = tokio::spawn(async move { other.check(peer(1), None).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut waiter)
            .await
            .is_err()
    );
    sqlx::query("UPDATE registration_limits SET attempts=array_fill(statement_timestamp()-interval '1 hour',ARRAY[20]),expires_at=statement_timestamp() WHERE kind=0").execute(&mut *holder).await.unwrap();
    holder.commit().await.unwrap();
    waiter.await.unwrap().unwrap();
    // Locks held too long fail closed rather than creating an unbounded queue.
    let mut holder = db.begin().await.unwrap();
    sqlx::query("SELECT subject FROM registration_limits WHERE kind=0 FOR UPDATE")
        .execute(&mut *holder)
        .await
        .unwrap();
    assert_eq!(
        b.check(peer(1), None).await,
        Err(ThrottleError::Unavailable)
    );
    holder.rollback().await.unwrap();

    // Cleanup is bounded, skips locked rows and cannot erase a concurrent debit.
    reset(&db).await;
    sqlx::query("INSERT INTO registration_limits(kind,subject,attempts,expires_at) SELECT 0,decode(lpad(to_hex(n),64,'0'),'hex'),ARRAY[statement_timestamp()-interval '61 minutes'],statement_timestamp()-interval '1 minute' FROM generate_series(1,101)n").execute(&db).await.unwrap();
    let mut holder = db.begin().await.unwrap();
    sqlx::query("SELECT subject FROM registration_limits ORDER BY subject LIMIT 1 FOR UPDATE")
        .execute(&mut *holder)
        .await
        .unwrap();
    assert_eq!(cleanup_expired(&second_db).await.unwrap(), 100);
    assert_eq!(cleanup_expired(&second_db).await.unwrap(), 0);
    holder.commit().await.unwrap();
    assert_eq!(cleanup_expired(&db).await.unwrap(), 1);
    a.check(peer(1), None).await.unwrap();
    sqlx::query("UPDATE registration_limits SET attempts=ARRAY[statement_timestamp()-interval '61 minutes'],expires_at=statement_timestamp()-interval '1 minute'").execute(&db).await.unwrap();
    let (cleaned, charged) = tokio::join!(cleanup_expired(&db), b.check(peer(1), None));
    cleaned.unwrap();
    charged.unwrap();
    assert_eq!(count(&db, 0).await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT cardinality(attempts) FROM registration_limits")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    // Exercise the real cleanup binary with only DATABASE_URL.
    sqlx::query("UPDATE registration_limits SET attempts=ARRAY[statement_timestamp()-interval '61 minutes'],expires_at=statement_timestamp()-interval '1 minute'").execute(&db).await.unwrap();
    let mut worker_url = Url::parse(&url).unwrap();
    worker_url
        .query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_account-cleanup"))
        .arg("--once")
        .env("DATABASE_URL", worker_url.as_str())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(count(&db, 0).await, 0);

    // HTTP integration uses the transport peer, despite attacker-supplied forwarding headers.
    reset(&db).await;
    let key = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, [7; 32]);
    let security = BrowserSecurity::new("https://crow.localhost:3000", &key).unwrap();
    let browser = security.ensure_preauth(&HeaderMap::new()).unwrap();
    let csrf = browser.csrf(FormAction::Register);
    let mut page = message_page("Form", "Ready");
    browser.apply_cookie(&mut page);
    let cookie = page.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let app = security.protect(
        Router::new()
            .route("/submit", post(submit))
            .with_state(Harness {
                security: security.clone(),
                throttle: a.clone(),
            }),
    );
    assert_eq!(
        request(&app, cookie, "wrong", "owner@example.test", false)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(count(&db, 0).await, 0);
    for n in 0..20 {
        assert_eq!(
            request(
                &app,
                cookie,
                csrf.expose_secret(),
                &format!("owner{n}@example.test"),
                n % 2 == 0
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    let response = request(
        &app,
        cookie,
        csrf.expose_secret(),
        "another@example.test",
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(
        (1..=3600).contains(
            &response.headers()[header::RETRY_AFTER]
                .to_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
        )
    );
    let body = String::from_utf8(
        to_bytes(response.into_body(), 10000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(!body.contains("another@example.test"));
    assert!(!body.contains("192.0.2.200"));
    // A failed account operation cannot undo previously committed admission.
    let before = ip_snapshot(&db).await;
    let mut tx = db.begin().await.unwrap();
    assert!(sqlx::query("SELECT 1/0").execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    assert!(before == ip_snapshot(&db).await);
    db.close().await;
    let response = request(
        &app,
        cookie,
        csrf.expose_secret(),
        "private@example.test",
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!response.headers().contains_key(header::RETRY_AFTER));
    let body = String::from_utf8(
        to_bytes(response.into_body(), 10000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(!body.contains("private@example.test"));
    assert!(!body.contains("private diagnostic marker"));
    second_db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
