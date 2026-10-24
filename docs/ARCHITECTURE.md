# Crow Auth architecture and security

## Status and authority

This is a pre-implementation draft. PRODUCT.md owns product behavior; README.md
describes runnable code. Only the Rust/Axum HTTP service, PostgreSQL pool,
startup connectivity check, and health endpoint currently exist.

Service responsibilities and session durations below reflect confirmed product
behavior. Sections labelled proposed defaults are recommendations, not previously
agreed requirements. Resolve the remaining decisions before implementing the
affected flow; do not silently choose a security contract in code.

## Service responsibilities

- Rust/Axum: accounts, password verification, app ownership and configuration,
  authorization codes, initial authentication, and subscription-status API.
- PostgreSQL: durable accounts, application configuration and subscription state.
- Go: refresh-session creation, rotation, revocation, and issuing refreshed JWT
  access tokens, backed by Redis.
- Redis: refresh-session records and replay detection. It does not replace
  PostgreSQL as the account store or make JWTs require database lookups.
- Next.js: required Crow-hosted login and registration, plus account and developer
  pages. Application backends integrate through the supported APIs.
- Stripe: planned billing integration; its detailed lifecycle contract is deferred.

Initial token issuance and refresh-session creation must form a defined Rust/Go
contract. Decide which service signs initial access tokens and how partial
failures are handled before implementation.

## Confirmed authentication behavior

Every integrating application must have a backend that authenticates with a
client secret. Browser-only integrations are outside the initial scope. Browser
code must never receive a client secret. Hosted login and registration are the
only supported application authentication path, including for first-party apps.
Applications must not collect or forward Crow passwords. The application backend
exchanges its authorization code using PKCE and client authentication. No
application-facing password login or registration API is exposed; Crow-hosted
pages submit credentials to Crow's own authentication backend.

Hosted login requires a registered client, exact registered callback, request
state, and PKCE. Successful authentication or registration returns a short-lived,
single-use authorization code and state, never tokens in a redirect.
Invalid application parameters produce an error. Each new application
authorization requires credentials; an existing portal session does not provide
SSO. Registration satisfies authentication for its pending flow.

An app's access token may retrieve only its authenticated user's subscription
status. Derive that user from the verified token. Subscription status is not
embedded in JWT claims. App business data remains owned by each app.

## Authorization enforcement

The permissions table in PRODUCT.md owns caller permissions. Every protected
endpoint must declare its accepted credential type and enforce the corresponding
user, client, audience, and ownership checks server-side. A client secret does
not establish user identity; an application access token does not authorize
portal account or developer operations. Derive resource ownership from verified
identity and stored ownership, never solely from caller-supplied IDs.

Reject missing credentials and unauthorized operations without exposing private
resource details. Subscription status has no public user/app write operation.
Internal subscription updates require their separately defined service contract.

## Confirmed session behavior

Access JWTs expire after 10 minutes and are bound to the application's audience.
Refresh sessions expire after 7 days idle or 30 days total, whichever occurs
first. Refresh tokens rotate on use. Reuse revokes the session family.
Loss of Redis session data requires affected users to log in again.

Normal app logout revokes the current refresh-session family and clears local
session state. Already issued JWTs expire naturally. Other apps and devices
remain signed in. Portal sessions last at most 8 hours; portal logout clears
the current portal session. Global logout is outside initial scope.

## Proposed defaults: passwords and abuse prevention

- Hash passwords with Argon2id, unique random salts, and stored algorithm/cost
  parameters. Benchmark costs on production hardware before choosing values.
- Never store plaintext passwords or log credentials, authorization headers,
  tokens, client secrets, or database connection strings.
- Define username normalization, casing, allowed characters, length limits,
  password limits, and uniqueness constraints before account migrations.
- Rate-limit registration, login, code exchange, and refresh using appropriate
  account, client, and network signals. Define numeric limits before release.
- Use generic login failures to avoid revealing whether a username exists.
- Use HTTPS for external traffic. Protect hosted credential-submission endpoints
  against CSRF and origin abuse; do not enable cross-origin credential submission
  by integrating apps. Prevent framing of hosted login and registration pages.
- No recovery is initially provided: document that forgotten passwords cannot
  currently be recovered. Do not add an undocumented recovery bypass.

## Proposed defaults: authorization codes and client secrets

- Require PKCE S256. Bind codes to client ID, exact redirect URI, PKCE challenge,
  and authenticated user; set an explicit short expiry and consume atomically.
- Confidential clients authenticate during code exchange as well as using PKCE.
- Never redirect to an unvalidated callback, including on an error.
- Generate high-entropy client secrets, show them only on creation/rotation,
  and store verifiers rather than retrievable plaintext.
- Check application ownership on every management operation.
- Define secret rotation/revocation, app disablement, and permitted production
  versus local-development callback schemes before those features are built.

## Proposed defaults: refresh storage and concurrency

Use high-entropy opaque refresh tokens, not reusable access JWTs as refresh
credentials. Store a cryptographic verifier, user/client IDs, family ID,
creation time, last successful use, absolute expiry, and revocation state.
Retain enough spent-token information to detect reuse until family expiry.

Go performs refresh validation, old-token consumption, successor creation, and
idle-expiry updates atomically in Redis. Rotation must preserve the original
30-day deadline. A revoked family cannot refresh. Fail closed if Redis is
unavailable; never reconstruct a session from an unverified token.

Define client retry and concurrent-refresh behavior explicitly. Under strict
reuse detection, presenting a spent token revokes its family even when the
cause is an accidental duplicate request; clients must serialize refreshes.
Do not introduce an undocumented grace window.

## Proposed defaults: JWTs and service trust

Use asymmetric signing and publish public verification keys through a JWKS
endpoint. Require an allowlisted algorithm and validate signature, issuer,
audience, subject, expiry, and applicable time claims. Include a key identifier.
Define issuer, claims, clock tolerance, rotation cadence, and key-retention
period before issuing tokens.

Prefer one signing authority for initial and refreshed tokens. If both Rust and
Go sign, define distinct private-key ownership and a consistent verification
contract. Applications receive public keys, never signing keys.

Keep Go and Redis off the public network. Authenticate Rust-to-Go calls over
an encrypted channel; network location alone is insufficient. Specify service
credentials, allowed operations, timeouts, and failure behavior. Bind every
session operation to its user and application; never trust caller-supplied
identity without authentication and authorization.

## Proposed defaults: browser and portal sessions

Use backend-mediated browser integrations. Prefer Secure, HttpOnly cookies
for refresh or server-side session handles. Avoid persistent browser storage
of refresh credentials. Define deployment origins and cross-site behavior
before choosing SameSite policy.

Protect cookie-authenticated mutations with CSRF controls and origin checking.
Use explicit CORS allowlists where needed; CORS is not client authentication.
Keep portal sessions distinct from application refresh sessions. Hosted pages
use Crow account/application APIs with the permissions required for each
operation; credential submission is reserved for Crow-hosted flows.

## Decisions required before implementation

1. Username/password policies and account schema.
2. Endpoint schemas, error codes, hosted registration behavior, and code lifetime.
3. Initial signer, Rust/Go service contract, service authentication, signing-key
   storage, JWKS publication, and rotation.
4. Portal session storage, browser token handling, cookie/CSRF/CORS rules.
5. Refresh concurrency semantics, Redis durability/expiry configuration, and
   behavior when account or application state changes.
6. Secret provisioning and revocation; application disablement/deletion.
7. Consent and user-facing disclosure for third-party identity access.
8. Numeric rate limits and operational handling of suspicious login attempts.

Billing implementation additionally requires the product's outstanding Stripe
ownership, eligible states, plans, grace-period precedence, and reconciliation
decisions. Billing does not block the first authentication milestone.

## Acceptance checks for the first authentication milestone

- A registered user can authenticate through hosted login, with code exchange
  by a confidential application backend; registration completes the pending
  hosted authorization.
- Integrating applications cannot exchange a username/password for tokens or
  register accounts directly; credential submission is limited to Crow-hosted
  flows, with CSRF/origin and framing protections.
- Invalid callbacks, mismatched PKCE, expired/reused codes, wrong audiences,
  and invalid signatures fail.
- Another user cannot manage an application they do not own.
- A client secret alone cannot read user data or manage accounts/applications;
  an application access token cannot authorize portal management operations.
- An app token queries only its current user's subscription status. Before Stripe
  integration, this endpoint returns non-subscriber for every user.
- Refresh rotates atomically; reuse revokes the family; idle and total expiry
  work independently; Redis failure does not grant access.
- Logout prevents further refresh for that session while other sessions remain.
- Portal sessions cannot silently authorize a new application.
- Authentication failures and logs expose no credentials or tokens.


