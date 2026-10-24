# Working on Crow Auth

Read README.md for implemented status and commands, docs/PRODUCT.md for behavior,
docs/ARCHITECTURE.md for security enforcement and docs/API_CONTRACT.md for protocol
details. docs/IMPLEMENTATION.md tracks bounded milestones. Planned is not implemented.

Decision labels used throughout: **Accepted** is chosen planned behavior;
**Proposed** is a candidate shape; **Unresolved** needs a decision before its
stated deadline; **Deferred** is outside the current release. None proves implementation.

- Standards requirements are mandatory. Each document owns the subject above.
  Flag conflicting requirements with both sources and a recommended resolution;
  continue independent work. Do not silently choose or invent behavior.
- Implement the requested milestone only. Deferred features are not implicitly
  authorized work. Resolve feature-blocking design questions before coding.
- Keep HTTP parsing/responses, domain rules and persistence separate as they grow;
  avoid creating empty layers or extra services in advance.
- Update affected contracts and milestone evidence with behavior changes. Keep each
  rule authoritative in one place and link to it elsewhere.
- Add meaningful verification for security behavior, persistence and races. Never
  mark a milestone verified merely because code compiles or unit tests pass.
- Never commit secrets or log credentials. Use maintained protocol/crypto libraries.

For Rust changes run cargo fmt --check, cargo check --locked,
cargo clippy --locked --all-targets -- -D warnings and cargo test --locked.
Run feature-specific database/integration checks where relevant; report checks
that could not run. For documentation-only changes check links, consistency and
git diff --check; Rust tests are unnecessary.
