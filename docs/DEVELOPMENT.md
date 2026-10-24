# Local development

This guide runs the **implemented skeleton**, not the planned identity provider.
For deployment requirements see [Operations](OPERATIONS.md); for future features
see the [implementation plan](IMPLEMENTATION.md).

## Prerequisites

- A stable Rust toolchain with edition 2024 support (Rust 1.85 or newer). Use the
  current stable release; the repository does not declare or test a minimum
  supported Rust version for its full dependency graph.
- A running PostgreSQL instance and credentials for an existing database.
- `curl` for the HTTP smoke check.
- Docker only if you use the disposable database example below.

Clone the repository and run subsequent commands from its root:

```sh
git clone https://github.com/agersk0v/crow-auth.git
cd crow-auth
```

## Start a disposable database

Skip this section if you already have a local PostgreSQL database. The example
uses PostgreSQL 17, binds only to loopback and stores no persistent volume. These
commands require Bash; choose a password for this local database when prompted.

```bash
read -rsp 'Local PostgreSQL password: ' POSTGRES_PASSWORD; echo
export POSTGRES_PASSWORD
docker run --name crow-auth-postgres --rm -d \
  -e POSTGRES_USER=crow_auth -e POSTGRES_PASSWORD -e POSTGRES_DB=crow_auth \
  -p 127.0.0.1:5432:5432 postgres:17
unset POSTGRES_PASSWORD
docker exec crow-auth-postgres pg_isready -U crow_auth -d crow_auth
```

Repeat the last command until PostgreSQL reports that it accepts connections.
`pg_isready` checks server availability; starting Crow below also checks the
actual connection credentials. If the container exits, inspect
`docker logs crow-auth-postgres` while it still exists. `--rm` removes it on exit.

## Configure and run

Set `DATABASE_URL` in the terminal that will start Crow. Replace the placeholders
with your local username and password; use `crow_auth` for both username and
database with the container above. Percent-encode special characters in username
and password, such as `@` as `%40`. Never commit the resulting connection string.

```sh
export DATABASE_URL='postgres://<user>:<url-encoded-password>@127.0.0.1:5432/crow_auth'
cargo run --locked
```

The service reads the process environment; it does not load a `.env` file.
No migrations, account tables, signing keys or email configuration are needed
for the skeleton. `--locked` keeps Cargo from changing the dependency lockfile.

Current configuration and behavior are defined in [main.rs](../src/main.rs):

| Setting | Current behavior |
| --- | --- |
| `DATABASE_URL` | Required PostgreSQL connection string; missing or invalid configuration stops startup |
| Database pool | At most five connections; five-second pool acquisition timeout |
| Startup check | Connect to PostgreSQL, then execute `SELECT 1` before binding HTTP |
| HTTP listener | Fixed `127.0.0.1:3000`; no port or bind-address setting |
| HTTP routes | `GET /health` returns HTTP 200 with plain text `ok` |

There is no ready/startup log message. Use the HTTP check below to confirm that
the listener is available. The listener uses local HTTP; production HTTPS and
proxy behavior remain planned. See [Operations](OPERATIONS.md#deployment-target).

## Verify the running service

Leave `cargo run` running and open another terminal:

```sh
curl --fail --show-error --include http://127.0.0.1:3000/health
```

Expect HTTP 200 and body `ok`. `/health` is liveness only: it does not query the
database on each request. A successful response cannot establish ongoing database
health, authentication support or production readiness. `/login` and the planned
OAuth/OIDC routes do not exist yet.

## Contributor checks

For Rust changes, run from the repository root:

```sh
cargo fmt --check
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Install the `rustfmt` and `clippy` toolchain components if missing. The current
[startup tests](../tests/startup.rs) supply/remove their own `DATABASE_URL`; they
do not need a running database. They cover startup rejection and credential-safe
errors, not successful database connectivity. Record the separate HTTP/database
smoke check when verifying that path. Planned features need the additional
[feature evidence](IMPLEMENTATION.md#build-order), not just these commands.

For documentation-only changes, check relative paths and heading anchors, compare
claims with code and the owning documents, then run `git diff --check`. Rust tests
are unnecessary when executable behavior is unchanged.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| `DATABASE_URL must be set` | Export it in the same terminal that runs Crow; a `.env` file alone is not loaded |
| `Failed to connect to PostgreSQL` | Confirm server availability, database existence, credentials, host/port and URL encoding; Crow deliberately omits the detailed connection error |
| `PostgreSQL startup connectivity check failed` | Confirm the database can execute `SELECT 1`; inspect local database diagnostics without sharing credentials |
| Address already in use | Stop the process using `127.0.0.1:3000`; changing a `PORT` variable has no effect |
| `curl` connection refused | Check whether Cargo is still compiling or startup exited; run curl on the same host as Crow |
| Docker name or port conflict | Check for an existing `crow-auth-postgres` container or PostgreSQL listener before starting another; do not delete an unrelated database |

The loopback listener is not reachable from another machine or a separate
container through that machine/container's own `127.0.0.1`. Changing the
PostgreSQL container's published port also requires changing `DATABASE_URL`.

## Stop and clean up

Stop Crow with Ctrl-C in its terminal. If you used the disposable database:

```sh
docker stop crow-auth-postgres
unset DATABASE_URL
```

Stopping this `--rm` container deletes its database contents. Do not use it for
data you need to preserve. Database volumes, backups and graceful service
shutdown are separate [operations requirements](OPERATIONS.md).
