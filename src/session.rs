use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub const COOKIE: &str = "__Host-crow";

pub fn random() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn verifier(raw: &str) -> Vec<u8> {
    Sha256::digest(raw.as_bytes()).to_vec()
}
pub fn csrf(raw: &str) -> String {
    Sha256::digest(format!("crow-csrf-v1:{raw}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn valid_handle(raw: &str) -> bool {
    raw.len() == 64
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub async fn live(db: &PgPool, raw: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM browser_sessions WHERE verifier=$1 AND NOT revoked AND expires_at > clock_timestamp())")
        .bind(verifier(raw)).fetch_one(db).await
}
pub async fn anonymous(db: &PgPool, raw: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO browser_sessions(verifier, expires_at) VALUES ($1, clock_timestamp() + interval '15 minutes')")
        .bind(verifier(raw)).execute(db).await?;
    Ok(())
}

// The row lock serializes credential submissions and logout from the same browser.
pub async fn lock(tx: &mut Transaction<'_, Postgres>, raw: &str) -> Result<bool, sqlx::Error> {
    sqlx::query("SELECT verifier FROM browser_sessions WHERE verifier=$1 FOR UPDATE")
        .bind(verifier(raw))
        .fetch_optional(&mut **tx)
        .await?;
    // Evaluate the deadline after any lock wait, with a fresh statement snapshot.
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM browser_sessions WHERE verifier=$1 AND NOT revoked AND expires_at > clock_timestamp())")
        .bind(verifier(raw)).fetch_one(&mut **tx).await
}
pub async fn rotate(
    tx: &mut Transaction<'_, Postgres>,
    old: &str,
    new: &str,
    user: Uuid,
) -> Result<(), sqlx::Error> {
    revoke(tx, old).await?;
    sqlx::query("INSERT INTO browser_sessions(verifier, user_id, authenticated_at, expires_at) VALUES ($1,$2,clock_timestamp(),clock_timestamp() + interval '8 hours')")
        .bind(verifier(new)).bind(user).execute(&mut **tx).await?;
    Ok(())
}
pub async fn revoke(tx: &mut Transaction<'_, Postgres>, raw: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE browser_sessions SET revoked=true WHERE verifier=$1")
        .bind(verifier(raw))
        .execute(&mut **tx)
        .await?;
    Ok(())
}
