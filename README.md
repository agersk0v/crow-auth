# Crow Auth

**Sign in with Crow.** A small OpenID Connect provider that external developers
can integrate using standard libraries.

Crow aims to give users one account across independent apps and give developers
one predictable sign-in integration. It is designed to be maintainable by one
person: one Rust service, PostgreSQL, hosted pages and a small developer portal.

## Current implementation

The repository contains an Axum/Tokio HTTP skeleton with SQLx PostgreSQL
connectivity. Startup requires `DATABASE_URL`, opens a pool of up to five
connections, runs `SELECT 1`, and listens on `127.0.0.1:3000`.
`GET /health` returns `ok`; it is liveness, not database readiness.
Two startup tests cover missing configuration and credential-safe failure.
Accounts, pages, email delivery, OAuth/OIDC and developer management are **not
implemented**. The documents below specify the intended first release.

## First release

- Hosted registration, email verification, login and password recovery.
- One Crow browser session for portal access and cross-app sign-in.
- Self-service registration of external developers' apps.
- Authorization Code Flow with PKCE, discovery, signed ID tokens and UserInfo.
- Confidential web clients: every integrating app has a backend that keeps secrets.
- Minimal app settings: name, callbacks, client secrets and enabled state.

Billing, subscriptions, refresh tokens, native/browser-only clients, custom SDKs,
embedded widgets, organizations and upstream social login are outside this release.
Crow provides identity; each integrating app owns its users' application data,
authorization rules and local sessions.

## Documentation

| Document | Authority |
| --- | --- |
| [Local development](docs/DEVELOPMENT.md) | Run, check and troubleshoot the current skeleton |
| [Product](docs/PRODUCT.md) | Scope, user journeys and lifecycle behavior |
| [Architecture](docs/ARCHITECTURE.md) | Storage, trust boundaries and security enforcement |
| [API contract](docs/API_CONTRACT.md) | Supported protocol and browser routes |
| [Developer integration](docs/INTEGRATION.md) | Planned integration walkthrough and client responsibilities |
| [Operations](docs/OPERATIONS.md) | Deployment, backups and incident procedures |
| [Implementation](docs/IMPLEMENTATION.md) | Build order, open decisions and completion evidence |
| [Contributor instructions](AGENTS.md) | Change discipline and verification |

**Accepted** means chosen planned behavior. **Open** means a decision is still
required at the stated gate. **Deferred** means outside the first release.
None of these labels means implemented. Implementation status belongs in this
README and the milestone ledger.

## Local development

Start with the [development guide](docs/DEVELOPMENT.md) for a local PostgreSQL
container, configuration, smoke checks and troubleshooting. You need current stable Rust with edition 2024 support, PostgreSQL and `curl`; Docker is
optional if PostgreSQL is already available.

From a checkout with a running local database:

```sh
export DATABASE_URL='postgres://<user>:<url-encoded-password>@127.0.0.1:5432/crow_auth'
cargo run --locked
```

This connection string is a placeholder. Supply your own credentials, URL-encode
special characters and keep the real value out of source control. In another
terminal, `curl --fail http://127.0.0.1:3000/health` should return `ok`.

For contributor checks, see [verification](AGENTS.md#verification).
[Production configuration](docs/OPERATIONS.md#configuration-and-release) is
planned; the running skeleton does not accept those settings yet.
