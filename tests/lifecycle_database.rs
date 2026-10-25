//! Exercises resend/consume/delivery/cleanup races and rollback with PostgreSQL and SMTP capture.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use crow_auth::{
    accounts::{AccountError, cleanup_expired_one},
    mail::{
        DeliveryOutcome, MailConfig, MailError, MailSettings, deliver_one, queue_initial,
        queue_resend,
    },
    verification::{VerificationError, consume},
};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::io::AsyncWriteExt;
use url::Url;
use uuid::Uuid;

fn config(port: u16) -> MailConfig {
    MailConfig::new(MailSettings {
        sender: "crow@example.test".into(),
        origin: "http://localhost:3000".into(),
        encryption_key: URL_SAFE_NO_PAD.encode([7; 32]),
        smtp_host: "127.0.0.1".into(),
        smtp_port: port,
        smtp_username: None,
        smtp_password: None,
        local_capture: true,
    })
    .unwrap()
}
async fn queued(db: &PgPool, mail: &MailConfig) -> (Uuid, String) {
    let id = Uuid::new_v4();
    let email = format!("{id}@example.test");
    let mut tx = db.begin().await.unwrap();
    sqlx::query("INSERT INTO accounts(id,username,email,password_hash,created_at,pending_expires_at) SELECT $1,$2,$3,'$argon2id$v=19$fixture',now,now+interval '24 hours' FROM (SELECT clock_timestamp() now) c")
        .bind(id).bind(format!("u{}",&id.simple().to_string()[..20])).bind(&email).execute(&mut *tx).await.unwrap();
    queue_initial(&mut tx, id, mail).await.unwrap();
    tx.commit().await.unwrap();
    (id, email)
}
async fn resend(db: &PgPool, id: Uuid, mail: &MailConfig) -> Result<(), MailError> {
    let mut tx = db.begin().await.unwrap();
    let result = queue_resend(&mut tx, id, mail).await;
    if result.is_ok() {
        tx.commit().await.unwrap();
    } else {
        tx.rollback().await.unwrap();
    }
    result
}
async fn verify(db: &PgPool, token: &str) -> Result<Uuid, VerificationError> {
    let mut tx = db.begin().await.unwrap();
    let result = consume(&mut tx, token).await;
    if result.is_ok() {
        tx.commit().await.unwrap();
    } else {
        tx.rollback().await.unwrap();
    }
    result
}
async fn tokens(capture: &str, email: &str) -> Vec<String> {
    let inbox: Value = reqwest::get(format!("{capture}/api/v1/messages"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut tokens = Vec::new();
    for message in inbox["messages"].as_array().unwrap() {
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
        let link = Url::parse(
            body["Text"]
                .as_str()
                .unwrap()
                .lines()
                .find(|line| line.starts_with("http://localhost:3000/"))
                .unwrap(),
        )
        .unwrap();
        tokens.push(
            link.query_pairs()
                .find(|(name, _)| name == "token")
                .unwrap()
                .1
                .into_owned(),
        );
    }
    tokens
}
async fn expire(db: &PgPool, id: Uuid) {
    sqlx::query("UPDATE accounts SET created_at=statement_timestamp()-interval '24 hours',pending_expires_at=statement_timestamp() WHERE id=$1")
        .bind(id).execute(db).await.unwrap();
}
async fn exists(db: &PgPool, id: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=$1)")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
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
async fn job(db: &PgPool) -> (Uuid, Vec<u8>, Vec<u8>) {
    sqlx::query_as("SELECT action_id,nonce,ciphertext FROM verification_mail_outbox")
        .fetch_one(db)
        .await
        .unwrap()
}
async fn no_jobs(db: &PgPool) {
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM verification_mail_outbox")
            .fetch_one(db)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and SMTP capture; CI invokes explicitly"]
async fn resend_cleanup_and_cross_operation_races() {
    let url = std::env::var("TEST_DATABASE_URL").expect("disposable database required");
    let capture = std::env::var("TEST_MAILPIT_URL").expect("local capture URL required");
    let parsed = Url::parse(&capture).unwrap();
    assert_eq!(parsed.scheme(), "http");
    assert!(matches!(
        parsed.host_str(),
        Some("127.0.0.1" | "localhost" | "[::1]")
    ));
    let port = std::env::var("TEST_SMTP_PORT").unwrap().parse().unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("lifecycle_{}", Uuid::new_v4().simple());
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
    let mail = config(port);

    // Replace a delivered token without changing password/version/reservation.
    let (id, email) = queued(&db, &mail).await;
    let before = snapshot(&db, id).await;
    assert_eq!(
        deliver_one(&db, &mail).await.unwrap(),
        DeliveryOutcome::Delivered
    );
    let old = tokens(&capture, &email).await.pop().unwrap();
    resend(&db, id, &mail).await.unwrap();
    assert!(before == snapshot(&db, id).await);
    assert_eq!(verify(&db, &old).await, Err(VerificationError::Invalid));
    assert_eq!(
        deliver_one(&db, &mail).await.unwrap(),
        DeliveryOutcome::Delivered
    );
    let new = tokens(&capture, &email)
        .await
        .into_iter()
        .find(|token| token != &old)
        .unwrap();
    assert_eq!(verify(&db, &new).await.unwrap(), id);
    assert_eq!(resend(&db, id, &mail).await, Err(MailError::InvalidAction));
    no_jobs(&db).await;

    // Replacement erases unsent ciphertext; failure rolls everything back.
    let (id, email) = queued(&db, &mail).await;
    let original = job(&db).await;
    sqlx::query("CREATE FUNCTION fail_lifecycle() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture failure'; END; $$")
        .execute(&db).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_resend BEFORE INSERT ON verification_mail_outbox FOR EACH ROW EXECUTE FUNCTION fail_lifecycle()")
        .execute(&db).await.unwrap();
    assert_eq!(resend(&db, id, &mail).await, Err(MailError::Unavailable));
    assert!(original == job(&db).await);
    sqlx::query("DROP TRIGGER fail_resend ON verification_mail_outbox")
        .execute(&db)
        .await
        .unwrap();
    resend(&db, id, &mail).await.unwrap();
    assert_ne!(original.0, job(&db).await.0);
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM verification_actions WHERE id=$1)"
        )
        .bind(original.0)
        .fetch_one(&db)
        .await
        .unwrap()
    );
    assert_eq!(
        deliver_one(&db, &mail).await.unwrap(),
        DeliveryOutcome::Delivered
    );
    assert_eq!(tokens(&capture, &email).await.len(), 1);

    // Concurrent resends serialize: only the final action/job remains deliverable.
    let (id, email) = queued(&db, &mail).await;
    let (a, b) = tokio::join!(resend(&db, id, &mail), resend(&db, id, &mail));
    a.unwrap();
    b.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM verification_actions WHERE account_id=$1"
        )
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        deliver_one(&db, &mail).await.unwrap(),
        DeliveryOutcome::Delivered
    );
    let current = tokens(&capture, &email).await;
    assert_eq!(current.len(), 1);
    verify(&db, &current[0]).await.unwrap();

    // If resend holds the account first, a waiting consumption rejects the old link.
    let (id, email) = queued(&db, &mail).await;
    deliver_one(&db, &mail).await.unwrap();
    let old = tokens(&capture, &email).await.pop().unwrap();
    let mut holder = db.begin().await.unwrap();
    queue_resend(&mut holder, id, &mail).await.unwrap();
    let other = db.clone();
    let mut waiting = tokio::spawn(async move { verify(&other, &old).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut waiting)
            .await
            .is_err()
    );
    holder.commit().await.unwrap();
    assert_eq!(waiting.await.unwrap(), Err(VerificationError::Invalid));
    deliver_one(&db, &mail).await.unwrap();

    // If consumption holds the account first, resend cannot modify the active account.
    let (id, email) = queued(&db, &mail).await;
    deliver_one(&db, &mail).await.unwrap();
    let old = tokens(&capture, &email).await.pop().unwrap();
    let mut holder = db.begin().await.unwrap();
    consume(&mut holder, &old).await.unwrap();
    let other = db.clone();
    let mut waiting = tokio::spawn(async move { resend(&other, id, &config(port)).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut waiting)
            .await
            .is_err()
    );
    holder.commit().await.unwrap();
    assert_eq!(waiting.await.unwrap(), Err(MailError::InvalidAction));
    no_jobs(&db).await;

    // A sender holds account/action locks through SMTP. Resend waits, then erases
    // that attempt's retry job and queues only the replacement action.
    let (id, email) = queued(&db, &mail).await;
    let original = job(&db).await.0;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let blocked_port = listener.local_addr().unwrap().port();
    let (accepted, connected) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let smtp = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        accepted.send(()).unwrap();
        released.await.unwrap();
        let _ = stream.write_all(b"421 fixture unavailable\r\n").await;
    });
    let other = db.clone();
    let sending = tokio::spawn(async move { deliver_one(&other, &config(blocked_port)).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), connected)
        .await
        .unwrap()
        .unwrap();
    let other = db.clone();
    let mut replacing = tokio::spawn(async move { resend(&other, id, &config(port)).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut replacing)
            .await
            .is_err()
    );
    release.send(()).unwrap();
    smtp.await.unwrap();
    assert_eq!(
        sending.await.unwrap().unwrap(),
        DeliveryOutcome::RetryScheduled
    );
    replacing.await.unwrap().unwrap();
    assert_ne!(original, job(&db).await.0);
    assert_eq!(
        sqlx::query_scalar::<_, i16>("SELECT attempts FROM verification_mail_outbox")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    deliver_one(&db, &mail).await.unwrap();
    assert_eq!(tokens(&capture, &email).await.len(), 1);

    // Resend after a lock wait rechecks the original deadline and preserves state on failure.
    let (id, _) = queued(&db, &mail).await;
    let original = job(&db).await;
    let mut holder = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM accounts WHERE id=$1 FOR UPDATE")
        .bind(id)
        .execute(&mut *holder)
        .await
        .unwrap();
    let other = db.clone();
    let mut waiting = tokio::spawn(async move { resend(&other, id, &config(port)).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut waiting)
            .await
            .is_err()
    );
    sqlx::query("UPDATE accounts SET created_at=statement_timestamp()-interval '24 hours',pending_expires_at=statement_timestamp() WHERE id=$1")
        .bind(id).execute(&mut *holder).await.unwrap();
    holder.commit().await.unwrap();
    assert_eq!(waiting.await.unwrap(), Err(MailError::InvalidAction));
    assert!(original == job(&db).await);
    assert!(cleanup_expired_one(&db).await.unwrap());
    assert!(!exists(&db, id).await);
    no_jobs(&db).await;

    // Cleanup failure cannot partially delete the account/actions/encrypted payload.
    let (id, email) = queued(&db, &mail).await;
    expire(&db, id).await;
    let original = job(&db).await;
    sqlx::query("CREATE TRIGGER fail_cleanup BEFORE DELETE ON accounts FOR EACH ROW EXECUTE FUNCTION fail_lifecycle()")
        .execute(&db).await.unwrap();
    assert_eq!(
        cleanup_expired_one(&db).await,
        Err(AccountError::Unavailable)
    );
    assert!(exists(&db, id).await);
    assert!(original == job(&db).await);
    sqlx::query("DROP TRIGGER fail_cleanup ON accounts")
        .execute(&db)
        .await
        .unwrap();
    // Busy accounts are skipped, then two cleaners compete for exactly one deletion.
    let mut holder = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM accounts WHERE id=$1 FOR UPDATE")
        .bind(id)
        .execute(&mut *holder)
        .await
        .unwrap();
    assert!(!cleanup_expired_one(&db).await.unwrap());
    holder.commit().await.unwrap();
    let (a, b) = tokio::join!(cleanup_expired_one(&db), cleanup_expired_one(&db));
    assert_ne!(a.unwrap(), b.unwrap());
    assert!(!exists(&db, id).await);
    no_jobs(&db).await;
    assert!(tokens(&capture, &email).await.is_empty());
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM verification_actions WHERE account_id=$1)"
        )
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap()
    );
    // Canonical username and email can be reused, with a new immutable UUID.
    let replacement = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts(id,username,email,password_hash,created_at,pending_expires_at) SELECT $1,$2,$3,'$argon2id$v=19$fixture',now,now+interval '24 hours' FROM (SELECT clock_timestamp() now) c")
        .bind(replacement).bind(format!("u{}",&id.simple().to_string()[..20])).bind(&email).execute(&db).await.unwrap();
    assert!(exists(&db, replacement).await);

    // Expired accounts cannot be activated or resent while cleanup races them.
    let (id, email) = queued(&db, &mail).await;
    deliver_one(&db, &mail).await.unwrap();
    let old = tokens(&capture, &email).await.pop().unwrap();
    expire(&db, id).await;
    let (cleaned, verified, resent) = tokio::join!(
        cleanup_expired_one(&db),
        verify(&db, &old),
        resend(&db, id, &mail)
    );
    // SKIP LOCKED may defer cleanup while verification/resend holds the account.
    // Once both have completed, the expired reservation must be removable.
    if !cleaned.unwrap() {
        assert!(cleanup_expired_one(&db).await.unwrap());
    }
    assert!(!exists(&db, id).await);
    assert_eq!(verified, Err(VerificationError::Invalid));
    assert_eq!(resent, Err(MailError::InvalidAction));
    // Active accounts survive even after their former pending deadline.
    expire(&db, replacement).await;
    sqlx::query("UPDATE accounts SET status='active',verified_at=clock_timestamp() WHERE id=$1")
        .bind(replacement)
        .execute(&db)
        .await
        .unwrap();
    assert!(!cleanup_expired_one(&db).await.unwrap());
    assert!(exists(&db, replacement).await);

    // Run the real cleanup process without any mail configuration.
    let (id, _) = queued(&db, &mail).await;
    expire(&db, id).await;
    let mut worker_url = Url::parse(&url).unwrap();
    worker_url
        .query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_account-cleanup"))
        .arg("--once")
        .env("DATABASE_URL", worker_url.as_str())
        .env_remove("MAIL_FROM")
        .env_remove("MAIL_ENCRYPTION_KEY")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!exists(&db, id).await);
    no_jobs(&db).await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
