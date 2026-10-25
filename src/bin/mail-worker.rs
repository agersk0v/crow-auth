//! Runs the independently configured encrypted verification-mail delivery worker.

use crow_auth::mail::{DeliveryOutcome, MailConfig, deliver_one};
use sqlx::postgres::PgPoolOptions;
use std::{io, time::Duration};

#[tokio::main]
async fn main() -> io::Result<()> {
    let once = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [arg] if arg == "--once" => true,
        _ => return Err(io::Error::other("Usage: mail-worker [--once]")),
    };
    // Validate all mail configuration before connecting or processing any job.
    let config = MailConfig::from_env().map_err(io::Error::other)?;
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| io::Error::other("DATABASE_URL must be configured"))?;
    let db = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&url)
        .await
        .map_err(|_| io::Error::other("Mail database unavailable"))?;
    // The HTTP service/migration owner applies DDL; workers need no CREATE rights.
    sqlx::query("SELECT action_id FROM verification_mail_outbox LIMIT 0")
        .execute(&db)
        .await
        .map_err(|_| io::Error::other("Mail schema unavailable; apply migrations first"))?;
    // Finish bounded in-flight delivery before honoring shutdown to avoid needless duplicates.
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        match deliver_one(&db, &config).await {
            Ok(DeliveryOutcome::Idle) => {}
            Ok(outcome) => eprintln!("Mail worker: {outcome:?}"),
            Err(_) if once => return Err(io::Error::other("Mail worker: delivery unavailable")),
            Err(_) => eprintln!("Mail worker: delivery unavailable"),
        }
        if once {
            break;
        }
        tokio::select! {
            _ = &mut shutdown => break,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {},
        }
    }
    db.close().await;
    Ok(())
}
