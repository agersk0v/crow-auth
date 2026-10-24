# crow-auth

Rust HTTP service using Axum and Tokio. Account registration, login and shared
Crow browser sessions are implemented.

## Design documents

Crow Auth is a planned OAuth 2.0/OpenID Connect identity provider and subscription
service with hosted login, browser SSO and backend-only app integrations.

- [Product behavior](docs/PRODUCT.md)
- [Architecture and security](docs/ARCHITECTURE.md)
- [Draft API contract](docs/API_CONTRACT.md)
- [Implementation milestones](docs/IMPLEMENTATION.md)
- [AI contributor instructions](AGENTS.md)

The design documents describe planned behavior; the runnable implementation
covers the account/session milestone below. See [AGENTS.md](AGENTS.md) for
document ownership and decision labels.

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
export APP_ORIGIN='https://crow.example'
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
container does not retain data after removal. Developer apps, OAuth/OIDC and
recovery are not implemented yet.

## Account/session prototype

Startup applies versioned SQLx migrations. Configure `APP_ORIGIN` as the exact
canonical HTTPS origin (no trailing slash) served by your TLS reverse proxy;
forward to the loopback listener. Secure cookies intentionally require HTTPS.
Do not expose this prototype publicly: shared abuse limits, hashing benchmarks,
recovery/password replacement and operational readiness remain release gates.

`GET /register` and `GET /login` render ordinary HTML forms. Registration creates
an account and an eight-hour browser session; login rotates the browser handle.
`GET /account` shows only the authenticated account. Logout uses a CSRF-protected
POST and revokes only that browser session. No recovery is available. Anonymous
form sessions expire after fifteen minutes. Every mutation requires the original
cookie, form CSRF token and exact configured Origin. HTMX requests use identical
full-page responses and security controls. There are no OAuth endpoints yet.

Run the required checks:

```sh
cargo fmt --check
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
# Explicitly run the database suite against a disposable PostgreSQL instance:
# TEST_DATABASE_URL uses the same connection-string format as DATABASE_URL.
cargo test --locked --test browser -- --ignored
```

The database test requires `TEST_DATABASE_URL`, creates a unique isolated schema
and drops it on success. It checks migrations, uniqueness and same-browser races,
CSRF/Origin (including HTMX), rotation/fixation, expiry/logout, persistence across
router recreation, transaction rollback and database failures. It is explicitly
ignored by default so missing PostgreSQL cannot silently count as verification.
See [implementation evidence](docs/IMPLEMENTATION.md) and the
[security decisions](docs/ARCHITECTURE.md#accountsession-implementation-decisions).
