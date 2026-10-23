# crow-auth

Rust HTTP service using Axum and Tokio. Authentication is not implemented yet.

## Development

Install a stable Rust toolchain, then run:

```sh
cargo run --locked
```

The server listens on `127.0.0.1:3000`. `GET /health` returns `ok`.

```sh
cargo check --locked
curl --fail http://127.0.0.1:3000/health
```
