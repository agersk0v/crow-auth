# Architecture

[PRODUCT](PRODUCT.md) owns behavior; [README](../README.md) owns working setup.
This document owns technical design and security. Only the baseline is implemented;
the remaining sections describe agreed plans or explicitly labeled recommendations.

## Implemented baseline

The root Rust crate runs Axum/Tokio from `src/main.rs` on `127.0.0.1:3000`.
SQLx reads `DATABASE_URL`, creates a pool (maximum five connections; five-second
acquisition timeout), and runs `SELECT 1` before HTTP startup. The pool is Axum
state. Missing configuration/connection failures stop startup without exposing
credentials. `GET /health` returns `200 ok`, independently of ongoing DB health.
`tests/startup.rs` checks missing/malformed configuration and password redaction.
There are no migrations, users, frontend, Go/Redis, or billing components yet.

## Components and data ownership

| Component | Responsibility |
| --- | --- |
| Nginx | One public TLS origin; route pages to Next.js and a distinct API prefix to Rust |
| Next.js | Hosted login, registration, account, and developer pages; calls Rust APIs |
| Rust / Axum / SQLx | Credentials, app ownership/callbacks, code exchange, JWT signing, account APIs, Stripe |
| PostgreSQL | Durable users, application registrations/callbacks, subscription records |
| Go microservice | Refresh-token issuance, validation, rotation, reuse detection, and revocation |
| Redis | Refresh-token/session-family state owned by Go |
| Stripe | Payments/subscriptions; Crow stores relevant external IDs and local status |

Keep service source in this repository. Go/Redis, PostgreSQL, and internal APIs
are private. Authenticate Rust-to-Go calls; network location alone is not trust.
Rust owns identity/application policy and access-token signing. Next.js does not
implement a second authentication authority. Trust proxy headers only from Nginx.
Runtime secrets never enter Git, browser bundles, or logs.

Store immutable user IDs as PostgreSQL UUIDs and serialize them as UUID strings
in JWT `sub` and APIs. Apps store their business data against this ID. Logical
records are users (username/hash), applications (client ID/owner/name), callbacks,
and subscriptions (Crow/Stripe association). Supporting code-transaction and
webhook-event records need schemas when implemented. No email is initially required.
Recommended migrations are forward-only, versioned, transactional, and serialized
across concurrent starts; tooling, service layout, and runtime versions remain open.

## Credentials and applications

| Policy | Agreed rule |
| --- | --- |
| Username | 3–32 ASCII letters, digits, or underscores; case-insensitive uniqueness enforced in PostgreSQL |
| Password | 15–128 characters; spaces allowed; no composition requirements; never silently trim/truncate |
| Password storage | Argon2id with unique salts; benchmark parameters before implementation |
| Backend login | Client ID plus randomly generated client secret; browser apps use hosted PKCE login |
| Secret rotation | At most two active secrets; issue replacement, deploy it, explicitly revoke old secret |

Store client-secret verifiers rather than recoverable secrets; show new secrets
only at issuance. Secret management requires app ownership. Client IDs, CORS,
origins, and referrers do not authenticate clients. Define bounded hashing work,
request limits, account/IP rate limits, and consistent password character counting.
Return generic invalid-login errors. Never log credentials, bearer tokens, codes,
connection strings, or secrets; preserve TLS/signature/checksum verification.

## Authentication flow

**Crow itself:** `/login` with no application parameters leads to `/account`.
Use Secure, HttpOnly cookies with an 8-hour server-enforced maximum from sign-in.
Define cookie scope/SameSite, CSRF defenses, and session storage before implementation.
This session authorizes Crow account/developer pages, not external application
sign-in. Connected-app tokens cannot authorize developer-management operations.

**Hosted apps:** the client retains state and a PKCE verifier, then opens Crow
with client ID, callback, state, and S256 challenge. Rust validates an exact
registered callback before authentication; reject incomplete/invalid parameters
without redirecting. Require HTTPS in production; define a loopback-only HTTP
development exception. No wildcards, suffix matching, or trust in request origin.

Preserve a server-validated transaction through login/registration and revalidate
it on submission. Every new app authorization requires credentials; no SSO.
Return a short-lived code plus state to the callback, never access/refresh tokens.
Bind the code to user, client, callback, and PKCE challenge. The client validates
state; exchange verifies PKCE and atomically consumes the code. Reject expiry,
reuse, or mismatches. Use unpredictable codes and hashed stored verifiers.

**Direct API:** authenticated backends submit user credentials and app details;
return tokens as JSON without a callback. Hosted login keeps passwords away from
external apps; even authenticated backends handling passwords must be trusted.
Exact API paths/payloads, error formats, code expiry, and secret transport remain
implementation contracts. Token/credential responses must not be cached. Do not
claim OAuth/OIDC compliance without implementing and testing the relevant standards.

## Tokens, refresh, and logout

- Rust signs asymmetric JWTs. Include `iss`, UUID `sub`, intended app `aud`,
  username, `iat`, and `exp = iat + 600`. APIs validate an allowlisted algorithm,
  signature, issuer, audience, and expiry. Never accept another app's audience.
- Publish public verification keys with key IDs; keep private keys server-side.
  Algorithm, key storage/rotation overlap, and clock tolerance remain open.
- Subscriber status is absent from JWTs. An authenticated account API returns
  UUID, username, and current locally recorded subscriber status for the token's
  subject, with explicit permitted-audience checks and no arbitrary-user lookup.
- Recommended refresh tokens are opaque random values. Go stores hashes and
  user/client/family bindings, deadlines, and revocation state in Redis, not raw
  bearer tokens. Never let a refresh caller choose a different user/audience.
- Refresh expires after 7 days idle or 30 days from initial issuance. Rotation
  updates idle expiry without extending the absolute deadline. Atomically consume
  and replace each token; retain consumed-token evidence through family expiry.
- Reuse revokes that family. Clients serialize refreshes; there is no retry grace
  window. A lost successful response may require login again. Rust signs access
  tokens using Go's trusted result; signing and Redis are not one transaction,
  so implementation must handle partial failures without permitting replay.
- Missing Redis state, data loss, or Go/Redis outages never bypass validation.
  Data loss requires reauthentication; do not restore stale state that resurrects
  consumed/revoked tokens. Existing access JWTs expire normally.
- Provide current-session and all-session logout, revoking relevant refresh
  families and Crow browser sessions. Coordinate concurrent refresh/logout so
  refresh cannot escape revocation. Already-issued JWTs remain valid until expiry;
  receiving apps need no online revocation check on every request.

## Subscriptions

Rust verifies Stripe webhook signatures against raw bodies, records event IDs,
and updates PostgreSQL idempotently. Browser success redirects do not grant roles.
Assign subscriber status automatically from verified events once eligible Stripe
states/customer ownership are defined. Cancellation retains status until paid-period
end. Failed payments preserve existing status for 7 days; unresolved failure then
removes it. Repeated events cannot reset grace or grant a new subscriber role.

Persist relevant deadlines and evaluate expiry reliably rather than assuming an
event arrives at every deadline. Account reads use current locally recorded status.
Pricing, refunds, payment recovery, overlapping-event precedence, and reconciliation
remain open. All features stay unlocked; no subscription-based access restriction.

## Validation and remaining design

Keep liveness independent of Stripe; recommend separate bounded readiness checks.
Use disposable PostgreSQL/Redis and Stripe test mode. Test ownership, shared IDs,
callback/PKCE/state failures, atomic code redemption, JWT audience/signature/expiry,
refresh races/reuse/logout/outages, secret rotation, CSRF, redaction, and duplicate/
out-of-order billing events. Add hosted and direct-API smoke flows when implemented.
Use actual README commands; do not claim nonexistent service tests pass.

Before affected implementation, settle UUID generation, profile edits/consent,
password counting/hash parameters/rate limits, schema/migrations, API/internal
contracts, cookie storage, signing keys, proxy routing, and remaining billing rules.
Do not reopen the agreed lifetimes or credential policies as unspecified choices.
