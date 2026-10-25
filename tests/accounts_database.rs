//! Exercises account migrations, constraints, concurrent conflicts and rollback
//! against an explicitly configured disposable PostgreSQL database.

use crow_auth::{
    accounts::{AccountError, create_pending},
    password::PasswordService,
    policy::{Email, Username},
};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;

// Explicit --ignored locally; CI always invokes this suite with a disposable DB.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL; CI invokes explicitly"]
async fn migrations_constraints_races_rollback_and_failures() {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must select a disposable database");
    let admin = PgPool::connect(&url)
        .await
        .expect("connect to disposable database");
    let schema = format!("test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let path = format!("SET search_path TO {schema}");
    let db = PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |connection, _| {
            let path = path.clone();
            Box::pin(async move {
                sqlx::query(&path).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    crow_auth::migrate(&db).await.unwrap();
    crow_auth::migrate(&db).await.unwrap();
    let passwords = PasswordService::load("tests/fixtures/compromised-passwords.txt").unwrap();
    let hash = passwords
        .hash(" an original account phrase 🦀 ".into())
        .await
        .unwrap();
    let username = Username::parse(" OWNER_ONE ").unwrap();
    let email = Email::parse(" OWNER@EXAMPLE.TEST ").unwrap();
    let mut tx = db.begin().await.unwrap();
    let id = create_pending(&mut tx, &username, &email, &hash)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let row = sqlx::query("SELECT username,email,password_hash,status,security_version,verified_at IS NULL AS unverified,pending_expires_at=created_at+interval '24 hours' AS deadline FROM accounts WHERE id=$1")
        .bind(id).fetch_one(&db).await.unwrap();
    assert_eq!(row.get::<String, _>("username"), "owner_one");
    assert_eq!(row.get::<String, _>("email"), "owner@example.test");
    assert_eq!(row.get::<String, _>("status"), "pending");
    assert_eq!(row.get::<i64, _>("security_version"), 0);
    assert!(row.get::<bool, _>("unverified") && row.get::<bool, _>("deadline"));
    assert_eq!(row.get::<String, _>("password_hash"), hash.as_phc());
    // Conflicting canonical identifiers cannot replace existing account state.
    for (name, mail, expected) in [
        (
            "owner_one",
            "other@example.test",
            AccountError::UsernameTaken,
        ),
        (
            "other_owner",
            "owner@example.test",
            AccountError::EmailReserved,
        ),
    ] {
        let mut tx = db.begin().await.unwrap();
        assert_eq!(
            create_pending(
                &mut tx,
                &Username::parse(name).unwrap(),
                &Email::parse(mail).unwrap(),
                &hash
            )
            .await,
            Err(expected)
        );
        tx.rollback().await.unwrap();
    }
    // Valid dot-atom punctuation and the documented 254-byte boundary agree
    // between the Rust policy and database constraints.
    for mail in [
        "a!#$%&'*+-/=?^_`{|}~@example.test".to_owned(),
        format!(
            "{}@{}.{}.com",
            "a".repeat(124),
            "b".repeat(63),
            "c".repeat(61)
        ),
    ] {
        let mut tx = db.begin().await.unwrap();
        create_pending(
            &mut tx,
            &Username::parse("policy_parity").unwrap(),
            &Email::parse(&mail).unwrap(),
            &hash,
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();
    }
    // Database checks reject bypassing the input types with noncanonical values.
    for (name, mail) in [
        ("UPPER_CASE", "valid@example.test"),
        ("valid_name", "UPPER@example.test"),
        ("valid_name", "a..b@example.test"),
        ("valid_name", "a@-bad.example"),
        ("valid_name", "ü@example.test"),
    ] {
        let result=sqlx::query("INSERT INTO accounts(id,username,email,password_hash,created_at,pending_expires_at) SELECT $1,$2,$3,$4,now,now+interval '24 hours' FROM (SELECT clock_timestamp() AS now) clock")
            .bind(Uuid::new_v4()).bind(name).bind(mail).bind(hash.as_phc()).execute(&db).await;
        assert!(
            result
                .unwrap_err()
                .as_database_error()
                .unwrap()
                .is_check_violation()
        );
    }
    // Pending status cannot claim verification; UUID/identity cannot change;
    // versions are nonnegative and monotonically increasing.
    for query in [
        "UPDATE accounts SET status='active'",
        "UPDATE accounts SET security_version=-1",
        "UPDATE accounts SET id=gen_random_uuid()",
        "UPDATE accounts SET email='new@example.test'",
    ] {
        assert!(
            sqlx::query(query)
                .execute(&db)
                .await
                .unwrap_err()
                .as_database_error()
                .unwrap()
                .is_check_violation()
        );
    }
    sqlx::query("UPDATE accounts SET security_version=1")
        .execute(&db)
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE accounts SET security_version=0")
            .execute(&db)
            .await
            .is_err()
    );
    // Independent transactions race on normalized username, then on email.
    for email_race in [false, true] {
        let name_a = Username::parse(if email_race { "email_racer_a" } else { "racer" }).unwrap();
        let name_b = Username::parse(if email_race {
            "email_racer_b"
        } else {
            " RACER "
        })
        .unwrap();
        let mail_a = Email::parse(if email_race {
            " SAME@EXAMPLE.TEST "
        } else {
            "racer_a@example.test"
        })
        .unwrap();
        let mail_b = Email::parse(if email_race {
            "same@example.test"
        } else {
            "racer_b@example.test"
        })
        .unwrap();
        let (left, right) = tokio::join!(
            async {
                let mut tx = db.begin().await.unwrap();
                let result = create_pending(&mut tx, &name_a, &mail_a, &hash).await;
                if result.is_ok() {
                    tx.commit().await.unwrap();
                } else {
                    tx.rollback().await.unwrap();
                }
                result
            },
            async {
                let mut tx = db.begin().await.unwrap();
                let result = create_pending(&mut tx, &name_b, &mail_b, &hash).await;
                if result.is_ok() {
                    tx.commit().await.unwrap();
                } else {
                    tx.rollback().await.unwrap();
                }
                result
            }
        );
        assert_ne!(left.is_ok(), right.is_ok());
        let loser = if left.is_err() {
            left.unwrap_err()
        } else {
            right.unwrap_err()
        };
        assert_eq!(
            loser,
            if email_race {
                AccountError::EmailReserved
            } else {
                AccountError::UsernameTaken
            }
        );
    }
    // A later operation failing rolls back the account insertion, not just itself.
    let rolled_back;
    {
        let mut tx = db.begin().await.unwrap();
        rolled_back = create_pending(
            &mut tx,
            &Username::parse("rollback_account").unwrap(),
            &Email::parse("rollback@example.test").unwrap(),
            &hash,
        )
        .await
        .unwrap();
        assert!(sqlx::query("SELECT 1/0").execute(&mut *tx).await.is_err());
        tx.rollback().await.unwrap();
    }
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=$1)")
            .bind(rolled_back)
            .fetch_one(&db)
            .await
            .unwrap()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 3);
    // Failure after beginning a transaction maps to a safe domain error, never
    // a database diagnostic with email, hash, password or connection credentials.
    let mut tx = db.begin().await.unwrap();
    sqlx::query("SET LOCAL search_path TO pg_catalog")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = create_pending(&mut tx, &username, &email, &hash)
        .await
        .unwrap_err();
    assert_eq!(error, AccountError::Unavailable);
    assert_eq!(error.to_string(), "Account storage unavailable");
    assert_eq!(format!("{error:?}"), "Unavailable");
    tx.rollback().await.unwrap();
    db.close().await;
    assert!(db.begin().await.is_err());
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
