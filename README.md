# Crow Auth

A small identity provider for adding **Sign in with Crow** to independent apps
using OpenID Connect.

The current implementation is a Rust HTTP skeleton with PostgreSQL connectivity
and a health endpoint. Authentication is not implemented yet.

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
# Run database checks against TEST_DATABASE_URL when available.
docker compose -f compose.test.yaml -p crow-auth-test down
```

The test stack uses a separate port, database, credentials and project, with
PostgreSQL data held in tmpfs. Stopping its container discards its data. Use
`TEST_POSTGRES_PASSWORD` from `.env` in the test URL, and match any custom
`TEST_POSTGRES_PORT`. Run the test Compose file alone, not as an override of the
development file. The current Rust tests only check startup configuration; no
application database integration suite is implemented yet. To smoke-test the
service against this database, set `DATABASE_URL` to the test URL for that run.

## Continuous integration

GitHub Actions runs formatting, compilation, Clippy, tests and a PostgreSQL 17
startup/HTTP health smoke test on pull requests to `main`, pushes to `main`, and
merge-queue groups. The workflow can also be started manually. To reproduce the
smoke check locally with a disposable database:

```sh
DATABASE_URL="$TEST_DATABASE_URL" bash .github/scripts/smoke-test.sh
```

In a GitHub ruleset for `main`, require the `Rust checks` status check and pull
requests before merging. Adding the workflow does not configure that repository
setting. No application code changes are included with this CI setup.

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
deletion, app ownership and failure cases are specified; all remain planned.
