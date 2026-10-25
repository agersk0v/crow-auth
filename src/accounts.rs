use crate::{
    password::PasswordDigest,
    policy::{Email, Username},
};
use sqlx::{Postgres, Transaction};
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

/// Insert only pending accounts. Caller owns the transaction so a future mail
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
