# Instructions for coding agents

Crow Auth is an API-first shared identity/subscription service. The owner delegates
implementation to agents and reviews their PRs; do not require the owner to code.
Planned components are not evidence that they exist in the checkout.

## Read and scope

- Read the issue/discussion and inspect current code, changes, and existing PRs.
- [README](README.md) owns setup/checks; [PRODUCT](docs/PRODUCT.md) owns behavior;
  [ARCHITECTURE](docs/ARCHITECTURE.md) owns technical/security rules. Follow these
  rather than copying their policies into this file.
- Follow current owner instructions. Keep unresolved decisions separate from
  approved requirements; resolve routine details yourself and ask only when a
  missing decision materially affects correctness. Continue independent work.
- Implement the authorized scope; avoid unrelated refactors, services, dependencies,
  or product decisions. Preserve user work, stable IDs, and application data.

## Branch and PR workflow

1. Confirm the base and scope; use a focused feature branch. Use the existing
   checkout, without a worktree unless requested; cloud tasks are already isolated.
2. Implement, update the owning documentation, review the diff, and run relevant
   checks. Feature-branch commits and pushes are permitted for authorized work.
3. Open a PR, link its issue (normally `Closes #<number>`), and report behavior,
   validation, limitations, and dependencies. Leave it open for owner review.
4. Address revisions on its branch. For dependent PRs, keep diffs focused and
   retarget/reconcile after the base merges when continuing the work.

**Never commit/push directly to `main`, merge PRs, or enable auto-merge.** The owner
handles merging. Do not change permissions/branch protection or rewrite shared
history without explicit authorization. These rules do not replace GitHub controls.

## Security and data care

- Enforce identity, ownership, callback, and audience rules server-side. Never
  bypass verification, introduce SSO, or weaken security to make checks pass.
- Keep secrets/credentials out of Git, bundles, logs, responses, and fixtures.
  Use hashes/verifiers where designed; preserve TLS/signature/checksum checks.
- Test against disposable PostgreSQL/Redis and Stripe test mode. Never overwrite
  owner data or use live payments; clean up only resources you created.
- Add migrations instead of changing applied ones. Preserve recorded data and
  do not restore Redis state that resurrects consumed/revoked tokens.

## Validation and handoff

Use real commands and relevant checks. Current Rust commands, from the root:

```sh
cargo fmt --check
cargo check --locked
cargo build --locked
cargo test --locked
```

Verify meaningful success, failure, authorization, concurrency, and data-integrity
cases for affected features; use real disposable services where mocks miss their
behavior. Add/document Next.js, Go, and integration checks when implemented.
Never hide failures, disable assertions, claim unrun checks passed, or count zero
tests as coverage. Report failures, skips, and unverified work accurately.

For docs-only work, check accuracy, links, examples, formatting, and scope;
application tests generally do not apply. Keep documents concise and update their
owning sections, not issue histories. Distinguish current code from planned work;
an open PR is not shipped. Handoff: what changed, why, checks, and remaining blockers.
