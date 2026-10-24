# Working on Crow Auth

## Purpose

Build a small OIDC provider for external developers that one maintainer can
operate. Preserve the external integration use case. Keep the supported surface
small rather than expanding toward a general authentication platform.

Read [README](README.md) for actual implementation status, then the owning
documents for the feature being changed:

- [Product](docs/PRODUCT.md): scope and user-visible behavior.
- [Architecture](docs/ARCHITECTURE.md): enforcement, storage and transactions.
- [API contract](docs/API_CONTRACT.md): protocol and endpoint behavior.
- [Integration](docs/INTEGRATION.md): client walkthrough.
- [Operations](docs/OPERATIONS.md): deployment and recovery.
- [Implementation](docs/IMPLEMENTATION.md): milestones and open decisions.

Accepted is a planned decision, Open is a decision gate, and Deferred is outside
the first release. Planned behavior is never evidence of working code.

## Change rules

1. Implement the requested bounded milestone. Do not add deferred features,
   speculative services, generic frameworks or empty abstraction layers.
2. Keep one service and one authoritative database. Separate HTTP handling,
   domain rules and persistence when real code needs that separation.
3. Use maintained protocol, cryptographic and password libraries. Evaluate actual
   provider-side coverage; an OAuth client library is not an OIDC server engine.
4. Standards and security requirements remain mandatory. Simplify features, not
   callback validation, session protection or credential handling.
5. Resolve blocking Open decisions before implementing the affected behavior.
   Flag document conflicts with both sources and a recommended resolution.
6. Keep each requirement authoritative in its owning document; link elsewhere.
   Update affected documentation and status when behavior changes.
7. Never commit or log secrets. Do not claim interoperability, production
   readiness or milestone completion without recorded evidence.

## Verification

For Rust changes run `cargo fmt --check`, `cargo check --locked`,
`cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked`.
Add meaningful feature checks for database constraints, ownership, protocol
bindings, expiry, replay and concurrency. Compilation alone is insufficient.
Report checks that could not run.

For documentation-only changes check relative links, section references,
implemented-versus-planned wording, cross-document consistency and
`git diff --check`. Rust tests are unnecessary when code is unchanged.
