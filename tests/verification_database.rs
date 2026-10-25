//! Verifies activation, token bindings, expiry, races and rollback in isolated PostgreSQL schemas.

use crow_auth::verification::{VerificationError, consume, issue};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

async fn pending(db: &PgPool) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts(id,username,email,password_hash,created_at,pending_expires_at) SELECT $1,$2,$3,'$argon2id$v=19$fixture',now,now+interval '24 hours' FROM (SELECT clock_timestamp() now) c")
        .bind(id).bind(format!("u{}", &id.simple().to_string()[..20]))
        .bind(format!("{id}@example.test")).execute(db).await.unwrap();
    id
}

async fn token(db: &PgPool, id: Uuid) -> String {
    let mut tx = db.begin().await.unwrap();
    let token = issue(&mut tx, id).await.unwrap();
    tx.commit().await.unwrap();
    token.expose_secret().to_owned()
}

async fn attempt(db: &PgPool, raw: &str) -> Result<Uuid, VerificationError> {
    let mut tx = db.begin().await.unwrap();
    let result = consume(&mut tx, raw).await;
    if result.is_ok() {
        tx.commit().await.unwrap();
    } else {
        tx.rollback().await.unwrap();
    }
    result
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL; CI invokes explicitly"]
async fn verification_bindings_expiry_races_and_rollback() {
    let url = std::env::var("TEST_DATABASE_URL").expect("explicit disposable database required");
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("verification_{}", Uuid::new_v4().simple());
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

    let id = pending(&db).await;
    let raw = token(&db, id).await;
    // Stored verifier is 32 bytes, never the 43-byte raw credential.
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT token_verifier FROM verification_actions WHERE account_id=$1")
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(stored.len(), 32);
    assert_ne!(stored, raw.as_bytes());
    // Initial issuance cannot silently become a resend that leaves old links valid.
    let mut tx = db.begin().await.unwrap();
    assert_eq!(
        issue(&mut tx, id).await.unwrap_err(),
        VerificationError::Invalid
    );
    tx.rollback().await.unwrap();
    for bad in [
        "".to_owned(),
        "x".repeat(10000),
        "!".repeat(43),
        "A".repeat(43),
    ] {
        assert_eq!(attempt(&db, &bad).await, Err(VerificationError::Invalid));
    }
    // Activation and consumption roll back together, leaving the original usable.
    let mut tx = db.begin().await.unwrap();
    assert_eq!(consume(&mut tx, &raw).await.unwrap(), id);
    tx.rollback().await.unwrap();
    assert_eq!(attempt(&db, &raw).await.unwrap(), id);
    assert_eq!(attempt(&db, &raw).await, Err(VerificationError::Invalid));
    let active: bool = sqlx::query_scalar("SELECT status='active' AND verified_at IS NOT NULL AND security_version=0 FROM accounts WHERE id=$1")
        .bind(id).fetch_one(&db).await.unwrap();
    assert!(active);

    // Every authoritative binding and deadline is checked under the account lock.
    for mutation in [
        "UPDATE verification_actions SET email_snapshot='other@example.test' WHERE account_id=$1",
        "UPDATE accounts SET security_version=security_version+1 WHERE id=$1",
        "UPDATE verification_actions SET created_at=clock_timestamp()-interval '30 minutes',expires_at=clock_timestamp() WHERE account_id=$1",
        "UPDATE accounts SET created_at=clock_timestamp()-interval '24 hours',pending_expires_at=clock_timestamp() WHERE id=$1",
        "UPDATE verification_actions SET consumed_at=created_at WHERE account_id=$1",
        "UPDATE verification_actions SET token_verifier=decode(repeat('00',32),'hex') WHERE account_id=$1",
        "DELETE FROM verification_actions WHERE account_id=$1",
    ] {
        let account = pending(&db).await;
        let raw = token(&db, account).await;
        // Use one clock value for exact schema deadline constraints.
        let mutation = mutation.replace("clock_timestamp()", "statement_timestamp()");
        sqlx::query(&mutation)
            .bind(account)
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(attempt(&db, &raw).await, Err(VerificationError::Invalid));
    }
    let source = pending(&db).await;
    let target = pending(&db).await;
    let raw = token(&db, source).await;
    sqlx::query("UPDATE verification_actions SET account_id=$2 WHERE account_id=$1")
        .bind(source)
        .bind(target)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(attempt(&db, &raw).await, Err(VerificationError::Invalid));
    let expired = pending(&db).await;
    sqlx::query("UPDATE accounts SET created_at=statement_timestamp()-interval '24 hours',pending_expires_at=statement_timestamp() WHERE id=$1")
        .bind(expired).execute(&db).await.unwrap();
    for ineligible in [expired, id, Uuid::new_v4()] {
        let mut tx = db.begin().await.unwrap();
        assert_eq!(
            issue(&mut tx, ineligible).await.unwrap_err(),
            VerificationError::Invalid
        );
        tx.rollback().await.unwrap();
    }
    // The schema rejects cross-purpose credentials and malformed verifiers.
    let account = pending(&db).await;
    let raw = token(&db, account).await;
    for mutation in [
        "UPDATE verification_actions SET purpose='reset_password' WHERE account_id=$1",
        "UPDATE verification_actions SET token_verifier=decode('00','hex') WHERE account_id=$1",
    ] {
        assert!(
            sqlx::query(mutation)
                .bind(account)
                .execute(&db)
                .await
                .is_err()
        );
    }
    let (a, b) = tokio::join!(attempt(&db, &raw), attempt(&db, &raw));
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(
        if a.is_err() { a } else { b },
        Err(VerificationError::Invalid)
    );

    // Expiry committed by a lock holder must be visible to a waiting consumer.
    let account = pending(&db).await;
    let raw = token(&db, account).await;
    let mut holder = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM accounts WHERE id=$1 FOR UPDATE")
        .bind(account)
        .execute(&mut *holder)
        .await
        .unwrap();
    let other_db = db.clone();
    let mut waiter = tokio::spawn(async move { attempt(&other_db, &raw).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut waiter)
            .await
            .is_err()
    );
    sqlx::query("UPDATE accounts SET created_at=statement_timestamp()-interval '24 hours',pending_expires_at=statement_timestamp() WHERE id=$1")
        .bind(account).execute(&mut *holder).await.unwrap();
    holder.commit().await.unwrap();
    assert_eq!(waiter.await.unwrap(), Err(VerificationError::Invalid));

    // Failure after token consumption cannot leave a spent token or active account.
    let account = pending(&db).await;
    let raw = token(&db, account).await;
    sqlx::query("CREATE FUNCTION fail_activation() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture failure'; END; $$")
        .execute(&db).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_activation BEFORE UPDATE ON accounts FOR EACH ROW EXECUTE FUNCTION fail_activation()")
        .execute(&db).await.unwrap();
    assert_eq!(
        attempt(&db, &raw).await,
        Err(VerificationError::Unavailable)
    );
    sqlx::query("DROP TRIGGER fail_activation ON accounts")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(attempt(&db, &raw).await.unwrap(), account);

    // Account + action creation shares the caller's transaction and rolls back.
    let mut tx = db.begin().await.unwrap();
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts(id,username,email,password_hash,created_at,pending_expires_at) SELECT $1,'rollback_action','rollback_action@example.test','$argon2id$v=19$fixture',now,now+interval '24 hours' FROM (SELECT clock_timestamp() now) c")
        .bind(account).execute(&mut *tx).await.unwrap();
    issue(&mut tx, account).await.unwrap();
    tx.rollback().await.unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=$1)")
            .bind(account)
            .fetch_one(&db)
            .await
            .unwrap()
    );
    let mut tx = db.begin().await.unwrap();
    sqlx::query("SET LOCAL search_path TO pg_catalog")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        issue(&mut tx, id).await.unwrap_err(),
        VerificationError::Unavailable
    );
    tx.rollback().await.unwrap();

    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
