//! Enforces shared rolling-hour registration/resend budgets before expensive account work.
//! IP then email locks and owned transactions keep admission separate from account rollback.

use crate::{browser::message_page, policy::Email};
use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use std::{
    fmt,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

#[derive(Debug, PartialEq, Eq)]
pub enum ThrottleError {
    Limited { retry_after_seconds: u64 },
    Unavailable,
}
impl fmt::Display for ThrottleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Limited { .. } => "Too many requests; try again later",
            Self::Unavailable => "Request throttling unavailable",
        })
    }
}
impl std::error::Error for ThrottleError {}
impl IntoResponse for ThrottleError {
    fn into_response(self) -> Response {
        let mut response = message_page("Request could not be completed", &self.to_string());
        match self {
            Self::Limited {
                retry_after_seconds,
            } => {
                *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                response.headers_mut().insert(
                    header::RETRY_AFTER,
                    HeaderValue::from_str(&retry_after_seconds.to_string())
                        .expect("integer header"),
                );
            }
            Self::Unavailable => *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE,
        }
        response
    }
}

#[derive(Clone)]
pub struct RegistrationThrottle {
    db: PgPool,
}
impl RegistrationThrottle {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// Call after Origin/CSRF checks, BEFORE hashing or opening an account transaction.
    /// Supply the transport peer, never X-Forwarded-For/Forwarded. None charges only
    /// IP for malformed email input. Registration and resend share the same budgets.
    /// All admitted IP attempts count, including email-throttled requests and later
    /// validation/account failures. A full IP budget never creates an email bucket.
    pub async fn check(
        &self,
        peer: SocketAddr,
        email: Option<&Email>,
    ) -> Result<(), ThrottleError> {
        tokio::time::timeout(Duration::from_secs(5), self.check_inner(peer.ip(), email))
            .await
            .unwrap_or(Err(ThrottleError::Unavailable))
    }

    async fn check_inner(&self, ip: IpAddr, email: Option<&Email>) -> Result<(), ThrottleError> {
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|_| ThrottleError::Unavailable)?;
        let result: Result<Option<u64>, sqlx::Error> = async {
            sqlx::query("SET LOCAL lock_timeout = '2s'")
                .execute(&mut *tx)
                .await?;
            sqlx::query("SET LOCAL statement_timeout = '3s'")
                .execute(&mut *tx)
                .await?;
            // Every path takes IP before email, never any account/action lock.
            if let Some(retry) = charge(&mut tx, 0, &ip_subject(ip), 20).await? {
                return Ok(Some(retry));
            }
            if let Some(email) = email {
                return charge(&mut tx, 1, &email_subject(email), 5).await;
            }
            Ok(None)
        }
        .await;
        match result {
            Ok(limited) => {
                // A denial can still debit the IP bucket; commit before returning it.
                tx.commit().await.map_err(|_| ThrottleError::Unavailable)?;
                match limited {
                    Some(retry_after_seconds) => Err(ThrottleError::Limited {
                        retry_after_seconds,
                    }),
                    None => Ok(()),
                }
            }
            Err(_) => {
                let _ = tx.rollback().await;
                Err(ThrottleError::Unavailable)
            }
        }
    }
}

async fn charge(
    tx: &mut Transaction<'_, Postgres>,
    kind: i16,
    subject: &[u8; 32],
    limit: i64,
) -> Result<Option<u64>, sqlx::Error> {
    // The upsert acquires the row lock even if cleanup deletes an expired bucket
    // concurrently. A separate INSERT DO NOTHING + SELECT could miss that row.
    sqlx::query("INSERT INTO registration_limits(kind,subject,expires_at) VALUES($1,$2,clock_timestamp()+interval '1 hour') ON CONFLICT(kind,subject) DO UPDATE SET subject=EXCLUDED.subject")
        .bind(kind).bind(subject.as_slice()).execute(&mut **tx).await?;
    // Fresh database time AFTER any row-lock wait. Equality at one hour expires.
    sqlx::query("WITH c AS MATERIALIZED (SELECT clock_timestamp() now) UPDATE registration_limits SET attempts=ARRAY(SELECT stamp FROM unnest(attempts) stamp,c WHERE stamp>c.now-interval '1 hour') WHERE kind=$1 AND subject=$2")
        .bind(kind).bind(subject.as_slice()).execute(&mut **tx).await?;
    let (used, retry): (i64, i64) = sqlx::query_as(
        "SELECT cardinality(attempts)::bigint, COALESCE(GREATEST(1,ceil(EXTRACT(EPOCH FROM ((SELECT min(stamp) FROM unnest(attempts) stamp)+interval '1 hour'-clock_timestamp())))::bigint),1) FROM registration_limits WHERE kind=$1 AND subject=$2",
    ).bind(kind).bind(subject.as_slice()).fetch_one(&mut **tx).await?;
    if used >= limit {
        return Ok(Some(retry as u64));
    }
    sqlx::query("WITH c AS MATERIALIZED (SELECT clock_timestamp() now) UPDATE registration_limits SET attempts=array_append(attempts,c.now),expires_at=GREATEST(expires_at,c.now+interval '1 hour') FROM c WHERE kind=$1 AND subject=$2")
        .bind(kind).bind(subject.as_slice()).execute(&mut **tx).await?;
    Ok(None)
}

fn ip_subject(ip: IpAddr) -> [u8; 32] {
    let ip = match ip {
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ip)),
        ip => ip,
    };
    let mut hash = Sha256::new();
    hash.update(b"crow-registration-ip-v1\0");
    match ip {
        IpAddr::V4(ip) => hash.update(ip.octets()),
        IpAddr::V6(ip) => hash.update(ip.octets()),
    }
    hash.finalize().into()
}
fn email_subject(email: &Email) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"crow-registration-email-v1\0");
    hash.update(email.as_str().as_bytes());
    hash.finalize().into()
}

/// Remove at most 100 idle expired buckets, skipping rows currently being charged.
/// This owned transaction takes no account/action locks; expiry does not require cleanup.
pub async fn cleanup_expired(db: &PgPool) -> Result<u64, ThrottleError> {
    let removed = sqlx::query("DELETE FROM registration_limits WHERE (kind,subject) IN (SELECT kind,subject FROM registration_limits WHERE expires_at<=clock_timestamp() ORDER BY expires_at,kind,subject LIMIT 100 FOR UPDATE SKIP LOCKED)")
        .execute(db).await.map_err(|_| ThrottleError::Unavailable)?.rows_affected();
    Ok(removed)
}
