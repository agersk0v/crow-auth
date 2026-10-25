//! Queues encrypted verification mail atomically and delivers it with bounded retries.
//! SMTP side effects are at least once; account/action locks serialize cancellation.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::Mailbox,
    transport::smtp::authentication::Credentials,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::{fmt, time::Duration};
use url::Url;
use uuid::Uuid;

use crate::verification;

/// Deliberately has no derived Debug: settings include key and SMTP credentials.
pub struct MailSettings {
    pub sender: String,
    pub origin: String,
    pub encryption_key: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
    pub local_capture: bool,
}

pub struct MailConfig {
    sender: Mailbox,
    origin: Url,
    key: [u8; 32],
    transport: AsyncSmtpTransport<Tokio1Executor>,
}
impl fmt::Debug for MailConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MailConfig([REDACTED])")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MailError {
    Configuration,
    InvalidAction,
    Unavailable,
}
impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "Mail configuration invalid",
            Self::InvalidAction => "Verification action invalid",
            Self::Unavailable => "Mail service unavailable",
        })
    }
}
impl std::error::Error for MailError {}
impl From<sqlx::Error> for MailError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}

fn loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

impl MailConfig {
    pub fn new(settings: MailSettings) -> Result<Self, MailError> {
        let sender = settings
            .sender
            .parse()
            .map_err(|_| MailError::Configuration)?;
        let origin = Url::parse(&settings.origin).map_err(|_| MailError::Configuration)?;
        if !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
            || origin.host_str().is_none()
            || !(origin.scheme() == "https"
                || (settings.local_capture
                    && origin.scheme() == "http"
                    && origin.host_str().is_some_and(loopback)))
            || settings.smtp_port == 0
            || settings.smtp_host.is_empty()
            || settings.smtp_username.is_some() != settings.smtp_password.is_some()
        {
            return Err(MailError::Configuration);
        }
        let key: [u8; 32] = URL_SAFE_NO_PAD
            .decode(&settings.encryption_key)
            .map_err(|_| MailError::Configuration)?
            .try_into()
            .map_err(|_| MailError::Configuration)?;
        let mut transport = if settings.local_capture {
            // This escape hatch cannot send plaintext credentials or use a remote relay.
            if !loopback(&settings.smtp_host) || settings.smtp_username.is_some() {
                return Err(MailError::Configuration);
            }
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&settings.smtp_host)
        } else {
            // STARTTLS is mandatory, with hostname and certificate validation.
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.smtp_host)
                .map_err(|_| MailError::Configuration)?
        }
        .port(settings.smtp_port)
        .timeout(Some(Duration::from_secs(10)));
        if let (Some(username), Some(password)) = (settings.smtp_username, settings.smtp_password) {
            transport = transport.credentials(Credentials::new(username, password));
        }
        Ok(Self {
            sender,
            origin,
            key,
            transport: transport.build(),
        })
    }

    pub fn from_env() -> Result<Self, MailError> {
        let required = |name| std::env::var(name).map_err(|_| MailError::Configuration);
        let optional = |name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(_) => Err(MailError::Configuration),
        };
        let local_capture = match optional("MAIL_LOCAL_CAPTURE")?.as_deref() {
            None | Some("false") => false,
            Some("true") => true,
            _ => return Err(MailError::Configuration),
        };
        Self::new(MailSettings {
            sender: required("MAIL_FROM")?,
            origin: required("CROW_ORIGIN")?,
            encryption_key: required("MAIL_ENCRYPTION_KEY")?,
            smtp_host: required("SMTP_HOST")?,
            smtp_port: required("SMTP_PORT")?
                .parse()
                .map_err(|_| MailError::Configuration)?,
            smtp_username: optional("SMTP_USERNAME")?,
            smtp_password: optional("SMTP_PASSWORD")?,
            local_capture,
        })
    }

    fn encrypt(&self, id: Uuid, mail: &MailPayload) -> Result<(Vec<u8>, Vec<u8>), MailError> {
        let mut nonce = [0; 24];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| MailError::Unavailable)?;
        let bytes = serde_json::to_vec(mail).map_err(|_| MailError::Unavailable)?;
        let ciphertext = XChaCha20Poly1305::new((&self.key).into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &bytes,
                    aad: id.as_bytes(),
                },
            )
            .map_err(|_| MailError::Unavailable)?;
        Ok((nonce.to_vec(), ciphertext))
    }

    fn decrypt(&self, id: Uuid, nonce: &[u8], ciphertext: &[u8]) -> Result<MailPayload, MailError> {
        if nonce.len() != 24 {
            return Err(MailError::Unavailable);
        }
        let bytes = XChaCha20Poly1305::new((&self.key).into())
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: id.as_bytes(),
                },
            )
            .map_err(|_| MailError::Unavailable)?;
        serde_json::from_slice(&bytes).map_err(|_| MailError::Unavailable)
    }
}

#[derive(Serialize, Deserialize)]
struct MailPayload {
    recipient: String,
    body: String,
}

/// Queue initial verification in the caller's account-creation transaction.
/// Roll back on EVERY error; only report success after confirmed commit. This
/// does not implement resend or expose the raw token to the caller/browser.
pub async fn queue_initial(
    tx: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    config: &MailConfig,
) -> Result<(), MailError> {
    let token = verification::issue(tx, account_id)
        .await
        .map_err(|e| match e {
            verification::VerificationError::Invalid => MailError::InvalidAction,
            verification::VerificationError::Unavailable => MailError::Unavailable,
        })?;
    // issue holds the account lock; now lock the action in the same order.
    let row = sqlx::query(
        "SELECT id,email_snapshot FROM verification_actions WHERE account_id=$1 FOR UPDATE",
    )
    .bind(account_id)
    .fetch_one(&mut **tx)
    .await?;
    let action_id: Uuid = row.get("id");
    let mut link = config
        .origin
        .join("/verify-email")
        .map_err(|_| MailError::Configuration)?;
    link.query_pairs_mut()
        .append_pair("token", token.expose_secret());
    let payload = MailPayload {
        recipient: row.get("email_snapshot"),
        body: format!(
            "Confirm your email address by opening this link and submitting the confirmation form:\n{link}\n\nThis link expires in 30 minutes. Verifying does not sign you in. If you did not register, ignore this email."
        ),
    };
    let (nonce, ciphertext) = config.encrypt(action_id, &payload)?;
    sqlx::query(
        "INSERT INTO verification_mail_outbox(action_id,nonce,ciphertext) VALUES($1,$2,$3)",
    )
    .bind(action_id)
    .bind(nonce)
    .bind(ciphertext)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Idle,
    Delivered,
    RetryScheduled,
    Discarded,
}

/// Process at most one due job, holding account -> action -> outbox locks through
/// the bounded send. A crash/uncertain commit after SMTP acceptance can duplicate
/// the SAME link; SMTP cannot participate in the database transaction.
pub async fn deliver_one(db: &PgPool, config: &MailConfig) -> Result<DeliveryOutcome, MailError> {
    let mut tx = db.begin().await?;
    let result = deliver_in_transaction(&mut tx, config).await;
    match result {
        Ok(outcome) => {
            tx.commit().await?;
            Ok(outcome)
        }
        Err(error) => {
            let _ = tx.rollback().await;
            Err(error)
        }
    }
}

async fn deliver_in_transaction(
    tx: &mut Transaction<'_, Postgres>,
    config: &MailConfig,
) -> Result<DeliveryOutcome, MailError> {
    // Metadata only: locking a queue row before the account would invert order.
    // Limit work per poll, skipping busy accounts so one sender cannot block others.
    let candidates = sqlx::query(
        "SELECT q.action_id,v.account_id FROM verification_mail_outbox q \
         JOIN verification_actions v ON v.id=q.action_id JOIN accounts a ON a.id=v.account_id \
         WHERE q.next_attempt_at<=clock_timestamp() OR v.expires_at<=clock_timestamp() \
         OR a.pending_expires_at<=clock_timestamp() OR a.status<>'pending' OR v.consumed_at IS NOT NULL \
         OR v.security_version<>a.security_version OR v.email_snapshot<>a.email OR v.purpose<>'verify_email' \
         ORDER BY q.next_attempt_at,q.action_id LIMIT 32",
    ).fetch_all(&mut **tx).await?;
    for candidate in candidates {
        let account_id: Uuid = candidate.get("account_id");
        let action_id: Uuid = candidate.get("action_id");
        let account: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM accounts WHERE id=$1 FOR UPDATE SKIP LOCKED")
                .bind(account_id)
                .fetch_optional(&mut **tx)
                .await?;
        if account.is_none() {
            continue;
        }
        // If a concurrent deletion removed the action, simply try on the next poll.
        let action: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM verification_actions WHERE id=$1 AND account_id=$2 FOR UPDATE",
        )
        .bind(action_id)
        .bind(account_id)
        .fetch_optional(&mut **tx)
        .await?;
        if action.is_none() {
            return Ok(DeliveryOutcome::Idle);
        }
        let job = sqlx::query("SELECT nonce,ciphertext,attempts FROM verification_mail_outbox WHERE action_id=$1 FOR UPDATE")
            .bind(action_id).fetch_optional(&mut **tx).await?;
        let Some(job) = job else {
            return Ok(DeliveryOutcome::Idle);
        };
        // Fresh time after all waits; cleanup never grants authority.
        let valid: bool = sqlx::query_scalar(
            "SELECT a.status='pending' AND clock_timestamp()<a.pending_expires_at \
             AND v.purpose='verify_email' AND v.email_snapshot=a.email AND v.security_version=a.security_version \
             AND v.consumed_at IS NULL AND clock_timestamp()>=v.created_at AND clock_timestamp()<v.expires_at \
             FROM verification_actions v JOIN accounts a ON a.id=v.account_id WHERE v.id=$1",
        ).bind(action_id).fetch_one(&mut **tx).await?;
        let attempts: i16 = job.get("attempts");
        if !valid || attempts >= 4 {
            erase(tx, action_id).await?;
            return Ok(DeliveryOutcome::Discarded);
        }
        // Another worker may already have scheduled this candidate's next retry.
        let due: bool = sqlx::query_scalar("SELECT next_attempt_at<=clock_timestamp() FROM verification_mail_outbox WHERE action_id=$1")
            .bind(action_id).fetch_one(&mut **tx).await?;
        if !due {
            return Ok(DeliveryOutcome::Idle);
        }
        let payload = config.decrypt(action_id, job.get("nonce"), job.get("ciphertext"))?;
        let message = Message::builder()
            .from(config.sender.clone())
            .to(payload
                .recipient
                .parse()
                .map_err(|_| MailError::Unavailable)?)
            .subject("Verify your Crow email address")
            .message_id(Some(format!("<verification-{action_id}@crow.invalid>")))
            .body(payload.body)
            .map_err(|_| MailError::Unavailable)?;
        let sent =
            tokio::time::timeout(Duration::from_secs(10), config.transport.send(message)).await;
        if matches!(sent, Ok(Ok(_))) {
            erase(tx, action_id).await?;
            return Ok(DeliveryOutcome::Delivered);
        }
        if attempts >= 3 {
            erase(tx, action_id).await?;
            return Ok(DeliveryOutcome::Discarded);
        }
        let delay = [60_f64, 300., 900.][attempts as usize];
        sqlx::query("UPDATE verification_mail_outbox SET attempts=attempts+1,next_attempt_at=clock_timestamp()+$2*interval '1 second' WHERE action_id=$1")
            .bind(action_id).bind(delay).execute(&mut **tx).await?;
        return Ok(DeliveryOutcome::RetryScheduled);
    }
    Ok(DeliveryOutcome::Idle)
}

async fn erase(tx: &mut Transaction<'_, Postgres>, action_id: Uuid) -> Result<(), MailError> {
    sqlx::query("DELETE FROM verification_mail_outbox WHERE action_id=$1")
        .bind(action_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> MailSettings {
        MailSettings {
            sender: "Crow <crow@example.test>".into(),
            origin: "https://crow.example.test".into(),
            encryption_key: URL_SAFE_NO_PAD.encode([7; 32]),
            smtp_host: "localhost".into(),
            smtp_port: 1025,
            smtp_username: None,
            smtp_password: None,
            local_capture: true,
        }
    }

    #[test]
    fn authenticated_encryption_and_redaction() {
        let config = MailConfig::new(settings()).unwrap();
        let id = Uuid::new_v4();
        let payload = MailPayload {
            recipient: "owner@example.test".into(),
            body: "secret-link".into(),
        };
        let (nonce, ciphertext) = config.encrypt(id, &payload).unwrap();
        let (other_nonce, other_ciphertext) = config.encrypt(id, &payload).unwrap();
        assert_ne!(nonce, other_nonce);
        assert_ne!(ciphertext, other_ciphertext);
        assert_eq!(
            config.decrypt(id, &nonce, &ciphertext).unwrap().body,
            "secret-link"
        );
        assert!(config.decrypt(Uuid::new_v4(), &nonce, &ciphertext).is_err());
        let mut corrupted = ciphertext.clone();
        corrupted[0] ^= 1;
        assert!(config.decrypt(id, &nonce, &corrupted).is_err());
        let mut wrong = settings();
        wrong.encryption_key = URL_SAFE_NO_PAD.encode([8; 32]);
        assert!(
            MailConfig::new(wrong)
                .unwrap()
                .decrypt(id, &nonce, &ciphertext)
                .is_err()
        );
        assert_eq!(format!("{config:?}"), "MailConfig([REDACTED])");
    }

    #[test]
    fn configuration_fails_closed() {
        let mut cases: Vec<MailSettings> = Vec::new();
        let mut s = settings();
        s.encryption_key = "short".into();
        cases.push(s);
        let mut s = settings();
        s.origin = "https://crow.test/path".into();
        cases.push(s);
        let mut s = settings();
        s.origin = "https://attacker@crow.test".into();
        cases.push(s);
        let mut s = settings();
        s.origin = "http://crow.test".into();
        cases.push(s);
        let mut s = settings();
        s.smtp_host = "remote.test".into();
        cases.push(s);
        let mut s = settings();
        s.smtp_username = Some("user".into());
        cases.push(s);
        let mut s = settings();
        s.smtp_username = Some("user".into());
        s.smtp_password = Some("secret".into());
        cases.push(s);
        let mut s = settings();
        s.sender = "bad\r\nheader".into();
        cases.push(s);
        for s in cases {
            assert_eq!(MailConfig::new(s).unwrap_err(), MailError::Configuration);
        }
        let mut s = settings();
        s.local_capture = false;
        s.origin = "http://localhost:3000".into();
        assert_eq!(MailConfig::new(s).unwrap_err(), MailError::Configuration);
    }
}
