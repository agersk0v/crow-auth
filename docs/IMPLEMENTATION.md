# Implementation milestones

[README](../README.md) owns runnable status; Product owns scope and lifetimes,
Architecture owns enforcement, and API contract owns wire behavior.
The evidence below verifies those requirements; it does not redefine them.
Planned does not mean implemented, and compilation alone is not verification.

## Current status

Rust startup, PostgreSQL connectivity and HTTP liveness are implemented.
Startup tests exist; successful database startup/HTTP smoke verification still
needs recorded evidence. All authentication milestones below are planned. Decision labels are defined in
[AGENTS.md](../AGENTS.md); evidence of implementation is separate from acceptance.

## Bounded milestones

| Milestone | Required completion evidence |
| --- | --- |
| Accounts and Crow browser sessions | Migrations and uniqueness; password-policy and bounded-hashing checks; session rotation, fixation, expiry/logout and CSRF/Origin checks; server-rendered forms and HTMX enforce identical controls |
| Owned developer apps | Ownership isolation; exact callback validation; once-only secret display, active-secret limit and rotation; disable/re-enable behavior |
| Hosted authorization and consent | Browser-bound multi-tab state machine; SSO, freshness and silent errors; consent/denial; registration continuation; logout/account-change and callback-removal races |
| Discovery, exchange, tokens and UserInfo | Library responsibility record; stable isolated pairwise subjects; atomic exchange and bound replay revocation; rollback/uncertain-commit failures; ID signing/key overlap; opaque-access audience/scope enforcement, ID-token audience/nonce and token-substitution rejection; standard OIDC-client integration without refresh |
| **Deferred:** refresh, revocation and subscription | Reviewed duplicate/retry design first; race, expiry and failure checks; disablement coordination; reference backend; non-disclosing revocation; non-subscriber result |
| Public production readiness | Secure verified-email recovery; abuse/proxy/request limits; deployment/readiness; restore and signing-key incident exercises; applicable protocol/conformance checks; credential-free logs |

Record implemented/verified status with evidence in the task or PR and update this
file as milestones complete. Each task should select the relevant scenarios;
this is not a requirement to finish the whole release before completing a task.

## Decision gates

The owning documents contain the requirements; this table records decision
status, deadline and the evidence needed to close each gate.

| Decision | Deadline | Completion evidence |
| --- | --- | --- |
| **Accepted:** Crow owns its identity-provider engine; use maintained libraries | Before each affected feature | Library responsibility record and scoped dependency approval |
| **Accepted:** opaque access tokens; signed JWT ID tokens | Before token implementation | Updated contract and durable token/grant validation checks |
| **Accepted:** first complete OIDC integration excludes refresh/offline access | First integration milestone | Standard client walkthrough; truthful discovery; unsupported-scope checks |
| **Unresolved:** template library, CSRF issuance and session rotation across tabs | Before browser implementation | Selected mechanics and session/flow race scenarios |
| **Unresolved:** schema constraints, isolation and lock order | Before each persistence feature | Migration/design review and relevant transaction/race checks |
| **Unresolved:** refresh duplicates, retries, theft and lost responses | Before deferred refresh implementation | Accepted Architecture policy, API outcomes and reference-backend scenarios |
| **Unresolved:** measured hashing costs, abuse/proxy limits, key caches and operational settings | Before public exposure | Recorded settings and applicable readiness/incident exercises |
| **Deferred:** billing, social login, MFA and other expanded contracts | Before their scheduled implementation | Accepted contracts in the owning documents |

Next bounded task: select suitable libraries, then design account/session
migrations and implement registration plus the shared browser session.
Do not implement the entire plan in one task.
