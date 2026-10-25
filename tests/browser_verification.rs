//! Exercises stateless browser CSRF with persistent, single-use verification actions in an Axum harness.

use axum::{
    Form, Router,
    body::{Body, to_bytes},
    extract::{Query, State},
    http::{HeaderMap, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use crow_auth::{
    browser::{BrowserError, BrowserSecurity, FormAction, confirmation_page, message_page},
    verification,
};
use serde::Deserialize;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://crow.localhost:3000";
#[derive(Clone)]
struct Harness {
    db: PgPool,
    security: BrowserSecurity,
}
#[derive(Deserialize)]
struct Token {
    token: String,
}
#[derive(Deserialize)]
struct Fields {
    #[serde(default)]
    csrf: String,
    token: String,
}
async fn form(
    State(h): State<Harness>,
    headers: HeaderMap,
    Query(query): Query<Token>,
) -> Result<Response, BrowserError> {
    let browser = h.security.ensure_preauth(&headers)?;
    let mut response = confirmation_page(&browser.csrf(FormAction::VerifyEmail), &query.token);
    browser.apply_cookie(&mut response);
    Ok(response)
}
async fn submit(
    State(h): State<Harness>,
    headers: HeaderMap,
    Form(fields): Form<Fields>,
) -> Response {
    if let Err(error) = h
        .security
        .authorize_post(&headers, &fields.csrf, FormAction::VerifyEmail)
    {
        return error.into_response();
    }
    let mut tx = h.db.begin().await.unwrap();
    match verification::consume(&mut tx, &fields.token).await {
        Ok(_) => {
            tx.commit().await.unwrap();
            message_page(
                "Verified",
                "The account is active; no login session was created.",
            )
        }
        Err(_) => {
            tx.rollback().await.unwrap();
            let mut response = message_page("Invalid link", "Request another link");
            *response.status_mut() = StatusCode::BAD_REQUEST;
            response
        }
    }
}
fn hidden(html: &str, name: &str) -> String {
    html.split(&format!("name=\"{name}\" value=\""))
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned()
}
async fn open(app: &Router, raw: &str) -> (String, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/verify-email?token={raw}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let html = String::from_utf8(
        to_bytes(response.into_body(), 20000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("name=\"token\""));
    assert!(!html.contains("name=\"context\""));
    assert!(hidden(&html, "token") == raw);
    (cookie, hidden(&html, "csrf"))
}
async fn post(app: &Router, cookie: &str, csrf: &str, raw: &str, htmx: bool) -> StatusCode {
    let mut request = Request::builder()
        .method("POST")
        .uri("/api/hosted/verify-email")
        .header("origin", ORIGIN)
        .header("cookie", cookie)
        .header("content-type", "application/x-www-form-urlencoded");
    if htmx {
        request = request.header("hx-request", "true");
    }
    app.clone()
        .oneshot(
            request
                .body(Body::from(format!("csrf={csrf}&token={raw}")))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL; CI invokes explicitly"]
async fn signed_browser_forms_use_existing_verification_actions_without_new_tables() {
    let url = std::env::var("TEST_DATABASE_URL").expect("disposable database required");
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("browser_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let path = format!("SET search_path TO {schema}");
    let db = PgPoolOptions::new()
        .max_connections(5)
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
    let removed:i64=sqlx::query_scalar("SELECT count(*) FROM information_schema.tables WHERE table_schema=current_schema() AND table_name IN ('preauth_browsers','verification_contexts')").fetch_one(&db).await.unwrap();
    assert_eq!(removed, 0);
    let id = Uuid::new_v4();
    let mut tx = db.begin().await.unwrap();
    sqlx::query("INSERT INTO accounts(id,username,email,password_hash,created_at,pending_expires_at) SELECT $1,'browser_owner','owner@example.test','$argon2id$v=19$fixture',now,now+interval '24 hours' FROM (SELECT clock_timestamp() now)c").bind(id).execute(&mut *tx).await.unwrap();
    let raw = verification::issue(&mut tx, id).await.unwrap();
    tx.commit().await.unwrap();
    let raw = raw.expose_secret();
    let key = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, [7; 32]);
    let security = BrowserSecurity::new(ORIGIN, &key).unwrap();
    let app = security.protect(
        Router::new()
            .route("/verify-email", get(form))
            .route("/api/hosted/verify-email", axum::routing::post(submit))
            .with_state(Harness {
                db: db.clone(),
                security: security.clone(),
            }),
    );
    // Scanner/PC/phone GETs are stateless and leave the account/action untouched.
    let (pc_cookie, pc_csrf) = open(&app, raw).await;
    let (phone_cookie, phone_csrf) = open(&app, raw).await;
    let untouched:bool=sqlx::query_scalar("SELECT a.status='pending' AND v.consumed_at IS NULL FROM accounts a JOIN verification_actions v ON v.account_id=a.id WHERE a.id=$1").bind(id).fetch_one(&db).await.unwrap();
    assert!(untouched);
    for htmx in [false, true] {
        assert_eq!(
            post(&app, &pc_cookie, &phone_csrf, raw, htmx).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            post(&app, &pc_cookie, "", raw, htmx).await,
            StatusCode::FORBIDDEN
        );
    }
    // Both browsers can legitimately confirm; the DB action has exactly one winner.
    let (pc, phone) = tokio::join!(
        post(&app, &pc_cookie, &pc_csrf, raw, false),
        post(&app, &phone_cookie, &phone_csrf, raw, true)
    );
    assert!(
        (pc == StatusCode::OK && phone == StatusCode::BAD_REQUEST)
            || (phone == StatusCode::OK && pc == StatusCode::BAD_REQUEST)
    );
    assert_eq!(
        post(&app, &phone_cookie, &phone_csrf, raw, false).await,
        StatusCode::BAD_REQUEST
    );
    assert!(sqlx::query_scalar::<_,bool>("SELECT status='active' AND verified_at IS NOT NULL AND security_version=0 FROM accounts WHERE id=$1").bind(id).fetch_one(&db).await.unwrap());
    // Escaping still applies when an untrusted query token is echoed into a hidden field.
    let browser = security.ensure_preauth(&HeaderMap::new()).unwrap();
    let response = confirmation_page(
        &browser.csrf(FormAction::VerifyEmail),
        "\"><script>alert(1)</script>",
    );
    let html = String::from_utf8(
        to_bytes(response.into_body(), 20000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(!html.contains("<script>"));
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
