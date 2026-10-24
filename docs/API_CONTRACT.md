# First authentication milestone: API contract draft

## Status

Behavior comes from [PRODUCT.md](PRODUCT.md); security rules come from
[ARCHITECTURE.md](ARCHITECTURE.md). Endpoint paths, field names, status codes,
header conventions and examples below are **proposals for implementation review**.
They are not implemented or a claim of OAuth/OIDC compatibility. No credentials
in examples are real. OpenAPI can follow when these shapes stabilize.

## Common conventions

Public origin: `https://<crow-host>`; JSON API prefix: `/api`. All external traffic
uses HTTPS except approved local-development callbacks. Sensitive responses use
`Cache-Control: no-store`; never place credentials in URLs or logs.

Backend client authentication uses proposed headers `X-Crow-Client-Id` and
`X-Crow-Client-Secret`. User-token endpoints additionally require
`Authorization: Bearer <access-token>`. This avoids competing uses of Authorization.
Redact all three headers. Client authentication is required on every backend
endpoint, not just issuance. Browser endpoints do not accept client secrets.

Portal/flow mutations use a proposed `X-CSRF-Token`, validated origin and the
corresponding browser-bound cookie. Define CSRF issuance during implementation;
SameSite alone is insufficient. Cookies and credentials do not cross origins.
Reject unsupported content types, unknown security-sensitive fields, duplicate
parameters and oversized requests. Define concrete request limits before release.

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

Do not expose whether private resources exist. Choose consistent 403/404 handling
for non-owned app IDs before implementation. Browser failures display safe text;
never redirect to an unvalidated callback. Client-visible errors contain no raw
internal-service messages. Responses below illustrate successful shapes only.

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

Credential submission is exclusively for Crow-hosted pages. Apps must not call
these endpoints to collect/forward passwords. Flow identifiers alone are not
proof of authentication. After credentials succeed, store a short-lived
server-side authenticated-flow stage; do not let a browser choose a user ID.
Successful prior consent skips the prompt, not credential verification.

Proposed credential body:

```json
{"username":"alice_123","password":"<15–128 character password>"}
```

Normalize username before validation and lookup. Never alter passwords. Invalid
password/unknown username yields the same login error. Registration must disclose
lack of recovery and validate the agreed common/compromised-password policy.

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

Consent shows name, destination domain, UUID/username/subscription disclosure and
lack of app endorsement. Decline returns only error=access_denied and original
state to the validated callback, consuming the flow. A registered account remains.
Flow expiry after 15 minutes requires restart. Codes start their 60-second lifetime
after approval/completion. Apply a referrer policy that avoids leaking codes.
Application backend validates state against its browser session before exchange.

## Application backend endpoints

### Exchange: POST /api/token/exchange

Require client headers and JSON:

```json
{"code":"<authorization-code>","redirect_uri":"https://app.example/auth/callback","code_verifier":"<PKCE-verifier>"}
```

Validate client, enabled state, exact registered callback, code binding, expiry,
S256 verifier and one-time use. Rust consumes the code in PostgreSQL and creates
the refresh family and token verifier in Redis. Return tokens only after code
consumption and session creation are confirmed and JWT signing succeeds.
There is no shared transaction across these stores. Proposed success:

```json
{"access_token":"<JWT>","token_type":"Bearer","expires_in":600,"refresh_token":"<opaque-token>"}
```

Confirmed policy: consume once, then restart authorization after failure.
Atomically consume the code and confirm PostgreSQL commit before creating its
Redis session. Only the winning consumer proceeds. A 503 after consumption or
an uncertain write requires a fresh hosted login; do not retry that code or
restore it. Retry within the original lifetime is allowed only when
non-consumption is confirmed. Concurrent requests cannot create multiple usable
families. A lost public success response also requires new authorization.
Best-effort revoke known orphan families; expiry bounds leftovers if cleanup
fails. Define signing order and cleanup details before implementation.

### Refresh: POST /api/token/refresh

Require client headers and `{"refresh_token":"<opaque-token>"}`. Validate client,
enabled state and ownership. Rust uses an atomic Redis operation/script to
validate and consume the token, create its successor and update the family.
Return the same success shape as exchange only after signing succeeds and the
Redis update is confirmed. Family revocation on reuse must take effect despite
returning invalid_grant. Define signing/write ordering and durable app-state
checks before implementation.
Preserve original 30-day deadline and update seven-day idle expiry. Expiry,
revocation or reuse yields invalid_grant; reuse also revokes the whole family.
Dependency failure yields 503 without granting access. **Never automatically retry
an ambiguously completed refresh**; a lost response requires login. Serialize
requests across application instances. No grace window or refresh idempotency.

### Logout: POST /api/token/revoke

Require client headers and `{"refresh_token":"<opaque-token>"}`. Revoke its
corresponding family; return 204 when revoked or already revoked. Preserve spent
token evidence so the family can be identified without allowing refresh. Invalid
or unrecognizable credentials must not affect any other session. Define a uniform
non-disclosing invalid-token response before implementation. Client clears local
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

No authentication. Return a JWKS containing public RSA keys with kid, kty=RSA,
use=sig, alg=RS256, n and e. Never include private RSA parameters. Set cache policy
compatible with ten-minute prepublication and rotation overlap. Issuer is a fixed
configured URL, not inferred from untrusted Host/proxy headers. Publish integration
instructions requiring issuer/audience/algorithm/claim validation and unknown-kid
refresh with throttling. Exact cache TTL remains an implementation setting.

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
effective across PostgreSQL app state and Redis refresh state, including
concurrent requests, Redis failure and later re-enablement. Choose the state-check
and invalidation mechanism before implementation; cleanup alone is insufficient.

## Rust session storage boundaries

Rust owns exchange, refresh, revocation and all JWT signing. PostgreSQL stores
authorization codes and durable app/account configuration; Redis is authoritative
for refresh sessions. Go is not required. Validate user/client/family bindings
server-side. Exchange uses the confirmed consume-once/restart policy. Define
atomic Redis operations,
signing failures, ambiguous writes and disablement enforcement before implementation.

## Review and validation

Before endpoint implementation, review proposed headers, paths, error mapping and
exchange locking/signing order, orphan cleanup and session-failure behavior. Before deployment, choose numeric rate limits,
request sizes, proxy trust, password dataset/costs, CSRF mechanics, cookie names,
Redis persistence/capacity/restore invalidation, PostgreSQL recovery and JWKS cache settings.
Use Architecture's first-milestone acceptance checks as behavioral verification.
Billing and profile features can be specified later. Add OpenAPI after these
request/response shapes stabilize.
