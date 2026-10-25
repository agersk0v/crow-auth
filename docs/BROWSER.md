# Hosted browser foundations

This is the first increment of issue #21. It supplies signed pre-auth cookies,
action-bound CSRF checks, escaped Askama templates and an Axum protection layer.
The production service still exposes only `/health`. Shared throttling is now
available as a separate library component; registration/resend handlers and the complete HTTPS/browser/mail flow remain later
PRs. No authenticated browser session or login is implemented.

## Configuration and signed cookies

`BrowserSecurity::from_env` requires `CROW_ORIGIN` and `BROWSER_SIGNING_KEY`.
Alternatively construct it with `BrowserSecurity::new(origin, signing_key)`.
The origin must be canonical HTTPS, such as `https://crow.localhost:3000`, without
a trailing slash, credentials, path, query or fragment. The existing HTTP liveness
listener is not a browser HTTPS fixture. The health-only server does not construct
BrowserSecurity or require the signing key yet.

Provision `BROWSER_SIGNING_KEY` as 32 random bytes encoded as unpadded base64url,
separate from mail encryption and future OIDC signing keys. Use the same key on every instance
and across restarts. Missing/malformed configuration fails closed; the library
never generates an ephemeral server key silently. For disposable local use:

```sh
export BROWSER_SIGNING_KEY="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
```

On a form GET, `ensure_preauth(headers)` checks the existing cookie or generates a
fresh 256-bit random nonce. The signed payload contains only a version, UTC issue
time and nonce. It is not encrypted and contains no account identity or password.
HMAC-SHA256 authenticates the payload with a purpose/origin-bound server key.
The cookie is `__Host-crow-preauth`, Secure, HttpOnly, SameSite=Lax, Path=/, no
Domain, with a 30-minute absolute lifetime. Valid reuse does not extend it.
Expired/forged cookies get fresh random state on GET; duplicate target cookies are
rejected. Missing/invalid cookies on POST are rejected, never silently replaced.

No pre-auth database record, context table or cleanup job is required. Validation
works across instances without a database lookup. Keep server clocks synchronized.
Replacing the signing key invalidates all outstanding pre-auth forms; this simple
version has no overlap/key-ring or individual browser revocation. Pre-auth state
never grants account access; future login must rotate it and authenticated forms
must bind their CSRF protection to the authenticated session.

## CSRF, Origin and request protections

`PreauthBrowser::csrf(FormAction)` derives a browser/action-bound hidden CSRF token
using HMAC-SHA256. Register, resend and verification use different values. On a
POST, `authorize_post(headers, csrf, expected_action)` checks exactly one Origin
matching configuration, the cookie signature/lifetime and the submitted CSRF token
in constant time. Cookie-signing and CSRF derivation use different MAC domains.
Missing/null/foreign Origin, expired cookies and another browser/action's CSRF
return 403. No Host, forwarding, Referer or HTMX header substitutes for these checks.

`BrowserSecurity::protect(router)` adds Origin checks for unsafe methods, 16 KiB
body limits, no-store/no-referrer, nosniff, framing/CSP protections and Vary:
HX-Request. It does not automatically validate CSRF: future form handlers must call
`authorize_post` before account writes. Ordinary and HTMX requests use the same
checks. The templates need no scripts, third-party assets or raw-HTML bypasses.

## Direct email-token confirmation

The accepted flow is deliberately simple:

1. The emailed GET opens `/verify-email?token=...`.
2. The GET sets/reuses a signed pre-auth cookie and renders a confirmation form.
   `confirmation_page` escapes the emailed token into a hidden `token` field and
   adds a hidden action-specific CSRF field. GET performs no account mutation.
3. POST sends both fields and the cookie to the fixed verification endpoint.
4. The future handler calls `authorize_post`, then the existing
   `verification::consume` in a database transaction, and reports success only
   after confirmed commit. Roll back all activation changes on failure.
5. Redirect to the specified token-free destination after successful POST.

The emailed secret stays in the initial URL and rendered form until submission;
hidden does not mean encrypted or unreadable in page source. The specification no
longer requires a clean redirect before showing the form. Keep no-store,
Referrer-Policy: no-referrer, no third-party assets and query/body/credential-redacted
access logs on these routes. The library installs no access logging.

There is no five-minute form-context lifetime. The signed cookie expires after
30 minutes and the emailed action retains its existing 30-minute deadline, bounded
by pending-account expiry. Reloading a form cannot extend the action's deadline.
The existing PostgreSQL verification action enforces single use, expiry, resend
invalidation and account binding. CSRF tokens themselves may be reused for the same
browser/action while the signed cookie remains valid. Opening the email on a phone
creates the phone's form/cookie and permits verification there, without transferring
a PC's app-flow binding or signing either browser in.

The actual routes, protected activation handler and end-to-end browser fixture are
not enabled by this increment. Only the test harness connects these primitives.

## Evidence

```sh
cargo test --locked --test browser_http
cargo test --locked --test browser_verification -- --ignored
```

The second command requires disposable PostgreSQL via `TEST_DATABASE_URL` and is
invoked explicitly in CI. Unit/HTTP tests cover signing-key configuration, signature
and field tampering, cookie expiry/reuse/rotation, browser/action/origin binding,
ordinary/HTMX checks, escaping, body bounds and error headers. The database harness
verifies scanner/PC/phone GETs leave actions untouched, hidden-field submissions
use the existing verification action, and concurrent valid confirmations have one
winner. No browser-state tables are created. This is not real-browser/TLS evidence.

PR #22 added no browser-state schema. Its earlier unmerged migration 0004 was
removed; disposable fixtures that applied that old revision must be recreated
before current migrations. The shared throttle now has its own migration 0004.

Upcoming registration/resend handlers must call the [shared throttle](THROTTLING.md)
after CSRF/Origin authorization and before password hashing or account transactions.
It uses transport IP and canonical email, independently of signed-cookie state.
