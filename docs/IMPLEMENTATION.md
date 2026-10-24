# Implementation milestones

Build order and completion evidence for the planned design. See
[AGENTS.md](../AGENTS.md) for document ownership, decision labels and verification rules.

## Current status

Rust startup, PostgreSQL connectivity, migrations and HTTP liveness are implemented.
The accounts/shared-browser-session milestone is implemented and verified for
prototype use. Developer apps and OAuth/OIDC remain planned; no discovery or
protocol compatibility is advertised. Public production gates remain open.

### Account/session evidence (2026-10-24)

- `cargo fmt --check`, `cargo check --locked`,
  `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked`
  passed (two password-policy/worker tests and two credential-safe startup tests).
- `TEST_DATABASE_URL=<disposable PostgreSQL URL> cargo test --locked --test browser
  -- --ignored` passed against PostgreSQL 17. The explicit database suite checks
  migration reruns, unique normalized usernames, same-browser and independent
  registration races, session fixation/rotation, original password bytes, ordinary
  and HTMX CSRF/Origin enforcement, cross-session CSRF rejection, absolute expiry,
  expiry after lock waits, logout versus authentication, rollback after injected
  session-insertion failure, persistence across router recreation, body limits and
  fail-closed database outages. Hash cost parameters and fresh salts are checked;
  measured cost tuning before exposure is still outstanding.
- An actual loopback HTTP process using the migrated PostgreSQL database passed
  `/health`, registration, account rendering, logout and stale-cookie rejection.
  The smoke client supplied the configured HTTPS Origin explicitly; TLS termination
  and a browser behind the production proxy have not been verified.
- Templates, session/CSRF mechanics, library responsibilities and transaction
  policy are recorded in
  [Architecture](ARCHITECTURE.md#accountsession-implementation-decisions).
  No hosted-flow races, password replacement or OIDC conformance are claimed.

## Bounded milestones

| Milestone | Required completion evidence |
| --- | --- |
| **Implemented/verified (prototype):** Accounts and Crow browser sessions | Migrations and uniqueness; password-policy and bounded-hashing checks; session rotation, fixation, expiry/logout and CSRF/Origin checks; server-rendered forms and HTMX enforce identical controls |
| Owned developer apps | Ownership isolation; exact callback validation; once-only secret display, active-secret limit and rotation; disable/re-enable behavior, including permanent grant revocation and rejection of old access tokens after re-enable |
| Hosted authorization and consent | Browser-bound multi-tab state machine; SSO, freshness and silent errors; consent/denial; registration continuation; logout/account-change and callback-removal races |
| Discovery, exchange, tokens and UserInfo | Library responsibility record; stable isolated pairwise subjects; atomic exchange and bound replay revocation; rollback/uncertain-commit failures; ID signing/key overlap; opaque-access audience/scope enforcement, ID-token audience/nonce, max_age/auth_time freshness and token-substitution rejection; standard OIDC-client integration without refresh |
| **Deferred:** subscription reads | After the first complete OIDC integration, independently of refresh and billing; scope/audience/grant enforcement, consent and discovery updates; non-subscriber result before Stripe |
| **Deferred:** refresh and revocation | Reviewed duplicate/retry design first; race, expiry and failure checks; disablement coordination; reference backend; non-disclosing revocation |
| Public production readiness | Secure verified-email recovery and authenticated password changes; atomic password replacement and account-wide session/flow/code/grant/family invalidation, including concurrency and rollback checks; abuse/proxy/request limits; deployment/readiness; restore and signing-key incident exercises; applicable protocol/conformance checks; credential-free logs |

Record implemented/verified status with evidence in the task or PR and update this
file as milestones complete. Each task should select the relevant scenarios;
this is not a requirement to finish the whole release before completing a task.

## Decision gates

The owning documents contain the requirements; this table records decision
status, deadline and the evidence needed to close each gate.

| Decision | Deadline | Completion evidence |
| --- | --- | --- |
| **Accepted:** Crow owns its identity-provider engine; use maintained libraries | Before each affected feature | Library responsibility record |
| **Accepted:** opaque access tokens; signed JWT ID tokens | Before token implementation | Updated contract and durable token/grant validation checks |
| **Accepted:** first complete OIDC integration excludes refresh/offline access | First integration milestone | Standard client walkthrough; truthful discovery; unsupported-scope checks |
| **Accepted for accounts/sessions:** Askama, browser-bound CSRF and strict rotation | Before browser implementation | Architecture decisions and browser suite above; hosted-flow race checks remain with authorization |
| **Accepted for accounts/sessions; unresolved for later features:** schema constraints, isolation and lock order | Before each persistence feature | Account/session migration and transaction/race evidence above; extend design before each later milestone |
| **Unresolved:** refresh duplicates, retries, theft and lost responses | Before deferred refresh implementation | Accepted Architecture policy, API outcomes and reference-backend scenarios |
| **Unresolved:** measured hashing costs, abuse/proxy limits, key caches and operational settings | Before public exposure | Recorded settings and applicable readiness/incident exercises |
| **Unresolved:** password change/reset endpoints and account-wide invalidation mechanics | Before password-replacement implementation; required before public production | Accepted contracts, verified-email enrollment/reset design and concurrency/failure evidence |
| **Deferred:** billing, social login, MFA and other expanded contracts | Before their scheduled implementation | Accepted contracts in the owning documents |

Next bounded task: owned developer apps. First document app/secret constraints,
callback-edit serialization and the lock order shared with later grants/exchange.
Do not implement the entire plan in one task.
