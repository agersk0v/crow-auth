# Working on Crow Auth

Read README.md for implemented status and commands, docs/PRODUCT.md for behavior,
docs/ARCHITECTURE.md for security enforcement and docs/API_CONTRACT.md for protocol
details. docs/IMPLEMENTATION.md tracks bounded milestones. Planned is not implemented.

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
- Dependency changes require explicit user approval before editing manifests or
  lockfiles or running dependency-add/update commands. This includes adding,
  replacing, removing or upgrading direct packages, enabling new features, and
  introducing package managers, frontend frameworks or external providers.
  First explain the purpose, alternatives and maintenance/security tradeoffs;
  recommendations and evaluation tasks alone do not authorize installation.
  Transitive dependencies brought in by an approved change need not be approved
  individually, but disclose material additions. Do not hand-roll cryptography or
  security protocols to bypass this rule; report a blocked dependency decision.

For Rust changes run cargo fmt --check, cargo check --locked,
cargo clippy --locked --all-targets -- -D warnings and cargo test --locked.
Run feature-specific database/integration checks where relevant; report checks
that could not run. For documentation-only changes check links, consistency and
git diff --check; Rust tests are unnecessary.
