# Architecture

Crow Auth uses one Rust service and PostgreSQL.

- Axum handles HTTP requests.
- Tokio provides the asynchronous runtime.
- SQLx connects to PostgreSQL.

The service currently checks database connectivity at startup and exposes
`GET /health` for basic HTTP liveness. It also applies versioned SQLx migrations.
Account input rules, pending-account persistence, bounded password hashing and
internal verification actions/activation live in library modules. An encrypted
transactional mail outbox and a separate SMTP worker deliver verification
links. Resend atomically replaces old links and queued mail. `account-cleanup`
removes expired pending accounts and releases reservations.
[Mail operation](docs/MAIL.md) documents configuration and guarantees.
No account HTTP handlers are implemented yet.

Hosted account pages, app management and OpenID Connect are planned to live in
the same service. Add structure as features need it; keep the design simple.

The [milestone security design](docs/ARCHITECTURE.md) defines planned trust
boundaries, sessions and transaction requirements. The
[API contract](docs/API_CONTRACT.md) owns protocol details. Neither document
implies that these authentication features are implemented.
