# Crow Auth architecture and security

## Status and authority

Pre-implementation design. [PRODUCT.md](PRODUCT.md) owns product behavior;
[API_CONTRACT.md](API_CONTRACT.md) proposes endpoint shapes; [README](../README.md)
describes runnable code. Only the Rust/Axum service, PostgreSQL pool, startup
connectivity check and health endpoint exist. The decisions below are agreed,
not implemented. Wire-level proposals remain subject to implementation review.

## Services and trust boundaries

- Rust/Axum owns accounts, password verification, app ownership/configuration,
  authorization flows, codes, portal sessions, refresh-session creation,
  rotation and revocation, and the public API.
- PostgreSQL stores durable accounts, application configuration, consent,
  subscription state, portal sessions, pending authorization flows and codes.
- Redis stores application refresh families, refresh-token verifiers,
  spent-token evidence and revocation state. Rust owns all Redis operations.
- Rust signs initial and refreshed access JWTs using a securely provisioned
  3072-bit RSA private key and the RS256 token specification.
- Go is not part of the architecture; no separate refresh service is required.
- Next.js serves hosted login/registration, account and developer pages.
- Stripe is deferred; billing does not block the authentication milestone.

Pages and `/api` share one public HTTPS origin, routed internally to Next.js and
Rust. The domain remains undecided. PostgreSQL and Redis are private; Rust uses
authenticated connections, with transport protection appropriate to deployment. There is no separate refresh service or internal
authentication protocol between signers.

Applications call Rust for exchange, refresh, logout, identity and subscription.
Refresh/logout require client credentials and a refresh token; Rust verifies the
session belongs to the authenticated client. Browser-only integrations and app
password collection are excluded. Hosted credential endpoints require CSRF and
origin checks and framing protection. CORS is not authentication.

## Accounts and secrets

Username/password rules are authoritative in PRODUCT.md. Enforce normalized
username uniqueness in PostgreSQL. Hash passwords with Argon2id, unique random
salts and stored cost parameters; benchmark costs before public deployment.
Count password length consistently as Unicode code points, without normalization.
Apply a bounded request size that permits the full supported UTF-8 password.
Select the common/compromised-password dataset and update process before release;
never send plaintext passwords to a third party.

Client secrets are high-entropy random values, shown once and stored as verifiers.
Allow two active secrets per app, with identifiers and creation dates. Owners can
revoke either immediately; ordinary rotation overlaps until explicit revocation.
No secret revocation automatically revokes user sessions. Enforce ownership on
every management operation. Never log passwords, tokens, authorization headers,
client secrets, signing keys or database credentials.

## Browser and portal sessions

Application backends retain both access and refresh tokens in protected server
storage. Their browser uses a Secure, HttpOnly random session-handle cookie.
Backends serialize refreshes across instances, not just within one process.
Each application is responsible for its own cookie/CSRF and session protection.

Crow portal sessions live in PostgreSQL with verifiers of high-entropy random
session tokens. Cookies are host-only, Secure, HttpOnly and SameSite=Lax. Expiry
is absolute at eight hours; logout invalidates the server record and clears the
cookie. Cookie-authenticated mutations require CSRF protection and origin checks.
Application login/registration does not create a portal session. Portal sessions
cannot silently authorize an application. Global logout is outside initial scope.

## Authorization flow and consent

Store validated pending requests in PostgreSQL for 15 minutes, bound to the
initiating browser with a random flow cookie. Retain client ID, exact callback,
state and PKCE S256 challenge, never passwords. A flow cookie grants no portal
permissions. Consume completion once, after credentials and first-use consent.
Registration authenticates the pending flow without a second login.

Consent discloses UUID, username and subscription access, app name and destination
domain. Remember consent by user/client and permission version. Do not infer app
trustworthiness from registration. Decline consumes the flow and returns
`error=access_denied` plus state only to an already validated callback.

Codes expire 60 seconds after issuance and bind user, client, exact callback and
PKCE challenge. Require client authentication and consume codes atomically.
Never redirect tokens. Reject unvalidated callbacks, incomplete parameters,
expired/reused codes or incorrect PKCE. HTTPS callbacks are required except HTTP
on exact localhost/127.0.0.1 development hosts; no fragments or wildcards.

## Initial exchange and partial failures

Rust validates the client, enabled app state, code bindings, expiry and PKCE.
Codes live in PostgreSQL; refresh sessions live in Redis. These writes cannot
share a database transaction. Return tokens only after code consumption is
confirmed and the refresh session exists. Concurrent exchanges must not create
multiple usable families for one code.

Decision pending before exchange implementation: choose consume-once with fresh
authorization after any subsequent failure, or a durable coordinated exchange
with bounded internal retries. Specify write ordering, signing failure, uncertain
write outcomes, crash cleanup and orphan-session handling for the chosen policy.
A lost public success response requires fresh authorization. Never assume a
timeout proves a write failed or make a successfully exchanged code reusable.

## JWT contract and key rotation

Access JWTs last ten minutes. Header: `alg=RS256`, `kid`, and `typ=JWT`.
Claims: `iss` (configured Crow issuer), `sub` (immutable user UUID), `aud` (one
client ID), `iat`, `exp`, and unique `jti`. Username and subscription are fetched
from APIs, not embedded. Tokens are signed, not encrypted.

Validate signature, an explicit RS256 allowlist, known key, exact issuer, expected
audience, UUID subject, claim types and time bounds. Allow 30 seconds of clock
tolerance; reject unreasonable future issuance. Applications receive public keys
only. Crow identity/subscription endpoints independently authenticate client
credentials and require JWT audience equal to that client, deriving user from sub.
They also reject disabled apps. App JWTs never authorize portal management.

Publish public keys through JWKS. Rotate every 90 days, publishing new public keys
at least ten minutes before activation across Rust instances. Retain old public
keys at least fifteen minutes after all instances stop signing with them.
All signing instances must acknowledge rollout; a lagging instance extends
retention. Verifiers refresh
JWKS on unknown kid with rate limiting and bounded caching. A compromised key
requires emergency replacement and rejection policy; cached external verification
keys can delay rejection. A compromised Rust signing instance can forge trusted
tokens. Never commit private keys or bake them into images.

## Refresh, logout and Redis

Refresh tokens are high-entropy opaque credentials. Store verifiers and user,
client, family ID, creation time, last successful use, original absolute deadline
and revocation state in Redis. Retain spent-token evidence until family expiry.
Rust uses an atomic Redis operation/script to validate the token and family,
consume the old token, create its successor and update idle expiry. Reuse
atomically revokes the family even though the request returns invalid_grant.
Return tokens only after signing succeeds and the session update is confirmed.
Specify signing/write ordering and ambiguous failures before implementation.
Sessions expire seven days idle or thirty days total, whichever comes first.
Rotation never extends the original thirty-day deadline.

Strict reuse detection revokes the family, including accidental duplicates. No
grace window. Application backends must serialize refreshes across instances;
a lost refresh response requires login rather than a blind retry. App logout
revokes its current family and clears local app state; existing JWTs expire
naturally. Other applications/devices remain signed in.

Fail closed on unavailable Redis or failed writes; missing records require
login. Never reconstruct sessions from unverified tokens. PostgreSQL remains
authoritative for app configuration and enabled state. Before implementation,
choose how refresh checks current app state and how disable/re-enable prevents
old families from becoming usable again, including concurrent requests and
failed Redis revocation. Cleanup alone is insufficient.

Before deployment, define Redis persistence, noeviction capacity handling,
expiration of family/token evidence and restart/restore invalidation. Session
loss requires login again. Restoring old Redis data must not resurrect consumed
tokens or revoked families. PostgreSQL portal sessions are separate; database
restore procedures must also cover portal sessions and pending flows/codes.

## Operations and remaining work

Use temporary throttling by normalized account and source IP, generic login
failures and separate registration/exchange/refresh limits. Choose numeric limits,
trusted proxy rules and limiter-outage behavior before public exposure. Keep
security logs free of credentials and define retention/access policies.

Remaining implementation details: account/session migrations and indexes; exact
Argon2 costs; password blocklist source; endpoint proposal review; cross-store
exchange failure policy; atomic Redis rotation; disablement checks and races;
Redis durability, capacity and restore invalidation; PostgreSQL recovery;
JWKS cache bounds; consent
versioning; trusted proxy configuration; numeric limits and deployment secrets.
Billing still needs customer ownership, eligible states, plans/trials, lifecycle
precedence, webhook idempotency and reconciliation. No billing writes are exposed.

## First milestone acceptance checks

- Hosted registration and login complete a consented code flow without creating
  a portal session; direct portal registration/login does create one.
- Apps cannot collect Crow passwords or use direct password grants.
- Callback/PKCE/state validation, single-use codes and expiry work under races.
- Failed exchanges return no tokens; concurrent exchanges cannot create duplicate usable families.
- Client credentials alone reveal no user; JWTs cannot authorize portal management.
- Cross-client audiences, other-user access and non-owner management fail.
- First-use consent, cancellation and flow expiry have the documented results.
- Initial and refreshed JWTs share the token contract; key overlap and unknown kid work.
- Refresh atomicity, strict reuse, idle/absolute expiry and lost responses behave
  as specified; Redis failures never grant sessions and unavailable app-state
  checks fail closed.
- Disable/re-enable and secret rotation preserve the documented boundaries.
- Portal expiry/logout and cookie/CSRF/origin controls work.
- Subscription is non-subscriber before Stripe; logs contain no credentials.
