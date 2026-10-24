# First authentication milestone: API contract draft

## Status

Behavior comes from [PRODUCT.md](PRODUCT.md); security rules come from
[ARCHITECTURE.md](ARCHITECTURE.md). Endpoint paths, field names, status codes,
header conventions and examples below are **proposals for implementation review**.
They are not implemented or a claim of OAuth/OIDC compatibility. No credentials
in examples are real. OpenAPI can follow when these shapes stabilize.

## Common conventions

Public origin: `https://<crow-host>`; JSON API prefix: `/api`. Use HTTPS.
Sensitive responses use `Cache-Control: no-store`. Never put passwords, client
secrets or access/refresh tokens in URLs or logs. Short-lived authorization codes
are the intentional callback-URL exception; redact callback query strings from
app/proxy logs and use a referrer policy that avoids leaking codes.

Backend client authentication uses proposed headers `X-Crow-Client-Id` and
`X-Crow-Client-Secret`. User-token endpoints additionally require
`Authorization: Bearer <access-token>`. This avoids competing uses of Authorization.
Redact all three headers. Every application-backend endpoint requires client
authentication, except public JWKS. Browser endpoints do not accept client secrets.

Portal/flow mutations use a proposed `X-CSRF-Token`, validated Origin and the
corresponding browser-bound cookie. Credential submission is same-origin to
Crow; backend integrations use their own server-held credentials. Apply
[Architecture browser controls](ARCHITECTURE.md#browser-sessions-and-hosted-flows).
Reject unsupported content types, unknown security-sensitive fields, duplicate
parameters and oversized requests.

Proposed error body:

```json
{"error":{"code":"invalid_request","message":"The request could not be completed."}}
```

| HTTP status | Example code | Meaning |
| --- | --- | --- |
| 400 | invalid_request / invalid_grant | Invalid parameters, expired/mismatched/used code or invalid refresh |
| 401 | invalid_client / invalid_token / invalid_credentials | Missing or invalid authentication; do not distinguish unknown usernames |
| 403 | forbidden / consent_required | Wrong owner, disabled app or missing consent |
| 409 | username_unavailable / secret_limit | Registration conflict or two active secrets |
| 429 | rate_limited | Temporary throttling; include Retry-After |
| 503 | temporarily_unavailable | Required dependency unavailable; issue no tokens |

Errors must not reveal private-resource existence, username existence on login,
or raw internal failures. Browser failures use safe text and never redirect to
an unvalidated callback. Unresolved conventions are tracked in the
[Architecture checklist](ARCHITECTURE.md#milestone-checklist).
Responses below illustrate successful shapes.

## Browser endpoints

| Method and path | Credentials | Request / success |
| --- | --- | --- |
| GET /login | None; existing portal session never replaces app authentication | No app parameters: portal login. With complete client_id, redirect_uri, state, code_challenge, code_challenge_method=S256: validate and create pending flow; show hosted login |
| GET /register | None; optional bound pending flow | Hosted registration; retain app flow when moving from login |
| POST /api/hosted/login | Flow binding where applicable, CSRF and Origin | username/password; app flow proceeds to consent/completion; portal flow sets portal cookie |
| POST /api/hosted/register | Flow binding where applicable, CSRF and Origin | username/password; create account and follow pending app flow or set portal cookie |
| POST /api/hosted/consent | Authenticated pending flow, CSRF and Origin | approve boolean; approval records consent and completes flow; denial consumes it |
| GET /api/portal/account | Portal cookie | Own UUID, username and subscription status |
| POST /api/portal/logout | Portal cookie, CSRF and Origin | 204; invalidate current session and expire cookie |

Hosted credential endpoints are exclusively for Crow pages, never app password
collection. Follow [Product login behavior](PRODUCT.md#hosted-login-and-consent)
and Architecture's browser-bound authenticated-flow rules. Existing consent
skips the prompt, never credential verification.

Proposed credential body:

```json
{"username":"alice_123","password":"<15–128 character password>"}
```

Apply [Product account rules](PRODUCT.md#accounts), including normalization,
password validation and registration's no-recovery disclosure. Login returns
the same error for unknown username and incorrect password.

For hosted JS submissions, propose a JSON next-step result:

```json
{"next":"consent"}
```

or, after app completion:

```json
{"next":"redirect","redirect_to":"https://app.example/auth/callback?code=<code>&state=<original-state>"}
```

Portal success returns `{"next":"redirect","redirect_to":"/account"}` and sets
the portal cookie. Only server-built, validated URLs are returned. Next.js performs
the navigation. Never return access or refresh tokens to browser flow handlers.
Direct `/register` handling of app parameters must use the same validation as
`/login`; page navigation must preserve the bound flow without trusting new inputs.

Consent denial returns `error=access_denied` and original state to the validated
callback. Flow expiry requires restart; registration and consent outcomes follow
Product. The app backend validates state against its browser session before exchange.

## Application backend endpoints

### Exchange: POST /api/token/exchange

Require client headers and JSON:

```json
{"code":"<authorization-code>","redirect_uri":"https://app.example/auth/callback","code_verifier":"<PKCE-verifier>"}
```

Validate client, enabled state, exact callback, code binding, expiry, PKCE S256
and one-time use. Follow [Architecture exchange ordering](ARCHITECTURE.md#initial-exchange-and-partial-failures).
Success is returned only after code consumption, Redis session creation and JWT
signing are confirmed:

```json
{"access_token":"<JWT>","token_type":"Bearer","expires_in":600,"refresh_token":"<opaque-token>"}
```

After code consumption, failure or uncertain writes require fresh hosted login;
do not retry or restore the consumed code. Retry within its lifetime is allowed
only when non-consumption is confirmed. A lost successful response also requires
fresh login.

### Refresh: POST /api/token/refresh

Require client headers and `{"refresh_token":"<opaque-token>"}`. Validate
client, enabled state and family ownership/version using
[Architecture refresh checks](ARCHITECTURE.md#refresh-and-redis). PostgreSQL
check failure returns 503 before token consumption. Disabled apps are rejected;
version mismatch, expiry, revocation or reuse returns invalid_grant. Reuse must
revoke the family despite the error response.

Return the exchange success shape with new tokens after signing and confirmed
Redis update. Preserve Product's idle/absolute deadlines. Dependency failure
returns 503 without granting access. Serialize refreshes across app instances.
**Never retry an ambiguously completed refresh**; a lost response requires login.
There is no grace window or refresh idempotency.

### Logout: POST /api/token/revoke

Require client headers and `{"refresh_token":"<opaque-token>"}`. Revoke its
corresponding family; return 204 when revoked or already revoked. Preserve spent
token evidence so the family can be identified without allowing refresh. Invalid
or unrecognizable credentials must not affect any other session. Invalid-token
response review is tracked in the Architecture checklist. Client clears local
state even if the network fails; do not claim server revocation succeeded on 503.
Existing access JWTs expire naturally; other families remain active.

### Identity: GET /api/identity

Require client headers and user Bearer JWT. Authenticate client independently,
require aud equal client ID, validate all JWT claims and enabled app state,
and derive user from sub. Proposed response:

```json
{"user_id":"<immutable-UUID>","username":"alice_123"}
```

### Subscription: GET /api/subscription

Same credentials and ownership derivation as identity. Proposed response:

```json
{"subscriber":false}
```

Before Stripe this is false for everyone. Never accept a caller-supplied user ID
or expose a public subscription write endpoint. Subscription does not gate features.
No application JWT authorizes portal or developer operations.

### Public keys: GET /.well-known/jwks.json

No authentication. Return public RSA keys with kid, kty=RSA, use=sig, alg=RS256,
n and e; never private parameters. Cache policy must support
[Architecture's JWT and rotation contract](ARCHITECTURE.md#jwt-contract-and-key-rotation).
Integrators must follow that validation contract, including rate-limited
unknown-kid refresh.

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
| POST /api/portal/applications/{id}/disable | Block authorization/exchange/refresh and app-token Crow APIs; invalidate refresh families |
| POST /api/portal/applications/{id}/enable | Re-enable configuration; never restore revoked families |

Disabled apps remain manageable by their owner. Disable/enable uses
[Architecture session versions](ARCHITECTURE.md#app-disablement); old families
remain invalid even if Redis cleanup failed. Removed callbacks must not be used
for later redirects/exchanges. No permanent app deletion, account editing/deletion
or billing mutations are offered in this milestone.

## Implementation review

Wire formats remain proposals; finalize each endpoint when implementing it.
The [Architecture checklist](ARCHITECTURE.md#milestone-checklist) collects
remaining implementation/deployment work and acceptance checks. Billing and
profile features can be specified later. Add OpenAPI when wire formats stabilize.
