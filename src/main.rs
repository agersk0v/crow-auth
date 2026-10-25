use axum::{Router, routing::get};
use sqlx::postgres::PgPoolOptions;
use std::{io, time::Duration};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let database_url = std::env::var("DATABASE_URL").map_err(|_| {
        io::Error::other("DATABASE_URL must be set to a valid PostgreSQL connection string")
    })?;
    let db = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await
        .map_err(|_| {
            io::Error::other(
                "Failed to connect to PostgreSQL; check DATABASE_URL and database availability",
            )
        })?;

    sqlx::query("SELECT 1")
        .execute(&db)
        .await
        .map_err(|_| io::Error::other("PostgreSQL startup connectivity check failed"))?;

    crow_auth::migrate(&db)
        .await
        .map_err(|_| io::Error::other("PostgreSQL migration failed"))?;

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .with_state(db);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;

    axum::serve(listener, app).await
}
