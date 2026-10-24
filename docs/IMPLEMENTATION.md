# Implementation plan

## Current status

Implemented: Rust/Axum/Tokio startup, SQLx PostgreSQL pool, startup `SELECT 1`,
loopback listener and `/health`. Two tests cover missing database configuration
and credential-safe startup failure. Successful database/HTTP smoke evidence is
not recorded here. Everything below is planned; no authentication milestone is
complete. See [local development](DEVELOPMENT.md) for reproducible setup and checks.

## Build order

Build one bounded milestone at a time. Each includes its own security checks.
Keep the first public release limited to the [Product](PRODUCT.md) scope.

| Milestone | Deliverable | Required evidence |
| --- | --- | --- |
| 1. Accounts and browser sessions | Migrations, hosted registration/login, required email verification, shared session and logout | Unique normalized identifiers, password policy/bounded hashing, pending-account restrictions, single-use email links, delivery retries, CSRF/Origin, fixation/rotation/expiry, parallel tabs |
| 2. Recovery and account security | Forgotten password, password change and account-wide Crow invalidation | Generic responses, verified-address-only delivery, expiry/replacement/single use, atomic hash/version change, rollback and races with authentication/issuance |
| 3. Developer apps | Owned app list/create/edit, exact callbacks, two-secret rotation and disable/enable | Cross-owner/nested-ID isolation, callback parsing, immutable sector, once-only secret display, atomic quotas and lifecycle checks |
| 4. Hosted authorization | Browser-bound flows, SSO, consent, denial and code issuance | State machine transitions, tab isolation, registration continuation, prompt/freshness/silent errors, logout/account change and callback removal races |
| 5. Complete OIDC integration | Discovery, JWKS, atomic exchange, ID tokens and UserInfo | Independent OIDC client, PKCE/client/callback binding, one-time exchange, bound replay revocation, pairwise stability/isolation, audience/nonce/time checks, token substitution rejection, disabled/re-enabled app behavior |
| 6. Public operation | Deployable configuration, limits, readiness, recovery procedures and operator documentation | All public release gates below, measured settings, backup/restore drill and key rotation/incident drill |

Use development-only isolated environments before milestone 6. Do not expose a
partially secured prototype as a public identity service. There is no billing,
subscription or refresh milestone in this plan.

## Open decisions

These are implementation choices, not invitations to expand scope. Record each
choice in its owning document before implementing the affected feature.

| Decision | Deadline | Evidence |
| --- | --- | --- |
| Provider-side library coverage and retained responsibilities | Before account/protocol implementation as applicable | Maintained library assessment; no client-library/provider assumption |
| Template engine and email transport/provider | Before milestone 1 | Escaping, delivery and operational support assessment |
| Schema, constraints, isolation and lock order | Before each persistence feature | Migration review and transaction/race scenarios |
| Email-job encryption, claim/retry mechanics and pending-account retention | Before milestone 1 | No plaintext durable bearer payloads; replacement/retry tests |
| Concrete numeric quotas, hash costs and abuse/proxy limits | Before public exposure of each affected route | Measurements, atomic enforcement and failure policy |
| Production config names, deployment target and migration runner | Before milestone 6 | Validated deployment walkthrough and secret provisioning |
| Recovery targets, support/deletion policy and audit retention | Before public release | Recorded policies and successful operator drills |

## Public release gates

- All first-release user and developer journeys work with real PostgreSQL and a
  tested delivery path, including recovery and password change.
- An independent standard OIDC client completes discovery, authorization, exchange,
  ID-token validation and UserInfo without a custom SDK or refresh support.
- Applicable OIDC/OAuth conformance and negative checks pass; record the suite,
  version, configuration and any limitations. Do not claim certification without it.
- Race checks cover duplicate exchange, password replacement, app disablement,
  callback removal, secret limits and email-credential consumption. Re-enabling an
  app cannot revive tokens. Transaction uncertainty never returns credentials.
- Missing/wrong issuer, audience, signature, nonce, PKCE, scope and security version
  are rejected. Freshness checks include missing/stale/future `auth_time` and zero max age.
- Numeric operational settings, resource bounds, trusted proxies and safe logs are
  recorded. Dependency/limiter outages fail closed where authorization depends on them.
- Normal/emergency signing rotation and an isolated restore drill succeed. Restored
  revoked credentials cannot become usable; subject mappings remain stable.
- Public discovery and developer docs match verified capabilities. Recovery,
  logout limitations, support and privacy behavior are documented truthfully.

## Recording progress

For each milestone record changed behavior, commands/scenarios, environment,
results and remaining limitations in the task or PR. Update this ledger and the
README only when implementation exists. Passing compilation is not evidence of
protocol security or production readiness.

Next bounded task: evaluate the account/session/template/email libraries, choose
the milestone 1 persistence mechanics and implement accounts with verified-email
activation and protected browser sessions. Do not implement all milestones in one
change.
