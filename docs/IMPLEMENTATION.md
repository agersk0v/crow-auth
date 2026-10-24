# Implementation milestones

README owns current runnable behavior. Product owns scope and build order;
Architecture owns security acceptance checks. Status here distinguishes planned,
implemented and verified. Change status only with evidence in a task/PR.

| Milestone | Status | Completion evidence |
| --- | --- | --- |
| Rust startup, PostgreSQL connectivity, HTTP liveness | Implemented | Existing startup tests; successful database startup/HTTP smoke check still needs recorded evidence |
| Accounts and Crow browser sessions | Planned | Migrations; username/password rules; session rotation/expiry; CSRF and bounded hashing tests |
| Owned developer apps | Planned | Ownership isolation, exact callbacks, secret rotation and disablement tests |
| Hosted authorization and consent | Planned | Browser-bound multi-tab flow state machine; SSO/freshness; consent; callback-removal races |
| Discovery, code exchange, tokens and UserInfo | Planned | Provider/library decision; protocol requests/errors; atomic exchange/replay; standard OIDC client integration |
| Refresh, revocation and subscription reads | Planned | Race/failure/expiry tests; disablement; reference backend refresh coordination; non-subscriber result |
| Public production readiness | Planned | Email recovery; external-subject privacy review; abuse controls; deployment, restore and key-incident exercises; applicable conformance checks |

Before each feature, resolve its schema constraints, transaction/lock order and
protocol ambiguities. Before public production, choose measured Argon2 costs,
numeric limits, proxy trust, cache bounds and operational settings. Do not defer
these past their exposure point. Define billing, social login, MFA and other
deferred contracts when their milestones are scheduled. Add Redis, partitioning,
multi-region support or service extraction only after measurements justify them.

Next bounded task: assess maintained provider/library options, then design the
account/session migrations and implement registration plus the shared browser
session with its security checks. Do not implement the entire plan in one task.
