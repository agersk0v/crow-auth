//! Provides unauthenticated browser security primitives; no registration or activation routes.
//! Callers own request transactions and must roll back on errors, then confirm commit before redirects/cookies.

mod contexts;
mod views;

pub use contexts::{Confirmation, PreparedConfirmation, cleanup_expired};
pub use views::{confirmation_page, message_page};

use argon2::password_hash::rand_core::{OsRng, RngCore};
use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::fmt;
use subtle::ConstantTimeEq;
use tower_http::limit::RequestBodyLimitLayer;
use url::Url;
use uuid::Uuid;

const COOKIE: &str = "__Host-crow-preauth";
const BODY_LIMIT: usize = 16 * 1024;

/// Secret values cannot accidentally appear in Debug output.
pub struct Secret(String);
impl Secret {
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

#[derive(Clone, Copy)]
pub enum FormAction {
    Register,
    ResendVerification,
    VerifyEmail,
}
impl FormAction {
    fn name(self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::ResendVerification => "resend-verification",
            Self::VerifyEmail => "verify-email",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BrowserError {
    Configuration,
    Forbidden,
    InvalidContext,
    Unavailable,
}
impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "Browser origin configuration invalid",
            Self::Forbidden => "Browser request rejected; restart the form",
            Self::InvalidContext => "Verification context invalid; request a new link",
            Self::Unavailable => "Browser service unavailable",
        })
    }
}
impl std::error::Error for BrowserError {}
impl From<sqlx::Error> for BrowserError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
impl IntoResponse for BrowserError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::InvalidContext => StatusCode::BAD_REQUEST,
            Self::Configuration | Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        let mut response = message_page("Request could not be completed", &self.to_string());
        *response.status_mut() = status;
        response
    }
}

#[derive(Clone)]
pub struct BrowserSecurity {
    origin: String,
}

pub struct PreauthBrowser {
    id: Uuid,
    csrf_secret: Vec<u8>,
    set_cookie: Option<HeaderValue>,
}
impl fmt::Debug for PreauthBrowser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PreauthBrowser([REDACTED])")
    }
}
impl PreauthBrowser {
    pub fn csrf(&self, action: FormAction) -> Secret {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.csrf_secret)
            .expect("HMAC accepts any key size");
        mac.update(b"crow-preauth-csrf-v1\0");
        mac.update(action.name().as_bytes());
        Secret(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
    }
    /// Apply only AFTER confirmed transaction commit. A pre-auth cookie never authenticates an account.
    pub fn apply_cookie(&self, response: &mut Response) {
        if let Some(value) = &self.set_cookie {
            response
                .headers_mut()
                .append(header::SET_COOKIE, value.clone());
        }
    }
}

impl BrowserSecurity {
    /// Canonical HTTPS origin only, e.g. https://crow.localhost:3000 (no trailing slash).
    pub fn new(origin: &str) -> Result<Self, BrowserError> {
        let parsed = Url::parse(origin).map_err(|_| BrowserError::Configuration)?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.origin().ascii_serialization() != origin
        {
            return Err(BrowserError::Configuration);
        }
        Ok(Self {
            origin: origin.into(),
        })
    }

    /// Layers for future hosted routes. Form handlers must ALSO call authorize_post/take_confirmation.
    pub fn protect<S: Clone + Send + Sync + 'static>(&self, router: Router<S>) -> Router<S> {
        router
            .layer(DefaultBodyLimit::max(BODY_LIMIT))
            .layer(RequestBodyLimitLayer::new(BODY_LIMIT))
            .layer(middleware::from_fn_with_state(self.clone(), origin_guard))
            .layer(middleware::from_fn(security_headers))
    }

    pub fn check_origin(&self, headers: &HeaderMap) -> Result<(), BrowserError> {
        let mut origins = headers.get_all(header::ORIGIN).iter();
        let value = origins.next().and_then(|value| value.to_str().ok());
        if origins.next().is_some() || value != Some(self.origin.as_str()) {
            return Err(BrowserError::Forbidden);
        }
        Ok(())
    }

    /// GET-only primitive. Reuse live state without extending its absolute lifetime;
    /// unrecognized/expired cookies get fresh random state, never attacker-selected identifiers.
    pub async fn ensure_preauth(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        headers: &HeaderMap,
    ) -> Result<PreauthBrowser, BrowserError> {
        if let Some(cookie) = cookie(headers)?
            && let Some(verifier) = verifier(&cookie)
            && let Some(row) = sqlx::query("SELECT id,csrf_secret,cookie_verifier FROM preauth_browsers WHERE cookie_verifier=$1 AND clock_timestamp()<expires_at")
                .bind(verifier.as_slice()).fetch_optional(&mut **tx).await? {
            let stored: Vec<u8> = row.get("cookie_verifier");
            if !matches_hash(&stored, &verifier) { return Err(BrowserError::Forbidden); }
            return Ok(PreauthBrowser { id: row.get("id"), csrf_secret: row.get("csrf_secret"), set_cookie: None });
        }
        let raw = random_secret()?;
        let csrf_secret = random_bytes()?;
        let id = Uuid::new_v4();
        sqlx::query("WITH c AS MATERIALIZED (SELECT clock_timestamp() now) INSERT INTO preauth_browsers(id,cookie_verifier,csrf_secret,created_at,expires_at) SELECT $1,$2,$3,now,now+interval '30 minutes' FROM c")
            .bind(id).bind(verifier(raw.expose_secret()).ok_or(BrowserError::Unavailable)?.as_slice())
            .bind(csrf_secret.as_slice()).execute(&mut **tx).await?;
        let set_cookie = HeaderValue::from_str(&format!(
            "{COOKIE}={}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=1800",
            raw.expose_secret()
        ))
        .map_err(|_| BrowserError::Unavailable)?;
        Ok(PreauthBrowser {
            id,
            csrf_secret: csrf_secret.to_vec(),
            set_cookie: Some(set_cookie),
        })
    }

    /// Validate before doing account writes. This never creates/refreshes pre-auth state.
    /// No row lock is taken, avoiding a pre-auth -> account lock inversion.
    pub async fn authorize_post(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        headers: &HeaderMap,
        csrf: &str,
        action: FormAction,
    ) -> Result<Uuid, BrowserError> {
        self.check_origin(headers)?;
        let raw = cookie(headers)?.ok_or(BrowserError::Forbidden)?;
        let hash = verifier(&raw).ok_or(BrowserError::Forbidden)?;
        let row = sqlx::query("SELECT id,csrf_secret,cookie_verifier FROM preauth_browsers WHERE cookie_verifier=$1 AND clock_timestamp()<expires_at")
            .bind(hash.as_slice()).fetch_optional(&mut **tx).await?.ok_or(BrowserError::Forbidden)?;
        let stored: Vec<u8> = row.get("cookie_verifier");
        if !matches_hash(&stored, &hash) {
            return Err(BrowserError::Forbidden);
        }
        let secret: Vec<u8> = row.get("csrf_secret");
        let decoded = decode_secret(csrf).ok_or(BrowserError::Forbidden)?;
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&secret)
            .map_err(|_| BrowserError::Unavailable)?;
        mac.update(b"crow-preauth-csrf-v1\0");
        mac.update(action.name().as_bytes());
        mac.verify_slice(&decoded)
            .map_err(|_| BrowserError::Forbidden)?;
        Ok(row.get("id"))
    }
}

fn random_bytes() -> Result<[u8; 32], BrowserError> {
    let mut bytes = [0; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| BrowserError::Unavailable)?;
    Ok(bytes)
}
fn random_secret() -> Result<Secret, BrowserError> {
    Ok(Secret(URL_SAFE_NO_PAD.encode(random_bytes()?)))
}
fn decode_secret(raw: &str) -> Option<[u8; 32]> {
    if raw.len() != 43 {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(raw).ok()?;
    if URL_SAFE_NO_PAD.encode(&bytes) != raw {
        return None;
    }
    bytes.try_into().ok()
}
fn verifier(raw: &str) -> Option<[u8; 32]> {
    decode_secret(raw)?;
    Some(Sha256::digest(raw.as_bytes()).into())
}
fn matches_hash(stored: &[u8], expected: &[u8]) -> bool {
    bool::from(stored.ct_eq(expected))
}
fn cookie(headers: &HeaderMap) -> Result<Option<String>, BrowserError> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        for pair in value
            .to_str()
            .map_err(|_| BrowserError::Forbidden)?
            .split(';')
        {
            if let Some((name, value)) = pair.trim().split_once('=')
                && name == COOKIE
            {
                if found.is_some() {
                    return Err(BrowserError::Forbidden);
                }
                found = Some(value.to_owned());
            }
        }
    }
    Ok(found)
}
async fn origin_guard(
    State(security): State<BrowserSecurity>,
    request: Request,
    next: Next,
) -> Response {
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) && let Err(error) = security.check_origin(request.headers())
    {
        return error.into_response();
    }
    next.run(request).await
}
async fn security_headers(request: Request, next: Next) -> Response {
    secure_response(next.run(request).await)
}
fn secure_response(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    if !headers.get_all(header::VARY).iter().any(|value| {
        value.to_str().is_ok_and(|value| {
            value
                .split(',')
                .any(|name| name.trim().eq_ignore_ascii_case("HX-Request"))
        })
    }) {
        headers.append(header::VARY, HeaderValue::from_static("HX-Request"));
    }
    response
}
