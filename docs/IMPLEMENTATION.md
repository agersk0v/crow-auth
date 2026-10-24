# Implementation milestones

[README](../README.md) owns runnable status; Product owns scope and lifetimes,
Architecture owns enforcement, and API contract owns wire behavior.
The evidence below verifies those requirements; it does not redefine them.
Planned does not mean implemented, and compilation alone is not verification.

## Current status

Rust startup, PostgreSQL connectivity and HTTP liveness are implemented.
Startup tests exist; successful database startup/HTTP smoke verification still
needs recorded evidence. All authentication milestones below are planned.

## Bounded milestones

| Milestone | Required completion evidence |
| --- | --- |
| Accounts and Crow browser sessions | Migrations and uniqueness; password-policy and bounded-hashing checks; session rotation, fixation, expiry/logout and CSRF/Origin checks; server-rendered forms and HTMX enforce identical controls |
| Owned developer apps | Ownership isolation; exact callback validation; once-only secret display, active-secret limit and rotation; disable/re-enable behavior |
| Hosted authorization and consent | Browser-bound multi-tab state machine; SSO, freshness and silent errors; consent/denial; registration continuation; logout/account-change and callback-removal races |
| Discovery, exchange, tokens and UserInfo | Library responsibility record; stable isolated pairwise subjects; atomic exchange and bound replay revocation; rollback/uncertain-commit failures; signing/key overlap; audience/nonce/scope and ID/access substitution rejection; standard OIDC-client integration |
| Refresh, revocation and subscription | Reviewed duplicate/retry design first; race, expiry and failure checks; disablement coordination; reference backend; non-disclosing revocation; non-subscriber result |
| Public production readiness | Secure verified-email recovery; abuse/proxy/request limits; deployment/readiness; restore and signing-key incident exercises; applicable protocol/conformance checks; credential-free logs |

Record implemented/verified status with evidence in the task or PR and update this
file as milestones complete. Each task should select the relevant scenarios;
this is not a requirement to finish the whole release before completing a task.

## Decision gates

Before each feature, resolve schema constraints, transaction isolation/lock order
and protocol ambiguities. Before browser implementation, select the template
library and CSRF mechanics. Before protocol implementation, assess maintained
libraries and their coverage; Crow owns the engine.

Before refresh implementation, settle duplicate handling, retries, theft response
and ambiguous/lost responses as specified in Architecture. Strict duplicate-based
family revocation is not a requirement, and no replacement is implicitly approved.

Before public exposure, set measured Argon2 costs, password blocklist maintenance,
numeric abuse limits, proxy trust, cache bounds and operational settings.
Billing, social login, MFA and other deferred contracts can wait until scheduled.

Next bounded task: select suitable libraries, then design account/session
migrations and implement registration plus the shared browser session.
Do not implement the entire plan in one task.
