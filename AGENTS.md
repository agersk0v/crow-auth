# Instructions for coding agents

## Project context

Crow Auth is an API-first identity and subscription service shared by connected
applications. One immutable Crow user UUID identifies a person across apps.
Connected applications own their business data and associate it with that UUID;
they do not need separate local account registration.

The agreed stack is Rust/Axum/SQLx with PostgreSQL, a Next.js hosted frontend,
Nginx providing one public origin, a Go refresh-token service backed by Redis,
and Stripe subscriptions. This describes the target system, not the components
already implemented. Inspect the checkout before assuming a service exists.

The owner delegates implementation to agents and reviews their pull requests;
do not require the owner to write code.

## Read before working

- Read the assigned issue, acceptance criteria, and relevant discussion.
- Read [README](README.md) for existing setup and check commands.
- Read [PRODUCT](docs/PRODUCT.md) for agreed behavior, scope, and open decisions.
- Read [ARCHITECTURE](docs/ARCHITECTURE.md) for technical design and security rules.
- Inspect applicable agent instructions, the repository, and existing PRs before
  making changes. Distinguish implemented behavior from plans and recommendations.

Follow the owner's current instructions and approved product requirements when
a technical proposal conflicts with them. Do not silently turn a recommendation
or unresolved product choice into an approved requirement. Resolve routine
implementation details using the documented scope and sound judgment. Ask for
a decision only when missing information materially affects correct behavior;
continue independent authorized work while it is unresolved.

## Branch and pull request workflow

1. Confirm scope, intended base, current changes, and existing PRs to avoid
   duplicated work. Preserve unrelated user work and local commits.
2. Create a focused feature branch from the intended base. Use the existing
   checkout; cloud tasks are already isolated, so do not create a Git worktree
   unless the owner requests one.
3. Implement the authorized scope. Feature-branch commits and pushes are allowed
   for that work. Avoid unrelated refactoring, dependencies, or infrastructure.
4. Update the documentation that owns any changed behavior or contract.
5. Review the complete diff and run appropriate checks. Fix relevant failures
   and explain anything that remains blocked or unverified.
6. Open a PR against the intended base, linking the issue when one exists,
   normally with `Closes #<number>`. Do not invent an issue number.
7. Explain the resulting behavior, material decisions, validation, and limitations.
   Leave the PR open for the owner's review and address revisions on its branch.

**Never merge PRs, enable auto-merge, or commit/push directly to `main`. The owner
handles merging.** Do not change repository permissions or branch protection as
part of ordinary implementation work. Avoid rewriting shared branch history
unless explicitly authorized.

When work depends on an open PR, state the dependency and choose a base that
keeps the diff reviewable. Retarget/reconcile the dependent PR after the base
merges when continuing that work; do not merge dependencies yourself.

These instructions describe the workflow, not a security boundary. Repository
permissions and rules must separately enforce any desired restrictions.

## Implementation and security

Use PRODUCT and ARCHITECTURE as the source of detailed policies rather than
maintaining a competing copy of token lifetimes, credential limits, or billing
rules here.

- Keep user/application policy and access-token signing in the Rust API, hosted
  pages in Next.js, and refresh-token state handling in Go/Redis as designed.
- Enforce authentication, application ownership, and audience boundaries on the
  server. A public client ID, browser origin, or UI restriction is not authorization.
- Use the agreed hosted code/PKCE flow for browser integrations. Do not introduce
  JWTs in callback URLs, arbitrary callbacks, browser client secrets, or SSO.
- Never store plaintext passwords, raw refresh tokens, or recoverable client
  secrets where the design calls for hashes/verifiers. Keep secrets out of Git,
  frontend bundles, error responses, logs, fixtures, and saved instructions.
- Preserve TLS, signature, and checksum verification. Do not bypass validation
  or security assertions to make a command or test pass.
- Do not invent billing states, paid feature restrictions, account-recovery
  behavior, or other unresolved product decisions to complete unrelated work.
- Add only the services and abstractions needed for the current issue. A planned
  microservice or frontend is not permission to implement it in an unrelated PR.

## Data care

- Use separate disposable PostgreSQL and Redis instances/state for tests. Never
  reset, migrate experimentally, overwrite, or delete the owner's data to validate
  a change. Clean up only resources created for that validation.
- Preserve stable user IDs and existing application relationships. Do not change
  identity semantics or invalidate sessions as an incidental refactor.
- Once migrations exist, add new versioned migrations rather than modifying
  already-applied SQL. Review compatibility and data preservation explicitly.
- Do not use real payments or change live Stripe subscriptions for routine tests.
  Use Stripe test mode or controlled test fixtures when billing is implemented.
- Treat Redis refresh state as security-sensitive. Do not restore stale state
  that could resurrect consumed or revoked tokens.

## Validation and documentation

Choose checks appropriate to the change and run them in their documented working
directories. Current Rust checks from the repository root are:

```sh
cargo fmt --check
cargo check --locked
cargo build --locked
cargo test --locked
```

These commands cover the existing Rust crate, not future Next.js, Go, or complete
authentication flows. Follow README setup instructions; document new working
commands as tooling is added. Do not invent commands, claim unexecuted checks
passed, or treat a zero-test run as functional coverage.

For implementation changes, verify affected behavior and meaningful failure,
authorization, concurrency, and data-integrity cases. Database or refresh-token
changes need tests against the relevant disposable services when mocks would
miss their constraints or atomic behavior. Add end-to-end checks as public flows
are implemented. Never disable assertions or hide failures to obtain a passing run.

For documentation-only changes, check accuracy, local links, examples, formatting,
and file scope. Application tests generally do not apply. Report passed, failed,
skipped, and unrun checks accurately, with concrete blockers where needed.

PRODUCT owns behavior and exclusions; ARCHITECTURE owns technical contracts and
security; README owns working setup and operations; this file owns agent workflow.
Update the owning section instead of appending issue histories or duplicating
policies across files. An open PR is not an implemented release. When an issue
implements planned work, update the baseline/status in the relevant documents so
they remain accurate when that PR merges.

Keep handoffs concise: what changed, why, how it was verified, and any remaining
decision, dependency, or limitation.
