# OAuth 2.0 and OpenID Connect API contract draft

## Status

[Product](PRODUCT.md) owns behavior and [Architecture](ARCHITECTURE.md) owns
security enforcement. OAuth 2.0 Authorization Code Flow with PKCE S256 and OIDC
Core are the chosen initial protocol. This is a planned contract, not implemented
compatibility or certification. Use OAuth 2.0 security best practice (RFC 9700),
PKCE (RFC 7636), discovery, Bearer token usage and RFC 7009 revocation.
Internal browser/management endpoint shapes are proposals; standard wire formats
follow the cited specifications. **Accepted:** protocol choices below apply to the planned initial integration
unless labeled **Proposed**, **Unresolved** or **Deferred**. Decision labels are
defined in [AGENTS.md](../AGENTS.md). Changes must update their owning document and the
relevant milestone evidence.

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
- `claims_supported`: the ID-token/UserInfo claims actually implemented below.

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
as its security policy. Validate PKCE syntax, length, S256 and the verifier/challenge relationship per
RFC 7636. Clients must generate verifiers with cryptographically secure randomness;
Crow cannot measure their entropy from a submitted value.
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
prompt values, combinations and request features with appropriate standard errors.

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
errors use OAuth/OIDC authorization error parameters, never custom JSON wrappers.
Preserve existing callback query parameters through proper URL encoding.

## Token: POST /oauth/token

Use `application/x-www-form-urlencoded` and HTTP Basic client authentication
(`client_secret_basic`, with OAuth credential encoding). Never accept client secrets
in URLs or require custom Crow authentication headers. Reject duplicate parameters
and multiple authentication mechanisms. Responses use `Cache-Control: no-store`
and `Pragma: no-cache`. Redact credentials, tokens and callback queries in logs.

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

Use standard errors/statuses: invalid_request, invalid_grant, invalid_scope,
unauthorized_client, unsupported_grant_type; invalid_client uses HTTP 401 and an
appropriate WWW-Authenticate challenge for failed Basic authentication. Dependency
failure returns HTTP 503 with a safe operational error response and no tokens;
this is an HTTP availability failure, not an additional OAuth token error code. A generic 503
never proves rollback. An uncertain code-exchange commit or lost exchange success requires fresh
authorization; refresh failure handling remains subject to its design gate. Define any explicit safe-retry
signal before allowing bounded retries after confirmed rollback.

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
claim types, expiry and issuance time with 30 seconds clock tolerance.
ID tokens cannot authorize resource APIs.
Clients validate ID-token client audience and nonce against the initiating
browser session, after validating state at the callback. Compare UserInfo sub to
the ID-token sub. Key provisioning and rotation belong to
[Architecture](ARCHITECTURE.md#token-signing-and-keys).

## UserInfo: GET /oauth/userinfo

Require `Authorization: Bearer <access-token>`, no additional client secret.
Apply [token validation](#token-claims-and-validation) and require openid scope.
Return `{"sub":"<pairwise-opaque-subject>"}` and include preferred_username
when profile is granted. Use RFC 6750 Bearer errors/challenges, including
insufficient_scope where applicable.

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

**Proposed:** internal browser handlers:

- POST /api/hosted/login and /api/hosted/register: username/password plus CSRF
  and an optional bound flow; enforce [Product account rules](PRODUCT.md#accounts).
- POST /api/hosted/consent: approve/deny an authenticated pending flow.
- GET /api/portal/account: render the signed-in user's own account.
- POST /api/portal/logout: revoke the browser session and clear its cookie.

Direct credential success continues to /account; app flows continue to consent
or callback. Both establish the Crow browser session under Product's rules.
Never return app tokens to browser handlers. Flow state and identity binding
follow [Architecture](ARCHITECTURE.md#authorization-state-machine).

## Developer management proposals

**Proposed:** handler shapes below; Product owns management behavior.

All routes require portal session; mutations additionally require CSRF and Origin.
Ownership comes from stored owner UUID, never supplied request owner IDs.

| Method and path | Request / result |
| --- | --- |
| GET /api/portal/applications | List only owned app summaries |
| POST /api/portal/applications | name and redirect_uris; create owned app and initial secret, displayed once |
| GET /api/portal/applications/{id} | Own configuration and secret metadata; no retrievable secret |
| PATCH /api/portal/applications/{id} | name and/or redirect_uris; validate full callback policy |
| POST /api/portal/applications/{id}/secrets | Create second secret if fewer than two active; return ID, created_at and plaintext once |
| DELETE /api/portal/applications/{id}/secrets/{secret_id} | Revoke owned app's secret; do not revoke user families |
| POST /api/portal/applications/{id}/disable | Block all supported app operations and revoke refresh families |
| POST /api/portal/applications/{id}/enable | Re-enable configuration; never restore revoked families |

No permanent app deletion, account editing/deletion or billing mutations in this
milestone. Disabled apps can still be managed by their owner. Specify atomic
callback changes versus in-flight flows before implementation; a removed callback
must not be used for a later redirect/exchange. Durable disablement must remain
effective through PostgreSQL state checks and transaction locking coordinated
with exchange and refresh; cleanup alone is insufficient.

## Implementation and validation

Rust owns signing and session operations; PostgreSQL is authoritative. Finalize
transaction isolation, lock order, uniqueness constraints and restore invalidation.
Keep portal errors separate from OAuth token/authorization and Bearer resource
errors. Publish a standard OIDC client walkthrough covering discovery, state,
nonce, PKCE, ID-token verification and UserInfo. Extend the walkthrough with the
selected refresh coordination policy when that milestone is implemented. Run applicable
protocol/conformance tests and cross-client/token-substitution tests before claiming
compatibility. Completion evidence belongs in [Implementation](IMPLEMENTATION.md). Dynamic registration, upstream Google/Discord login, public
browser clients and OIDC logout are deferred; do not advertise them in discovery.
