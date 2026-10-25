# Crow Auth

A small identity provider for adding **Sign in with Crow** to independent apps
using OpenID Connect.

The service connects to PostgreSQL, runs versioned migrations and serves a health
endpoint. Account persistence and password-security primitives are implemented;
hosted registration, browser email verification and sign-in are not available yet.

## Run locally

Install stable Rust supporting edition 2024 and Docker with Compose v2. Create
local database settings, edit both password placeholders, then start PostgreSQL:

```sh
cp .env.example .env
# Edit .env; keep its passwords private.
docker compose up -d --wait
```

Compose runs PostgreSQL 17 on `127.0.0.1:5432`, checks readiness with `pg_isready`,
and keeps development data in a named volume. Set `DATABASE_URL` in your shell
using the development password from `.env` (URL-encode special characters):

```sh
export DATABASE_URL='postgres://crow_auth:<url-encoded-password>@127.0.0.1:5432/crow_auth'
cargo run --locked
```

Compose reads `.env`; `cargo run` does not load it. The URL above is a placeholder.
If you change `POSTGRES_PORT`, use the same port in `DATABASE_URL`.
The server listens on `127.0.0.1:3000`. `GET /health` returns `ok`:

```sh
curl --fail http://127.0.0.1:3000/health
```

Stop the database with `docker compose down`; its data remains available on the
next start. `docker compose down -v` **deletes the development database**.
Changing the password in `.env` does not update an existing database volume;
change it in PostgreSQL or deliberately recreate disposable local data.

### Disposable test database

Use the standalone test stack rather than pointing tests at development data:

```sh
docker compose -f compose.test.yaml -p crow-auth-test up -d --wait
export TEST_DATABASE_URL='postgres://crow_auth_test:<url-encoded-test-password>@127.0.0.1:55432/crow_auth_test'
# Run the database suites before shutting down:
cargo test --locked --test accounts_database -- --ignored
cargo test --locked --test verification_database -- --ignored
cargo test --locked --test browser_verification -- --ignored
# Mail suite also needs the capture service from this test stack:
export TEST_SMTP_PORT=1025
export TEST_MAILPIT_URL=http://127.0.0.1:8025
cargo test --locked --test mail_database -- --ignored
cargo test --locked --test lifecycle_database -- --ignored
docker compose -f compose.test.yaml -p crow-auth-test down
```

The test stack uses a separate port, database, credentials and project, with
PostgreSQL data held in tmpfs. Stopping its container discards its data. Use
`TEST_POSTGRES_PASSWORD` from `.env` in the test URL, and match any custom
`TEST_POSTGRES_PORT`. Run the test Compose file alone, not as an override of the
development file. The account persistence, uniqueness-race and rollback suite
creates a unique schema and removes it after a successful run; CI always runs this suite against disposable PostgreSQL.
Missing `TEST_DATABASE_URL` fails the explicit invocation. To smoke-test the service
against this database, set `DATABASE_URL` to the test URL for that run.

## Account foundations

Startup applies SQLx migrations. The library normalizes usernames/emails, creates
pending account records inside caller-owned transactions, and hashes/verifies
passwords with Argon2id. It has no registration or login routes and creates no
browser sessions. Pending accounts carry a 24-hour deadline; releasing expired
reservations is handled by the separate `account-cleanup` process. Internal verification primitives issue and consume purpose-bound links and
activate pending accounts atomically. Initial and resent verification mail can be queued in an encrypted
transactional outbox and delivered by a separate SMTP worker. Resend invalidates
all earlier links without changing the password or extending the reservation. There is no browser
verification route yet; see [mail setup and operation](docs/MAIL.md).
See [the primitive contract](docs/ARCHITECTURE.md#verification-action-primitives).

Before building registration/password-change handlers, provision the local
compromised-password list with `python3 scripts/provision-password-blocklist.py`.
See [dataset provenance and monthly updates](data/README.md). The password service
cannot initialize with a missing or empty list. The current health-only HTTP server
does not initialize that service. Never use the tiny test fixture in a deployment.

Hashing uses four blocking workers and at most sixteen waiting requests; overload
is an explicit error for future handlers to map to 503. These are prototype costs,
not a production benchmark. See [foundation evidence](docs/IMPLEMENTATION.md) and
[transaction rules](docs/ARCHITECTURE.md#account-foundation-implementation).

## Hosted browser foundations

The library now includes signed pre-auth cookies, action-bound CSRF, strict Origin
validation, escaped Askama templates and security-header/body-limit middleware.
Verification forms use hidden email-token and CSRF fields with no browser-state
or confirmation-context tables. These are tested primitives; they are not yet exposed by
the health-only HTTP service. See [browser integration and limits](docs/BROWSER.md).
Shared throttling and hosted registration/verification handlers remain next.

## Continuous integration

GitHub Actions runs formatting, compilation, Clippy, unit tests, account and
verification-action tests, encrypted-outbox/SMTP-capture and lifecycle race suites,
signed-cookie CSRF/verification checks, and a PostgreSQL 17 startup/HTTP health
smoke test on pull requests to `main`, pushes to `main`, and
merge-queue groups. The workflow can also be started manually. To reproduce the
smoke check locally with a disposable database:

```sh
DATABASE_URL="$TEST_DATABASE_URL" bash .github/scripts/smoke-test.sh
```

In a GitHub ruleset for `main`, require the `Rust checks` status check and pull
requests before merging. Adding the workflow does not configure that repository
setting.

## Documentation

- [Product](PRODUCT.md): purpose and basic scope.
- [Architecture](ARCHITECTURE.md): technical structure.
- [First milestone specification](docs/PRODUCT.md): account lifecycle, SSO and app management.
- [Security design](docs/ARCHITECTURE.md): sessions, persistence and enforcement.
- [OIDC and browser contracts](docs/API_CONTRACT.md): endpoints and validation.
- [Implementation and acceptance](docs/IMPLEMENTATION.md): build order, readiness and two-app scenario.
- [Verification mail](docs/MAIL.md): worker configuration, local inbox and delivery guarantees.
- [Browser foundations](docs/BROWSER.md): signed cookies, CSRF, hidden fields and integration boundaries.
- [Agent guidance](AGENTS.md): working in this repository.

The first milestone requires a verified account to sign into two backend apps
through one Crow browser session without a second credential prompt. Recovery,
deletion, app ownership and failure cases are specified; all remain planned.
