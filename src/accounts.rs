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
/// EmailReserved is internal: HTTP handlers must give a generic response.
pub async fn create_pending(
    tx: &mut Transaction<'_, Postgres>,
    username: &Username,
    email: &Email,
    hash: &PasswordDigest,
) -> Result<Uuid, AccountError> {
    let id = Uuid::new_v4();
    let inserted: Option<Uuid> = sqlx::query_scalar("INSERT INTO accounts(id,username,email,password_hash,created_at,pending_expires_at) SELECT $1,$2,$3,$4,now,now+interval '24 hours' FROM (SELECT clock_timestamp() AS now) clock ON CONFLICT DO NOTHING RETURNING id")
        .bind(id).bind(username.as_str()).bind(email.as_str()).bind(hash.as_phc())
        .fetch_optional(&mut **tx).await.map_err(|_| AccountError::Unavailable)?;
    if let Some(id) = inserted {
        return Ok(id);
    }
    // Classify committed conflicts after the insert's uniqueness wait. Email wins
    // if both identifiers conflict, preserving the generic email response even
    // for concurrent exact duplicate submissions. No existing row is modified.
    let (email_reserved, username_taken): (bool, bool) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE email=$1),EXISTS(SELECT 1 FROM accounts WHERE username=$2)",
    ).bind(email.as_str()).bind(username.as_str()).fetch_one(&mut **tx).await
        .map_err(|_| AccountError::Unavailable)?;
    Err(if email_reserved {
        AccountError::EmailReserved
    } else if username_taken {
        AccountError::UsernameTaken
    } else {
        AccountError::Unavailable
    })
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
    delete_expired(tx, id).await
}

/// Reclaim only matching expired reservations before registration inserts. Lock
/// matching accounts in UUID order, then their actions. The caller owns rollback
/// and commit, including the replacement account/action/outbox writes.
pub async fn release_expired_reservations(
    tx: &mut Transaction<'_, Postgres>,
    username: &Username,
    email: &Email,
) -> Result<u64, AccountError> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM accounts WHERE username=$1 OR email=$2 ORDER BY id FOR UPDATE",
    )
    .bind(username.as_str())
    .bind(email.as_str())
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| AccountError::Unavailable)?;
    let mut removed = 0;
    for id in ids {
        removed += u64::from(
            delete_expired(tx, id)
                .await
                .map_err(|_| AccountError::Unavailable)?,
        );
    }
    Ok(removed)
}

async fn delete_expired(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query("SELECT id FROM verification_actions WHERE account_id=$1 ORDER BY id FOR UPDATE")
        .bind(id)
        .fetch_all(&mut **tx)
        .await?;
    let removed = sqlx::query(
        "DELETE FROM accounts WHERE id=$1 AND status='pending' AND pending_expires_at<=clock_timestamp()",
    ).bind(id).execute(&mut **tx).await?.rows_affected();
    Ok(removed == 1)
}
