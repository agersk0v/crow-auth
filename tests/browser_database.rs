//! Tests database-backed pre-auth, action-bound CSRF, scanner-safe contexts and rollback/expiry races.

use axum::{
    Form, Router,
    body::Body,
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    routing::post,
};
use crow_auth::{
    browser::{
        self, BrowserError, BrowserSecurity, Confirmation, FormAction, PreparedConfirmation, Secret,
    },
    verification,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "https://crow.localhost:3000";
async fn new_browser(db: &PgPool, security: &BrowserSecurity) -> (HeaderMap, Secret, Secret) {
    let mut tx = db.begin().await.unwrap();
    let browser = security
        .ensure_preauth(&mut tx, &HeaderMap::new())
        .await
        .unwrap();
    let verify = browser.csrf(FormAction::VerifyEmail);
    let register = browser.csrf(FormAction::Register);
    assert!(!format!("{browser:?}").contains(verify.expose_secret()));
    assert_eq!(format!("{verify:?}"), "Secret([REDACTED])");
    tx.commit().await.unwrap();
    let mut response = browser::message_page("Ready", "A form");
    browser.apply_cookie(&mut response);
    let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    for flag in [
        "__Host-crow-preauth=",
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
    (headers, verify, register)
}
async fn action(db: &PgPool) -> (Uuid, String) {
    let id = Uuid::new_v4();
    let mut tx = db.begin().await.unwrap();
    sqlx::query("INSERT INTO accounts(id,username,email,password_hash,created_at,pending_expires_at) SELECT $1,$2,$3,'$argon2id$v=19$fixture',now,now+interval '24 hours' FROM (SELECT clock_timestamp() now)c")
        .bind(id).bind(format!("u{}",&id.simple().to_string()[..20])).bind(format!("{id}@example.test")).execute(&mut *tx).await.unwrap();
    let token = verification::issue(&mut tx, id).await.unwrap();
    tx.commit().await.unwrap();
    (id, token.expose_secret().into())
}
async fn prepare(
    db: &PgPool,
    s: &BrowserSecurity,
    headers: &HeaderMap,
    raw: &str,
) -> Result<PreparedConfirmation, BrowserError> {
    let mut tx = db.begin().await.unwrap();
    let browser = s.ensure_preauth(&mut tx, headers).await.unwrap();
    let result = s.prepare_verification(&mut tx, &browser, raw).await;
    if result.is_ok() {
        tx.commit().await.unwrap();
    } else {
        tx.rollback().await.unwrap();
    }
    result
}
async fn take(
    db: &PgPool,
    s: &BrowserSecurity,
    headers: &HeaderMap,
    csrf: &str,
    context: &str,
) -> Result<Confirmation, BrowserError> {
    let mut tx = db.begin().await.unwrap();
    let result = s.take_confirmation(&mut tx, headers, csrf, context).await;
    if result.is_ok() {
        tx.commit().await.unwrap();
    } else {
        tx.rollback().await.unwrap();
    }
    result
}
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL; CI invokes explicitly"]
async fn preauth_csrf_contexts_and_expiry_races() {
    let url = std::env::var("TEST_DATABASE_URL").expect("disposable database required");
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("browser_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let path = format!("SET search_path TO {schema}");
    let db = PgPoolOptions::new()
        .max_connections(6)
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
    let security = BrowserSecurity::new(ORIGIN).unwrap();
    let (headers, csrf, register) = new_browser(&db, &security).await;
    // Reusing the browser does not rotate its token or extend its absolute deadline.
    let mut tx = db.begin().await.unwrap();
    let same = security.ensure_preauth(&mut tx, &headers).await.unwrap();
    assert!(same.csrf(FormAction::VerifyEmail).expose_secret() == csrf.expose_secret());
    let mut response = browser::message_page("Ready", "same");
    same.apply_cookie(&mut response);
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    assert!(
        security
            .authorize_post(
                &mut tx,
                &headers,
                csrf.expose_secret(),
                FormAction::VerifyEmail
            )
            .await
            .is_ok()
    );
    assert_eq!(
        security
            .authorize_post(
                &mut tx,
                &headers,
                register.expose_secret(),
                FormAction::VerifyEmail
            )
            .await,
        Err(BrowserError::Forbidden)
    );
    tx.rollback().await.unwrap();
    // Exercise CSRF validation through the ordinary/HTMX Axum form path.
    #[derive(serde::Deserialize)]
    struct CsrfForm {
        csrf: String,
    }
    let route_db = db.clone();
    let route_security = security.clone();
    let router = security.protect(Router::new().route(
        "/form",
        post(move |headers: HeaderMap, Form(form): Form<CsrfForm>| {
            let db = route_db.clone();
            let security = route_security.clone();
            async move {
                let mut tx = db.begin().await.map_err(|_| BrowserError::Unavailable)?;
                security
                    .authorize_post(&mut tx, &headers, &form.csrf, FormAction::VerifyEmail)
                    .await?;
                tx.rollback().await.map_err(|_| BrowserError::Unavailable)?;
                Ok::<_, BrowserError>(browser::message_page("Authorized", "No account mutation"))
            }
        }),
    ));
    for htmx in [false, true] {
        for valid in [false, true] {
            let mut req = Request::builder()
                .method("POST")
                .uri("/form")
                .header("origin", ORIGIN)
                .header("cookie", headers[header::COOKIE].clone())
                .header("content-type", "application/x-www-form-urlencoded");
            if htmx {
                req = req.header("hx-request", "true");
            }
            let token = if valid {
                csrf.expose_secret()
            } else {
                register.expose_secret()
            };
            let response = router
                .clone()
                .oneshot(req.body(Body::from(format!("csrf={token}"))).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if valid {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                }
            );
        }
    }
    let mut duplicates = headers.clone();
    duplicates.append(header::COOKIE, headers[header::COOKIE].clone());
    let mut tx = db.begin().await.unwrap();
    assert_eq!(
        security
            .authorize_post(
                &mut tx,
                &duplicates,
                csrf.expose_secret(),
                FormAction::VerifyEmail
            )
            .await,
        Err(BrowserError::Forbidden)
    );
    tx.rollback().await.unwrap();
    // An attacker-chosen but well-formed cookie is replaced, not adopted.
    let guessed =
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, [42; 32]);
    let mut supplied = HeaderMap::new();
    supplied.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("__Host-crow-preauth={guessed}")).unwrap(),
    );
    let mut tx = db.begin().await.unwrap();
    let generated = security.ensure_preauth(&mut tx, &supplied).await.unwrap();
    tx.commit().await.unwrap();
    let mut response = browser::message_page("Ready", "Fresh browser");
    generated.apply_cookie(&mut response);
    assert!(
        !response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains(&guessed)
    );
    let (other_headers, other_csrf, _) = new_browser(&db, &security).await;
    let (id, raw) = action(&db).await;
    let context = prepare(&db, &security, &headers, &raw).await.unwrap();
    // A clean URL carries only a non-secret selector. A later GET can rebuild
    // the form for the same browser, without storing another raw email token.
    let clean = context.clean_path();
    assert!(!clean.contains(&raw));
    assert!(!clean.contains(context.expose_secret()));
    let mut tx = db.begin().await.unwrap();
    let owner = security.ensure_preauth(&mut tx, &headers).await.unwrap();
    let loaded = security
        .load_confirmation(&mut tx, &owner, context.selector())
        .await
        .unwrap();
    assert!(loaded.expose_secret() == context.expose_secret());
    let stranger = security
        .ensure_preauth(&mut tx, &other_headers)
        .await
        .unwrap();
    assert!(matches!(
        security
            .load_confirmation(&mut tx, &stranger, context.selector())
            .await,
        Err(BrowserError::InvalidContext)
    ));
    let response = browser::confirmation_page(&csrf, &loaded);
    assert_eq!(response.status(), StatusCode::OK);
    let html = axum::body::to_bytes(response.into_body(), 20000)
        .await
        .unwrap();
    let html = String::from_utf8(html.to_vec()).unwrap();
    assert!(!html.contains(&raw));
    assert!(html.contains("method=\"post\""));
    tx.commit().await.unwrap();
    // Repeated/scanner GETs may create forms but never spend the emailed token.
    let scanner = prepare(&db, &security, &other_headers, &raw).await.unwrap();
    let repeat = prepare(&db, &security, &headers, &raw).await.unwrap();
    let bounds:bool=sqlx::query_scalar("SELECT bool_and(c.expires_at<=c.created_at+interval '5 minutes' AND c.expires_at<=v.expires_at AND c.expires_at<=b.expires_at AND c.expires_at<=a.pending_expires_at) FROM verification_contexts c JOIN verification_actions v ON v.id=c.action_id JOIN accounts a ON a.id=v.account_id JOIN preauth_browsers b ON b.id=c.browser_id").fetch_one(&db).await.unwrap();
    assert!(bounds);
    let untouched:bool=sqlx::query_scalar("SELECT a.status='pending' AND v.consumed_at IS NULL FROM accounts a JOIN verification_actions v ON v.account_id=a.id WHERE a.id=$1").bind(id).fetch_one(&db).await.unwrap();
    assert!(untouched);
    assert!(matches!(
        take(
            &db,
            &security,
            &other_headers,
            other_csrf.expose_secret(),
            context.expose_secret()
        )
        .await,
        Err(BrowserError::InvalidContext)
    ));
    assert!(matches!(
        take(
            &db,
            &security,
            &headers,
            register.expose_secret(),
            context.expose_secret()
        )
        .await,
        Err(BrowserError::Forbidden)
    ));
    // A downstream failure rolls back context spending, not merely account writes.
    let mut tx = db.begin().await.unwrap();
    security
        .take_confirmation(
            &mut tx,
            &headers,
            csrf.expose_secret(),
            context.expose_secret(),
        )
        .await
        .unwrap();
    assert!(sqlx::query("SELECT 1/0").execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    let (a, b) = tokio::join!(
        take(
            &db,
            &security,
            &headers,
            csrf.expose_secret(),
            context.expose_secret()
        ),
        take(
            &db,
            &security,
            &headers,
            csrf.expose_secret(),
            context.expose_secret()
        )
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let winner = if let Ok(winner) = a {
        winner
    } else {
        b.unwrap()
    };
    assert_eq!(winner.account_id(), id);
    assert_eq!(
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM verification_actions WHERE account_id=$1")
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap(),
        winner.action_id()
    );
    // Spending a form context is not activation. The later confirmation handler
    // must consume/activate atomically in its transaction; that route is not shipped here.
    let mut tx = db.begin().await.unwrap();
    assert_eq!(verification::consume(&mut tx, &raw).await.unwrap(), id);
    tx.commit().await.unwrap();
    assert!(matches!(
        take(
            &db,
            &security,
            &headers,
            csrf.expose_secret(),
            repeat.expose_secret()
        )
        .await,
        Err(BrowserError::InvalidContext)
    ));
    assert!(matches!(
        take(
            &db,
            &security,
            &other_headers,
            other_csrf.expose_secret(),
            scanner.expose_secret()
        )
        .await,
        Err(BrowserError::InvalidContext)
    ));
    // Bindings are rechecked instead of trusting a previously generated context.
    for mutation in [
        "UPDATE verification_actions SET created_at=statement_timestamp()-interval '30 minutes',expires_at=statement_timestamp() WHERE account_id=$1",
        "UPDATE accounts SET security_version=security_version+1 WHERE id=$1",
        "UPDATE verification_actions SET email_snapshot='other@example.test' WHERE account_id=$1",
        "UPDATE accounts SET created_at=statement_timestamp()-interval '24 hours',pending_expires_at=statement_timestamp() WHERE id=$1",
    ] {
        let (id, raw) = action(&db).await;
        let ctx = prepare(&db, &security, &headers, &raw).await.unwrap();
        sqlx::query(mutation).bind(id).execute(&db).await.unwrap();
        assert!(matches!(
            take(
                &db,
                &security,
                &headers,
                csrf.expose_secret(),
                ctx.expose_secret()
            )
            .await,
            Err(BrowserError::InvalidContext)
        ));
        assert!(matches!(
            prepare(&db, &security, &headers, &raw).await,
            Err(BrowserError::InvalidContext)
        ));
    }
    // Replacing a link cascades away all its confirmation contexts.
    let (id, raw) = action(&db).await;
    let ctx = prepare(&db, &security, &headers, &raw).await.unwrap();
    let mail = crow_auth::mail::MailConfig::new(crow_auth::mail::MailSettings {
        sender: "crow@example.test".into(),
        origin: ORIGIN.into(),
        encryption_key: base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            [7; 32],
        ),
        smtp_host: "127.0.0.1".into(),
        smtp_port: 1025,
        smtp_username: None,
        smtp_password: None,
        local_capture: true,
    })
    .unwrap();
    let mut tx = db.begin().await.unwrap();
    crow_auth::mail::queue_resend(&mut tx, id, &mail)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        take(
            &db,
            &security,
            &headers,
            csrf.expose_secret(),
            ctx.expose_secret()
        )
        .await,
        Err(BrowserError::InvalidContext)
    ));
    // Expiry while waiting for the account lock must be observed after the wait.
    let (id, raw) = action(&db).await;
    let ctx = prepare(&db, &security, &headers, &raw).await.unwrap();
    let mut holder = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM accounts WHERE id=$1 FOR UPDATE")
        .bind(id)
        .execute(&mut *holder)
        .await
        .unwrap();
    let other_db = db.clone();
    let other_security = security.clone();
    let h = headers.clone();
    let c = csrf.expose_secret().to_owned();
    let handle = ctx.expose_secret().to_owned();
    let mut waiting =
        tokio::spawn(async move { take(&other_db, &other_security, &h, &c, &handle).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut waiting)
            .await
            .is_err()
    );
    sqlx::query("UPDATE verification_contexts SET created_at=statement_timestamp()-interval '6 minutes',expires_at=statement_timestamp()-interval '1 minute' WHERE action_id IN(SELECT id FROM verification_actions WHERE account_id=$1)").bind(id).execute(&mut *holder).await.unwrap();
    holder.commit().await.unwrap();
    assert!(matches!(
        waiting.await.unwrap(),
        Err(BrowserError::InvalidContext)
    ));
    // Expired contexts can be erased while their browser and action remain live.
    assert!(browser::cleanup_expired(&db).await.unwrap() >= 1);
    let untouched: bool = sqlx::query_scalar("SELECT a.status='pending' AND v.consumed_at IS NULL FROM accounts a JOIN verification_actions v ON v.account_id=a.id WHERE a.id=$1")
        .bind(id).fetch_one(&db).await.unwrap();
    assert!(untouched);
    // Pre-auth expiry invalidates CSRF immediately, independently of cleanup.
    sqlx::query("UPDATE preauth_browsers SET created_at=statement_timestamp()-interval '31 minutes',expires_at=statement_timestamp()-interval '1 minute'").execute(&db).await.unwrap();
    let mut tx = db.begin().await.unwrap();
    assert_eq!(
        security
            .authorize_post(
                &mut tx,
                &headers,
                csrf.expose_secret(),
                FormAction::VerifyEmail
            )
            .await,
        Err(BrowserError::Forbidden)
    );
    let fresh = security.ensure_preauth(&mut tx, &headers).await.unwrap();
    assert!(fresh.csrf(FormAction::VerifyEmail).expose_secret() != csrf.expose_secret());
    tx.commit().await.unwrap();
    browser::cleanup_expired(&db).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM verification_contexts")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    // The cleanup batch is bounded and parallel invocations can drain remaining rows.
    sqlx::query("WITH clock AS MATERIALIZED(SELECT statement_timestamp() now) INSERT INTO preauth_browsers(id,cookie_verifier,csrf_secret,created_at,expires_at) SELECT gen_random_uuid(),decode(lpad(to_hex(n),64,'0'),'hex'),decode(repeat('00',32),'hex'),now-interval '31 minutes',now-interval '1 minute' FROM generate_series(1,101)n,clock").execute(&db).await.unwrap();
    assert_eq!(browser::cleanup_expired(&db).await.unwrap(), 100);
    let (a, b) = tokio::join!(browser::cleanup_expired(&db), browser::cleanup_expired(&db));
    assert_eq!(a.unwrap() + b.unwrap(), 1);
    // Storage failure diagnostics are redacted.
    let mut tx = db.begin().await.unwrap();
    sqlx::query("SET LOCAL search_path TO pg_catalog")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(matches!(
        security.ensure_preauth(&mut tx, &HeaderMap::new()).await,
        Err(BrowserError::Unavailable)
    ));
    tx.rollback().await.unwrap();
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
