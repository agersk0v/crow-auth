use axum::{Router, routing::get};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let app = Router::new().route("/health", get(|| async { "ok" }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;

    axum::serve(listener, app).await
}
