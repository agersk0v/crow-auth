# OAuth 2.0 and OpenID Connect API contract

## Status

This document owns milestone endpoint and protocol details, including browser
and management handlers. **Accepted** means chosen planned behavior; **Deferred**
is outside this milestone. Neither implies implementation.

The milestone contracts are accepted specifications. Internal account/mail and
pre-auth/CSRF/verification-context primitives exist, but the browser, OAuth and
portal endpoints below remain unimplemented. The HTTP service exposes only
GET /health; see [browser foundations](BROWSER.md) and
[implementation evidence](IMPLEMENTATION.md) for current status.
The current server exposes only `GET /health`. See
[implementation status and evidence](IMPLEMENTATION.md#status-and-readiness).

Follow OIDC Core, OAuth 2.0 security best practice (RFC 9700), PKCE (RFC 7636),
discovery, Bearer token usage and RFC 7009 revocation for supported features.

## Discovery and public keys

The issuer is one configured HTTPS URL, never inferred from Host headers.
`GET /.well-known/openid-configuration` publishes that exact `issuer` plus
`authorization_endpoint`, `token_endpoint`, `userinfo_endpoint` and `jwks_uri`
using absolute URLs. Publish only implemented, verified capabilities:

- `response_types_supported`: `["code"]`
- `response_modes_supported`: `["query"]`
- `authorization_response_iss_parameter_supported`: `true` (RFC 9207)
- `grant_types_supported`: `["authorization_code"]`; add `refresh_token` only
  after the deferred refresh milestone is implemented and verified.
- `subject_types_supported`: `["pairwise"]` (stable opaque subjects per sector)
- `id_token_signing_alg_values_supported`: `["RS256"]`
- `token_endpoint_auth_methods_supported`: `["client_secret_basic"]`
- **Deferred:** publish `revocation_endpoint` and
  `revocation_endpoint_auth_methods_supported: ["client_secret_basic"]` only
  when refresh-token revocation is supported.
- `code_challenge_methods_supported`: `["S256"]`
- `scopes_supported`: `["openid", "profile"]` initially. Add `crow.subscription`
  and `offline_access` only when their respective milestones are supported.
- `claims_supported`: `["iss", "sub", "aud", "iat", "exp", "auth_time", "nonce", "preferred_username"]`
  when the milestone is implemented. No email claim or email scope is supported.

`GET /.well-known/jwks.json` exposes public RSA keys only: `kid`, `kty=RSA`,
`use=sig`, `alg=RS256`, `n`, `e`. Follow Architecture key rotation/cache rules.
These two endpoints are public and do not need client authentication.

## Pairwise subject sectors

Follow OIDC pairwise subject rules. Initially the sector is the hostname of the
registered redirect URI; all callbacks for one client must share that hostname.
Clients in the same sector receive the same subject for a user.
Custom sector_identifier_uri registration is unsupported initially. A change of
sector changes subject identity: reject sector-changing callback edits until an
explicit migration procedure exists.

## Authorization: GET /oauth/authorize

Required: `response_type=code`, `client_id`, exact registered `redirect_uri`,
`scope` including `openid`, unpredictable browser-session-bound `state`, `nonce`,
`code_challenge` and `code_challenge_method=S256`. Crow requires state and nonce
as its security policy. Validate PKCE challenge syntax (43 base64url characters without padding) and S256;
validate the verifier/challenge relationship at exchange per RFC 7636. Clients must generate verifiers with cryptographically secure randomness;
Crow cannot measure their entropy from a submitted value.
Bound state/nonce to 1–512 ASCII unreserved characters; require nonempty client_id
and reject request URLs longer than 8 KiB. Client backends generate state and nonce
from at least 32 random bytes each and compare exact strings.
Only confidential web clients are supported; no implicit or password grants.

Validate the complete request before creating a browser-bound 15-minute flow.
Store nonce, scopes, client, callback, state and challenge server-side. Show
Crow-hosted login/registration when no usable Crow browser session exists, then
obtain required consent. A valid session enables SSO with its original auth_time.
`prompt=login` forces fresh authentication; `prompt=consent` forces consent.
`prompt=none` must be used alone: return a code without UI only when session,
freshness and consent requirements are satisfied; otherwise return login_required
or consent_required to the validated callback. Offline-access grants requiring
explicit consent cannot complete silently.
Support nonnegative integer `max_age` in seconds: reauthenticate when the original
authentication is older; zero always requires fresh authentication. Include the
actual credential authentication time as `auth_time` in ID tokens. Reject unsupported
prompt values/combinations and unsupported request parameters with invalid_request.
Reject duplicate parameters, response_mode other than query, request/request_uri,
claims, resource and unsupported display/ui_locales/login_hint parameters.
Use unsupported_response_type for response_type other than code. First validate
client and redirect without navigation; only then return parameter errors to that
callback. Disabled/unknown clients and unregistered callbacks produce local HTTP
400. Expired, consumed or browser-mismatched flows show local HTTP 400 with a
restart link and never emit a code. An authorization cancellation with a still
valid bound flow returns access_denied; dependency failure shows local HTTP 503.

| prompt | Behavior |
| --- | --- |
| omitted | Reuse valid SSO; obtain missing consent |
| login | Require fresh authentication |
| consent | Force consent; authenticate if necessary |
| login consent (either order) | Require both fresh authentication and consent |
| none | No UI; return login_required or consent_required when necessary |
| Any other value, duplicate value, or combination with none | invalid_request |

If both authentication and consent are missing, return login_required first.
For prompt=none with max_age=0, return login_required without UI.
Do not advertise request objects, dynamic client registration or other extensions.

Supported scope meanings and consent requirements are owned by
[Product](PRODUCT.md#login-and-consent). Reject unknown or not-yet-supported scopes
with invalid_scope. The first integration rejects offline_access and
crow.subscription and never issues refresh tokens.

Successful completion redirects using query response mode with `code` and original
`state` (and issuer identification per RFC 9207). Codes last 60 seconds and bind
user, client, callback, nonce, scopes, authentication time and PKCE challenge.
Denial returns `error=access_denied` and original state to a validated callback.
Include `iss` on success and applicable error redirects as required by RFC 9207.
Invalid client/callback requests display a local error, never redirect. Other
errors use OAuth/OIDC authorization error parameters, never custom JSON wrappers. Never reflect raw request values in error_description;
use stable safe messages. Unknown scopes use invalid_scope, missing/malformed
parameters invalid_request, and user denial access_denied.
Preserve existing callback query parameters through proper URL encoding.

## Token: POST /oauth/token

Use `application/x-www-form-urlencoded` and HTTP Basic client authentication
(`client_secret_basic`, with OAuth credential encoding). Never accept client secrets
in URLs or require custom Crow authentication headers. Reject duplicate parameters
and multiple authentication mechanisms. Responses use `Cache-Control: no-store`
and `Pragma: no-cache`. Redact credentials, tokens and callback queries in logs.

Reject a verifier outside 43–128 ASCII unreserved characters or an incorrect
S256 digest with invalid_grant. Reject missing required fields with invalid_request.

Code exchange fields: `grant_type=authorization_code`, `code`, `redirect_uri`,
`code_verifier`. Validate client, enabled state, exact callback, expiry and all
code/PKCE bindings. Follow [Architecture's transaction and replay rules](ARCHITECTURE.md#transactions-and-failures).
Code replay returns invalid_grant; invalid bindings must not revoke legitimate grants.

Initial code-exchange success:

```json
{"access_token":"<opaque-access-token>","token_type":"Bearer","expires_in":600,"id_token":"<ID-JWT>","scope":"openid profile"}
```

Token claims and validation are defined below.

**Deferred:** refresh fields: `grant_type=refresh_token`, `refresh_token`, optional `scope`.
Omitting scope preserves the grant; supplied scopes must be a subset. Rotation
cannot regain dropped scopes. Validate enabled client and family ownership.
Refresh returns a new access and refresh token, expires_in, token_type and scope;
omit ID tokens on refresh (OIDC permits this).
[Architecture](ARCHITECTURE.md#refresh-and-revocation) owns atomic rotation and the
unresolved duplicate/retry policy. Do not implement or publish refresh support
until that policy and its protocol outcomes are resolved.

OAuth token errors are top-level JSON, for example:

```json
{"error":"invalid_grant","error_description":"The grant is invalid or expired."}
```

Use HTTP 400 for standard errors: invalid_request, invalid_grant, invalid_scope,
unauthorized_client, unsupported_grant_type; invalid_client uses HTTP 401 and an
appropriate WWW-Authenticate challenge for failed Basic authentication. Dependency
failure returns HTTP 503 with `{"message":"Service unavailable."}` and no tokens;
this body is operational diagnostic information; clients treat the HTTP status
as an availability failure, not a retryable OAuth grant outcome. A generic 503
never proves rollback. An uncertain code-exchange commit or lost exchange success requires fresh
authorization; refresh failure handling remains subject to its design gate.
Milestone-one clients never retry code exchange, even after confirmed rollback;
no safe-retry signal is offered.

## Token claims and validation

Access tokens are opaque Bearer credentials, not JWTs. Their storage and durable
validation are owned by [Architecture](ARCHITECTURE.md#transactions-and-failures).
Clients must not parse them. Crow resources require a live token, their resource
audience, enabled issuing client, required scopes and a non-revoked grant, then
resolve its subject in the issuing client's sector context.

ID tokens use `typ=JWT`, RS256 and the single integrating client as `aud`;
include `iss`, the same pairwise `sub`, `iat`, `exp`, original `auth_time`
and request `nonce`. The profile scope permits preferred_username.
Lifetimes are owned by [Product](PRODUCT.md#sessions-and-failure-behavior).
Subscription status is never a token claim.

For ID tokens, validate signature, explicit RS256 allowlist, known key, exact configured issuer,
claim types, expiry and issuance time with 30 seconds clock tolerance. Require
nonempty sub, iat <= now+30, exp > iat, and exactly the client ID as string aud;
Crow does not issue multi-audience ID tokens. Header kid is required; ignore
untrusted jku/x5u and never select algorithms from the token alone.
ID tokens cannot authorize resource APIs.
Clients validate ID-token client audience and nonce against the initiating
browser session, after validating state at the callback. Store requested max_age
with that initiating session. When max_age was requested, require a numeric auth_time,
reject future values beyond the 30-second clock tolerance, and, for positive max_age, verify authentication
age against the requested max_age with that tolerance. For max_age=0, verify fresh
authentication against the initiating request time (allowing clock tolerance),
rather than requiring zero elapsed time at token receipt. Reject missing, malformed
or stale auth_time. Compare UserInfo sub to
the ID-token sub. Key provisioning and rotation belong to
[Architecture](ARCHITECTURE.md#token-signing-and-keys).

## UserInfo: GET /oauth/userinfo

Require `Authorization: Bearer <access-token>`, no additional client secret.
Apply [token validation](#token-claims-and-validation) and require openid scope.
The access-token resource audience is the fixed configured UserInfo endpoint URL;
no audience selection parameter is supported. Return `{"sub":"<pairwise-opaque-subject>"}` and include preferred_username
when profile is granted. Use RFC 6750: missing credentials receive HTTP 401 with `WWW-Authenticate: Bearer`
and no error field; invalid/expired/revoked/wrong-audience credentials receive 401
with `Bearer error="invalid_token"`; insufficient scope receives 403 with
`Bearer error="insufficient_scope", scope="openid"`. Malformed/multiple credential
methods receive 400 invalid_request. Success is JSON with Cache-Control: no-store.

## Subscription: GET /api/subscription

**Deferred:** implement after the first complete OIDC integration.

Require Bearer access token with the same validation and crow.subscription scope.
Return `{"subscriber":false}` before Stripe. This is a custom protected resource,
not an OIDC claim. No caller-supplied user ID or public subscription writes.

## Revocation: POST /oauth/revoke

**Deferred:** publish with refresh support after its decision gate is resolved.

Follow RFC 7009: form fields `token` and optional `token_type_hint=refresh_token`,
with client_secret_basic authentication. Revoke only the authenticated client's
corresponding refresh family, retaining spent-token evidence. Return HTTP 200
with an empty body for revoked, already revoked or unknown tokens, without
revealing other clients' sessions. Only refresh-token revocation is supported;
report unsupported_token_type for unsupported token types as RFC 7009 specifies.
The hint is advisory: ignore unrecognized hints and search supported token stores
when the hinted lookup misses. An unsupported hint alone is not an error. A known
access token may return unsupported_token_type; unknown tokens still receive 200.
Dependency failure is 503, not a successful revocation. The app clears local state
even on failure; existing access tokens expire naturally. This is app logout, not
OIDC RP-initiated logout; discovery must not advertise end_session_endpoint.

## Crow browser and portal endpoints

These are same-origin internal browser handlers, not OAuth grants.
Axum serves templates and accepts ordinary form submissions; HTMX may request
HTML fragments. Full-page and enhanced requests use identical authorization and
[browser security controls](ARCHITECTURE.md#browser-security-and-sessions).
Successful ordinary forms use a 303 redirect to a server-selected next page or
validated callback. HTMX responses may use HX-Redirect for full navigation;
never let submitted URLs select an unchecked redirect. Sensitive responses use
no-store, and responses varying by HX-Request must vary their cache key.

GET /login without a bound app flow is portal login; /oauth/authorize starts app
flows and routes to hosted UI. GET /register preserves a validated flow.
Expired/invalid app flows require restart, never a fallback to portal login.

**Accepted:** internal browser handlers:

- POST /api/hosted/login and /api/hosted/register: username/password (registration additionally requires email) plus CSRF
  and an optional bound flow; enforce [Product account rules](PRODUCT.md#accounts).
- POST /api/hosted/consent: approve/deny an authenticated pending flow.
- GET /api/portal/account: render the signed-in user's own account.
- POST /api/portal/logout: revoke the browser session and clear its cookie.

Verified-account password login continues to /account; bound app login continues
to consent or callback. These logins establish the Crow browser session under Product's rules.
Never return app tokens to browser handlers. Flow state and identity binding
follow [Architecture](ARCHITECTURE.md#authorization-state-machine).

## Account lifecycle browser contracts

All paths below are **Accepted**, same-origin form handlers using the browser
security rules above. GET forms issue CSRF state; POST requires it and Origin.
Passwords, action tokens and secrets are never query parameters on POST. Email
links use a 256-bit random token in the GET query; immediately exchange it for a
short-lived server-bound form context and 303 to a clean URL. Use no-store,
Referrer-Policy: no-referrer, no third-party assets and redacted access logs on
these routes. GET never verifies, resets or deletes an account.

| Method/path | Fields and committed outcome |
| --- | --- |
| POST /api/hosted/register | username, email, password, optional flow; generic 303 to /verify-email; no login cookie |
| GET /verify-email | Instructions/resend form; token link prepares confirmation form |
| POST /api/hosted/verify-email | Bound action context; activate account; 303 /login, preserving only valid original-browser flow |
| POST /api/hosted/resend-verification | email; generic 303 /verify-email |
| GET /recover | Email request form |
| POST /api/hosted/recover | email; generic 303 /recover/sent |
| GET /reset-password | Token link prepares new-password form |
| POST /api/hosted/reset-password | Bound action context, new_password; consume, invalidate account credentials; 303 /login |
| POST /api/portal/password | current_password, new_password; replace/invalidate; 303 /login |
| POST /api/portal/reauthenticate | current_password; refresh server-side sensitive-action timestamp only; 303 /account |
| POST /api/portal/account/delete | confirmation=Delete account, fresh reauthentication; delete and invalidate; 303 /account-deleted |

Ordinary login uses username/password. Invalid login returns generic HTTP 400
for unknown, wrong-password and deleted accounts; no identity disclosure. Valid
credentials for pending accounts show verification guidance without a session.
Validation failures return HTML 400 with safe field errors, invalid action contexts
400 with restart guidance, unauthenticated portal access 303 /login, stale sensitive
reauthentication 403 with reauthentication link, CSRF/Origin failure 403, throttling
429 and dependency failure 503. Never return successful redirect before commit.
Register/reset/change enforce identical password rules. Invalid inputs never echo
passwords/tokens. Unknown/deleted recovery emails produce the same status, page and
asynchronous timing path as active emails. Failed delivery does not undo a committed
registration; resend can recover. Deletion commit uncertainty shows a safe 503;
retry resolves to signed-out/deleted state if committed, otherwise requires fresh
authentication. No endpoint returns another user's account or a raw Crow token.

## Developer management

**Accepted:** handler shapes below; Product owns management behavior.

All routes require an active verified portal session; mutations additionally require
CSRF and Origin. App IDs are opaque UUIDs distinct from public client IDs; ownership
checks use app IDs. Successful mutations use 303 to the owned app page; creation
and secret rotation redirect to a no-store once-only secret page bound to that
owner/session, with plaintext erased from transient storage on first view or after
five minutes. Failed validation or the two-active-secret limit returns HTML 400;
unknown/non-owned objects 404, stale reauthentication 403, dependency failure 503.
Revocation/disable/enable are idempotent for the owner. All mutations and active
secret counting serialize under the app row lock. Revoking the last secret is
allowed, with a warning; the owner can issue a replacement through the portal.
Ownership comes from stored owner UUID, never supplied request owner IDs.
Browser mutations use POST action routes accepting ordinary form submissions;
no method override or JavaScript is required. Apply the browser response rules above.

| Method and path | Request / result |
| --- | --- |
| GET /api/portal/applications | List only owned app summaries |
| POST /api/portal/applications | name and redirect_uris; create owned app and initial secret, displayed once |
| GET /api/portal/applications/{id} | Own configuration and secret metadata; no retrievable secret |
| POST /api/portal/applications/{id}/update | name and/or redirect_uris; validate full callback policy |
| POST /api/portal/applications/{id}/secrets | Create second secret if fewer than two active; return ID, created_at and plaintext once |
| POST /api/portal/applications/{id}/secrets/{secret_id}/revoke | Revoke owned app's secret; do not revoke user families |
| POST /api/portal/applications/{id}/disable | Block app operations and permanently revoke existing grants and refresh families |
| POST /api/portal/applications/{id}/enable | Re-enable configuration; never restore revoked grants or families |

No permanent app deletion, profile editing or billing mutations in this
milestone. Account deletion is specified in the lifecycle contracts above. Disabled apps can still be managed by their owner. Callback edits take the app row lock used by completion and exchange; a removed
callback must not be used for a later redirect/exchange. Recheck registration
immediately before committing completion/exchange; if removal commits first,
completion shows a local error and exchange returns invalid_grant. Durable disablement must remain
effective through PostgreSQL state checks and transaction locking coordinated
with exchange and refresh; cleanup alone is insufficient.

## Implementation and validation

Keep portal errors separate from OAuth token/authorization and Bearer resource
errors. Publish a standard OIDC client walkthrough covering discovery, state,
nonce, PKCE, ID-token verification (including max_age/auth_time checks) and UserInfo.
Cover missing, malformed, stale and future auth_time and max_age=0 in the reference
integration's verification scenarios. Extend the walkthrough with the
selected refresh coordination policy when that milestone is implemented. Run applicable
protocol/conformance tests and cross-client/token-substitution tests before claiming
compatibility. Completion evidence belongs in [Implementation](IMPLEMENTATION.md). Dynamic registration, upstream Google/Discord login, public
browser clients and OIDC logout are deferred; do not advertise them in discovery.
