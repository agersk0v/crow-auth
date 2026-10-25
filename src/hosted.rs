//! Serves registration/resend forms with signed-cookie CSRF, shared budgets and atomic mail queueing.
//! Confirmation activation and login are later increments; no authenticated session is created here.

use crate::{
    accounts::{self, AccountError},
    browser::{self, BrowserError, BrowserSecurity, FormAction},
    mail::{self, MailConfig, MailError},
    password::{PasswordError, PasswordService},
    policy::{Email, Username},
    throttle::RegistrationThrottle,
};
use axum::{
    Form, Router,
    extract::{
        ConnectInfo, Query, State,
        rejection::{FormRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use sqlx::{PgPool, Postgres, Transaction};
use std::{io, net::SocketAddr, path::PathBuf, sync::Arc};
use uuid::Uuid;

#[derive(Clone)]
pub struct HostedRegistration {
    db: PgPool,
    security: BrowserSecurity,
    passwords: PasswordService,
    mail: Arc<MailConfig>,
    throttle: RegistrationThrottle,
}
impl HostedRegistration {
    pub fn new(
        db: PgPool,
        security: BrowserSecurity,
        passwords: PasswordService,
        mail: MailConfig,
    ) -> Self {
        Self {
            throttle: RegistrationThrottle::new(db.clone()),
            db,
            security,
            passwords,
            mail: Arc::new(mail),
        }
    }
    /// Load once at startup, before any registration route is served. Never log configuration values.
    pub fn from_env(db: PgPool) -> io::Result<Self> {
        let security = BrowserSecurity::from_env().map_err(io::Error::other)?;
        let mail = MailConfig::from_env().map_err(io::Error::other)?;
        let path = std::env::var_os("PASSWORD_BLOCKLIST_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/common-passwords.txt"));
        let passwords = PasswordService::load(path).map_err(io::Error::other)?;
        Ok(Self::new(db, security, passwords, mail))
    }
    pub fn router(&self) -> Router {
        self.security.protect(
            Router::new()
                .route("/register", get(register_form))
                .route("/api/hosted/register", post(register))
                .route("/verify-email", get(instructions))
                .route("/api/hosted/resend-verification", post(resend))
                .with_state(self.clone()),
        )
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    flow: Option<String>,
    token: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RegisterForm {
    csrf: String,
    username: String,
    email: String,
    password: String,
    flow: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ResendForm {
    csrf: String,
    email: String,
    flow: Option<String>,
}

fn error(status: StatusCode, message: &str) -> Response {
    let mut response = browser::message_page("Request could not be completed", message);
    *response.status_mut() = status;
    response
}
fn malformed(status: StatusCode) -> Response {
    error(
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            status
        } else {
            StatusCode::BAD_REQUEST
        },
        "Invalid form. Restart the request.",
    )
}
fn unsupported_flow() -> Response {
    error(
        StatusCode::BAD_REQUEST,
        "This app sign-in request cannot continue. Restart from the application.",
    )
}
fn registered() -> Response {
    Redirect::to("/verify-email").into_response()
}
fn register_error(form: &RegisterForm, message: &str) -> Response {
    let mut response = browser::registration_page(&form.csrf, &form.username, &form.email, message);
    *response.status_mut() = StatusCode::BAD_REQUEST;
    response
}
fn resend_error(form: &ResendForm, message: &str) -> Response {
    let mut response = browser::resend_page(&form.csrf, &form.email, message);
    *response.status_mut() = StatusCode::BAD_REQUEST;
    response
}

async fn register_form(
    State(state): State<HostedRegistration>,
    headers: HeaderMap,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(_) => return malformed(StatusCode::BAD_REQUEST),
    };
    if query.flow.is_some() || query.token.is_some() {
        return unsupported_flow();
    }
    let browser = match state.security.ensure_preauth(&headers) {
        Ok(browser) => browser,
        Err(error) => return error.into_response(),
    };
    let mut response = browser::registration_page(
        browser.csrf(FormAction::Register).expose_secret(),
        "",
        "",
        "",
    );
    browser.apply_cookie(&mut response);
    response
}
async fn instructions(
    State(state): State<HostedRegistration>,
    headers: HeaderMap,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(_) => return malformed(StatusCode::BAD_REQUEST),
    };
    if query.flow.is_some() {
        return unsupported_flow();
    }
    if query.token.is_some() {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Email confirmation is temporarily unavailable. Please try this link again later.",
        );
    }
    let browser = match state.security.ensure_preauth(&headers) {
        Ok(browser) => browser,
        Err(error) => return error.into_response(),
    };
    let mut response = browser::resend_page(
        browser.csrf(FormAction::ResendVerification).expose_secret(),
        "",
        "",
    );
    browser.apply_cookie(&mut response);
    response
}

async fn transaction(db: &PgPool) -> Result<Transaction<'_, Postgres>, BrowserError> {
    let mut tx = db.begin().await.map_err(|_| BrowserError::Unavailable)?;
    sqlx::query("SET LOCAL lock_timeout='2s'")
        .execute(&mut *tx)
        .await
        .map_err(|_| BrowserError::Unavailable)?;
    sqlx::query("SET LOCAL statement_timeout='3s'")
        .execute(&mut *tx)
        .await
        .map_err(|_| BrowserError::Unavailable)?;
    Ok(tx)
}

async fn register(
    State(state): State<HostedRegistration>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    form: Result<Form<RegisterForm>, FormRejection>,
) -> Response {
    let mut form = match form {
        Ok(Form(form)) => form,
        Err(error) => return malformed(error.status()),
    };
    if let Err(error) = state
        .security
        .authorize_post(&headers, &form.csrf, FormAction::Register)
    {
        return error.into_response();
    }
    let email = Email::parse(&form.email);
    if let Err(error) = state.throttle.check(peer, email.as_ref().ok()).await {
        return error.into_response();
    }
    if form.flow.is_some() {
        return unsupported_flow();
    }
    let email = match email {
        Ok(email) => email,
        Err(error) => return register_error(&form, &error.to_string()),
    };
    let username = match Username::parse(&form.username) {
        Ok(username) => username,
        Err(error) => return register_error(&form, &error.to_string()),
    };
    if let Err(error) = state.passwords.validate(&form.password) {
        return register_error(&form, &error.to_string());
    }
    let hash = match state
        .passwords
        .hash(std::mem::take(&mut form.password))
        .await
    {
        Ok(hash) => hash,
        Err(PasswordError::Length | PasswordError::Compromised) => {
            return register_error(&form, "Choose a password that meets the password policy");
        }
        Err(_) => return BrowserError::Unavailable.into_response(),
    };
    let mut tx = match transaction(&state.db).await {
        Ok(tx) => tx,
        Err(error) => return error.into_response(),
    };
    if accounts::release_expired_reservations(&mut tx, &username, &email)
        .await
        .is_err()
    {
        let _ = tx.rollback().await;
        return BrowserError::Unavailable.into_response();
    }
    match accounts::create_pending(&mut tx, &username, &email, &hash).await {
        Ok(id) => {
            if mail::queue_initial(&mut tx, id, &state.mail).await.is_err() {
                let _ = tx.rollback().await;
                return BrowserError::Unavailable.into_response();
            }
            if tx.commit().await.is_err() {
                return BrowserError::Unavailable.into_response();
            }
            registered()
        }
        Err(outcome) => {
            if tx.rollback().await.is_err() {
                return BrowserError::Unavailable.into_response();
            }
            match outcome {
                AccountError::EmailReserved => registered(), // Generic no-op after confirmed rollback; no mail/account success is claimed.
                AccountError::UsernameTaken => register_error(&form, "Username unavailable"),
                AccountError::Unavailable => BrowserError::Unavailable.into_response(),
            }
        }
    }
}

async fn resend(
    State(state): State<HostedRegistration>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    form: Result<Form<ResendForm>, FormRejection>,
) -> Response {
    let form = match form {
        Ok(Form(form)) => form,
        Err(error) => return malformed(error.status()),
    };
    if let Err(error) =
        state
            .security
            .authorize_post(&headers, &form.csrf, FormAction::ResendVerification)
    {
        return error.into_response();
    }
    let email = Email::parse(&form.email);
    if let Err(error) = state.throttle.check(peer, email.as_ref().ok()).await {
        return error.into_response();
    }
    if form.flow.is_some() {
        return unsupported_flow();
    }
    let email = match email {
        Ok(email) => email,
        Err(error) => return resend_error(&form, &error.to_string()),
    };
    let mut tx = match transaction(&state.db).await {
        Ok(tx) => tx,
        Err(error) => return error.into_response(),
    };
    let id: Option<Uuid> = match sqlx::query_scalar("SELECT id FROM accounts WHERE email=$1")
        .bind(email.as_str())
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(id) => id,
        Err(_) => {
            let _ = tx.rollback().await;
            return BrowserError::Unavailable.into_response();
        }
    };
    if let Some(id) = id {
        match mail::queue_resend(&mut tx, id, &state.mail).await {
            Ok(()) => {}
            Err(MailError::InvalidAction) => {
                // Replacement may have begun before expiry. Roll it back fully;
                // the generic no-op response never promises that a message was queued.
                return if tx.rollback().await.is_ok() {
                    registered()
                } else {
                    BrowserError::Unavailable.into_response()
                };
            }
            Err(_) => {
                let _ = tx.rollback().await;
                return BrowserError::Unavailable.into_response();
            }
        }
    }
    if tx.commit().await.is_err() {
        return BrowserError::Unavailable.into_response();
    }
    registered()
}
