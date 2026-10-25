# Crow Auth

A small identity provider for adding **Sign in with Crow** to independent apps
using OpenID Connect.

The service runs migrations and serves registration and verification-mail resend
forms. Registration creates a pending account and encrypted mail job atomically;
a separate worker sends the email. Browser confirmation and sign-in are not
available yet: emailed links return a safe temporary-unavailable page until the
next increment.

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
python3 scripts/provision-password-blocklist.py
# Export the browser/mail settings described below before starting:
cargo run --locked
```

Compose reads `.env`; `cargo run` does not load it. The URL above is a placeholder.
If you change `POSTGRES_PORT`, use the same port in `DATABASE_URL`.
Before startup, export `CROW_ORIGIN`, `BROWSER_SIGNING_KEY`, `MAIL_FROM`,
`MAIL_ENCRYPTION_KEY`, `SMTP_HOST`, `SMTP_PORT` and the appropriate SMTP/local-capture
settings from [mail operation](docs/MAIL.md). Provision separate random 32-byte
unpadded-base64url keys for browser signing and mail encryption; never use the
placeholder values. `PASSWORD_BLOCKLIST_PATH` defaults to `data/common-passwords.txt`.
Missing/invalid browser, mail or blocklist settings fail startup, including health.

The server listens over HTTP on `127.0.0.1:3000`. To use browser forms, put a trusted
local HTTPS terminator in front of it and set the exact external `CROW_ORIGIN`, for
example `https://crow.localhost:3443`. Secure cookies require HTTPS. The service does
not supply TLS termination yet; the reproducible HTTPS/browser fixture is the next
increment. Forwarding headers do not override Origin or transport IP; a proxy shares
its 20/hour IP budget across users until trusted-peer handling is implemented.
`GET /health` returns `ok`:

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
cargo test --locked --test throttle_database -- --ignored
# Mail suite also needs the capture service from this test stack:
export TEST_SMTP_PORT=1025
export TEST_MAILPIT_URL=http://127.0.0.1:8025
cargo test --locked --test mail_database -- --ignored
cargo test --locked --test lifecycle_database -- --ignored
cargo test --locked --test hosted_database -- --ignored
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

## Registration and account foundations

`GET /register` renders an escaped form. Its POST checks signed-cookie CSRF/Origin,
charges shared throttling, validates input and hashes the unchanged password with
one shared bounded Argon2id service. Account, verification action and encrypted mail
job commit together. Expired matching reservations are reclaimed in that transaction.
Duplicate emails return the same generic 303 instructions without changing the
existing account; unavailable usernames get a safe field error. No session is created.

`GET /verify-email` displays instructions and the resend form. Resend replaces
pending verification links without changing the password or 24-hour reservation.
Unknown, active and expired emails get the same generic response. A separate
`mail-worker` delivers committed jobs; `account-cleanup` removes expired pending
accounts and idle throttle buckets. See [mail setup](docs/MAIL.md).

Email-token confirmation and login remain unimplemented. Token-bearing GETs return
503 without consuming or echoing the token. The parent hosted-registration checklist
stays unchecked until the protected confirmation handler and HTTPS/browser tests land.

Provision and maintain the real blocklist as described in [dataset operation](data/README.md).
Never use the tiny test fixture in deployment. Four hashing workers and sixteen
waiting requests bound load; overload returns 503. These are prototype costs,
not production benchmarks. See [implementation evidence](docs/IMPLEMENTATION.md).

## Browser protections

Signed pre-auth cookies, action-bound CSRF, strict Origin checks, escaped Askama
forms and security headers/body limits protect the hosted routes. No browser-state
or confirmation-context table is needed. See [browser operation](docs/BROWSER.md).
Registration/resend share rolling PostgreSQL budgets of 5 per canonical email and
20 per transport IP per hour, with 429/Retry-After and fail-closed 503 responses.
See [throttling](docs/THROTTLING.md).

## Continuous integration

GitHub Actions runs formatting, compilation, Clippy, unit tests, account and
verification-action tests, encrypted-outbox/SMTP-capture and lifecycle race suites,
signed-cookie CSRF/verification, shared-throttling and hosted registration/resend
checks, and a PostgreSQL 17 startup/HTTP health/form smoke test on pull requests to `main`, pushes to `main`, and
merge-queue groups. The workflow can also be started manually. To reproduce the
smoke check locally with a disposable database (the script supplies public test
keys and the tiny fixture blocklist; it tests HTTP rendering, not browser HTTPS):

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
- [Shared throttling](docs/THROTTLING.md): rolling budgets, counting, storage and handler integration.
- [Agent guidance](AGENTS.md): working in this repository.

The first milestone requires a verified account to sign into two backend apps
through one Crow browser session without a second credential prompt. Recovery,
deletion, app ownership and failure cases are specified; all remain planned.
