# OAuth 2.0 and OpenID Connect API contract draft

## Status

[Product](PRODUCT.md) owns behavior and [Architecture](ARCHITECTURE.md) owns
security enforcement. OAuth 2.0 Authorization Code Flow with PKCE S256 and OIDC
Core are the chosen initial protocol. This is a planned contract, not implemented
compatibility or certification. Use OAuth 2.0 security best practice (RFC 9700),
PKCE (RFC 7636), discovery, Bearer token usage and RFC 7009 revocation.
Portal/browser-internal JSON shapes remain proposals; standard wire formats do not.

## Discovery and public keys

The issuer is one configured HTTPS URL, never inferred from Host headers.
`GET /.well-known/openid-configuration` publishes that exact `issuer` plus
`authorization_endpoint`, `token_endpoint`, `userinfo_endpoint`, `jwks_uri` and
`revocation_endpoint` using absolute URLs. Publish truthful supported metadata:

- `response_types_supported`: `["code"]`
- `response_modes_supported`: `["query"]`
- `authorization_response_iss_parameter_supported`: `true` (RFC 9207)
- `grant_types_supported`: `["authorization_code", "refresh_token"]`
- `subject_types_supported`: `["public"]` (the immutable shared Crow UUID)
- `id_token_signing_alg_values_supported`: `["RS256"]`
- `token_endpoint_auth_methods_supported`: `["client_secret_basic"]`
- `revocation_endpoint_auth_methods_supported`: `["client_secret_basic"]`
- `code_challenge_methods_supported`: `["S256"]`
- `scopes_supported`: `["openid", "profile", "crow.subscription", "offline_access"]`
- `claims_supported`: the ID-token/UserInfo claims actually implemented below.

`GET /.well-known/jwks.json` exposes public RSA keys only: `kid`, `kty=RSA`,
`use=sig`, `alg=RS256`, `n`, `e`. Follow Architecture key rotation/cache rules.
These two endpoints are public and do not need client authentication.

## Authorization: GET /oauth/authorize

Required: `response_type=code`, `client_id`, exact registered `redirect_uri`,
`scope` including `openid`, unpredictable browser-session-bound `state`, `nonce`,
`code_challenge` and `code_challenge_method=S256`. Crow requires state and nonce
as its security policy. Validate PKCE syntax and entropy per RFC 7636.
Only confidential web clients are supported; no implicit or password grants.

Validate the complete request before creating a browser-bound 15-minute flow.
Store nonce, scopes, client, callback, state and challenge server-side. Show
Crow-hosted login/registration and required consent. A portal session never
silently authenticates this flow. `prompt=none` returns `login_required`;
`prompt=login` forces authentication and `prompt=consent` forces consent.
Support `max_age` with fresh authentication and `auth_time`; reject unsupported
prompt values, combinations and request features with appropriate standard errors.
Do not advertise request objects, dynamic client registration or other extensions.

Scopes: `openid` releases `sub`; `profile` releases `preferred_username`;
`crow.subscription` authorizes subscription reads. `offline_access` requests a
refresh token and requires explicit consent for each new authorization grant.
Without that grant, issue no refresh token. Other scopes are invalid_scope.
Remember ordinary consent by user/client/permission version; expanding permissions
requires consent again. Registration authenticates the pending flow without a
second login; declining consent leaves the new account intact.

Successful completion redirects using query response mode with `code` and original
`state` (and issuer identification per RFC 9207). Codes last 60 seconds and bind
user, client, callback, nonce, scopes, authentication time and PKCE challenge.
Denial returns `error=access_denied` and original state to a validated callback.
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
`code_verifier`. Validate client, enabled state, exact callback and all code/PKCE
bindings. Consume the code and create the refresh family (only if offline access
was granted) in one PostgreSQL transaction. Prepare signed tokens before commit;
return them only after confirmed commit. Signing failure rolls back everything.

Example success with offline access:

```json
{"access_token":"<access-JWT>","token_type":"Bearer","expires_in":600,"id_token":"<ID-JWT>","refresh_token":"<opaque-token>","scope":"openid profile crow.subscription offline_access"}
```

ID tokens follow Architecture: RS256, client audience, issuer, UUID sub, timestamps,
auth_time and original nonce; profile permits preferred_username. Access tokens
have the Crow resource audience, issuing client_id and scopes. Never interchange
the two token types. The backend validates state before exchange and validates
the ID token including nonce before establishing its local browser session.

Refresh fields: `grant_type=refresh_token`, `refresh_token`, optional `scope`.
Omitting scope preserves the grant; supplied scopes must be a subset. Rotation
cannot regain dropped scopes. Validate enabled client and family ownership, lock
records and atomically consume/create successors and update idle expiry.
Commit family revocation on reuse even when returning invalid_grant. Preserve
the original 30-day deadline and seven-day idle limit. Refresh returns a new access
and refresh token, expires_in, token_type and scope; omit ID tokens on refresh
(OIDC permits this). No grace window or idempotent response replay.

OAuth token errors are top-level JSON, for example:

```json
{"error":"invalid_grant","error_description":"The grant is invalid or expired."}
```

Use standard errors/statuses: invalid_request, invalid_grant, invalid_scope,
unauthorized_client, unsupported_grant_type; invalid_client uses HTTP 401 and an
appropriate WWW-Authenticate challenge for failed Basic authentication. Dependency
failure returns HTTP 503 with a safe error response and no tokens. A generic 503
never proves rollback. An uncertain commit or lost success response requires fresh
authorization; clients must not blindly retry. Define any explicit safe-retry
signal before allowing bounded retries after confirmed rollback.

## UserInfo: GET /oauth/userinfo

Require `Authorization: Bearer <access-token>`, no additional client secret.
Validate signature, issuer, Crow resource audience, expiry, enabled client_id and
openid scope; derive user from sub. Return `{"sub":"<UUID>"}` and include
`preferred_username` when profile is granted. Clients must compare this sub to
the ID token's sub. Never return another user's data or accept an ID token.
Use RFC 6750 Bearer errors/challenges, including insufficient_scope where applicable.

## Subscription: GET /api/subscription

Require Bearer access token with the same validation and crow.subscription scope.
Return `{"subscriber":false}` before Stripe. This is a custom protected resource,
not an OIDC claim. No caller-supplied user ID or public subscription writes.

## Revocation: POST /oauth/revoke

Follow RFC 7009: form fields `token` and optional `token_type_hint=refresh_token`,
with client_secret_basic authentication. Revoke only the authenticated client's
corresponding refresh family, retaining spent-token evidence. Return HTTP 200
with an empty body for revoked, already revoked or unknown tokens, without
revealing other clients' sessions. Only refresh-token revocation is supported;
report unsupported_token_type for unsupported token types as RFC 7009 specifies.
Dependency failure is 503, not a successful revocation. The app clears local state
even on failure; existing access JWTs expire naturally. This is app logout, not
OIDC RP-initiated logout; discovery must not advertise end_session_endpoint.

## Crow browser and portal endpoints

These are internal JSON APIs, not OAuth grants. GET /login without a bound app
flow is portal login; /oauth/authorize starts app flows and routes to hosted UI.
GET /register preserves a validated flow. Expired/invalid app flows require restart
and never silently fall back to portal login. POST /api/hosted/login and
/api/hosted/register accept username/password only from Crow pages with browser
binding, CSRF and Origin checks. Passwords contain 15–128 Unicode code points.
POST /api/hosted/consent approves/denies an authenticated pending flow.

Use a server-side authenticated-flow stage; the browser cannot select a user ID.
Navigation preserves the binding, including login/register transitions. Success
returns an internal next-step or validated redirect URL; never app tokens.
Direct portal login/registration sets a separate eight-hour portal cookie and
redirects to /account. App login never sets that cookie. GET /api/portal/account
reads the portal session; POST /api/portal/logout invalidates it with CSRF/Origin
checks. Secure, HttpOnly, host-only cookies, framing protection, request bounds,
credential-free logs and generic login failures follow Architecture. Registration
discloses unavailable recovery. No application-facing password grant exists.

## Developer management proposals

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
nonce, PKCE, ID-token verification, UserInfo and serialized refresh. Run applicable
protocol/conformance tests and cross-client/token-substitution tests before claiming
compatibility. Dynamic registration, upstream Google/Discord login, SSO, public
browser clients and OIDC logout are deferred; do not advertise them in discovery.
