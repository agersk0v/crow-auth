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
freshness and start time, bound to the initiating browser session. Generate them
with secure randomness. Do not keep a single global verifier or overwrite another
tab's pending transaction. Expire and consume each transaction once.

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
