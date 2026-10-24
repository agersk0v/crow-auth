# Integrating Sign in with Crow

## Availability

This guide describes the planned first release. The repository currently exposes
only `/health`; no public issuer or working sign-in service is claimed.

Use an existing OIDC library for a server-backed web application. Browser-only
SPAs and native apps are not supported directly. A SPA may use an application
backend that owns the complete integration and keeps credentials off the browser.

## Register an app

Create and verify a Crow account, then open Applications. Supply an app name and
exact callback URLs. Save the client ID and once-displayed client secret in your
backend's secret store. Use separate apps for development and production.

Production callbacks require HTTPS. Local HTTP is allowed only on exact localhost
or 127.0.0.1 hosts. All callbacks for one app share a hostname. See the
[callback policy](API_CONTRACT.md#callbacks) before registering multiple callbacks.
Registration does not mean Crow endorses your app.

## Configure your library

Use the operator-published issuer and discovery; never guess a production domain.
Configure confidential client authentication with `client_secret_basic`, code
flow, PKCE S256, query response mode and scopes `openid profile`.
No custom Crow SDK, password forwarding or refresh-token handler is needed.

Your backend must retain per-transaction state, nonce, PKCE verifier, requested
freshness and start time, bound to the initiating browser session. Generate state,
nonce and the PKCE verifier with secure randomness; record the actual request
start time for freshness validation. Do not keep a single global verifier or
overwrite another tab's pending transaction. Expire and consume each transaction once.

## Complete sign-in

1. Redirect the browser to the discovered authorization endpoint with client ID,
   exact callback, code response type, scopes, state, nonce and S256 challenge.
2. Crow authenticates the user and obtains consent. Existing Crow login may skip
   password entry; it does not bypass required consent.
3. At your callback, validate state and response issuer against the pending
   transaction before accepting success or handling an error. Reject unsolicited
   or already-used responses. Handle `access_denied` as a normal cancellation.
4. Exchange the code from your backend with your client secret and PKCE verifier.
   Do not send the secret or returned tokens to browser JavaScript.
5. Let your OIDC library validate the ID token against the configured issuer,
   JWKS, client audience, nonce and time requirements in the
   [token contract](API_CONTRACT.md#tokens). Merely decoding a JWT is insufficient.
6. Optionally fetch UserInfo using the access token. Its subject must equal the
   validated ID-token subject.
7. Look up or create a local user by `(issuer, subject)`, rotate your local session
   handle and set your own Secure, HttpOnly cookie with CSRF protection.

Never link existing accounts by username. Subjects differ across sectors and may
be shared across clients in the same sector. Changing callback hostname requires
a new app and an explicit identity migration on your side.

## Keep credentials in the right place

| Value | Destination | Handling |
| --- | --- | --- |
| Issuer and client ID | Library configuration and authorization request as applicable | Public identifiers; pin the trusted issuer in server configuration |
| Client secret | Backend authentication at the discovered token endpoint | Secret store; never an authorization URL or browser form |
| State and nonce | Authorization request and server-side pending transaction | Bind to the initiating browser and validate the returned state/ID-token nonce |
| PKCE challenge | Authorization request | Derive from the transaction's verifier using S256 |
| PKCE verifier | Backend code exchange and pending transaction | Never include in the authorization URL |
| Authorization code | Callback, then backend exchange | Short-lived and single-use; exclude callback queries from logs |
| ID token | Backend OIDC validation | Establish identity only after all validation succeeds |
| Access token | Backend call to UserInfo in the Bearer header | Opaque; do not put in query strings or use for your app's APIs |
| Local session handle | Your app's protected browser cookie | Your app controls session expiry, logout and CSRF protection |

The [API contract](API_CONTRACT.md#authorization) defines exact request fields and
[token response examples](API_CONTRACT.md#exchange). These examples describe the
future protocol; they cannot be exercised against the current skeleton.

## Diagnose integration failures

These are planned protocol outcomes, not routes available today. Keep diagnostic
logs to safe error codes and request IDs, without raw request URLs or credentials.

| Outcome | Action |
| --- | --- |
| Local Crow error before callback | Check enabled client and exact registered callback, including scheme, hostname, port, path and query |
| `invalid_scope` | Request only `openid` and optional `profile`; no email, subscription or offline-access scope |
| `access_denied` | End the pending transaction and let the user choose whether to start again |
| `login_required` or `consent_required` with `prompt=none` | Start a new interactive authorization if the user chooses to continue |
| Token endpoint `invalid_client` | Check client ID, active secret and library support for `client_secret_basic` with OAuth credential encoding |
| Token endpoint `invalid_grant` | Start new authorization; inspect callback/verifier binding and code expiry without logging their values |
| State, issuer, nonce, signature, audience or UserInfo subject mismatch | Reject sign-in; inspect trusted library configuration and transaction binding; never disable the failing check |
| Unknown signing-key `kid` | Use the library's bounded refresh of the trusted issuer's JWKS; reject if no trusted matching key exists |
| Timeout, HTTP 503 or lost exchange response | Show login unavailable or start fresh authorization; do not replay the code automatically |

## Sessions and failures

Your app owns local-session duration and permissions. ID-token expiry is not a
local logout mechanism. Crow tokens last ten minutes; there are no refresh tokens.
Repeat authorization when you need new Crow tokens. Use `prompt=login` or `max_age`
when your action requires recent authentication.

App logout clears your local session. It leaves Crow SSO intact, so another sign-in
may reuse it. Crow logout does not clear your app's existing sessions. Do not
promise global logout or immediate local-session revocation after a Crow password
reset or app disablement.

A failed or lost code-exchange response requires a new authorization transaction;
do not automatically replay the code. Treat provider outages as login unavailable,
not permission to skip validation. Show a retry path without displaying tokens,
client secrets or raw sensitive callback URLs.

## Credential rotation

Create a second client secret, deploy it, confirm new exchanges work, then revoke
the old secret. At most two secrets are active. A lost once-only secret cannot be
retrieved. Disabling an app invalidates its existing Crow grants permanently;
re-enabling requires new authorization.

## Integration acceptance checklist

Before releasing your integration verify successful sign-in, cancellation,
expired state, state/issuer/nonce mismatch, wrong audience, invalid signature,
expired token, PKCE failure, UserInfo subject mismatch and parallel-tab isolation.
If requesting freshness, verify missing/stale/future `auth_time` and `max_age=0`.
Exercise secret rotation, outages and the distinct Crow/app logout behavior.
Never advertise compatibility based only on a decoded successful token.
