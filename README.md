# Crow Auth

A small identity provider for adding **Sign in with Crow** to independent apps
using OpenID Connect.

The current implementation includes PostgreSQL migrations, HTTP liveness and an
earlier account/session prototype. It does not yet meet the accepted verified-email
account lifecycle or two-app milestone; see [prototype gaps](docs/PROTOTYPE.md).

## Run locally

With stable Rust supporting edition 2024 and a running PostgreSQL database, set
`DATABASE_URL` to your local connection string, then run:

```sh
export APP_ORIGIN='https://crow.example'
cargo run --locked
```

The server listens on `127.0.0.1:3000`. `GET /health` returns `ok`.

## Documentation

- [Product](PRODUCT.md): purpose and basic scope.
- [Architecture](ARCHITECTURE.md): technical structure.
- [First milestone specification](docs/PRODUCT.md): account lifecycle, SSO and app management.
- [Security design](docs/ARCHITECTURE.md): sessions, persistence and enforcement.
- [OIDC and browser contracts](docs/API_CONTRACT.md): endpoints and validation.
- [Implementation and acceptance](docs/IMPLEMENTATION.md): build order, readiness and two-app scenario.
- [Agent guidance](AGENTS.md): working in this repository.

The first milestone requires a verified account to sign into two backend apps
through one Crow browser session without a second credential prompt. Recovery,
deletion, app ownership and failure cases are specified; the complete milestone
remains planned. Prototype evidence is not milestone acceptance.

## Prototype setup and verification


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
[prototype decisions and gaps](docs/PROTOTYPE.md).
