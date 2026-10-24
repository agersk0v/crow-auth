# Crow Auth

A small identity provider for adding **Sign in with Crow** to independent apps
using OpenID Connect.

The current implementation is a Rust HTTP skeleton with PostgreSQL connectivity
and a health endpoint. Authentication is not implemented yet.

## Run locally

With stable Rust supporting edition 2024 and a running PostgreSQL database, set
`DATABASE_URL` to your local connection string, then run:

```sh
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
deletion, app ownership and failure cases are specified; all remain planned.
