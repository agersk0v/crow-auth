//! Provides unauthenticated browser security primitives; no registration or activation routes.
//! Signed pre-auth cookies and action-bound CSRF need no database or login session.

mod views;

pub use views::{confirmation_page, message_page, registration_page, resend_page};

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
use sha2::Sha256;
use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::limit::RequestBodyLimitLayer;
use url::Url;

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
    Unavailable,
}
impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "Browser origin or signing-key configuration invalid",
            Self::Forbidden => "Browser request rejected; restart the form",
            Self::Unavailable => "Browser service unavailable",
        })
    }
}
impl std::error::Error for BrowserError {}
impl IntoResponse for BrowserError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Forbidden => StatusCode::FORBIDDEN,
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
    signing_key: [u8; 32],
}

pub struct PreauthBrowser {
    csrf_secret: [u8; 32],
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
    /// Attach the signed browser cookie to a form GET response; it grants no account access.
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
    pub fn new(origin: &str, signing_key: &str) -> Result<Self, BrowserError> {
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
        let signing_key = decode_secret(signing_key).ok_or(BrowserError::Configuration)?;
        Ok(Self {
            origin: origin.into(),
            signing_key,
        })
    }

    /// Layers for future hosted routes. Form handlers must ALSO call authorize_post.
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

    /// Read the same key on every instance/restart; never generate it silently at startup.
    pub fn from_env() -> Result<Self, BrowserError> {
        let origin = std::env::var("CROW_ORIGIN").map_err(|_| BrowserError::Configuration)?;
        let key = std::env::var("BROWSER_SIGNING_KEY").map_err(|_| BrowserError::Configuration)?;
        Self::new(&origin, &key)
    }

    /// GET-only primitive: reuse a valid signed cookie without extending its
    /// absolute lifetime; absent/expired/forged cookies get fresh random state.
    pub fn ensure_preauth(&self, headers: &HeaderMap) -> Result<PreauthBrowser, BrowserError> {
        self.ensure_preauth_at(headers, unix_seconds()?)
    }

    pub fn authorize_post(
        &self,
        headers: &HeaderMap,
        csrf: &str,
        action: FormAction,
    ) -> Result<(), BrowserError> {
        self.authorize_post_at(headers, csrf, action, unix_seconds()?)
    }

    fn ensure_preauth_at(
        &self,
        headers: &HeaderMap,
        now: u64,
    ) -> Result<PreauthBrowser, BrowserError> {
        if let Some(raw) = cookie(headers)?
            && let Ok(payload) = self.valid_cookie(&raw, now)
        {
            return Ok(PreauthBrowser {
                csrf_secret: self.csrf_secret(payload),
                set_cookie: None,
            });
        }
        let nonce = URL_SAFE_NO_PAD.encode(random_bytes()?);
        let payload = format!("v1.{now}.{nonce}");
        let signature =
            URL_SAFE_NO_PAD.encode(self.mac(b"crow-preauth-cookie-v1\0", payload.as_bytes()));
        let value = HeaderValue::from_str(&format!(
            "{COOKIE}={payload}.{signature}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=1800"
        ))
        .map_err(|_| BrowserError::Unavailable)?;
        Ok(PreauthBrowser {
            csrf_secret: self.csrf_secret(&payload),
            set_cookie: Some(value),
        })
    }

    fn authorize_post_at(
        &self,
        headers: &HeaderMap,
        csrf: &str,
        action: FormAction,
        now: u64,
    ) -> Result<(), BrowserError> {
        self.check_origin(headers)?;
        let raw = cookie(headers)?.ok_or(BrowserError::Forbidden)?;
        let payload = self.valid_cookie(&raw, now)?;
        let browser = PreauthBrowser {
            csrf_secret: self.csrf_secret(payload),
            set_cookie: None,
        };
        let decoded = decode_secret(csrf).ok_or(BrowserError::Forbidden)?;
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&browser.csrf_secret)
            .map_err(|_| BrowserError::Unavailable)?;
        mac.update(b"crow-preauth-csrf-v1\0");
        mac.update(action.name().as_bytes());
        mac.verify_slice(&decoded)
            .map_err(|_| BrowserError::Forbidden)
    }

    fn valid_cookie<'a>(&self, raw: &'a str, now: u64) -> Result<&'a str, BrowserError> {
        if raw.len() > 128 {
            return Err(BrowserError::Forbidden);
        }
        let (payload, signature) = raw.rsplit_once('.').ok_or(BrowserError::Forbidden)?;
        let signature = decode_secret(signature).ok_or(BrowserError::Forbidden)?;
        let mut mac = self.signer(b"crow-preauth-cookie-v1\0");
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature)
            .map_err(|_| BrowserError::Forbidden)?;
        let mut parts = payload.split('.');
        if parts.next() != Some("v1") {
            return Err(BrowserError::Forbidden);
        }
        let issued = parts.next().ok_or(BrowserError::Forbidden)?;
        let timestamp: u64 = issued.parse().map_err(|_| BrowserError::Forbidden)?;
        let nonce = parts.next().ok_or(BrowserError::Forbidden)?;
        if parts.next().is_some()
            || decode_secret(nonce).is_none()
            || timestamp.to_string() != issued
            || timestamp > now
            || timestamp
                .checked_add(1800)
                .is_none_or(|expires| now >= expires)
        {
            return Err(BrowserError::Forbidden);
        }
        Ok(payload)
    }

    fn signer(&self, purpose: &[u8]) -> Hmac<Sha256> {
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(&self.signing_key).expect("fixed-size HMAC key");
        mac.update(purpose);
        mac.update(self.origin.as_bytes());
        mac.update(b"\0");
        mac
    }
    fn mac(&self, purpose: &[u8], payload: &[u8]) -> [u8; 32] {
        let mut mac = self.signer(purpose);
        mac.update(payload);
        mac.finalize().into_bytes().into()
    }
    fn csrf_secret(&self, payload: &str) -> [u8; 32] {
        self.mac(b"crow-preauth-csrf-key-v1\0", payload.as_bytes())
    }
}

impl fmt::Debug for BrowserSecurity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BrowserSecurity([REDACTED])")
    }
}
fn unix_seconds() -> Result<u64, BrowserError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| BrowserError::Unavailable)
}

fn random_bytes() -> Result<[u8; 32], BrowserError> {
    let mut bytes = [0; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| BrowserError::Unavailable)?;
    Ok(bytes)
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

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://crow.localhost:3000";
    fn security(key: u8) -> BrowserSecurity {
        BrowserSecurity::new(ORIGIN, &URL_SAFE_NO_PAD.encode([key; 32])).unwrap()
    }
    fn headers(browser: &PreauthBrowser) -> HeaderMap {
        let mut response = message_page("Form", "Ready");
        browser.apply_cookie(&mut response);
        let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        for flag in [
            "Path=/",
            "Secure",
            "HttpOnly",
            "SameSite=Lax",
            "Max-Age=1800",
        ] {
            assert!(cookie.contains(flag));
        }
        assert!(!cookie.contains("Domain="));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(cookie.split(';').next().unwrap()).unwrap(),
        );
        headers.insert(header::ORIGIN, HeaderValue::from_static(ORIGIN));
        headers
    }
    #[test]
    fn signed_cookies_bind_csrf_to_browser_action_origin_and_key() {
        let server = security(7);
        let browser = server.ensure_preauth_at(&HeaderMap::new(), 10000).unwrap();
        let headers = headers(&browser);
        for action in [
            FormAction::Register,
            FormAction::ResendVerification,
            FormAction::VerifyEmail,
        ] {
            let csrf = browser.csrf(action);
            assert_eq!(
                server.authorize_post_at(&headers, csrf.expose_secret(), action, 10000),
                Ok(())
            );
            // Another instance with the same key can validate without any DB state.
            assert_eq!(
                security(7).authorize_post_at(&headers, csrf.expose_secret(), action, 10001),
                Ok(())
            );
        }
        let register = browser.csrf(FormAction::Register);
        assert_eq!(
            server.authorize_post_at(
                &headers,
                register.expose_secret(),
                FormAction::VerifyEmail,
                10000
            ),
            Err(BrowserError::Forbidden)
        );
        let other = server.ensure_preauth_at(&HeaderMap::new(), 10000).unwrap();
        assert_eq!(
            server.authorize_post_at(
                &headers,
                other.csrf(FormAction::Register).expose_secret(),
                FormAction::Register,
                10000
            ),
            Err(BrowserError::Forbidden)
        );
        assert_eq!(
            security(8).authorize_post_at(
                &headers,
                register.expose_secret(),
                FormAction::Register,
                10000
            ),
            Err(BrowserError::Forbidden)
        );
        let other_origin =
            BrowserSecurity::new("https://other.test", &URL_SAFE_NO_PAD.encode([7; 32])).unwrap();
        let mut changed = headers.clone();
        changed.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://other.test"),
        );
        assert_eq!(
            other_origin.authorize_post_at(
                &changed,
                register.expose_secret(),
                FormAction::Register,
                10000
            ),
            Err(BrowserError::Forbidden)
        );
        assert_eq!(format!("{server:?}"), "BrowserSecurity([REDACTED])");
        assert_eq!(format!("{browser:?}"), "PreauthBrowser([REDACTED])");
        assert_eq!(format!("{register:?}"), "Secret([REDACTED])");
    }
    #[test]
    fn expiry_reuse_tampering_duplicates_and_key_rotation_fail_safely() {
        let server = security(7);
        let browser = server.ensure_preauth_at(&HeaderMap::new(), 10000).unwrap();
        let headers = headers(&browser);
        let csrf = browser.csrf(FormAction::Register);
        let reused = server.ensure_preauth_at(&headers, 10001).unwrap();
        assert!(reused.set_cookie.is_none());
        assert!(reused.csrf(FormAction::Register).expose_secret() == csrf.expose_secret());
        assert_eq!(
            server.authorize_post_at(&headers, csrf.expose_secret(), FormAction::Register, 11799),
            Ok(())
        );
        for now in [9999, 11800, 20000] {
            assert_eq!(
                server.authorize_post_at(&headers, csrf.expose_secret(), FormAction::Register, now),
                Err(BrowserError::Forbidden)
            );
        }
        assert!(
            server
                .ensure_preauth_at(&headers, 11800)
                .unwrap()
                .set_cookie
                .is_some()
        );
        assert!(
            security(8)
                .ensure_preauth_at(&headers, 10001)
                .unwrap()
                .set_cookie
                .is_some()
        );
        let raw = cookie(&headers).unwrap().unwrap();
        for forged in [
            raw.replacen("10000", "10001", 1),
            "x".repeat(200),
            "v1.10000.forged.signature".into(),
            raw.trim_end_matches('=').to_owned() + "=",
        ] {
            let mut h = headers.clone();
            h.insert(
                header::COOKIE,
                HeaderValue::from_str(&format!("{COOKIE}={forged}")).unwrap(),
            );
            assert_eq!(
                server.authorize_post_at(&h, csrf.expose_secret(), FormAction::Register, 10001),
                Err(BrowserError::Forbidden)
            );
            assert!(
                server
                    .ensure_preauth_at(&h, 10001)
                    .unwrap()
                    .set_cookie
                    .is_some()
            );
        }
        let mut duplicates = headers.clone();
        duplicates.append(header::COOKIE, headers[header::COOKIE].clone());
        assert!(matches!(
            server.ensure_preauth_at(&duplicates, 10001),
            Err(BrowserError::Forbidden)
        ));
        assert_eq!(
            server.authorize_post_at(
                &duplicates,
                csrf.expose_secret(),
                FormAction::Register,
                10001
            ),
            Err(BrowserError::Forbidden)
        );
        for invalid in ["", "!", &"x".repeat(10000)] {
            assert_eq!(
                server.authorize_post_at(&headers, invalid, FormAction::Register, 10001),
                Err(BrowserError::Forbidden)
            );
        }
        assert!(matches!(
            BrowserSecurity::new(ORIGIN, "invalid-key"),
            Err(BrowserError::Configuration)
        ));
    }
}
