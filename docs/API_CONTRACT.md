# API contract

## Status and compatibility

Accepted first-release protocol contract; none of these endpoints except `/health`
is implemented yet. Browser route names below are also planned. Follow OIDC Core,
OIDC Discovery, OAuth 2.0, RFC 9700 security best practice, PKCE RFC 7636, Bearer
usage RFC 6750 and issuer identification RFC 9207 for the supported surface.

Only confidential web clients are supported. There is no password grant, implicit
flow, client-credentials grant, refresh grant, dynamic registration, introspection,
revocation or OIDC logout endpoint. Unsupported features are not advertised.

## Discovery

Use one configured HTTPS issuer with no query or fragment and no trailing slash;
never derive it from untrusted request headers. Publish absolute endpoint URLs at
`GET /.well-known/openid-configuration`:

| Metadata | Value |
| --- | --- |
| `issuer` | Configured issuer |
| `authorization_endpoint` | `{issuer}/oauth/authorize` |
| `token_endpoint` | `{issuer}/oauth/token` |
| `userinfo_endpoint` | `{issuer}/oauth/userinfo` |
| `jwks_uri` | `{issuer}/.well-known/jwks.json` |
| `response_types_supported` | `["code"]` |
| `response_modes_supported` | `["query"]` |
| `grant_types_supported` | `["authorization_code"]` |
| `subject_types_supported` | `["pairwise"]` |
| `id_token_signing_alg_values_supported` | `["RS256"]` |
| `token_endpoint_auth_methods_supported` | `["client_secret_basic"]` |
| `code_challenge_methods_supported` | `["S256"]` |
| `scopes_supported` | `["openid", "profile"]` |
| `claims_supported` | `["iss", "sub", "aud", "exp", "iat", "auth_time", "nonce", "preferred_username"]` |
| `authorization_response_iss_parameter_supported` | `true` |

`GET /.well-known/jwks.json` publishes RSA public signing keys with `kid`, `kty`,
`use`, `alg`, `n` and `e`. Never publish private parameters. These endpoints are
public. Discovery must describe verified behavior, not future milestones.

## Callbacks

Register full absolute callback URLs. Require HTTPS, except HTTP on the exact
hosts `localhost` or `127.0.0.1` for development. No wildcards, fragments, embedded
credentials or user-controlled redirect forwarding. Match the full registered
string exactly, including path, port and query. Do not fetch callbacks.

All callbacks for a client must share a hostname. Removed callbacks are rejected
at authorization completion and exchange. Hostname-changing edits are rejected;
register a new app instead. Ports and paths may differ within the same hostname.

## Subjects

Use a stable cryptographically random opaque subject per account and sector.
The sector is the callback hostname, following OIDC pairwise subject rules.
Clients sharing that hostname receive the same subject for an account; different
sectors receive different subjects. Custom `sector_identifier_uri` is unsupported.
Do not equate a sector with an individual client. Never expose internal UUIDs or
use usernames/emails as subjects. Clients identify users by `(iss, sub)`.

## Authorization

`GET /oauth/authorize` accepts:

| Parameter | Requirement |
| --- | --- |
| `response_type` | Required, `code` |
| `client_id` | Required, enabled registered client |
| `redirect_uri` | Required, exact registered callback |
| `scope` | Required, includes `openid`; optional `profile` |
| `state` | Required nonempty client-generated unpredictable transaction binding |
| `nonce` | Required nonempty client-generated unpredictable ID-token binding |
| `code_challenge` | Required valid S256 challenge |
| `code_challenge_method` | Required, `S256` |
| `response_mode` | Optional, only `query` |
| `prompt` | Optional, rules below |
| `max_age` | Optional nonnegative integer seconds |

Reject duplicate parameters and malformed supported values. Unknown extension
parameters are ignored as required by OAuth; recognized unsupported features
such as request objects are rejected with the applicable standard error.
Unknown scopes, including `offline_access`, return `invalid_scope`.
PKCE syntax follows RFC 7636; plain challenges are unsupported.

Validate client and callback before any redirect. Invalid client/callback produces
a local error. Other errors use standard OAuth/OIDC error parameters at the
validated callback. Preserve its existing query parameters using proper URL
encoding. Include original state and issuer `iss` on successful and error
responses where RFC 9207 requires it. Never redirect to a removed callback.

| `prompt` | Result |
| --- | --- |
| Omitted | Reuse SSO, request missing consent |
| `login` | Require fresh credentials |
| `consent` | Force consent, authenticate if necessary |
| `login consent` in either order | Require both |
| `none` | No UI; return `login_required` or `consent_required` if needed |
| Other, duplicate, or `none` combined with another | `invalid_request` |

For `max_age`, require fresh authentication when the prior authentication is too
old; zero always forces it. `prompt=none` with `max_age=0` returns `login_required`.
When authentication and consent are both missing, report `login_required` first.

Store a browser-bound pending flow under Architecture rules. Approval atomically
completes it and issues a 60-second single-use code. Success redirects with
`code`, `state` and `iss`. Denial returns `error=access_denied`, state and issuer.
An expired or invalid flow must restart; it cannot become a direct portal login.

## Exchange

`POST /oauth/token` uses `application/x-www-form-urlencoded` and HTTP Basic client
authentication with OAuth credential encoding. Reject multiple authentication
methods, duplicate fields and secrets in the URL. Accept these form fields:

- `grant_type=authorization_code`
- `code`
- `redirect_uri`
- `code_verifier`

Check enabled client, active secret, exact callback, expiry, one-time code use
and all client/PKCE bindings. Follow Architecture transaction and replay rules.
Success is HTTP 200:

```json
{
  "access_token": "<opaque-token>",
  "token_type": "Bearer",
  "expires_in": 600,
  "id_token": "<signed-jwt>",
  "scope": "openid profile"
}
```

Return the actual granted scopes; never return a refresh token. All token
responses use `Cache-Control: no-store` and `Pragma: no-cache`.

Errors are top-level OAuth JSON, normally HTTP 400: `invalid_request`,
`invalid_grant`, `invalid_scope`, `unauthorized_client` or `unsupported_grant_type`.
Failed Basic authentication uses HTTP 401, `invalid_client` and an appropriate
`WWW-Authenticate` challenge. Descriptions must not disclose credentials or
account existence. An unavailable dependency returns HTTP 503 with a safe
operational message and no tokens; it is not a new OAuth error code. Timeouts,
503s and lost responses do not prove rollback. Start a new authorization rather
than automatically retrying exchange.

## Tokens

Access tokens are opaque Bearer credentials for Crow UserInfo only. Clients do
not parse them or use them as credentials for their own APIs. Crow checks current
account, app and grant state on every use, as specified in Architecture.

ID tokens are RS256 JWTs with a unique signing-key `kid`. Claims:

| Claim | Meaning |
| --- | --- |
| `iss` | Exact configured issuer |
| `sub` | Stable sector-specific opaque subject |
| `aud` | Single integrating client ID |
| `iat`, `exp` | Issuance and expiry NumericDates, ten-minute lifetime |
| `auth_time` | Actual credential authentication time, preserved during SSO |
| `nonce` | Original request nonce |
| `preferred_username` | Username, only when `profile` was granted |

ID tokens do not authorize UserInfo or portal actions. Clients validate the
signature with an explicit RS256 allowlist and trusted issuer JWKS, known key,
issuer, audience, required claim types, expiry, issuance time and nonce. Allow at
most 30 seconds clock tolerance. Reject future `auth_time` beyond that tolerance.

When `max_age` was requested, require valid `auth_time` and check authentication
age. For zero, compare authentication time to the initiating request time with
clock tolerance, rather than requiring zero elapsed time at token receipt.
Check state and authorization-response issuer before exchange. Match UserInfo
`sub` to the validated ID token. Never follow a token-provided key URL.

## UserInfo

`GET /oauth/userinfo` requires `Authorization: Bearer <access-token>` and `openid`.
Return HTTP 200 JSON:

```json
{"sub":"<opaque-subject>","preferred_username":"example_user"}
```

Omit `preferred_username` without `profile`. No email claim or other user ID input.
Use no-store and RFC 6750 challenges: missing/invalid credentials yield HTTP 401;
insufficient scope yields HTTP 403 with `insufficient_scope`. Database failure
returns 503, never a cached positive authorization decision.

## Browser routes

These are same-origin HTML handlers, not developer APIs. GET renders pages; POST
accepts form data with the CSRF and Origin controls in Architecture. Success uses
303 to a server-selected page or validated flow destination. Never return app
tokens from these handlers. Sensitive responses use no-store.

| Routes | Purpose |
| --- | --- |
| `GET/POST /register` | Create a pending account |
| `GET/POST /verify-email` | Display confirmation, then consume verification credential |
| `POST /verify-email/resend` | Replace verification credential and queue email |
| `GET/POST /login` | Authenticate, rotate session, continue portal or bound flow |
| `GET/POST /forgot-password` | Request reset; generic response whether account exists or not |
| `GET/POST /reset-password` | Show form, then atomically replace password using reset credential |
| `GET /account` | Own account summary |
| `POST /account/password` | Replace password after checking current password |
| `POST /logout` | End current Crow session and clear cookie |
| `GET/POST /consent` | Display or decide a bound authenticated app flow |

Email GET handlers stage the link credential in protected browser state and
redirect to a clean URL before rendering; GET never consumes it. POST requires
that staged credential, CSRF and Origin checks. Verification on another browser
does not authenticate the original flow. Pending accounts cannot gain normal
portal access through login.

## Developer portal routes

All routes require a verified portal session. Ownership is derived from that
session and checked on every request, including nested secret IDs. Return the
same not-found response for missing and other owners' apps. Mutations require
CSRF and Origin checks.

| Routes | Purpose |
| --- | --- |
| `GET /applications` | List owned apps and creation form |
| `POST /applications` | Create app with name, callbacks and one initial secret |
| `GET /applications/{id}` | Own configuration and secret metadata |
| `POST /applications/{id}/update` | Update name/callbacks within immutable sector |
| `POST /applications/{id}/secrets` | Create secret if fewer than two are active |
| `POST /applications/{id}/secrets/{secret_id}/revoke` | Revoke owned secret |
| `POST /applications/{id}/disable` | Disable and permanently revoke grants |
| `POST /applications/{id}/enable` | Allow new authorizations without restoring grants |

Show plaintext secrets once in a no-store response after confirmed commit; do not
put them in redirects, logs or persistent flash messages. If the response is lost,
revoke that secret and create another. Disabled apps remain manageable by owners.
There is no app deletion, ownership transfer or API-based registration contract.
