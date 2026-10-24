# Crow Auth architecture and security

## Status and authority

Pre-implementation design. [PRODUCT.md](PRODUCT.md) owns product behavior;
[API_CONTRACT.md](API_CONTRACT.md) proposes endpoint shapes; [README](../README.md)
describes runnable code. Only the Rust/Axum service, PostgreSQL pool, startup
connectivity check and health endpoint exist. The decisions below are agreed,
not implemented. Wire-level proposals remain subject to implementation review.

## Services and trust boundaries

- Rust/Axum owns accounts, password verification, app ownership/configuration,
  authorization flows, codes, portal sessions and the public API.
- PostgreSQL stores durable accounts, application configuration, consent,
  subscription state, portal sessions and pending authorization flows.
- Private Go owns refresh-session creation, rotation and revocation in Redis.
- Rust signs initial access JWTs; Go signs refreshed JWTs. Both use the same
  securely provisioned 3072-bit RSA private key and RS256 token specification.
- Next.js serves hosted login/registration, account and developer pages.
- Stripe is deferred; billing does not block the authentication milestone.

Pages and `/api` share one public HTTPS origin, routed internally to Next.js and
Rust. The domain remains undecided. Go and Redis are private. Rust calls Go over
TLS using a dedicated service secret, separate from JWT keys and client secrets.
Go permits only explicit internal operations and validates user/client/session
bindings. Network location alone does not authenticate a caller.

Applications call Rust for exchange, refresh, logout, identity and subscription.
Refresh/logout require client credentials and a refresh token; Go verifies the
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
client/service secrets, signing keys or database credentials.

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

Rust verifies the code and client and orchestrates session creation in Go.
Exchange succeeds only once the refresh session exists. Failures issue no tokens.
Use a stable internal operation ID bound to the code, user and client so retrying
session creation cannot create duplicate families. A successful code cannot be
exchanged again. Temporary retries must finish within the original code lifetime.

Before implementing this boundary, specify a durable exchange state machine for
pending, in-progress and completed exchanges, including races, lost internal
responses, cleanup and orphan sessions. Go must resolve a repeated operation to
one outcome without logging credentials. A lost public success response requires
fresh authorization; idempotency of internal creation does not make codes reusable.
The response-recovery mechanism and secret retention for internal retries remain
implementation details requiring review, not an assumed distributed transaction.

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
at least ten minutes before coordinated Rust/Go activation. Retain old public
keys at least fifteen minutes after both stop signing with them. Both services
must acknowledge rollout; a lagging signer extends retention. Verifiers refresh
JWKS on unknown kid with rate limiting and bounded caching. A compromised key
requires emergency replacement and rejection policy; cached external verification
keys can delay rejection. Shared keys mean either compromised signer can forge
trusted tokens. Never commit private keys or bake them into images.

## Refresh, logout and Redis

Refresh tokens are high-entropy opaque credentials. Store verifiers and user,
client, family ID, creation time, last successful use, original absolute deadline
and revocation state. Retain spent-token evidence until family expiry. Atomically
consume old tokens, create successors and update idle expiry. Sessions expire
seven days idle or thirty days total, whichever comes first. Rotation never
extends the original thirty-day deadline.

Strict reuse detection revokes the family, including accidental duplicates. No
grace window. Application backends must serialize refreshes across instances;
a lost refresh response requires login rather than a blind retry. App logout
revokes its current family and clears local app state; existing JWTs expire
naturally. Other applications/devices remain signed in.

Redis uses AOF with one-second flushing and noeviction. Fail closed on unavailable
Redis or failed writes; missing records require login. Crashes may lose recent
rotation or revocation writes. A known restore/rollback invalidates all refresh
sessions before serving requests; define an operational generation/invalidation
procedure before deployment, including disabling persistence recovery until done.
Never reconstruct sessions from unverified tokens. PostgreSQL portal sessions
are unaffected. Go must enforce app disablement even if Redis revocation fails;
define durable app-state propagation/checks before implementing disablement.

## Operations and remaining work

Use temporary throttling by normalized account and source IP, generic login
failures and separate registration/exchange/refresh limits. Choose numeric limits,
trusted proxy rules and limiter-outage behavior before public exposure. Keep
security logs free of credentials and define retention/access policies.

Remaining implementation details: account/session migrations and indexes; exact
Argon2 costs; password blocklist source; endpoint proposal review; exchange failure
state machine; internal operation schemas/timeouts; service-secret rotation;
Redis restore procedure; disablement propagation; JWKS cache bounds; consent
versioning; trusted proxy configuration; numeric limits and deployment secrets.
Billing still needs customer ownership, eligible states, plans/trials, lifecycle
precedence, webhook idempotency and reconciliation. No billing writes are exposed.

## First milestone acceptance checks

- Hosted registration and login complete a consented code flow without creating
  a portal session; direct portal registration/login does create one.
- Apps cannot collect Crow passwords or use direct password grants.
- Callback/PKCE/state validation, single-use codes and expiry work under races.
- Failed internal creation produces no successful exchange or duplicate families.
- Client credentials alone reveal no user; JWTs cannot authorize portal management.
- Cross-client audiences, other-user access and non-owner management fail.
- First-use consent, cancellation and flow expiry have the documented results.
- Both signers produce the same token contract; key overlap and unknown kid work.
- Refresh atomicity, strict reuse, idle/absolute expiry and lost responses behave
  as specified; Redis failures never grant sessions.
- Disable/re-enable and secret rotation preserve the documented boundaries.
- Portal expiry/logout and cookie/CSRF/origin controls work.
- Subscription is non-subscriber before Stripe; logs contain no credentials.
