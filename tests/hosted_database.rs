//! Checks real registration/resend routes, canonical conflicts, atomic mail and startup failures.

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
    response::Response,
};
use crow_auth::{
    browser::BrowserSecurity,
    hosted::HostedRegistration,
    mail::{DeliveryOutcome, MailConfig, MailSettings, deliver_one},
    password::{PasswordDigest, PasswordService},
    verification,
};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::net::SocketAddr;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;

const ORIGIN: &str = "https://crow.localhost:3000";
const PASSWORD: &str = " an original account phrase 🦀 ";
fn key(byte: u8) -> String {
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        [byte; 32],
    )
}
fn mail(port: u16) -> MailConfig {
    MailConfig::new(MailSettings {
        sender: "crow@example.test".into(),
        origin: ORIGIN.into(),
        encryption_key: key(8),
        smtp_host: "127.0.0.1".into(),
        smtp_port: port,
        smtp_username: None,
        smtp_password: None,
        local_capture: true,
    })
    .unwrap()
}
fn hidden(html: &str, name: &str) -> String {
    html.split(&format!("name=\"{name}\" value=\""))
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .into()
}
async fn text(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 30000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
async fn page(app: &Router, path: &str, cookie: Option<&str>) -> Response {
    let mut request = Request::builder().uri(path);
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}
async fn form(app: &Router, path: &str) -> (String, String) {
    let response = page(app, path, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
    let set = response.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(set.starts_with("__Host-crow-preauth="));
    assert!(set.contains("Secure"));
    assert!(set.contains("HttpOnly"));
    assert!(!set.contains("Domain="));
    let cookie = set.split(';').next().unwrap().to_owned();
    let html = text(response).await;
    (cookie, hidden(&html, "csrf"))
}
async fn submit(
    app: &Router,
    path: &str,
    cookie: &str,
    fields: &[(&str, &str)],
    peer: u8,
    htmx: bool,
) -> Response {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().copied())
        .finish();
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("origin", ORIGIN)
        .header("cookie", cookie)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-forwarded-for", "203.0.113.99");
    if htmx {
        request = request.header("hx-request", "true");
    }
    let mut request = request.body(Body::from(body)).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([192, 0, 2, peer], 40000))));
    app.clone().oneshot(request).await.unwrap()
}
async fn register(
    app: &Router,
    cookie: &str,
    csrf: &str,
    username: &str,
    email: &str,
    password: &str,
) -> Response {
    submit(
        app,
        "/api/hosted/register",
        cookie,
        &[
            ("csrf", csrf),
            ("username", username),
            ("email", email),
            ("password", password),
        ],
        1,
        false,
    )
    .await
}
fn generic(response: &Response) {
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/verify-email");
    assert!(!response.headers().contains_key(header::SET_COOKIE));
}
async fn limits(db: &PgPool) {
    sqlx::query("TRUNCATE registration_limits")
        .execute(db)
        .await
        .unwrap();
}
async fn snapshot(db: &PgPool, id: Uuid) -> (String, i64, String) {
    sqlx::query_as(
        "SELECT password_hash,security_version,pending_expires_at::text FROM accounts WHERE id=$1",
    )
    .bind(id)
    .fetch_one(db)
    .await
    .unwrap()
}
async fn id(db: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM accounts WHERE email=$1")
        .bind(email)
        .fetch_one(db)
        .await
        .unwrap()
}
async fn job(db: &PgPool, id: Uuid) -> (Uuid, Vec<u8>, Vec<u8>) {
    sqlx::query_as("SELECT q.action_id,q.nonce,q.ciphertext FROM verification_mail_outbox q JOIN verification_actions v ON v.id=q.action_id WHERE v.account_id=$1").bind(id).fetch_one(db).await.unwrap()
}
async fn expire(db: &PgPool, id: Uuid) {
    sqlx::query("UPDATE accounts SET created_at=statement_timestamp()-interval '24 hours',pending_expires_at=statement_timestamp() WHERE id=$1").bind(id).execute(db).await.unwrap();
}
async fn drain(db: &PgPool, mail: &MailConfig) {
    for _ in 0..30 {
        match deliver_one(db, mail).await.unwrap() {
            DeliveryOutcome::Idle => return,
            DeliveryOutcome::Delivered | DeliveryOutcome::Discarded => {}
            DeliveryOutcome::RetryScheduled => panic!("capture service unavailable"),
        }
    }
    panic!("unexpected queue size")
}
async fn tokens(capture: &str, email: &str) -> Vec<String> {
    let messages: Value = reqwest::get(format!("{capture}/api/v1/messages"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut result = Vec::new();
    for message in messages["messages"].as_array().unwrap() {
        if !message["To"]
            .as_array()
            .unwrap()
            .iter()
            .any(|to| to["Address"] == email)
        {
            continue;
        }
        let body: Value = reqwest::get(format!(
            "{capture}/api/v1/message/{}",
            message["ID"].as_str().unwrap()
        ))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        let url = Url::parse(
            body["Text"]
                .as_str()
                .unwrap()
                .lines()
                .find(|line| line.starts_with(ORIGIN))
                .unwrap(),
        )
        .unwrap();
        result.push(
            url.query_pairs()
                .find(|(name, _)| name == "token")
                .unwrap()
                .1
                .into_owned(),
        );
    }
    result
}
async fn consume(db: &PgPool, raw: &str) -> bool {
    let mut tx = db.begin().await.unwrap();
    let success = verification::consume(&mut tx, raw).await.is_ok();
    if success {
        tx.commit().await.unwrap();
    } else {
        tx.rollback().await.unwrap();
    }
    success
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL and SMTP capture; CI invokes explicitly"]
async fn registration_resend_conflicts_failures_and_startup() {
    let url = std::env::var("TEST_DATABASE_URL").expect("disposable database required");
    let capture = std::env::var("TEST_MAILPIT_URL").expect("local capture required");
    let port = std::env::var("TEST_SMTP_PORT").unwrap().parse().unwrap();
    let capture_url = Url::parse(&capture).unwrap();
    assert_eq!(capture_url.scheme(), "http");
    assert!(matches!(
        capture_url.host_str(),
        Some("127.0.0.1" | "localhost" | "[::1]")
    ));
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("hosted_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let path = format!("SET search_path TO {schema}");
    let db = PgPoolOptions::new()
        .max_connections(10)
        .after_connect(move |conn, _| {
            let path = path.clone();
            Box::pin(async move {
                sqlx::query(&path).execute(conn).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    crow_auth::migrate(&db).await.unwrap();
    crow_auth::migrate(&db).await.unwrap();
    let passwords = PasswordService::load("tests/fixtures/compromised-passwords.txt").unwrap();
    let app = HostedRegistration::new(
        db.clone(),
        BrowserSecurity::new(ORIGIN, &key(7)).unwrap(),
        passwords.clone(),
        mail(port),
    )
    .router();
    let sender = mail(port);
    let (cookie, csrf) = form(&app, "/register").await;
    generic(
        &register(
            &app,
            &cookie,
            &csrf,
            " OWNER_ONE ",
            " OWNER@EXAMPLE.TEST ",
            PASSWORD,
        )
        .await,
    );
    let owner = id(&db, "owner@example.test").await;
    let original = snapshot(&db, owner).await;
    assert!(
        passwords
            .verify(
                PASSWORD.into(),
                PasswordDigest::from_phc(original.0.clone()).unwrap()
            )
            .await
            .unwrap()
    );
    assert!(
        !passwords
            .verify(
                PASSWORD.trim().into(),
                PasswordDigest::from_phc(original.0.clone()).unwrap()
            )
            .await
            .unwrap()
    );
    assert!(sqlx::query_scalar::<_,bool>("SELECT status='pending' AND security_version=0 AND verified_at IS NULL FROM accounts WHERE id=$1").bind(owner).fetch_one(&db).await.unwrap());
    job(&db, owner).await;
    drain(&db, &sender).await;
    let old = tokens(&capture, "owner@example.test").await.pop().unwrap();
    let response = page(&app, &format!("/verify-email?token={old}"), None).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!text(response).await.contains(&old));
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT consumed_at IS NOT NULL FROM verification_actions WHERE account_id=$1"
        )
        .bind(owner)
        .fetch_one(&db)
        .await
        .unwrap()
    );

    // Email conflicts, including exact duplicates, are generic no-ops and cannot replace credentials.
    for username in ["different_owner", "owner_one"] {
        generic(
            &register(
                &app,
                &cookie,
                &csrf,
                username,
                " OWNER@EXAMPLE.TEST ",
                " a different valid password phrase ",
            )
            .await,
        );
        assert!(original == snapshot(&db, owner).await);
    }
    let response = register(
        &app,
        &cookie,
        &csrf,
        "owner_one",
        "other@example.test",
        PASSWORD,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let html = text(response).await;
    assert!(html.contains("Username unavailable"));
    assert!(!html.contains(PASSWORD));
    limits(&db).await;
    for (username, email, password) in [
        ("ab", "invalid_user@example.test", PASSWORD),
        ("valid_name", "\"><script>alert(1)</script>", PASSWORD),
        ("valid_name", "short@example.test", "short"),
        ("valid_name", "blocked@example.test", "passwordpassword"),
    ] {
        let response = register(&app, &cookie, &csrf, username, email, password).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let html = text(response).await;
        assert!(!html.contains("<script>"));
        assert!(!html.contains(&format!("value=\"{password}\"")));
    }
    // Missing/wrong CSRF or Origin never reaches the limiter/account code.
    limits(&db).await;
    let (resend_cookie, resend_csrf) = form(&app, "/verify-email").await;
    for token in ["", resend_csrf.as_str()] {
        assert_eq!(
            register(
                &app,
                &cookie,
                token,
                "forbidden_user",
                "forbidden@example.test",
                PASSWORD
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    for origin in [None, Some("null"), Some("https://evil.test")] {
        let mut req = Request::builder()
            .method("POST")
            .uri("/api/hosted/register")
            .header("cookie", &cookie)
            .header("content-type", "application/x-www-form-urlencoded");
        if let Some(origin) = origin {
            req = req.header("origin", origin);
        }
        let mut req = req.body(Body::from(format!("csrf={csrf}"))).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([192, 0, 2, 1], 40000))));
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM registration_limits")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        register(
            &app,
            &cookie,
            &csrf,
            "too_large",
            "large@example.test",
            &"x".repeat(20000)
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        page(&app, "/register?flow=untrusted", None).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        submit(
            &app,
            "/api/hosted/register",
            &cookie,
            &[
                ("csrf", &csrf),
                ("email", "flow@example.test"),
                ("username", "flow_user"),
                ("password", PASSWORD),
                ("flow", "untrusted")
            ],
            1,
            true
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    // Resend uses its own action-specific CSRF, atomically replaces the link and keeps credentials/deadline.
    limits(&db).await;
    generic(
        &submit(
            &app,
            "/api/hosted/resend-verification",
            &resend_cookie,
            &[("csrf", &resend_csrf), ("email", " OWNER@EXAMPLE.TEST ")],
            2,
            true,
        )
        .await,
    );
    assert!(original == snapshot(&db, owner).await);
    assert!(!consume(&db, &old).await);
    drain(&db, &sender).await;
    let replacement = tokens(&capture, "owner@example.test")
        .await
        .into_iter()
        .find(|token| token != &old)
        .unwrap();
    assert!(consume(&db, &replacement).await);
    for email in ["owner@example.test", "unknown@example.test"] {
        generic(
            &submit(
                &app,
                "/api/hosted/resend-verification",
                &resend_cookie,
                &[("csrf", &resend_csrf), ("email", email)],
                2,
                false,
            )
            .await,
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM verification_mail_outbox")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );

    // Expired reservations and queued payloads are reclaimed in the replacement transaction.
    limits(&db).await;
    generic(
        &register(
            &app,
            &cookie,
            &csrf,
            "expired_owner",
            "expired@example.test",
            PASSWORD,
        )
        .await,
    );
    let expired = id(&db, "expired@example.test").await;
    let old_job = job(&db, expired).await;
    expire(&db, expired).await;
    generic(
        &submit(
            &app,
            "/api/hosted/resend-verification",
            &resend_cookie,
            &[("csrf", &resend_csrf), ("email", "expired@example.test")],
            3,
            false,
        )
        .await,
    );
    assert!(old_job == job(&db, expired).await);
    generic(
        &register(
            &app,
            &cookie,
            &csrf,
            "expired_owner",
            "expired@example.test",
            " a replacement account password ",
        )
        .await,
    );
    let renewed = id(&db, "expired@example.test").await;
    assert_ne!(renewed, expired);
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM verification_actions WHERE id=$1)"
        )
        .bind(old_job.0)
        .fetch_one(&db)
        .await
        .unwrap()
    );
    assert!(
        passwords
            .verify(
                " a replacement account password ".into(),
                PasswordDigest::from_phc(snapshot(&db, renewed).await.0).unwrap()
            )
            .await
            .unwrap()
    );
    drain(&db, &sender).await;

    // Concurrent exact duplicate submissions create one pending identity and one mail job.
    limits(&db).await;
    let (a, b) = tokio::join!(
        register(
            &app,
            &cookie,
            &csrf,
            "race_owner",
            "race@example.test",
            PASSWORD
        ),
        register(
            &app,
            &cookie,
            &csrf,
            "race_owner",
            "race@example.test",
            PASSWORD
        )
    );
    generic(&a);
    generic(&b);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM accounts WHERE email='race@example.test'"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        1
    );
    let raced = id(&db, "race@example.test").await;
    job(&db, raced).await;

    // An outbox failure leaves neither a partial new account nor partially reclaimed old state.
    limits(&db).await;
    sqlx::query("CREATE FUNCTION fail_hosted_mail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'private diagnostic marker'; END; $$").execute(&db).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_hosted_mail BEFORE INSERT ON verification_mail_outbox FOR EACH ROW EXECUTE FUNCTION fail_hosted_mail()").execute(&db).await.unwrap();
    let response = register(
        &app,
        &cookie,
        &csrf,
        "failed_owner",
        "failed@example.test",
        PASSWORD,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let html = text(response).await;
    assert!(!html.contains(PASSWORD));
    assert!(!html.contains("failed@example.test"));
    assert!(!html.contains("private diagnostic marker"));
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE email='failed@example.test')"
        )
        .fetch_one(&db)
        .await
        .unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT sum(cardinality(attempts)) FROM registration_limits")
            .fetch_one(&db)
            .await
            .unwrap(),
        2
    );
    let before_job = job(&db, raced).await;
    let before = snapshot(&db, raced).await;
    let response = submit(
        &app,
        "/api/hosted/resend-verification",
        &resend_cookie,
        &[("csrf", &resend_csrf), ("email", "race@example.test")],
        3,
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(before_job == job(&db, raced).await);
    assert!(before == snapshot(&db, raced).await);
    expire(&db, raced).await;
    let before = snapshot(&db, raced).await;
    assert_eq!(
        register(
            &app,
            &cookie,
            &csrf,
            "race_owner",
            "race@example.test",
            " another replacement password "
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(id(&db, "race@example.test").await, raced);
    assert!(before == snapshot(&db, raced).await);
    assert!(before_job == job(&db, raced).await);
    sqlx::query("DROP TRIGGER fail_hosted_mail ON verification_mail_outbox")
        .execute(&db)
        .await
        .unwrap();

    // The complete Unicode policy fits the body bound and preserves original bytes.
    limits(&db).await;
    let unicode = "🦀".repeat(128);
    generic(
        &register(
            &app,
            &cookie,
            &csrf,
            "unicode_owner",
            "unicode@example.test",
            &unicode,
        )
        .await,
    );
    let unicode_id = id(&db, "unicode@example.test").await;
    assert!(
        passwords
            .verify(
                unicode,
                PasswordDigest::from_phc(snapshot(&db, unicode_id).await.0).unwrap()
            )
            .await
            .unwrap()
    );
    // Application integration actually enforces the shared IP limit before hashing/account writes.
    limits(&db).await;
    let limiter = crow_auth::throttle::RegistrationThrottle::new(db.clone());
    for _ in 0..20 {
        limiter
            .check(SocketAddr::from(([192, 0, 2, 1], 40000)), None)
            .await
            .unwrap();
    }
    let response = register(
        &app,
        &cookie,
        &csrf,
        "limited_owner",
        "limited@example.test",
        PASSWORD,
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key(header::RETRY_AFTER));
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE email='limited@example.test')"
        )
        .fetch_one(&db)
        .await
        .unwrap()
    );

    // Startup must reject missing browser/mail/blocklist settings without logging their values.
    let mut process_url = Url::parse(&url).unwrap();
    process_url
        .query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    for (name, value) in [
        ("BROWSER_SIGNING_KEY", None),
        ("MAIL_ENCRYPTION_KEY", Some("private-key-marker")),
        ("PASSWORD_BLOCKLIST_PATH", Some("private-path-marker")),
    ] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_crow-auth"));
        command
            .env("DATABASE_URL", process_url.as_str())
            .env("CROW_ORIGIN", ORIGIN)
            .env("BROWSER_SIGNING_KEY", key(7))
            .env("MAIL_ENCRYPTION_KEY", key(8))
            .env("MAIL_FROM", "crow@example.test")
            .env("SMTP_HOST", "127.0.0.1")
            .env("SMTP_PORT", port.to_string())
            .env("MAIL_LOCAL_CAPTURE", "true")
            .env(
                "PASSWORD_BLOCKLIST_PATH",
                "tests/fixtures/compromised-passwords.txt",
            )
            .env_remove("SMTP_USERNAME")
            .env_remove("SMTP_PASSWORD");
        if let Some(value) = value {
            command.env(name, value);
        } else {
            command.env_remove(name);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("private-key-marker"));
        assert!(!stderr.contains("private-path-marker"));
    }
    db.close().await;
    let response = register(
        &app,
        &cookie,
        &csrf,
        "unavailable_owner",
        "unavailable@example.test",
        PASSWORD,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
