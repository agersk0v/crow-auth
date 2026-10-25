//! Persists pending accounts, cleans expired reservations and reports safe storage errors.
//! Callers own transactions so account creation can commit atomically with mail.

use crate::{
    password::PasswordDigest,
    policy::{Email, Username},
};
use sqlx::{PgPool, Postgres, Transaction};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, PartialEq, Eq)]
pub enum AccountError {
    UsernameTaken,
    EmailReserved,
    Unavailable,
}
impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UsernameTaken => "Username unavailable",
            Self::EmailReserved => "Email reservation exists",
            Self::Unavailable => "Account storage unavailable",
        })
    }
}
impl std::error::Error for AccountError {}

/// Insert only pending accounts. Caller owns the transaction so the encrypted mail
/// outbox insert can commit atomically. Return no browser success before commit.
/// EmailReserved is internal: a future HTTP handler must give a generic response.
pub async fn create_pending(
    tx: &mut Transaction<'_, Postgres>,
    username: &Username,
    email: &Email,
    hash: &PasswordDigest,
) -> Result<Uuid, AccountError> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts(id,username,email,password_hash,created_at,pending_expires_at) SELECT $1,$2,$3,$4,now,now+interval '24 hours' FROM (SELECT clock_timestamp() AS now) clock")
        .bind(id).bind(username.as_str()).bind(email.as_str()).bind(hash.as_phc()).execute(&mut **tx).await.map_err(|error| {
            match error.as_database_error().and_then(|e| e.constraint()) {
                Some("accounts_username_key") => AccountError::UsernameTaken,
                Some("accounts_email_key") => AccountError::EmailReserved,
                _ => AccountError::Unavailable,
            }
        })?;
    Ok(id)
}

/// Remove at most one expired pending reservation in an owned transaction.
/// Skip busy accounts; active accounts are never eligible. Cascading deletion
/// removes verification actions and encrypted mail in account-first lock order.
/// Report removal only after confirmed commit; uncertainty returns Unavailable.
pub async fn cleanup_expired_one(db: &PgPool) -> Result<bool, AccountError> {
    let mut tx = db.begin().await.map_err(|_| AccountError::Unavailable)?;
    match cleanup_in_transaction(&mut tx).await {
        Ok(removed) => {
            tx.commit().await.map_err(|_| AccountError::Unavailable)?;
            Ok(removed)
        }
        Err(_) => {
            let _ = tx.rollback().await;
            Err(AccountError::Unavailable)
        }
    }
}

async fn cleanup_in_transaction(tx: &mut Transaction<'_, Postgres>) -> Result<bool, sqlx::Error> {
    let account: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM accounts WHERE status='pending' AND pending_expires_at<=clock_timestamp() \
         ORDER BY pending_expires_at,id LIMIT 1 FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut **tx)
    .await?;
    let Some(id) = account else {
        return Ok(false);
    };
    sqlx::query("SELECT id FROM verification_actions WHERE account_id=$1 ORDER BY id FOR UPDATE")
        .bind(id)
        .fetch_all(&mut **tx)
        .await?;
    // Recheck authoritative status/deadline after locks; never trust a stale scan.
    let removed = sqlx::query(
        "DELETE FROM accounts WHERE id=$1 AND status='pending' AND pending_expires_at<=clock_timestamp()",
    ).bind(id).execute(&mut **tx).await?.rows_affected();
    Ok(removed == 1)
}
