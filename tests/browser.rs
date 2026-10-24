use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tower::ServiceExt;

const ORIGIN: &str = "https://crow.example";
const PASSWORD: &str = " Correct horse nebula 🦀 battery ";

struct Reply {
    status: StatusCode,
    body: String,
    cookie: Option<String>,
    location: Option<String>,
}
async fn request(
    app: &Router,
    path: &str,
    cookie: Option<&str>,
    fields: Option<Vec<(&str, String)>>,
    origin: Option<&str>,
    htmx: bool,
) -> Reply {
    let mut builder =
        Request::builder()
            .uri(path)
            .method(if fields.is_some() { "POST" } else { "GET" });
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    if htmx {
        builder = builder.header("hx-request", "true");
    }
    let body = if let Some(fields) = fields {
        builder = builder.header("content-type", "application/x-www-form-urlencoded");
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish()
    } else {
        String::new()
    };
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    let status = response.status();
    let cookie = response.headers().get("set-cookie").map(|h| {
        let value = h.to_str().unwrap();
        assert!(
            value.contains("Secure")
                && value.contains("HttpOnly")
                && value.contains("SameSite=Lax")
                && value.contains("Path=/")
        );
        assert!(!value.contains("Domain="));
        value.split(';').next().unwrap().to_owned()
    });
    let location = response
        .headers()
        .get("location")
        .map(|h| h.to_str().unwrap().to_owned());
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    Reply {
        status,
        body,
        cookie,
        location,
    }
}
fn csrf(reply: &Reply) -> String {
    reply
        .body
        .split("name=\"csrf\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned()
}
fn fields(user: &str, pass: &str, csrf: &str) -> Vec<(&'static str, String)> {
    vec![
        ("username", user.into()),
        ("password", pass.into()),
        ("csrf", csrf.into()),
    ]
}
async fn form(app: &Router) -> (String, String) {
    let reply = request(app, "/register", None, None, None, false).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.body.contains("Password recovery is unavailable"));
    (reply.cookie.clone().unwrap(), csrf(&reply))
}
fn verifier(cookie: &str) -> Vec<u8> {
    Sha256::digest(cookie.split_once('=').unwrap().1.as_bytes()).to_vec()
}
async fn count(db: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(db)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL via TEST_DATABASE_URL"]
async fn browser_security_persistence_and_races() {
    let url =
        std::env::var("TEST_DATABASE_URL").expect("set TEST_DATABASE_URL to a disposable database");
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let path = format!("SET search_path TO {schema}");
    let db = PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |connection, _| {
            let path = path.clone();
            Box::pin(async move {
                sqlx::query(&path).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    crow_auth::migrate(&db).await.unwrap();
    crow_auth::migrate(&db).await.unwrap(); // migration reruns safely
    assert!(
        crow_auth::app(db.clone(), "http://crow.example")
            .await
            .is_err()
    );
    for origin in [
        "https://crow.example/",
        "https://u@crow.example",
        "https://crow.example/path",
        "https://crow.example?x=1",
    ] {
        assert!(crow_auth::app(db.clone(), origin).await.is_err());
    }
    let app = crow_auth::app(db.clone(), ORIGIN).await.unwrap();
    let (anonymous, token) = form(&app).await;
    // Plain and enhanced requests enforce the same cookie/CSRF/Origin controls.
    for htmx in [false, true] {
        for origin in [
            None,
            Some("https://evil.example"),
            Some("null"),
            Some("https://crow.example.evil"),
        ] {
            let r = request(
                &app,
                "/api/hosted/register",
                Some(&anonymous),
                Some(fields("alice", PASSWORD, &token)),
                origin,
                htmx,
            )
            .await;
            assert_eq!(r.status, StatusCode::FORBIDDEN);
        }
        let r = request(
            &app,
            "/api/hosted/register",
            Some(&anonymous),
            Some(fields("alice", PASSWORD, "wrong")),
            Some(ORIGIN),
            htmx,
        )
        .await;
        assert_eq!(r.status, StatusCode::FORBIDDEN);
        let r = request(
            &app,
            "/api/hosted/register",
            None,
            Some(fields("alice", PASSWORD, &token)),
            Some(ORIGIN),
            htmx,
        )
        .await;
        assert_eq!(r.status, StatusCode::FORBIDDEN);
    }
    assert_eq!(count(&db, "accounts").await, 0);
    let r = request(&app, "/register?flow=expired", None, None, None, false).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.cookie.is_none());
    for (user, password) in [
        ("ab", PASSWORD),
        ("valid_name", "123456789123456789"),
        ("valid_name", "short"),
    ] {
        let r = request(
            &app,
            "/api/hosted/register",
            Some(&anonymous),
            Some(fields(user, password, &token)),
            Some(ORIGIN),
            false,
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
    }
    // One winner when two tabs submit the same initiating browser session.
    let (a, b) = tokio::join!(
        request(
            &app,
            "/api/hosted/register",
            Some(&anonymous),
            Some(fields(" ALICE ", PASSWORD, &token)),
            Some(ORIGIN),
            false
        ),
        request(
            &app,
            "/api/hosted/register",
            Some(&anonymous),
            Some(fields("other_name", PASSWORD, &token)),
            Some(ORIGIN),
            true
        )
    );
    assert!([a.status, b.status].contains(&StatusCode::SEE_OTHER));
    assert!([a.status, b.status].contains(&StatusCode::FORBIDDEN));
    let success = if a.status == StatusCode::SEE_OTHER {
        a
    } else {
        b
    };
    let account_cookie = success.cookie.unwrap();
    assert_ne!(anonymous, account_cookie);
    assert_eq!(success.location.as_deref(), Some("/account"));
    assert_eq!(count(&db, "accounts").await, 1);
    let row = sqlx::query("SELECT username,password_hash FROM accounts")
        .fetch_one(&db)
        .await
        .unwrap();
    let user: String = row.get("username");
    assert!(
        row.get::<String, _>("password_hash")
            .starts_with("$argon2id$v=19$m=19456,t=2,p=1$")
    );
    let r = request(&app, "/account", Some(&anonymous), None, None, false).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let account = request(&app, "/account", Some(&account_cookie), None, None, true).await;
    assert_eq!(account.status, StatusCode::OK);
    assert!(account.body.contains(&format!("Username: {user}")));
    assert!(account.body.contains("Subscriber: false"));
    let info = sqlx::query("SELECT extract(epoch FROM expires_at-authenticated_at)::bigint AS lifetime FROM browser_sessions WHERE verifier=$1").bind(verifier(&account_cookie)).fetch_one(&db).await.unwrap();
    assert_eq!(info.get::<i64, _>("lifetime"), 28800);
    // Reloads/SSO-style reads preserve the absolute deadline and authentication time.
    assert!(
        request(&app, "/login", Some(&account_cookie), None, None, false)
            .await
            .cookie
            .is_none()
    );
    // Independent sessions race on normalized username uniqueness.
    let (c1, t1) = form(&app).await;
    let (c2, t2) = form(&app).await;
    let (a, b) = tokio::join!(
        request(
            &app,
            "/api/hosted/register",
            Some(&c1),
            Some(fields("unique_race", PASSWORD, &t1)),
            Some(ORIGIN),
            false
        ),
        request(
            &app,
            "/api/hosted/register",
            Some(&c2),
            Some(fields(" UNIQUE_RACE ", PASSWORD, &t2)),
            Some(ORIGIN),
            true
        )
    );
    assert!([a.status, b.status].contains(&StatusCode::SEE_OTHER));
    assert!([a.status, b.status].contains(&StatusCode::BAD_REQUEST));
    assert_eq!(count(&db, "accounts").await, 2);
    // A database failure after account insertion rolls the whole operation back.
    sqlx::raw_sql("CREATE FUNCTION fail_session() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.user_id IS NOT NULL AND EXISTS(SELECT 1 FROM accounts WHERE id=NEW.user_id AND username='rollback_case') THEN RAISE EXCEPTION 'injected session failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER fail_session BEFORE INSERT ON browser_sessions FOR EACH ROW EXECUTE FUNCTION fail_session();").execute(&db).await.unwrap();
    let (rollback, rollback_token) = form(&app).await;
    let r = request(
        &app,
        "/api/hosted/register",
        Some(&rollback),
        Some(fields("rollback_case", PASSWORD, &rollback_token)),
        Some(ORIGIN),
        false,
    )
    .await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(r.cookie.is_none());
    assert_eq!(count(&db, "accounts").await, 2);
    let r = request(&app, "/register", Some(&rollback), None, None, false).await;
    assert!(r.cookie.is_none());
    // Generic failure text, then credential authentication rotates the handle.
    let (login_cookie, login_token) = form(&app).await;
    let wrong = request(
        &app,
        "/api/hosted/login",
        Some(&login_cookie),
        Some(fields(&user, "incorrect password value", &login_token)),
        Some(ORIGIN),
        false,
    )
    .await;
    let unknown = request(
        &app,
        "/api/hosted/login",
        Some(&login_cookie),
        Some(fields(
            "unknown_account",
            "incorrect password value",
            &login_token,
        )),
        Some(ORIGIN),
        true,
    )
    .await;
    assert_eq!(wrong.body, unknown.body);
    let r = request(
        &app,
        "/api/hosted/login",
        Some(&login_cookie),
        Some(fields(&user.to_uppercase(), PASSWORD, &login_token)),
        Some(ORIGIN),
        true,
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let logged_in = r.cookie.unwrap();
    assert_ne!(logged_in, login_cookie);
    let r = request(
        &app,
        "/api/hosted/login",
        Some(&login_cookie),
        Some(fields(&user, PASSWORD, &login_token)),
        Some(ORIGIN),
        false,
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // CSRF tokens cannot transfer between browser sessions.
    let r = request(
        &app,
        "/api/portal/logout",
        Some(&logged_in),
        Some(vec![("csrf", csrf(&account))]),
        Some(ORIGIN),
        false,
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let page = request(&app, "/account", Some(&logged_in), None, None, false).await;
    let r = request(
        &app,
        "/api/portal/logout",
        Some(&logged_in),
        Some(vec![("csrf", csrf(&page))]),
        Some(ORIGIN),
        true,
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(r.cookie.as_deref(), Some("__Host-crow="));
    assert_eq!(
        request(&app, "/account", Some(&logged_in), None, None, false)
            .await
            .status,
        StatusCode::SEE_OTHER
    );
    // Logout affects only its current browser session.
    assert_eq!(
        request(&app, "/account", Some(&account_cookie), None, None, false)
            .await
            .status,
        StatusCode::OK
    );
    // Absolute expiry is enforced without background cleanup.
    sqlx::query("UPDATE browser_sessions SET expires_at=clock_timestamp()-interval '1 second', created_at=clock_timestamp()-interval '9 hours' WHERE verifier=$1").bind(verifier(&account_cookie)).execute(&db).await.unwrap();
    assert_eq!(
        request(&app, "/account", Some(&account_cookie), None, None, false)
            .await
            .status,
        StatusCode::SEE_OTHER
    );
    let r = request(
        &app,
        "/api/portal/logout",
        Some(&account_cookie),
        Some(vec![("csrf", csrf(&account))]),
        Some(ORIGIN),
        false,
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // Request limits also apply to enhanced requests.
    let r = request(
        &app,
        "/api/hosted/login",
        Some(&rollback),
        Some(fields(&user, &"x".repeat(17000), &rollback_token)),
        Some(ORIGIN),
        true,
    )
    .await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
    // A session expiring while queued for its row lock must fail after the wait.
    let (waiting_cookie, waiting_csrf) = form(&app).await;
    let mut blocker = db.begin().await.unwrap();
    sqlx::query("UPDATE browser_sessions SET expires_at=clock_timestamp()+interval '300 milliseconds' WHERE verifier=$1")
        .bind(verifier(&waiting_cookie)).execute(&mut *blocker).await.unwrap();
    blocker.commit().await.unwrap();
    let mut blocker = db.begin().await.unwrap();
    sqlx::query("SELECT verifier FROM browser_sessions WHERE verifier=$1 FOR UPDATE")
        .bind(verifier(&waiting_cookie))
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (waiting_reply, _) = tokio::join!(
        request(
            &app,
            "/api/portal/logout",
            Some(&waiting_cookie),
            Some(vec![("csrf", waiting_csrf)]),
            Some(ORIGIN),
            false
        ),
        async {
            sqlx::query("SELECT pg_sleep(0.5)")
                .execute(&mut *blocker)
                .await
                .unwrap();
            blocker.commit().await.unwrap();
        }
    );
    assert_eq!(waiting_reply.status, StatusCode::FORBIDDEN);
    assert!(waiting_reply.cookie.is_none());
    // A fresh router uses persisted account/session state.
    let restarted = crow_auth::app(db.clone(), ORIGIN).await.unwrap();
    let (c, t) = form(&restarted).await;
    let r = request(
        &restarted,
        "/api/hosted/login",
        Some(&c),
        Some(fields(&user, PASSWORD, &t)),
        Some(ORIGIN),
        false,
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    // Logout and a credential submission serialize on the authenticated handle.
    let current = r.cookie.clone().unwrap();
    let page = request(&restarted, "/account", Some(&current), None, None, false).await;
    let current_csrf = csrf(&page);
    let (logout_reply, login_reply) = tokio::join!(
        request(
            &restarted,
            "/api/portal/logout",
            Some(&current),
            Some(vec![("csrf", current_csrf.clone())]),
            Some(ORIGIN),
            false
        ),
        request(
            &restarted,
            "/api/hosted/login",
            Some(&current),
            Some(fields(&user, PASSWORD, &current_csrf)),
            Some(ORIGIN),
            true
        )
    );
    assert!([logout_reply.status, login_reply.status].contains(&StatusCode::SEE_OTHER));
    assert!([logout_reply.status, login_reply.status].contains(&StatusCode::FORBIDDEN));
    assert_eq!(
        request(&restarted, "/account", Some(&current), None, None, false)
            .await
            .status,
        StatusCode::SEE_OTHER
    );
    // Lost DB access never yields account information or authentication success.
    db.close().await;
    assert_eq!(
        request(
            &restarted,
            "/account",
            r.cookie.as_deref(),
            None,
            None,
            false
        )
        .await
        .status,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        request(&restarted, "/register", None, None, None, false)
            .await
            .status,
        StatusCode::SERVICE_UNAVAILABLE
    );
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
