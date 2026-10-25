//! Exposes account, password and verification primitives and versioned migrations.

pub mod accounts;
pub mod password;
pub mod policy;
pub mod verification;

pub async fn migrate(pool: &sqlx::PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
