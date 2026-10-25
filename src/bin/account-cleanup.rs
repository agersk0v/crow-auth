//! Removes expired pending accounts and idle throttle buckets independently of SMTP.

use crow_auth::accounts::cleanup_expired_one;
use sqlx::postgres::PgPoolOptions;
use std::{io, time::Duration};

#[tokio::main]
async fn main() -> io::Result<()> {
    let once = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [arg] if arg == "--once" => true,
        _ => return Err(io::Error::other("Usage: account-cleanup [--once]")),
    };
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| io::Error::other("DATABASE_URL must be configured"))?;
    let db = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&url)
        .await
        .map_err(|_| io::Error::other("Cleanup database unavailable"))?;
    sqlx::query("SELECT a.id FROM accounts a LEFT JOIN verification_actions v ON v.account_id=a.id LEFT JOIN verification_mail_outbox q ON q.action_id=v.id LEFT JOIN registration_limits l ON false LIMIT 0")
        .execute(&db)
        .await
        .map_err(|_| io::Error::other("Account schema unavailable; apply migrations first"))?;
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        match cleanup_expired_one(&db).await {
            Ok(true) => eprintln!("Cleanup: expired pending account removed"),
            Ok(false) => {}
            Err(_) if once => return Err(io::Error::other("Cleanup storage unavailable")),
            Err(_) => eprintln!("Cleanup: storage unavailable"),
        }
        if crow_auth::throttle::cleanup_expired(&db).await.is_err() {
            if once {
                return Err(io::Error::other("Throttle cleanup storage unavailable"));
            }
            eprintln!("Cleanup: throttle storage unavailable");
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
