# Crow Auth architecture and security

## Status and authority

**Accepted:** planned security requirements unless labeled otherwise. See
[AGENTS.md](../AGENTS.md) for document ownership and decision labels.

Before protocol implementation, evaluate library coverage and document which
security responsibilities remain Crow's.

## Services and trust boundaries

One Rust/Axum application owns accounts, app management, browser sessions,
authorization, token issuance and resource APIs. It renders hosted and portal
pages using templates with HTMX enhancements. The template library is not yet
selected. Standard forms and redirects must work without HTMX; client-side state
and HTMX headers are never authorization evidence.

Pages, API, OAuth and discovery endpoints share one configured HTTPS origin.
No separate Next.js server, refresh service or internal signing protocol is needed.
PostgreSQL is private, authenticated and authoritative for durable state.
Subscription billing is deferred.

Opaque access tokens suit the initial deployment because Crow is their only
resource server and every resource request already needs durable grant/app
checks. ID tokens remain signed JWTs for OIDC interoperability.

Application integrations and user-visible session boundaries follow
[Product](PRODUCT.md). Enforce app ownership on every management operation;
derive the owner from the browser session, never a supplied owner ID.
App tokens never authorize portal management.

## Accounts, identifiers and secrets

Product owns username/password input rules. Enforce normalized username uniqueness
in PostgreSQL. Hash passwords with Argon2id, unique random salts and stored cost
parameters. Benchmark costs before public deployment. Run hashing on a bounded
blocking-worker pool with bounded queues, away from Tokio async workers.
Request-size limits must permit the full supported UTF-8 password.
Select a common/compromised-password dataset and update process before release;
never send plaintext passwords to a third party.

Keep the internal account UUID separate from external subjects. Store a stable,
cryptographically random opaque subject for each user/sector pair, with uniqueness
constraints on the pair and the subject. Use the same mapping in ID tokens, access
tokens and UserInfo for that client's sector. Resolve it server-side to the internal account
for resource access. Do not regenerate subjects on login or secret rotation.
Preserve mappings in backups; an identifier is not an authentication credential.

Generate high-entropy client secrets and store only verifiers. Product owns
display, active-secret limits and rotation behavior. Never log passwords, tokens,
authorization headers, client secrets, signing keys, database credentials or
sensitive callback queries.

## Browser security and sessions

Application backends store tokens server-side; their browsers receive random
session handles in Secure, HttpOnly cookies. Each app owns its cookie, CSRF and
local-session protection.

Store verifiers of Crow browser-session tokens with user UUID, original
authentication time, creation time, absolute expiry and revocation state.
Cookies are host-only, Secure, HttpOnly and SameSite=Lax. Rotate the handle on
credential authentication, revoke the prior handle and establish a new session.
SSO reuse preserves the original authentication time and deadline.
Product owns lifetimes and the effects of portal versus app logout.

Cookie-authenticated mutations, including login/registration, require CSRF tokens
and validated Origin checks. Protect pages against framing, escape untrusted
template content, bound request sizes and prevent sensitive-response caching.
SameSite and CORS are not substitutes for CSRF or authorization. HTMX requests
receive the same checks as ordinary forms. Define CSRF issuance before browser
endpoint implementation.

Top-level redirects are the supported SSO path. Hidden iframes and third-party
cookies are not a reliability requirement.

## Authorization state machine

Store validated pending requests in PostgreSQL, bound to an initiating-browser
secret. Retain client, exact callback, state, nonce, scopes and PKCE challenge.
Use opaque flow IDs to support independent tabs; an ID alone grants no authority.
Flow credentials grant no portal permissions.

Stages are pending authentication, pending consent, completed, denied and expired.
Only one terminal transition is allowed. Store authenticated user, authentication
time and associated browser session server-side. Never accept a browser-selected
user ID or silently change a flow's authenticated account.
Logout or replacement of its session invalidates unfinished authenticated flows.

Enforce Product/API freshness and consent requirements. Recheck browser-session
validity and identity before completion, including after credential login.
Code creation and flow completion are atomic. Callback validation and protocol
bindings follow [API contract](API_CONTRACT.md#authorization-get-oauthauthorize).

Recheck enabled app state and current callback registration at completion and
exchange. Serialize callback removal and app disablement with these operations
using a consistent lock order. If removal wins, show a local error; do not redirect
to the removed URL. A response committed before removal cannot be recalled.

## Transactions and failures

Codes, browser sessions, consent, grants and refresh state live in PostgreSQL.
Before implementing each operation, define isolation, lock order, uniqueness
constraints, expiry and cleanup. Database failures fail closed.

Code exchange atomically consumes the code and creates its grant and optional
refresh family. Constraints and locking prevent duplicate consumption/families.
Prepare signed ID tokens and opaque credentials before commit; return them only after
confirmed commit. Signing failures roll back the transaction.

A confirmed rollback leaves a code usable within its original lifetime.
Allow a bounded client retry only with an explicitly defined safe-retry signal.
An uncertain exchange commit or lost success response requires fresh authorization;
a generic error or timeout never proves rollback.

Generate high-entropy opaque access tokens and store only their verifiers with
the issuing client, user/sector subject mapping, scopes, Crow resource audience,
issuance/expiry and grant relationship. Each resource request resolves the
verifier and checks current app/grant state; missing, expired or revoked state
grants no access. Raw tokens are returned only after confirmed commit.

Retain consumed-code verifiers and issued-grant relationships until the grant can
no longer be used. A replay passing original-client authentication, callback and
PKCE bindings revokes its refresh family, if any, and marks its access-token grant
revoked. Invalid bindings must not revoke another grant. Resource APIs check grant
state until token expiry. App-local sessions cannot be remotely cleared this way.
The access-token record links directly to its stored grant.

## Token signing and keys

[API contract](API_CONTRACT.md#token-claims-and-validation) owns ID-token claims
and protocol validation. Sign ID tokens with a securely provisioned 3072-bit RSA private key.
Never commit private keys or bake them into images.

Rotate keys every 90 days. Publish new public keys at least ten minutes before
activation across instances; retain old public keys for at least fifteen minutes
after all instances stop signing with them. All signers must acknowledge rollout;
a lagging signer extends retention. Set bounded JWKS cache lifetimes and
rate-limited refresh on unknown kid before exposure.

Define emergency replacement and rejection procedures. External caches can delay
rejection, and a compromised signer can forge trusted tokens.

## Refresh and revocation

**Deferred:** refresh is outside the first complete OIDC integration.

Use high-entropy opaque refresh credentials stored as verifiers with user,
client, family, scopes, creation time, last successful use, absolute deadline and
revocation state. Retain spent-token evidence until family expiry.
Lock family/token state; rotate atomically and return credentials only after
confirmed commit. Never extend the original absolute deadline.

**Unresolved:** duplicate/retry behavior, any grace window, theft response and
recovery after an ambiguous commit or lost response. Do not implement refresh
until the policy is accepted and its protocol outcomes are recorded in the API
contract. The decision must prevent independent successor branches and define
whether spent credentials can produce any response.
If replay is selected, define protected response storage, retention and deletion;
the existing verifier-only model cannot reconstruct a returned refresh token.
Provide a reference backend covering concurrent requests and failures.

Coordinate refresh and disablement through durable app state and transaction
locking. Disablement revokes families; re-enabling never restores them.
Missing, expired or revoked state grants no tokens. Product owns logout effects.

## Operations and deployment gates

Use generic login failures, temporary normalized-account/IP throttling and
separate registration/exchange/refresh limits. Shared deployments need shared
budgets. Choose numeric limits, trusted proxies and limiter-outage behavior before
exposure. Define security-log access and retention without storing credentials.

Before deployment, define connection budgets, timeouts, graceful shutdown,
migrations, metrics, readiness, durability, backups and restore procedures.
HTTP liveness remains distinct from database readiness.
A restore can resurrect consumed tokens and revoked sessions: invalidate affected
sessions, flows/codes and grants before serving restored state.

Any app/grant cache needs explicit invalidation and maximum staleness. Add caches,
partitioning or services only when measurements justify them.
[Implementation](IMPLEMENTATION.md) assigns verification to each milestone.
