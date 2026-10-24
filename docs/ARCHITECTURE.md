# Crow Auth architecture and security

[Product](PRODUCT.md) owns behavior and lifetimes; this document owns technical
enforcement. [API contract](API_CONTRACT.md) proposes wire formats.
[README](../README.md) owns runnable status and development commands.
These are agreed design decisions, not implemented features.

## Services and trust boundaries

| Component | Responsibility |
| --- | --- |
| Rust/Axum | Accounts, password verification, app ownership/configuration, authorization flows/codes, portal sessions, refresh creation/rotation/revocation, all JWT signing and public APIs |
| PostgreSQL | Accounts, app configuration, consent, subscriptions, portal sessions, pending flows and codes |
| Redis | Refresh families, token verifiers, spent-token evidence and revocation state |
| Next.js | Hosted login/registration, account and developer pages |
| Stripe | Deferred billing integration |

Rust owns all session operations; there is no Go service. Pages and `/api`
share one public HTTPS origin, routed to Next.js and Rust. The production domain
is undecided. PostgreSQL/Redis are private, with authenticated connections and
transport protection appropriate to deployment.

Enforce the [Product permissions](PRODUCT.md#permissions) server-side. Derive
ownership from verified identity and stored records. Identity/subscription APIs
require client credentials plus a user JWT with matching audience; derive user
from `sub` and reject disabled apps. App JWTs never authorize portal management.
Do not expose private-resource existence through errors.

## Passwords and secrets

Apply [Product account rules](PRODUCT.md#accounts), enforcing normalized username
uniqueness in PostgreSQL. Count passwords as Unicode code points without
normalization. Hash with Argon2id, unique random salts and stored cost parameters.
Request-size bounds must permit the full supported UTF-8 password. Never send
plaintext passwords to third parties.

Generate high-entropy client secrets, show once and store only verifiers.
Use the Product secret limits and revocation behavior. Never log passwords,
tokens, authorization headers, client secrets, signing keys or database
credentials; never commit private keys or bake them into images.

## Browser sessions and hosted flows

App backends retain both Crow tokens in protected server storage. Their browsers
receive Secure, HttpOnly random session-handle cookies. Apps own their cookie
and CSRF protection and must serialize refreshes across backend instances.

Portal sessions use PostgreSQL records with verifiers of high-entropy random
tokens. Cookies are host-only, Secure, HttpOnly and SameSite=Lax. Logout
invalidates the record and expires the cookie. Cookie-authenticated mutations,
including hosted credential submission, require CSRF and origin checks.
Prevent framing of hosted login/registration. CORS is not authentication.

Bind pending requests to the initiating browser using a random flow cookie.
Store client ID, exact callback, state and PKCE S256 challenge in PostgreSQL;
never passwords. The cookie grants no portal permissions. After credential
verification, keep an authenticated-flow stage server-side; a browser cannot
choose the user ID. Consume flow completion once, after required consent.
Navigation between login/register preserves the validated flow.

Remember consent by user/client and permission version. Apply
[Product consent behavior](PRODUCT.md#hosted-login-and-consent), including denial
and registration outcomes. Remembered consent never replaces authentication.

Require HTTPS callbacks except HTTP on exact `localhost` or `127.0.0.1` hosts.
Match the complete registered URL exactly, including path, query and port;
reject fragments and wildcards. A removed callback cannot be used for later
redirect or exchange. Codes bind user, client, callback and PKCE challenge.
Require client authentication, PKCE S256 and atomic code consumption. Reject
invalid/incomplete parameters, expired/reused codes and incorrect verifiers.
Never redirect to an unvalidated callback, including on errors.

## Initial exchange and partial failures

Codes live in PostgreSQL and refresh sessions in Redis; there is no shared
transaction. The confirmed policy is consume once, then restart after failure:

1. Validate client, enabled state, code bindings, expiry and PKCE.
2. Atomically consume the code and confirm PostgreSQL commit.
3. Only the winning consumer creates the Redis refresh family.
4. Return tokens only after signing succeeds and session creation is confirmed.

Never restore a consumed code or retry creation after an uncertain result.
Failure after consumption, uncertain PostgreSQL commit or a lost public success
response requires fresh hosted authorization. Retry within the original code
lifetime is allowed only when non-consumption is confirmed. A timeout does not
prove rollback.

Best-effort revoke known orphan families after failure; expiry bounds records
if cleanup fails. Concurrent exchanges cannot create duplicate usable families.
No durable exchange-recovery state machine or token-response replay is required.

## JWT contract and key rotation

Rust signs initial and refreshed JWTs with a securely provisioned 3072-bit RSA
private key and RS256. Header: `alg=RS256`, `kid`, `typ=JWT`.
Claims: `iss` (configured issuer), `sub` (immutable user UUID), `aud` (one
client ID), `iat`, `exp`, unique `jti`. Username/subscription are fetched from
APIs. JWTs are signed, not encrypted; use Product's access-token lifetime.

Validate signature, explicit RS256 allowlist, known key, exact issuer, expected
audience, UUID subject, claim types and time bounds. Allow 30 seconds of clock
tolerance and reject unreasonable future issuance. Issuer is configured, never
derived from untrusted Host/proxy headers. Apps receive only public keys.

Publish JWKS. Rotate every 90 days; prepublish public keys at least ten minutes
before activation. Retain old public keys at least fifteen minutes after all
Rust instances stop using them. Require rollout acknowledgement; lagging signers
extend retention. Verifiers refresh on unknown `kid` with bounded caching and
rate limiting. Emergency compromise requires replacement/rejection policy;
external caches can delay rejection. A compromised signer can forge tokens.

## Refresh and Redis

Use high-entropy opaque refresh tokens. Store verifiers, user/client IDs, family
ID, original app session version, creation/last-use times, original absolute
deadline and revocation state. Retain spent-token evidence until family expiry.

Rust uses an atomic Redis operation/script to validate the token/family, consume
the old token, create its successor and update idle expiry. Rotation never
extends the original absolute deadline. Reuse atomically revokes the family
despite returning an error. Follow [Product session behavior](PRODUCT.md#sessions-and-failure-behavior)
for lifetimes, strict reuse, logout and lost responses.

Return tokens only after signing and confirmed session update. Fail closed on
Redis unavailability or failed writes; never reconstruct sessions from unverified
tokens. Missing records require login. Known restore/rollback must not resurrect
spent tokens or revoked sessions.

Every refresh authenticates client credentials and reads current enabled state
and session version directly from PostgreSQL, without a Redis app-status cache.
If that check is unavailable, return 503 before rotating or consuming the token.

## App disablement

Each app has a durable `session_version` in PostgreSQL. Disablement atomically
marks the app disabled and increments the version. Exchange copies the validated
version into its Redis family; refresh rejects disabled apps and version
mismatches. Rotation preserves the original version. Re-enabling does not reset
it, so failed Redis cleanup cannot revive old families.

Redis cleanup is best-effort; PostgreSQL version checks enforce logical
revocation. Coordinate concurrent exchange/refresh and disablement so stale
checks cannot bypass it. A family must not adopt a newer version without fresh
authorization valid for that version. Existing access JWTs expire naturally.

## Milestone checklist

Resolve implementation details as each feature is built; deployment settings
must be ready before public exposure. Future features remain in Product scope.

### During implementation

- Design account/session migrations, indexes and uniqueness constraints.
- Finalize endpoint schemas/errors, including non-owned app 403/404 handling and
  uniform non-disclosing invalid-token revocation responses.
- Implement browser CSRF issuance before login, cookie names and flow binding,
  including navigation, multiple tabs and expiry.
- Define exchange signing/write order, locking/constraints, ambiguous-write
  handling and orphan cleanup; implement atomic Redis rotation.
- Coordinate disablement/version and callback changes with in-flight requests.
- Specify consent permission versions and effects of app configuration changes.
- Publish app integration instructions covering backend token storage,
  serialized refresh, state/PKCE and JWT verification.

### Before public deployment

- Benchmark Argon2 costs; select/update the compromised-password dataset.
- Configure temporary account-and-IP login throttling, separate registration,
  exchange and refresh limits, generic login errors and limiter-outage behavior.
- Set request limits, trusted proxy rules, HTTPS origin and deployment secrets.
- Configure Redis persistence, noeviction/capacity handling, evidence expiry and
  restart/restore invalidation; test that recovery cannot revive old sessions.
- Define PostgreSQL durability/backups and invalidation of portal sessions,
  pending flows/codes and other affected state after restore.
- Set JWKS cache bounds, key provisioning/rotation and emergency rejection.
- Configure credential-free security logs, retention/access, monitoring and alerts.

## First milestone acceptance checks

- Hosted login/registration completes consent and code flow without creating a
  portal session; direct portal login/registration creates one.
- Apps cannot collect passwords or use direct password grants.
- Callback, PKCE, state, expiry and single-use validation work under races.
- Exchange failure returns no tokens; concurrent exchanges cannot create
  duplicate usable families.
- Client credentials alone reveal no user; app JWTs cannot manage the portal.
- Cross-client/user access and non-owner app management fail.
- Consent denial, flow expiry and registration outcomes match Product.
- Initial/refreshed JWTs, rotation overlap and unknown-key handling work.
- Atomic rotation, strict reuse, idle/absolute expiry and response loss match
  Product; failed Redis or PostgreSQL app-state checks grant no tokens.
- Disable/re-enable races and failed cleanup cannot revive old families;
  secret rotation preserves documented boundaries.
- Portal expiry/logout, cookie, CSRF, origin and framing controls work.
- Subscription is non-subscriber before Stripe; logs contain no credentials.
