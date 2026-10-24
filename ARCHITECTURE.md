# Architecture

Crow Auth uses one Rust service and PostgreSQL.

- Axum handles HTTP requests.
- Tokio provides the asynchronous runtime.
- SQLx connects to PostgreSQL.

The service currently checks database connectivity at startup and exposes
`GET /health` for basic HTTP liveness.

Hosted account pages, app management and OpenID Connect are planned to live in
the same service. Add structure as features need it; keep the design simple.
