//! Exposes account and password foundations and the versioned migration runner.

pub mod accounts;
pub mod password;
pub mod policy;

pub async fn migrate(pool: &sqlx::PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
