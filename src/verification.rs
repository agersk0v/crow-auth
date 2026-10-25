//! Issues and consumes email-verification actions in caller-owned transactions.
//! Callers must roll back on every error and confirm commit before reporting success.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use std::fmt;
use subtle::ConstantTimeEq;
use uuid::Uuid;

/// A raw credential for the future encrypted outbox. Debug never prints it.
pub struct VerificationToken(String);
impl VerificationToken {
    fn generate() -> Result<Self, VerificationError> {
        let mut bytes = [0; 32];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| VerificationError::Unavailable)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    fn verifier(&self) -> [u8; 32] {
        Sha256::digest(self.0.as_bytes()).into()
    }
}
impl fmt::Debug for VerificationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerificationToken([REDACTED])")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum VerificationError {
    Invalid,
    Unavailable,
}
impl fmt::Display for VerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "Verification action invalid",
            Self::Unavailable => "Verification storage unavailable",
        })
    }
}
impl std::error::Error for VerificationError {}
impl From<sqlx::Error> for VerificationError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}

/// Initial issuance only; resend and encrypted outbox integration are follow-up work.
/// A pending account is locked before action access. The raw token must only be
/// handed to encrypted delivery storage, never logs or a registration response.
pub async fn issue(
    tx: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
) -> Result<VerificationToken, VerificationError> {
    lock_pending(tx, account_id).await?;
    // Prevent accidentally using initial issuance as an incomplete resend API.
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_actions WHERE account_id=$1)")
            .bind(account_id)
            .fetch_one(&mut **tx)
            .await?;
    if exists {
        return Err(VerificationError::Invalid);
    }
    let token = VerificationToken::generate()?;
    let affected = sqlx::query(
        "WITH clock AS MATERIALIZED (SELECT clock_timestamp() AS now) \
         INSERT INTO verification_actions(id,account_id,purpose,email_snapshot,security_version,token_verifier,created_at,expires_at) \
         SELECT $1,id,'verify_email',email,security_version,$3,now,now+interval '30 minutes' \
         FROM accounts CROSS JOIN clock WHERE id=$2 AND status='pending' AND now<pending_expires_at",
    )
    .bind(Uuid::new_v4()).bind(account_id).bind(token.verifier().as_slice())
    .execute(&mut **tx).await?.rows_affected();
    if affected != 1 {
        return Err(VerificationError::Invalid);
    }
    Ok(token)
}

async fn lock_pending(
    tx: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
) -> Result<(), VerificationError> {
    // Acquire the lock first; evaluate deadlines in subsequent statements so a
    // lock wait cannot leave us trusting a pre-wait clock or stale account state.
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM accounts WHERE id=$1 FOR UPDATE")
            .bind(account_id)
            .fetch_optional(&mut **tx)
            .await?;
    if status.as_deref() != Some("pending") {
        return Err(VerificationError::Invalid);
    }
    Ok(())
}

/// Consume and activate atomically, without a session or security-version bump.
/// This is an internal primitive, not an HTTP endpoint. Future handlers must
/// require a confirmation POST with pre-auth CSRF/Origin and action-context checks.
pub async fn consume(
    tx: &mut Transaction<'_, Postgres>,
    raw_token: &str,
) -> Result<Uuid, VerificationError> {
    // Reject malformed input before storage work and bound credential hashing.
    if raw_token.len() != 43
        || URL_SAFE_NO_PAD.decode(raw_token).map_or(true, |bytes| {
            bytes.len() != 32 || URL_SAFE_NO_PAD.encode(bytes) != raw_token
        })
    {
        return Err(VerificationError::Invalid);
    }
    let verifier: [u8; 32] = Sha256::digest(raw_token.as_bytes()).into();
    // Metadata lookup takes no row lock; authoritative state is reread below.
    let account_id: Option<Uuid> =
        sqlx::query_scalar("SELECT account_id FROM verification_actions WHERE token_verifier=$1")
            .bind(verifier.as_slice())
            .fetch_optional(&mut **tx)
            .await?;
    let account_id = account_id.ok_or(VerificationError::Invalid)?;
    lock_pending(tx, account_id).await?;
    let action = sqlx::query(
        "SELECT id,token_verifier FROM verification_actions \
         WHERE account_id=$1 AND token_verifier=$2 FOR UPDATE",
    )
    .bind(account_id)
    .bind(verifier.as_slice())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(VerificationError::Invalid)?;
    let stored: Vec<u8> = action.get("token_verifier");
    if !bool::from(stored.as_slice().ct_eq(verifier.as_slice())) {
        return Err(VerificationError::Invalid);
    }
    let changed = sqlx::query(
        "WITH clock AS MATERIALIZED (SELECT clock_timestamp() AS now) \
         UPDATE verification_actions v SET consumed_at=clock.now FROM accounts a,clock \
         WHERE v.id=$1 AND a.id=v.account_id AND v.purpose='verify_email' \
         AND v.email_snapshot=a.email AND v.security_version=a.security_version \
         AND v.consumed_at IS NULL AND clock.now>=v.created_at AND clock.now<v.expires_at \
         AND a.status='pending' AND clock.now<a.pending_expires_at",
    )
    .bind(action.get::<Uuid, _>("id"))
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(VerificationError::Invalid);
    }
    sqlx::query("UPDATE accounts SET status='active',verified_at=clock_timestamp() WHERE id=$1")
        .bind(account_id)
        .execute(&mut **tx)
        .await?;
    Ok(account_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_canonical_and_redacted() {
        let a = VerificationToken::generate().unwrap();
        let b = VerificationToken::generate().unwrap();
        assert_eq!(a.expose_secret().len(), 43);
        assert_eq!(URL_SAFE_NO_PAD.decode(a.expose_secret()).unwrap().len(), 32);
        assert_ne!(a.expose_secret(), b.expose_secret());
        assert_ne!(a.verifier(), b.verifier());
        assert_eq!(format!("{a:?}"), "VerificationToken([REDACTED])");
        assert_eq!(
            VerificationError::Unavailable.to_string(),
            "Verification storage unavailable"
        );
    }
}
