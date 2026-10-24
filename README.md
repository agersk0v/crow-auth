# crow-auth

Rust HTTP service using Axum and Tokio. Authentication is not implemented yet.

## Design documents

Crow Auth is a planned OAuth 2.0/OpenID Connect identity provider and subscription
service with hosted login and browser SSO,
backend-only app integrations and a separate account/developer portal.
Rust/Axum owns authentication and token signing; PostgreSQL stores accounts,
authorization flows, portal sessions and refresh sessions. Refresh rotation and
revocation use database transactions.

- [Product behavior](docs/PRODUCT.md)
- [Architecture and security](docs/ARCHITECTURE.md)
- [Draft API contract](docs/API_CONTRACT.md)
- [Implementation milestones](docs/IMPLEMENTATION.md)
- [AI contributor instructions](AGENTS.md)

OAuth/OIDC endpoints, discovery and authentication are not implemented yet.
These describe planned behavior; the runnable implementation remains the skeleton below.

## Development

Install a stable Rust toolchain and PostgreSQL (or Docker). To start a local database (Bash):

```sh
read -rsp 'Local PostgreSQL password: ' POSTGRES_PASSWORD; echo
export POSTGRES_PASSWORD
docker run --name crow-auth-postgres --rm -d \
  -e POSTGRES_USER=crow_auth -e POSTGRES_PASSWORD -e POSTGRES_DB=crow_auth \
  -p 127.0.0.1:5432:5432 postgres:17
docker exec crow-auth-postgres pg_isready -U crow_auth -d crow_auth
unset POSTGRES_PASSWORD
```

Wait until `pg_isready` reports that the database accepts connections. Set
`DATABASE_URL` in your shell using your local credentials (URL-encode special
characters in the username and password). This is a placeholder, not a working credential:

```sh
export DATABASE_URL='postgres://<user>:<url-encoded-password>@127.0.0.1:5432/crow_auth'
```

Then run:

```sh
cargo run --locked
```

The server listens on `127.0.0.1:3000`. `GET /health` returns `ok`.
At startup it connects a SQLx PostgreSQL pool, runs `SELECT 1`, and stores the pool
in Axum application state. Missing configuration or an unavailable database stops
startup with an error that does not include credentials. `/health` is a basic HTTP
liveness endpoint; it does not check ongoing database health.

```sh
cargo check --locked
cargo test --locked
curl --fail http://127.0.0.1:3000/health
```

Stop the local database with `docker stop crow-auth-postgres`. This disposable
container does not retain data after removal. User tables and authentication
flows are not included yet.
