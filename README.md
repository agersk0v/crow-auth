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
- [Agent guidance](AGENTS.md): working in this repository.
