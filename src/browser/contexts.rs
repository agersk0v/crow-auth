//! Binds verification actions to short-lived pre-auth forms without consuming emailed tokens.

use super::*;

/// Internal action reference returned only after browser, CSRF and live binding checks.
/// The future handler must consume/activate this action IN THE SAME transaction.
/// This value is not a browser credential and must never be accepted from a client.
pub struct Confirmation {
    action_id: Uuid,
    account_id: Uuid,
}
impl Confirmation {
    pub fn action_id(&self) -> Uuid {
        self.action_id
    }
    pub fn account_id(&self) -> Uuid {
        self.account_id
    }
}
impl fmt::Debug for Confirmation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Confirmation([REDACTED])")
    }
}

/// Non-secret selector for the clean URL plus a browser-bound POST credential.
/// The selector alone grants no authority and can never select another browser's form.
pub struct PreparedConfirmation {
    selector: Uuid,
    secret: Secret,
}
impl PreparedConfirmation {
    pub fn selector(&self) -> Uuid {
        self.selector
    }
    pub fn clean_path(&self) -> String {
        format!("/verify-email?confirmation={}", self.selector)
    }
    pub fn secret(&self) -> &Secret {
        &self.secret
    }
    pub fn expose_secret(&self) -> &str {
        self.secret.expose_secret()
    }
}
impl fmt::Debug for PreparedConfirmation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PreparedConfirmation([REDACTED])")
    }
}
fn context_secret(browser: &PreauthBrowser, selector: Uuid) -> Secret {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&browser.csrf_secret)
        .expect("HMAC accepts any key size");
    mac.update(b"crow-verification-context-v1\0");
    mac.update(selector.as_bytes());
    Secret(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

impl BrowserSecurity {
    /// GET primitive: create a form context, never consume an action or activate.
    /// Publish its opaque handle in form POST data/server state, never a raw token in the clean URL.
    pub async fn prepare_verification(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        browser: &PreauthBrowser,
        raw_token: &str,
    ) -> Result<PreparedConfirmation, BrowserError> {
        let hash = verifier(raw_token).ok_or(BrowserError::InvalidContext)?;
        let row = sqlx::query(
            "SELECT id,account_id,token_verifier FROM verification_actions WHERE token_verifier=$1",
        )
        .bind(hash.as_slice())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(BrowserError::InvalidContext)?;
        let stored: Vec<u8> = row.get("token_verifier");
        if !matches_hash(&stored, &hash) {
            return Err(BrowserError::InvalidContext);
        }
        let action_id: Uuid = row.get("id");
        let account_id: Uuid = row.get("account_id");
        lock_action(tx, account_id, action_id).await?;
        let selector = Uuid::new_v4();
        let raw = context_secret(browser, selector);
        let affected=sqlx::query(
            "WITH c AS MATERIALIZED (SELECT clock_timestamp() now) \
             INSERT INTO verification_contexts(id,context_verifier,browser_id,action_id,created_at,expires_at) \
             SELECT $1,$2,b.id,v.id,now,LEAST(now+interval '5 minutes',b.expires_at,v.expires_at,a.pending_expires_at) \
             FROM preauth_browsers b,verification_actions v,accounts a,c \
             WHERE b.id=$3 AND v.id=$4 AND a.id=v.account_id AND a.status='pending' \
             AND v.purpose='verify_email' AND v.email_snapshot=a.email AND v.security_version=a.security_version \
             AND v.consumed_at IS NULL AND now>=v.created_at AND now<v.expires_at \
             AND now<a.pending_expires_at AND now<b.expires_at",
        ).bind(selector).bind(verifier(raw.expose_secret()).ok_or(BrowserError::Unavailable)?.as_slice())
            .bind(browser.id).bind(action_id).execute(&mut **tx).await?.rows_affected();
        if affected != 1 {
            return Err(BrowserError::InvalidContext);
        }
        Ok(PreparedConfirmation {
            selector,
            secret: raw,
        })
    }

    /// GET after the clean redirect: regenerate only this browser's POST credential,
    /// rechecking live bindings without spending the token or extending any deadline.
    pub async fn load_confirmation(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        browser: &PreauthBrowser,
        selector: Uuid,
    ) -> Result<Secret, BrowserError> {
        let secret = context_secret(browser, selector);
        let stored: Option<Vec<u8>> = sqlx::query_scalar(
            "WITH clock AS MATERIALIZED (SELECT clock_timestamp() now) SELECT c.context_verifier FROM verification_contexts c JOIN preauth_browsers b ON b.id=c.browser_id \
             JOIN verification_actions v ON v.id=c.action_id JOIN accounts a ON a.id=v.account_id,clock \
             WHERE c.id=$1 AND b.id=$2 AND c.context_verifier=$3 AND now<c.expires_at AND now<b.expires_at \
             AND a.status='pending' AND now<a.pending_expires_at AND v.purpose='verify_email' \
             AND v.email_snapshot=a.email AND v.security_version=a.security_version \
             AND v.consumed_at IS NULL AND now>=v.created_at AND now<v.expires_at",
        ).bind(selector).bind(browser.id).bind(verifier(secret.expose_secret()).ok_or(BrowserError::Unavailable)?.as_slice())
            .fetch_optional(&mut **tx).await?;
        if !stored.is_some_and(|stored| {
            matches_hash(
                &stored,
                &verifier(secret.expose_secret()).expect("derived credential is canonical"),
            )
        }) {
            return Err(BrowserError::InvalidContext);
        }
        Ok(secret)
    }

    /// POST primitive: spend one context after account-first locks and fresh checks.
    /// No activation is performed here. Roll back on downstream failure; commit
    /// only together with the future action-consumption/activation operation.
    pub async fn take_confirmation(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        headers: &HeaderMap,
        csrf: &str,
        context: &str,
    ) -> Result<Confirmation, BrowserError> {
        let browser_id = self
            .authorize_post(tx, headers, csrf, FormAction::VerifyEmail)
            .await?;
        let hash = verifier(context).ok_or(BrowserError::InvalidContext)?;
        let row=sqlx::query("SELECT c.id,c.action_id,v.account_id,c.context_verifier FROM verification_contexts c JOIN verification_actions v ON v.id=c.action_id WHERE c.context_verifier=$1 AND c.browser_id=$2")
            .bind(hash.as_slice()).bind(browser_id).fetch_optional(&mut **tx).await?.ok_or(BrowserError::InvalidContext)?;
        let stored: Vec<u8> = row.get("context_verifier");
        if !matches_hash(&stored, &hash) {
            return Err(BrowserError::InvalidContext);
        }
        let id: Uuid = row.get("id");
        let action_id: Uuid = row.get("action_id");
        let account_id: Uuid = row.get("account_id");
        lock_action(tx, account_id, action_id).await?;
        let found:Option<Uuid>=sqlx::query_scalar("SELECT id FROM verification_contexts WHERE id=$1 AND browser_id=$2 AND action_id=$3 FOR UPDATE")
            .bind(id).bind(browser_id).bind(action_id).fetch_optional(&mut **tx).await?;
        if found.is_none() {
            return Err(BrowserError::InvalidContext);
        }
        // Check all deadlines/bindings AFTER lock waits, including pre-auth expiry.
        let changed=sqlx::query(
            "WITH clock AS MATERIALIZED (SELECT clock_timestamp() now) \
             DELETE FROM verification_contexts c USING preauth_browsers b,verification_actions v,accounts a,clock \
             WHERE c.id=$1 AND c.browser_id=b.id AND c.action_id=v.id AND v.account_id=a.id \
             AND b.id=$2 AND c.action_id=$3 AND now<c.expires_at AND now<b.expires_at \
             AND a.status='pending' AND now<a.pending_expires_at AND v.purpose='verify_email' \
             AND v.email_snapshot=a.email AND v.security_version=a.security_version \
             AND v.consumed_at IS NULL AND now>=v.created_at AND now<v.expires_at",
        ).bind(id).bind(browser_id).bind(action_id).execute(&mut **tx).await?.rows_affected();
        if changed != 1 {
            return Err(BrowserError::InvalidContext);
        }
        Ok(Confirmation {
            action_id,
            account_id,
        })
    }
}

async fn lock_action(
    tx: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    action_id: Uuid,
) -> Result<(), BrowserError> {
    let account: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM accounts WHERE id=$1 FOR UPDATE")
            .bind(account_id)
            .fetch_optional(&mut **tx)
            .await?;
    if account.is_none() {
        return Err(BrowserError::InvalidContext);
    }
    let action: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM verification_actions WHERE id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(action_id)
    .bind(account_id)
    .fetch_optional(&mut **tx)
    .await?;
    if action.is_none() {
        return Err(BrowserError::InvalidContext);
    }
    Ok(())
}

/// Erase at most 100 expired browsers (cascading contexts) and 100 standalone
/// expired contexts per invocation. Cleanup takes no account/action locks.
pub async fn cleanup_expired(db: &PgPool) -> Result<u64, BrowserError> {
    let mut tx = db.begin().await?;
    let browsers=sqlx::query("DELETE FROM preauth_browsers WHERE id IN (SELECT id FROM preauth_browsers WHERE expires_at<=clock_timestamp() ORDER BY expires_at,id LIMIT 100 FOR UPDATE SKIP LOCKED)")
        .execute(&mut *tx).await?.rows_affected();
    let contexts=sqlx::query("DELETE FROM verification_contexts WHERE id IN (SELECT id FROM verification_contexts WHERE expires_at<=clock_timestamp() ORDER BY expires_at,id LIMIT 100 FOR UPDATE SKIP LOCKED)")
        .execute(&mut *tx).await?.rows_affected();
    tx.commit().await?;
    Ok(browsers + contexts)
}
