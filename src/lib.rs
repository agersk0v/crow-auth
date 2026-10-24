mod password;
mod session;

use askama::Template;
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: PgPool,
    origin: String,
    passwords: password::Passwords,
}

pub async fn app(db: PgPool, origin: &str) -> Result<Router, &'static str> {
    let parsed = url::Url::parse(origin).map_err(|_| "APP_ORIGIN must be an HTTPS origin")?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.origin().ascii_serialization() != origin
    {
        return Err("APP_ORIGIN must be a canonical HTTPS origin without a path");
    }
    let state = AppState {
        db,
        origin: origin.into(),
        passwords: password::Passwords::new()
            .await
            .map_err(|_| "Password worker initialization failed")?,
    };
    Ok(Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/login", get(login_page))
        .route("/register", get(register_page))
        .route("/account", get(account))
        .route("/api/portal/account", get(account))
        .route("/api/hosted/register", post(register))
        .route("/api/hosted/login", post(login))
        .route("/api/portal/logout", post(logout))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn(security_headers))
        .with_state(state))
}

pub async fn migrate(db: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(db).await
}

async fn security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (name, value) in [
        ("cache-control", "no-store"),
        ("pragma", "no-cache"),
        ("x-frame-options", "DENY"),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'none'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
        ),
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(name),
            axum::http::HeaderValue::from_static(value),
        );
    }
    response
}

#[derive(Template, Default)]
#[template(path = "page.html")]
struct Page<'a> {
    title: &'a str,
    message: &'a str,
    csrf: String,
    register: bool,
    account: bool,
    username: String,
    id: String,
    subscriber: bool,
}
fn render(page: Page<'_>) -> Response {
    match page.render() {
        Ok(body) => Html(body).into_response(),
        Err(_) => unavailable(),
    }
}
fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Service temporarily unavailable. Please try again.",
    )
        .into_response()
}
fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        "Invalid or expired form. Reload the page and try again.",
    )
        .into_response()
}
fn handle(jar: &CookieJar) -> Option<String> {
    jar.get(session::COOKIE)
        .map(|c| c.value().to_owned())
        .filter(|s| session::valid_handle(s))
}
fn cookie(raw: String, seconds: i64) -> Cookie<'static> {
    Cookie::build((session::COOKIE, raw))
        .path("/")
        .secure(true)
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(seconds))
        .build()
}
async fn form_page(
    state: AppState,
    jar: CookieJar,
    query: HashMap<String, String>,
    register: bool,
) -> Response {
    // Never reinterpret an app-flow URL as portal login, even before OAuth exists.
    if !query.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "Invalid or unsupported flow. Restart authorization.",
        )
            .into_response();
    }
    let raw = if let Some(raw) = handle(&jar) {
        match session::live(&state.db, &raw).await {
            Ok(true) => Some(raw),
            Ok(false) => None,
            Err(_) => return unavailable(),
        }
    } else {
        None
    };
    let (raw, jar) = if let Some(raw) = raw {
        (raw, jar)
    } else {
        let raw = session::random();
        if session::anonymous(&state.db, &raw).await.is_err() {
            return unavailable();
        }
        let jar = jar.add(cookie(raw.clone(), 900));
        (raw, jar)
    };
    (
        jar,
        render(Page {
            title: if register { "Register" } else { "Log in" },
            register,
            csrf: session::csrf(&raw),
            ..Default::default()
        }),
    )
        .into_response()
}
async fn login_page(
    State(s): State<AppState>,
    jar: CookieJar,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    form_page(s, jar, q, false).await
}
async fn register_page(
    State(s): State<AppState>,
    jar: CookieJar,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    form_page(s, jar, q, true).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    username: String,
    password: String,
    csrf: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Logout {
    csrf: String,
}
fn guard(state: &AppState, jar: &CookieJar, headers: &HeaderMap, csrf: &str) -> Option<String> {
    let origins: Vec<_> = headers.get_all(header::ORIGIN).iter().collect();
    if origins.len() != 1 || origins[0].as_bytes() != state.origin.as_bytes() {
        return None;
    }
    let raw = handle(jar)?;
    bool::from(session::csrf(&raw).as_bytes().ct_eq(csrf.as_bytes())).then_some(raw)
}
fn invalid_credentials(raw: &str, register: bool, message: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        render(Page {
            title: if register { "Register" } else { "Log in" },
            register,
            csrf: session::csrf(raw),
            message,
            ..Default::default()
        }),
    )
        .into_response()
}
async fn register(
    State(s): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(f): Form<Credentials>,
) -> Response {
    let Some(raw) = guard(&s, &jar, &headers, &f.csrf) else {
        return forbidden();
    };
    match session::live(&s.db, &raw).await {
        Ok(true) => (),
        Ok(false) => return forbidden(),
        Err(_) => return unavailable(),
    }
    let Some(username) = password::username(&f.username) else {
        return invalid_credentials(&raw, true, "Use a valid username.");
    };
    if !password::allowed(&f.password) {
        return invalid_credentials(
            &raw,
            true,
            "Use 15–128 characters and avoid common or compromised passwords.",
        );
    }
    let Ok(hash) = s.passwords.hash(f.password).await else {
        return unavailable();
    };
    let Ok(mut tx) = s.db.begin().await else {
        return unavailable();
    };
    match session::lock(&mut tx, &raw).await {
        Ok(true) => (),
        Ok(false) => return forbidden(),
        Err(_) => return unavailable(),
    }
    let user = Uuid::new_v4();
    let result = sqlx::query("INSERT INTO accounts(id,username,password_hash) VALUES ($1,$2,$3)")
        .bind(user)
        .bind(username)
        .bind(hash)
        .execute(&mut *tx)
        .await;
    if let Err(error) = result {
        return if error
            .as_database_error()
            .is_some_and(|e| e.is_unique_violation())
        {
            invalid_credentials(&raw, true, "That username is unavailable.")
        } else {
            unavailable()
        };
    }
    let next = session::random();
    if session::rotate(&mut tx, &raw, &next, user).await.is_err() || tx.commit().await.is_err() {
        return unavailable();
    }
    (jar.add(cookie(next, 28800)), Redirect::to("/account")).into_response()
}
async fn login(
    State(s): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(f): Form<Credentials>,
) -> Response {
    let Some(raw) = guard(&s, &jar, &headers, &f.csrf) else {
        return forbidden();
    };
    match session::live(&s.db, &raw).await {
        Ok(true) => (),
        Ok(false) => return forbidden(),
        Err(_) => return unavailable(),
    }
    if f.password.chars().count() > 128 {
        return invalid_credentials(&raw, false, "Invalid username or password.");
    }
    let username = password::username(&f.username).unwrap_or_default();
    let row = match sqlx::query("SELECT id,password_hash FROM accounts WHERE username=$1")
        .bind(username)
        .fetch_optional(&s.db)
        .await
    {
        Ok(row) => row,
        Err(_) => return unavailable(),
    };
    let verified = s
        .passwords
        .verify(f.password, row.as_ref().map(|r| r.get("password_hash")))
        .await;
    match verified {
        Err(_) => return unavailable(),
        Ok(false) => return invalid_credentials(&raw, false, "Invalid username or password."),
        Ok(true) => (),
    }
    let user: Uuid = row.expect("verified account").get("id");
    let Ok(mut tx) = s.db.begin().await else {
        return unavailable();
    };
    match session::lock(&mut tx, &raw).await {
        Ok(true) => (),
        Ok(false) => return forbidden(),
        Err(_) => return unavailable(),
    }
    let next = session::random();
    if session::rotate(&mut tx, &raw, &next, user).await.is_err() || tx.commit().await.is_err() {
        return unavailable();
    }
    (jar.add(cookie(next, 28800)), Redirect::to("/account")).into_response()
}
async fn account(State(s): State<AppState>, jar: CookieJar) -> Response {
    let Some(raw) = handle(&jar) else {
        return Redirect::to("/login").into_response();
    };
    let row = sqlx::query("SELECT a.id,a.username,a.subscriber FROM accounts a JOIN browser_sessions s ON s.user_id=a.id WHERE s.verifier=$1 AND NOT s.revoked AND s.expires_at > clock_timestamp()")
        .bind(session::verifier(&raw)).fetch_optional(&s.db).await;
    match row {
        Err(_) => unavailable(),
        Ok(None) => Redirect::to("/login").into_response(),
        Ok(Some(row)) => render(Page {
            title: "Account",
            account: true,
            csrf: session::csrf(&raw),
            username: row.get("username"),
            id: row.get::<Uuid, _>("id").to_string(),
            subscriber: row.get("subscriber"),
            ..Default::default()
        }),
    }
}
async fn logout(
    State(s): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(f): Form<Logout>,
) -> Response {
    let Some(raw) = guard(&s, &jar, &headers, &f.csrf) else {
        return forbidden();
    };
    let Ok(mut tx) = s.db.begin().await else {
        return unavailable();
    };
    match session::lock(&mut tx, &raw).await {
        Ok(true) => (),
        Ok(false) => return forbidden(),
        Err(_) => return unavailable(),
    }
    if session::revoke(&mut tx, &raw).await.is_err() || tx.commit().await.is_err() {
        return unavailable();
    }
    (jar.add(cookie(String::new(), 0)), Redirect::to("/login")).into_response()
}
