//! Checks transactional encrypted mail, retries and multi-worker delivery against PostgreSQL and Mailpit.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use crow_auth::{
    mail::{DeliveryOutcome, MailConfig, MailError, MailSettings, deliver_one, queue_initial},
    verification,
};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgPoolOptions};
use tokio::io::AsyncWriteExt;
use url::Url;
use uuid::Uuid;

fn settings(port: u16) -> MailSettings {
    MailSettings {
        sender: "Crow <crow@example.test>".into(),
        origin: "http://localhost:3000".into(),
        encryption_key: URL_SAFE_NO_PAD.encode([7; 32]),
        smtp_host: "127.0.0.1".into(),
        smtp_port: port,
        smtp_username: None,
        smtp_password: None,
        local_capture: true,
    }
}
async fn pending(tx: &mut Transaction<'_, Postgres>) -> (Uuid, String) {
    let id = Uuid::new_v4();
    let email = format!("{id}@example.test");
    sqlx::query("INSERT INTO accounts(id,username,email,password_hash,created_at,pending_expires_at) SELECT $1,$2,$3,'$argon2id$v=19$fixture',now,now+interval '24 hours' FROM (SELECT clock_timestamp() now) c")
        .bind(id).bind(format!("u{}", &id.simple().to_string()[..20])).bind(&email).execute(&mut **tx).await.unwrap();
    (id, email)
}
async fn queue(tx: &mut Transaction<'_, Postgres>, config: &MailConfig) -> (Uuid, String) {
    let result = pending(tx).await;
    queue_initial(tx, result.0, config).await.unwrap();
    result
}
async fn queued(db: &PgPool, config: &MailConfig) -> (Uuid, String) {
    let mut tx = db.begin().await.unwrap();
    let result = queue(&mut tx, config).await;
    tx.commit().await.unwrap();
    result
}
async fn count(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM verification_mail_outbox")
        .fetch_one(db)
        .await
        .unwrap()
}
async fn inbox(base: &str, email: &str) -> Vec<Value> {
    let response: Value = reqwest::get(format!("{base}/api/v1/messages"))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    response["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| {
            message["To"]
                .as_array()
                .unwrap()
                .iter()
                .any(|to| to["Address"] == email)
        })
        .cloned()
        .collect()
}

#[tokio::test]
#[ignore = "requires disposable TEST_DATABASE_URL and SMTP capture; CI invokes explicitly"]
async fn encrypted_outbox_delivery_retries_and_failures() {
    let url = std::env::var("TEST_DATABASE_URL").expect("disposable database required");
    let capture = std::env::var("TEST_MAILPIT_URL").expect("local SMTP capture HTTP URL required");
    let port = std::env::var("TEST_SMTP_PORT")
        .expect("local SMTP capture port required")
        .parse()
        .unwrap();
    // The capture service is unauthenticated, so fixture access is loopback-only.
    let capture_url = Url::parse(&capture).unwrap();
    assert_eq!(capture_url.scheme(), "http");
    assert!(matches!(
        capture_url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]")
    ));
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("mail_{}", Uuid::new_v4().simple());
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
    let config = MailConfig::new(settings(port)).unwrap();

    // Nothing can be sent before the account/action/outbox transaction commits.
    let mut tx = db.begin().await.unwrap();
    let (id, email) = queue(&mut tx, &config).await;
    assert_eq!(
        deliver_one(&db, &config).await.unwrap(),
        DeliveryOutcome::Idle
    );
    tx.rollback().await.unwrap();
    assert_eq!(count(&db).await, 0);
    assert!(inbox(&capture, &email).await.is_empty());
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=$1)")
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap()
    );

    // An outbox write failure aborts the account and action writes too.
    sqlx::query("CREATE FUNCTION fail_mail_write() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture failure'; END; $$")
        .execute(&db).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_mail_write BEFORE INSERT ON verification_mail_outbox FOR EACH ROW EXECUTE FUNCTION fail_mail_write()")
        .execute(&db).await.unwrap();
    let mut tx = db.begin().await.unwrap();
    let (failed, _) = pending(&mut tx).await;
    assert_eq!(
        queue_initial(&mut tx, failed, &config).await,
        Err(MailError::Unavailable)
    );
    tx.rollback().await.unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=$1)")
            .bind(failed)
            .fetch_one(&db)
            .await
            .unwrap()
    );
    assert_eq!(count(&db).await, 0);
    sqlx::query("DROP TRIGGER fail_mail_write ON verification_mail_outbox")
        .execute(&db)
        .await
        .unwrap();

    let (id, email) = queued(&db, &config).await;
    let row = sqlx::query("SELECT nonce,ciphertext FROM verification_mail_outbox")
        .fetch_one(&db)
        .await
        .unwrap();
    let ciphertext: Vec<u8> = row.get("ciphertext");
    assert_eq!(row.get::<Vec<u8>, _>("nonce").len(), 24);
    assert!(
        !ciphertext
            .windows(email.len())
            .any(|w| w == email.as_bytes())
    );
    assert!(!ciphertext.windows(7).any(|w| w == b"http://"));
    // Wrong keys fail closed, retaining the encrypted job for operator recovery.
    let mut wrong = settings(port);
    wrong.encryption_key = URL_SAFE_NO_PAD.encode([8; 32]);
    assert_eq!(
        deliver_one(&db, &MailConfig::new(wrong).unwrap()).await,
        Err(MailError::Unavailable)
    );
    assert_eq!(count(&db).await, 1);
    assert!(inbox(&capture, &email).await.is_empty());
    let (a, b) = tokio::join!(deliver_one(&db, &config), deliver_one(&db, &config));
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| **o == DeliveryOutcome::Delivered)
            .count(),
        1
    );
    assert_eq!(count(&db).await, 0);
    let messages = inbox(&capture, &email).await;
    assert_eq!(messages.len(), 1);
    let message: Value = reqwest::get(format!(
        "{capture}/api/v1/message/{}",
        messages[0]["ID"].as_str().unwrap()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let text = message["Text"].as_str().unwrap();
    let link = Url::parse(
        text.lines()
            .find(|line| line.starts_with("http://localhost:3000/"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(link.path(), "/verify-email");
    let raw = link
        .query_pairs()
        .find(|(key, _)| key == "token")
        .unwrap()
        .1
        .into_owned();
    let mut tx = db.begin().await.unwrap();
    assert_eq!(verification::consume(&mut tx, &raw).await.unwrap(), id);
    tx.commit().await.unwrap();

    // SMTP acceptance cannot be rolled back: a failed DB delete retains the
    // same action for retry. A duplicate email never creates a second token.
    let (_, duplicate_email) = queued(&db, &config).await;
    sqlx::query("CREATE TRIGGER fail_mail_delete BEFORE DELETE ON verification_mail_outbox FOR EACH ROW EXECUTE FUNCTION fail_mail_write()")
        .execute(&db).await.unwrap();
    assert_eq!(deliver_one(&db, &config).await, Err(MailError::Unavailable));
    assert_eq!(count(&db).await, 1);
    sqlx::query("DROP TRIGGER fail_mail_delete ON verification_mail_outbox")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        deliver_one(&db, &config).await.unwrap(),
        DeliveryOutcome::Delivered
    );
    assert_eq!(inbox(&capture, &duplicate_email).await.len(), 2);

    // SMTP failure schedules only the specified bounded retries; time is advanced
    // in fixture rows, never with production clock overrides or real minute sleeps.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let failure_port = listener.local_addr().unwrap().port();
    let rejecting = tokio::spawn(async move {
        loop {
            let (mut connection, _) = listener.accept().await.unwrap();
            let _ = connection.write_all(b"421 fixture unavailable\r\n").await;
        }
    });
    let failing = MailConfig::new(settings(failure_port)).unwrap();
    let (_, email) = queued(&db, &config).await;
    for (attempt, expected_delay) in [60., 300., 900.].into_iter().enumerate() {
        assert_eq!(
            deliver_one(&db, &failing).await.unwrap(),
            DeliveryOutcome::RetryScheduled
        );
        let row=sqlx::query("SELECT attempts,EXTRACT(EPOCH FROM next_attempt_at-clock_timestamp())::float8 AS delay FROM verification_mail_outbox").fetch_one(&db).await.unwrap();
        assert_eq!(row.get::<i16, _>("attempts"), attempt as i16 + 1);
        let delay: f64 = row.get("delay");
        assert!(delay > expected_delay - 5. && delay <= expected_delay);
        assert_eq!(
            deliver_one(&db, &failing).await.unwrap(),
            DeliveryOutcome::Idle
        );
        sqlx::query("UPDATE verification_mail_outbox SET next_attempt_at=clock_timestamp()-interval '1 second'").execute(&db).await.unwrap();
    }
    assert_eq!(
        deliver_one(&db, &failing).await.unwrap(),
        DeliveryOutcome::Discarded
    );
    assert_eq!(count(&db).await, 0);
    assert!(inbox(&capture, &email).await.is_empty());
    rejecting.abort();

    // Production SMTP refuses the plaintext capture relay (no STARTTLS downgrade).
    let mut tls = settings(port);
    tls.local_capture = false;
    tls.origin = "https://crow.example.test".into();
    queued(&db, &config).await;
    assert_eq!(
        deliver_one(&db, &MailConfig::new(tls).unwrap())
            .await
            .unwrap(),
        DeliveryOutcome::RetryScheduled
    );
    sqlx::query("DELETE FROM verification_mail_outbox")
        .execute(&db)
        .await
        .unwrap();

    for invalidation in [
        "UPDATE verification_actions SET created_at=statement_timestamp()-interval '30 minutes',expires_at=statement_timestamp() WHERE account_id=$1",
        "UPDATE accounts SET created_at=statement_timestamp()-interval '24 hours',pending_expires_at=statement_timestamp() WHERE id=$1",
        "UPDATE accounts SET security_version=security_version+1 WHERE id=$1",
        "UPDATE verification_actions SET consumed_at=created_at WHERE account_id=$1",
        "UPDATE verification_actions SET email_snapshot='different@example.test' WHERE account_id=$1",
    ] {
        let (id, email) = queued(&db, &config).await;
        // Even a future retry must be erased promptly on expiry/consumption.
        sqlx::query("UPDATE verification_mail_outbox SET next_attempt_at=clock_timestamp()+interval '1 hour'")
            .execute(&db).await.unwrap();
        sqlx::query(invalidation)
            .bind(id)
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(
            deliver_one(&db, &config).await.unwrap(),
            DeliveryOutcome::Discarded
        );
        assert_eq!(count(&db).await, 0);
        assert!(inbox(&capture, &email).await.is_empty());
    }
    // Cascading account deletion cancels the encrypted delivery job.
    let (id, _) = queued(&db, &config).await;
    sqlx::query("DELETE FROM accounts WHERE id=$1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(count(&db).await, 0);

    // Exercise the actual standalone worker binary and its environment contract.
    let (_, email) = queued(&db, &config).await;
    let mut worker_url = Url::parse(&url).unwrap();
    worker_url
        .query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mail-worker"))
        .arg("--once")
        .env("DATABASE_URL", worker_url.as_str())
        .env("MAIL_FROM", "Crow <crow@example.test>")
        .env("CROW_ORIGIN", "http://localhost:3000")
        .env("MAIL_ENCRYPTION_KEY", URL_SAFE_NO_PAD.encode([7; 32]))
        .env("SMTP_HOST", "127.0.0.1")
        .env("SMTP_PORT", port.to_string())
        .env("MAIL_LOCAL_CAPTURE", "true")
        .env_remove("SMTP_USERNAME")
        .env_remove("SMTP_PASSWORD")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "worker failed safely: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&email));
    assert_eq!(count(&db).await, 0);
    assert_eq!(inbox(&capture, &email).await.len(), 1);
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
