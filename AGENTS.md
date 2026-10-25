# Working on Crow Auth

Read [README.md](README.md), [PRODUCT.md](PRODUCT.md) and
[ARCHITECTURE.md](ARCHITECTURE.md) before making changes.

- Keep changes focused on the request and the implementation simple.
- Clearly distinguish planned features from working code.
- Update documentation when behavior changes; keep it brief.
- Start each source file with a short comment explaining its purpose and
  responsibility. In Rust, use a module-level `//!` comment; keep script shebangs
  first. Keep existing useful summaries and avoid repeating the filename.
  Skip generated files, lockfiles and formats that do not support comments.
  Do not edit existing versioned migrations solely for comments; preserve checksums.
- Never commit or log secrets. Use maintained authentication and cryptography libraries.

For Rust changes, run `cargo fmt --check`, `cargo check --locked`,
`cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked`.
For documentation changes, check links, consistency and `git diff --check`.
Report any checks that could not run.
