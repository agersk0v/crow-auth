# Implementation milestones

Build order and completion evidence for the planned design. See
[AGENTS.md](../AGENTS.md) for document ownership, decision labels and verification rules.

## Current status

Rust startup, PostgreSQL connectivity and HTTP liveness are implemented.
Startup tests exist; successful database startup/HTTP smoke verification still
needs recorded evidence. All authentication milestones below are planned.

## First integration milestone: two example apps

**Accepted; planned, not implemented.** Run Crow and two separate backend example
apps with distinct client IDs, secrets, callback URLs and local session cookies.
Use a standard OIDC client library in each app and document reproducible setup
without committing credentials. Same-sector clients may share a pairwise subject;
different sectors must receive different subjects for the same Crow account.

Completion evidence must demonstrate:

1. Register one account, verify its email, then begin sign-in from app A in a clean
   browser. Crow obtains required consent; A exchanges the code using client
   authentication and PKCE S256, validates the ID token and creates a local session.
2. Begin sign-in from app B in the same browser. Crow reuses its valid session
   without another credential entry, obtains B's consent and creates B's separate
   local session through its own code exchange. No Crow passwords reach either app.
3. Log out of A: A's local session ends; B and Crow remain signed in. Signing back
   into A can reuse Crow SSO. Log out of Crow: subsequent authorization requires
   authentication, while existing A/B local sessions remain governed by those apps.
4. Reject missing/wrong PKCE, wrong client or callback, expired/replayed codes,
   invalid state/nonce and disabled apps. A cannot redeem B's code or manage B.
5. Record browser walkthrough and integration/security checks with observed results.
   Compilation alone does not establish milestone completion.

Recovery and deletion remain required for the first release even if implemented
after this integration demonstration. Production readiness is a separate gate.

## Bounded milestones

| Milestone | Required completion evidence |
| --- | --- |
| Accounts and Crow browser sessions | Migrations and uniqueness; password-policy and bounded-hashing checks; session rotation, fixation, expiry/logout and CSRF/Origin checks; server-rendered forms and HTMX enforce identical controls |
| Verified email and recovery | Verification gate, single-use/purpose-bound expiring links, resend and delivery failures, generic recovery responses, reset concurrency and account-wide invalidation |
| Account deletion | Fresh authentication and confirmation; atomic account/app disablement and credential invalidation; ownership and race checks; purge and backup/restore suppression evidence |
| Owned developer apps | Ownership isolation; exact callback validation; once-only secret display, active-secret limit and rotation; disable/re-enable behavior, including permanent grant revocation and rejection of old access tokens after re-enable |
| Hosted authorization and consent | Browser-bound multi-tab state machine; SSO, freshness and silent errors; consent/denial; registration continuation; logout/account-change and callback-removal races |
| Discovery, exchange, tokens and UserInfo | Library responsibility record; stable isolated pairwise subjects; atomic exchange and bound replay revocation; rollback/uncertain-commit failures; ID signing/key overlap; opaque-access audience/scope enforcement, ID-token audience/nonce, max_age/auth_time freshness and token-substitution rejection; standard OIDC-client integration without refresh |
| Two-app SSO demonstration | All acceptance evidence in [the first integration milestone](#first-integration-milestone-two-example-apps) |
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
| **Unresolved:** template library, CSRF issuance and session rotation across tabs | Before browser implementation | Selected mechanics and session/flow race scenarios |
| **Unresolved:** schema constraints, isolation and lock order | Before each persistence feature | Migration/design review and relevant transaction/race checks |
| **Unresolved:** refresh duplicates, retries, theft and lost responses | Before deferred refresh implementation | Accepted Architecture policy, API outcomes and reference-backend scenarios |
| **Unresolved:** measured hashing costs, abuse/proxy limits, key caches and operational settings | Before public exposure | Recorded settings and applicable readiness/incident exercises |
| **Unresolved:** email normalization/uniqueness, token expiry/resend rules and delivery | Before email implementation | Accepted account and browser contracts, delivery and abuse scenarios |
| **Unresolved:** deletion confirmation, retention/purge and restore suppression | Before deletion implementation | Accepted lifecycle contract and revocation/purge evidence |
| **Unresolved:** password change/reset endpoints and account-wide invalidation mechanics | Before password-replacement implementation; required before public production | Accepted contracts, verified-email enrollment/reset design and concurrency/failure evidence |
| **Deferred:** billing, social login, MFA and other expanded contracts | Before their scheduled implementation | Accepted contracts in the owning documents |

Next bounded task: select suitable libraries, then design account/session
migrations and implement registration plus the shared browser session.
Do not implement the entire plan in one task.
