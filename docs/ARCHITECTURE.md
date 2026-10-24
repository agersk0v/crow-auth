# Crow Auth architecture and security

## Status and authority

Pre-implementation design. [PRODUCT.md](PRODUCT.md) owns product behavior;
[API_CONTRACT.md](API_CONTRACT.md) proposes endpoint shapes; [README](../README.md)
describes runnable code. Only the Rust/Axum service, PostgreSQL pool, startup
connectivity check and health endpoint exist. The decisions below are agreed,
not implemented. Standards requirements are mandatory. Crow-specific protocol
choices and internal endpoint shapes remain draft until implementation review.

## Services and trust boundaries

- Rust/Axum owns accounts, password verification, app ownership/configuration,
  authorization flows, codes, portal sessions, refresh-session creation,
  rotation and revocation, and the public API.
- PostgreSQL stores durable accounts, application configuration, consent,
  subscription state, portal sessions, pending authorization flows, codes,
  refresh families and refresh-token verifiers.
- Rust signs initial and refreshed access JWTs using a securely provisioned
  3072-bit RSA private key and RS256 signing algorithm.
- Next.js serves hosted login/registration, account and developer pages.
- Stripe is deferred; billing does not block the authentication milestone.

Pages, `/api`, `/oauth` and discovery/JWKS endpoints share one public HTTPS
origin. Route pages to Next.js and protocol/API endpoints to Rust. The domain remains undecided. PostgreSQL is private; Rust authenticates
database connections. There is no separate refresh service or internal
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
Run hashing on a bounded blocking-worker pool, away from Tokio async workers.
Bound concurrent hashes and queued requests to protect CPU and memory.
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

## Browser sessions, SSO and portal access

Application backends retain access, ID and refresh tokens in protected server
storage. Their browser uses a Secure, HttpOnly random session-handle cookie.
Backends serialize refreshes across instances, not just within one process.
Each application is responsible for its own cookie/CSRF and session protection.

Crow browser sessions serve both SSO and portal access. Store verifiers of
high-entropy random session tokens in PostgreSQL with user UUID, original
authentication time, creation time, absolute expiry and revocation state. Cookies
are host-only, Secure, HttpOnly and SameSite=Lax. Rotate the session handle on
credential authentication to prevent fixation; never extend the original deadline
merely by reusing SSO. Fresh credential authentication establishes a new eight-hour
session and authentication time. Clear/revoke the prior handle when replacing it.
Cookie-authenticated mutations require CSRF protection and origin checks.

Both direct portal and hosted app credential login/registration establish this
session. Validate it server-side on authorization and bind its authenticated user
to the pending flow; no browser-supplied user ID. Enforce prompt/max_age and consent
rules in Product/API. Preserve original auth_time on SSO; consent is not fresh
authentication. Recheck session validity before issuing a code from an SSO flow.
No account-selection UI is initially supported; unsupported prompt values are
rejected. Top-level redirects are the supported SSO path; no third-party-cookie
or hidden-iframe reliability is promised.

Portal logout revokes this browser session and expires its cookie. It does not
revoke app families or clear remote app cookies. App logout leaves SSO intact.
Global logout and OIDC logout protocols remain outside scope.

## Authorization flow and consent

Store validated pending requests in PostgreSQL for 15 minutes, bound to the
initiating browser with a random flow cookie. Retain client ID, exact callback,
state, nonce, requested/granted scopes and PKCE S256 challenge, never passwords.
A flow cookie grants no portal permissions. Store authenticated identity and
authentication time server-side after credential verification; never accept a
browser-supplied user ID. Consume completion once after required consent.
Registration authenticates the pending flow without a second login.

Use explicit stages: pending authentication, pending consent, completed, denied
and expired. A flow can reach only one terminal stage; code creation and flow
completion are atomic. Fresh authentication may advance directly to completion
when no consent is needed. Recheck session validity and identity before completion
for both SSO and credential flows. Logout or replacement of the associated browser
session invalidates its unfinished authenticated flows. Never silently change a
flow's authenticated user. Support independent flows in multiple tabs using
opaque flow IDs bound to a browser secret; an ID alone grants no authority.

Recheck enabled app state and current callback registration at completion and
exchange. Serialize callback removal/disablement with these operations using a
consistent lock order. If removal wins, show a local error; never redirect to the
removed URL. A response already committed before removal cannot be recalled.

Consent discloses requested scopes, app name and destination
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
Consume the code and, when offline access is granted, create its refresh family
and token verifier in one
PostgreSQL transaction. Lock the relevant records and use database constraints
so concurrent exchanges cannot consume a code twice or create duplicate families.
Prepare the signed access/ID tokens and response credentials before committing; return tokens
only after a confirmed commit. Signing failures roll back the transaction.

A confirmed rollback leaves the code usable within its original lifetime.
Clients may retry only with an explicit safe-retry signal defined in the API
contract; a generic error never proves rollback.
An uncertain commit outcome or lost public success response requires fresh
authorization; never assume a timeout means rollback. Do not persist plaintext
response tokens for replay. Specify transaction isolation, lock ordering, unique
constraints and cleanup before implementing this flow.

Retain a verifier of each consumed code and its issued-grant relationship until
the grant can no longer be used. Deny replay with invalid_grant. For a replay
authenticated as the original client and passing the original callback and PKCE
bindings, revoke its refresh family, if any, and mark its access-token grant
revoked. Crow resource APIs check that grant state until token expiry. Invalid
client or binding attempts must not revoke another grant. Existing app-local
sessions cannot be cleared remotely by this mechanism.

## JWT contract and key rotation

Access JWTs last ten minutes and use `alg=RS256`, `kid`, `typ=at+jwt`.
Follow RFC 9068: include `iss`, UUID `sub`, `aud` (configured Crow resource API
identifier), `client_id`, `scope`, `iat`, `exp` and unique `jti`.
OIDC ID tokens use `typ=JWT`, RS256 and the same signing-key lifecycle, but
`aud` is the single integrating client ID. Include `iss`, UUID `sub`, `iat`,
`exp`, `auth_time` and the authorization request's `nonce`. ID tokens last ten
minutes. `profile` permits `preferred_username`; subscription stays in its API.

Validate signatures, explicit RS256 allowlist, known key, exact configured issuer,
claim types and time bounds with 30 seconds clock tolerance. Resource APIs accept
only access tokens with their resource audience, appropriate scopes, an enabled
issuing `client_id` and no replay-revoked grant; ID tokens cannot authorize them. Clients validate ID-token
issuer, client audience, expiry and nonce against their initiating browser session.
UserInfo `sub` must equal the ID token's `sub`. Never use a refreshed ID token to
establish a new browser login. No app token authorizes portal management.

Use mature OAuth/OIDC libraries where practical. Implement Authorization Code
Flow only, PKCE S256, confidential clients with `client_secret_basic`, discovery,
UserInfo and RFC 7009 refresh revocation. Token/revocation requests are form
encoded; resource APIs use Bearer tokens without extra client-secret headers.
Validate scopes, nonce, state and OIDC prompt behavior as specified in Product and
API contract. Client secrets never enter authorization URLs. No Google/Discord
upstream identity integration is included yet. Publish compatibility only after
protocol tests pass; use applicable OpenID conformance tests before launch and
state supported features without claiming certification.

Publish public keys through JWKS. Rotate every 90 days, publishing new public keys
at least ten minutes before activation across Rust instances. Retain old public
keys at least fifteen minutes after all instances stop signing with them.
All signing instances must acknowledge rollout; a lagging instance extends
retention. Verifiers refresh JWKS on unknown kid with rate limiting and bounded caching. A compromised key
requires emergency replacement and rejection policy; cached external verification
keys can delay rejection. A compromised Rust signing instance can forge trusted
tokens. Never commit private keys or bake them into images.

## Refresh, logout and PostgreSQL

Refresh tokens are high-entropy opaque credentials. Store verifiers and user,
client, family ID, granted scopes, creation time, last successful use, original absolute deadline
and revocation state in PostgreSQL. Retain spent-token evidence until family
expiry. Rust locks the family and token records and atomically consumes old
tokens, creates successors and updates idle expiry in one transaction. Commit
family revocation on reuse even though the request returns invalid_grant;
do not roll back that revocation with the rejected request.
Sign the new access JWT before commit and return tokens only after a confirmed commit.
Use Product session lifetimes; rotation never extends the original absolute
deadline. Requested refresh scopes may narrow but never expand the grant.

Strict reuse detection revokes the family, including accidental duplicates. No
grace window. Application backends must serialize refreshes across instances;
a lost refresh response requires login rather than a blind retry. App logout
revokes its current family and clears local app state; existing JWTs expire
naturally. Other applications/devices remain signed in.

This is an intentional initial security/availability tradeoff: a duplicate may be
theft, so Crow rejects both copies at the cost of restarting legitimate sessions.
Fresh authorization may use existing SSO, subject to consent/freshness rules; it
does not always require another password entry. Before external integrations,
provide a reference backend showing distributed refresh serialization, crash/lost
response handling and reauthorization. Retry grace or response replay requires a
separate reviewed design; do not add either implicitly.

Fail closed on unavailable PostgreSQL or failed writes; missing records require
login. Never reconstruct sessions from unverified tokens. App disablement and
refresh must coordinate through the same durable app state and transaction
locking so a concurrent refresh cannot bypass disablement. Disablement revokes
existing families; re-enabling never restores them. Define lock ordering before
implementation.

A known database restore/rollback can resurrect consumed tokens or revoked
sessions. Define a procedure to invalidate affected refresh and portal sessions
and outstanding authorization flows/codes before serving traffic from restored
state. Specify database durability, backup and recovery settings before deployment.

## Operations and remaining work

Use temporary throttling by normalized account and source IP, generic login
failures and separate registration/exchange/refresh limits. Choose numeric limits,
trusted proxy rules and limiter-outage behavior before public exposure. Keep
security logs free of credentials and define retention/access policies.

Before deployment, budget database connections across replicas, define timeouts,
graceful shutdown, migrations, metrics and a separate readiness endpoint. /health
remains HTTP liveness. Throttling must enforce a shared budget across instances.
App/grant-state caching needs explicit invalidation and maximum staleness; do not
silently weaken disablement or replay revocation. Add caches, partitioning or
additional services only when measurements justify them.

Before protocol implementation, evaluate maintained OAuth/OIDC provider libraries
and existing providers. Record the selected approach, maintenance status, covered
protocol features and Crow-owned security responsibilities. Prefer maintained
protocol implementations where suitable; do not invent cryptography or claim
that a JWT signing library implements an identity provider.

Remaining implementation details: account/session migrations and indexes; exact
Argon2 costs; password blocklist source; endpoint proposal review; exchange failure
transaction boundaries, lock ordering and constraints; PostgreSQL durability and
restore invalidation procedure; disablement races; JWKS cache bounds; consent
versioning; trusted proxy configuration; numeric limits and deployment secrets.
Billing still needs customer ownership, eligible states, plans/trials, lifecycle
precedence, webhook idempotency and reconciliation. No billing writes are exposed.

## First milestone acceptance checks

- Direct and hosted credential login/registration establish the Crow browser
  session; a second app uses SSO while still enforcing required consent.
- Expired/revoked sessions, prompt=login, max_age and prompt=none enforce
  authentication/consent correctly and preserve original auth_time on SSO.
- Portal logout ends browser SSO without revoking app families; app logout leaves
  SSO intact. Session fixation and cross-browser flow completion fail.
- Apps cannot collect Crow passwords or use direct password grants.
- Callback/PKCE/state validation, single-use codes and expiry work under races.
- Bound multi-tab flows, logout/account changes, callback-removal races and code
  replay revocation behave as specified; invalid bindings cannot revoke grants.
- Failed or concurrent exchanges issue no tokens before commit or duplicate families.
- Client credentials alone reveal no user; JWTs cannot authorize portal management.
- Wrong resource/ID-token audiences, other-user access and non-owner management fail.
- First-use consent, cancellation and flow expiry have the documented results.
- Discovery, authorization, standard token errors, UserInfo and revocation match
  the supported OAuth/OIDC contract; test with a standard OIDC client library.
- ID/access token substitution fails; nonce, resource/client audiences, scopes,
  prompt behavior, key overlap and unknown kid are validated.
- Refresh atomicity, strict reuse, idle/absolute expiry and lost responses behave
  as specified; PostgreSQL failures never grant sessions.
- Disable/re-enable and secret rotation preserve the documented boundaries.
- Portal expiry/logout and cookie/CSRF/origin controls work.
- Subscription is non-subscriber before Stripe; logs contain no credentials.
