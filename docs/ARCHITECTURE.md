# Crow Auth architecture

[PRODUCT](PRODUCT.md) owns intended behavior and scope. [README](../README.md)
owns working setup and check commands. This document owns technical design,
including authentication and security. Product requirements take precedence
over technical proposals.

The implemented baseline below describes existing code. Everything else is a
planned design for review, not a claim that services or endpoints already exist.
Recommendations do not settle the open product decisions listed at the end.

## Implemented baseline

- The repository root is a Rust binary crate. `src/main.rs` starts Axum on
  `127.0.0.1:3000` using Tokio.
- SQLx reads `DATABASE_URL`, creates a PostgreSQL pool with at most five
  connections and a five-second acquisition timeout, and runs `SELECT 1` before
  starting HTTP. The pool is available as Axum state.
- Missing configuration and connection failures stop startup with generic errors
  that do not include the connection string.
- `GET /health` returns `200` with body `ok`. It does not check database health.
- `tests/startup.rs` covers missing configuration and malformed connection strings,
  including password redaction. There are no user tables or migrations yet.

Next.js, the Go service, Redis, authentication routes, JWT issuance, and Stripe
integration do not exist yet. Current source paths are not a commitment to the
eventual multi-service directory layout.

## Planned components and boundaries

| Component | Responsibility |
| --- | --- |
| Nginx | One public origin; routes hosted pages to Next.js and backend API requests to Rust |
| Next.js frontend | Hosted login, registration, account, and developer application pages; calls the Rust API |
| Rust / Axum / SQLx API | User credentials, application ownership, callback validation, authorization-code exchange, access-token signing, account APIs, and Stripe integration |
| Go refresh-token service | Refresh-token issuance, validation, rotation, reuse detection, and revocation, backed by Redis |
| PostgreSQL | Durable identities, application registrations, and subscription records |
| Redis | Refresh-token/session-family state owned by the Go service |
| Stripe | Payment and subscription processing; Crow stores the relevant external identifiers and local state |

Keep all service source in this repository. Separate directories can be introduced
when those services are implemented; do not move the Rust crate merely to match a
speculative layout. The Next.js and Go versions and the exact deployment layout
are not selected yet.

Nginx combines Next.js pages and the Rust API under one public Crow origin.
Use a distinct API prefix so hosted `/login` and API operations do not conflict;
the exact prefix, upstream addresses, and proxy configuration remain to be defined.
The Go service and databases should be internal; browsers and connected applications
must not access Redis or internal refresh-service operations directly.

Rust owns persistent identity and application policy. Go should not independently
decide which user or audience receives access, and Next.js should not implement
a second password-validation or token-signing authority.

## Identity and data ownership

One user has one immutable UUID Crow ID across applications. JWT `sub` and account API
responses identify that same user. Connected applications associate their own
records with this ID; they do not need a separate local account registration.
Crow does not store their business data or control their database structure.

Planned logical records, not finalized SQL schemas:

| Record | Required purpose |
| --- | --- |
| User | Stable ID, unique username, password hash, timestamps |
| Application | Public client ID, display name, owning Crow user ID, timestamps |
| Callback | Exact allowed URL associated with one application |
| Subscription | Crow account association and relevant Stripe identifiers/state; mapping awaits billing decisions |

Recommended supporting records include short-lived authorization transactions
and codes, and processed Stripe event IDs. Their storage and schema need a
focused implementation design. Do not add email, organizations, paid entitlements,
or profile fields without a corresponding product decision.

Use PostgreSQL constraints for identity uniqueness and application ownership
relationships. Store user IDs as PostgreSQL UUIDs and serialize them as UUID
strings in JWT `sub` and API responses. UUID generation/version and username
normalization must implement case-insensitive uniqueness for ASCII usernames;
UUID generation/version remains to be selected before the first user migration. Keep initial
credentials to simple usernames and passwords, without requiring an email address.
A username is mutable display/login data only if editing
is later approved; it must never substitute for the stable ID.

Recommended migration policy: versioned, forward-only SQL migrations, with
transactional application and serialized concurrent startup. Add migrations
rather than rewriting applied ones. Migration tooling and startup behavior remain
to be selected. Test databases must be separate and disposable.

## Hosted pages and login flows

Next.js provides `/login`, `/register`, `/account`, `/applications`, and an
application detail page as defined in PRODUCT. These pages use the same API
capabilities available to permitted non-frontend clients.

### Crow's own account login

A visit to `/login` without application-login parameters authenticates the user
to Crow itself and leads to `/account`. Account and application-management APIs
require Crow-scoped authentication, not a JWT intended for an arbitrary connected
application. Only an application's owner may manage it.

Use secure, HttpOnly cookies for the Crow session, with a server-enforced
maximum duration of 8 hours from sign-in. Define cookie scope, SameSite behavior,
CSRF protection, and session storage before implementing this flow. The cookie authenticates Crow account and
developer pages only; it must not bypass credentials in a new application login.
Single sign-on is outside the initial scope. Never allow an arbitrary internal
post-login destination to become an open redirect.

### Hosted application login

1. The application creates request state and a PKCE verifier, keeps them locally,
   and opens Crow login with its client ID, callback URL, state, and challenge.
2. Rust validates the client and exact registered callback. Only the absence of
   all application-flow parameters selects Crow's own login. Partial or invalid
   parameters produce an error without redirecting to an unvalidated destination.
3. Next.js renders login or registration while preserving a server-validated
   transaction. Each new application authorization requires username/password
   entry, even when a Crow account-page session already exists. Revalidate
   transaction data on submission; do not trust editable
   form fields to determine the eventual callback or audience.
4. After authentication, Crow issues a short-lived, single-use authorization
   code bound to the user, client, callback, and PKCE challenge. The browser goes
   to the validated callback with the code and original state.
5. The client checks state and exchanges the code and verifier. Rust verifies
   the transaction and issues tokens for the bound application.

Use PKCE S256. Redemption must atomically consume the code so concurrent requests
cannot both redeem it. Expired, reused, or mismatched codes are rejected. The
callback carries neither access tokens nor refresh tokens. Use unpredictable
codes and do not store their raw values where a hash will suffice.

Use exact callback matching, without wildcards or suffix matching. Require HTTPS
in production. Define an explicit loopback-only development policy; do not
accept arbitrary HTTP hosts. Domain verification is not initially required.
Registered callbacks establish destinations, not application trustworthiness.

### Direct API login

Direct API login accepts credentials and registered application identification
and returns tokens as JSON, without a return URL. An application using this flow
handles the user's Crow password. Hosted login avoids that exposure.

A public client ID, CORS header, or claimed origin is not client authentication.
Direct username/password login is restricted to authenticated backend
applications. Browser applications use hosted authorization-code login with PKCE.
Authenticate backend applications using a client ID and a randomly generated
client secret. Do not expose backend client secrets to browsers. Store secret
verifiers rather than recoverable raw secrets and show a new secret only at
issuance. Permit at most two active secrets during rotation: provision the new
secret, deploy it, then explicitly revoke the old one. Secret-management APIs
require application-owner authentication. Secret transport and provisioning
contracts still need definition. A public client ID alone never authorizes token
issuance for that client.

## Planned API capabilities

The following describes operations, not implemented or finalized endpoint paths.
Only the existing `GET /health` is an implemented HTTP contract.

| Operation | Boundary and result |
| --- | --- |
| Account registration | Validate agreed username/password rules and create one Crow identity |
| Direct login | Authenticate the user and permitted client; return audience-bound tokens |
| Hosted transaction / code exchange | Validate client, callback, state binding, and PKCE; exchange a single-use code |
| Account read | Return the authenticated user’s UUID, username, and current locally recorded subscriber status; no arbitrary-user lookup |
| Application management | Crow-authenticated owner creates, lists, and updates owned applications/callbacks |
| Refresh | Rust public entry point delegates refresh-state decisions to Go and signs the resulting access token |
| Logout | Support current-session and all-session refresh/browser revocation; access JWTs expire naturally |
| Public signing keys | Publish verification keys, never signing secrets |
| Stripe webhook | Verify signatures and process events idempotently; apply the agreed role policies once eligible states are defined |

Define paths, body limits, content types, validation rules, status codes, and a
consistent JSON error format in implementation issues before callers depend on
them. Tokens and credential responses must not be cached. Do not claim OAuth or
OpenID Connect compliance without implementing and testing the relevant standards.

## Security and token design

### Credentials and request protection

Usernames contain 3–32 ASCII letters, digits, or underscores. Enforce uniqueness
case-insensitively in PostgreSQL as well as validation; an ASCII lowercase lookup
key is a suitable implementation. Define display casing before supporting edits.
Passwords contain 15–128 characters, permit spaces, and have no required character
combinations. Do not silently truncate or trim passwords. Define consistent
character-counting semantics for frontend/backend validation.

Hash passwords with a maintained Argon2id implementation and unique salts;
benchmark parameters before implementation. Never store plaintext passwords or
reversible password encryption. Define bounded hashing concurrency, request size
limits, and account/IP rate limits before exposing login publicly.

Return generic invalid-login responses rather than exposing account existence.
Never log passwords, raw tokens, authorization codes, connection strings, or
secrets. Apply credential redaction to access logs and tracing as well as errors.
Browser cookie-authenticated mutations need CSRF defenses; CORS is not an
authentication or CSRF substitute. Define allowed frontend origins explicitly.

### Access tokens

Rust is the proposed sole access-token signing authority. Use asymmetric signing
so connected APIs receive public verification keys, not a shared signing secret.
Select the algorithm and key-management mechanism before token implementation.

JWTs identify Crow as `iss`, the immutable UUID user ID as `sub`, and the intended
application as `aud`. Set expiry to 10 minutes (600 seconds) after issuance
(`exp = iat + 600`). Receiving APIs must verify the signature with an allowlisted algorithm, issuer, audience, and time validity.
They must reject another application's token, even when `sub` names the same user.
Access tokens contain the user UUID and username alongside validation claims.
Do not include subscriber status in JWTs. Applications read the current locally
recorded status through an authenticated account API for the token’s subject;
validate that the requesting application audience is permitted for that operation.
A connected-app token must not authorize Crow developer-management operations.
JWT claims are readable, not encrypted secrets.

Publish public keys with key IDs and support overlapping verification keys during
rotation. Private signing keys remain outside Git and frontend bundles. Define
clock tolerance and rotation overlap before implementation. The 10-minute
access-token lifetime does not determine refresh-token or browser-session expiry.
Already-issued JWTs generally remain valid until expiry unless receiving APIs
perform additional revocation checks; do not promise immediate global logout.

### Refresh tokens and Go / Redis

Recommended refresh tokens are opaque random values. Go stores hashes and
associated user/client, session-family, expiration, and revocation state in
Redis, not raw bearer tokens. Rust authenticates its internal calls to Go;
network location alone is not sufficient authorization.

Go atomically consumes the current token and creates the next token during
rotation. Retain enough consumed-token history to detect reuse and revoke the
affected family; storing only the newest token would lose that evidence.
The user and audience are bound to server-side state, never selected freely
by a refresh caller. Rust signs the access token using the trusted result.

Refresh sessions have a 7-day idle timeout and a 30-day absolute maximum from
initial issuance. A successful rotation renews the idle deadline without moving
the absolute deadline. Each refresh token is single-use; detected reuse revokes
its session family. Clients must serialize refresh attempts. There is no retry
grace window; a lost successful response may require fresh login.

Retain consumed-token evidence through the family’s absolute lifetime, even
after rotation, so reuse remains detectable. Redis updates and JWT signing are
not one cross-service transaction; define safe response-loss/failure handling
without bypassing single-use checks or permitting unlimited reissuance.

If Redis or Go is unavailable, refreshing must fail without inventing a session
or skipping validation. Existing locally validated JWTs can remain valid until
expiry. If Redis loses session data, affected refresh sessions are invalid and
users must log in again. Missing state never permits refresh. Do not restore
stale token state in a way that resurrects consumed or revoked tokens. How Go
learns about account/application changes still needs an internal contract.

Support current-session logout and all-session logout. Revoke the corresponding
refresh families and Crow browser sessions. Client-secret rotation does not
implicitly change a user’s password or global identity. Account for races between
refresh and logout so a concurrent refresh cannot escape revocation. Connected
APIs need not perform online revocation checks; existing access JWTs remain
valid until their normal 10-minute expiry.

## Subscriptions

Keep Stripe integration and durable subscription records in the Rust/PostgreSQL
boundary. Verify webhook signatures against the raw request body, deduplicate
event IDs, and make database updates idempotent. Browser success redirects alone
must not grant a subscriber role. Stripe credentials remain server-side.

Subscriber-role assignment is automatic from verified Stripe events. Eligible
Stripe states and customer-to-account ownership still require a billing decision.
Cancellation preserves existing subscriber status through the paid period. A
failed payment preserves existing subscriber status for a 7-day grace period;
an unresolved failure removes it at the deadline. Duplicate/retried events must
not reset grace or grant a role that was not already held. Define lifecycle
precedence for overlapping cancellation/failure events before implementation.

Persist paid-through and grace deadlines and evaluate them through a reliable
expiry mechanism; do not assume Stripe will send an event at every local deadline.
Account reads return current locally recorded status instead of a JWT snapshot.
Refunds, recovery after payment, out-of-order reconciliation, and pricing remain
undefined. All features stay unlocked; subscriber status does not gate access.

## Runtime, failures, and validation

Use Nginx to expose Next.js and the Rust API under one origin through TLS in
production. Trust forwarded host/protocol headers only from the configured proxy;
public callers must not be able to spoof the external origin used for security
checks. Apply request-size and timeout limits at the proxy and service boundaries.
Keep
PostgreSQL, Redis, and internal service operations private. Configure connection
strings, signing material, and service credentials through runtime configuration,
never committed files. Exact variables beyond today's `DATABASE_URL` await the
relevant service implementations.

Retain the current `/health` liveness behavior. A separate readiness operation
is recommended, with bounded checks and an explicit dependency policy. Startup
and migration failures, database outages, Go/Redis outages, and Stripe failures
need distinct handling; do not make a liveness check depend on Stripe.

Future validation should cover:

- Stable identity across applications, uniqueness, and owner-only management.
- Callback mismatch, incomplete transactions, state mismatch, wrong PKCE,
  expiry, and concurrent/repeated code redemption.
- Wrong-audience, wrong-issuer, expired, and tampered JWT rejection.
- Refresh rotation, concurrent requests, reuse detection, revocation, and outages.
- Cookie/CSRF boundaries and credential-safe logs/responses.
- Signed, duplicate, and out-of-order Stripe events once lifecycle rules exist.
- A representative hosted Next.js flow and a direct API flow using disposable
  databases and Redis, without touching user data.

Use the README's existing commands for current code. Introduce and document real
Next.js, Go, database, and end-to-end checks as those components are implemented;
do not claim nonexistent test commands or suites pass.

## Open decisions

Before implementing each affected feature, settle:

1. Profile edits, username display casing, consistent password character counting,
   UUID generation, and Argon2id parameters/rate limits.
2. Backend secret provisioning, revocation, and transport contracts, and consent.
3. Crow browser-session storage, cookie/CSRF details, and atomic logout/refresh
   coordination. The 8-hour duration and both logout scopes are agreed; no SSO.
4. Authorization-code lifetime, signing algorithm/keys, cross-service failure
   contracts, and prevention of stale-state resurrection during Redis recovery.
5. Migration tooling, service layout/contracts, and deployment/routing details.
6. Subscription ownership, eligible states, lifecycle precedence, refunds, payment
   recovery, reconciliation, pricing, and billing interfaces.

The recommendations in this document provide a reviewable direction. They do
not silently convert these open choices into implemented product requirements.
