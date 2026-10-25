# Hosted browser foundations

This is the first increment of issue #21. It supplies reusable library primitives,
escaped Askama templates and an Axum protection layer. The production service still
exposes only `/health`: registration, resend and verification routes are not wired
up. No authenticated browser session or login is implemented. Shared throttling,
registration handlers and the complete HTTPS/browser/mail flow remain later PRs.

## Pre-auth state and forms

Construct `BrowserSecurity::new` with the canonical configured HTTPS origin, such
as `https://crow.localhost:3000`. Paths, credentials, queries, fragments, trailing
slashes and noncanonical origins are rejected. The existing HTTP liveness listener
is not a browser HTTPS fixture; configure trusted local HTTPS before exposing forms.
The health-only server does not construct BrowserSecurity or read origin settings.

In a caller-owned GET transaction, `ensure_preauth` reuses a live pre-auth cookie
or creates fresh state. The cookie contains 256 OS-random bits; PostgreSQL stores
only its SHA-256 verifier, a separate 256-bit CSRF secret and an absolute 30-minute
deadline. Reuse does not extend the deadline. Unknown/expired cookie values are
replaced with independently generated values; duplicate target cookies are rejected.
The cookie is `__Host-crow-preauth`, Secure, HttpOnly, SameSite=Lax, Path=/, no Domain.
It grants no account permissions. Apply its Set-Cookie header only after confirmed
commit using `PreauthBrowser::apply_cookie`.

`PreauthBrowser::csrf(FormAction)` derives an action-bound HMAC-SHA256 synchronizer
token. Register, resend and verification use different values. POST handlers must
call `authorize_post` with the expected server-selected action before account writes.
It requires exactly one Origin equal to the configured origin, a live cookie and
the correct browser/action CSRF token; HMAC verification uses constant-time checking.
Missing/null/foreign Origin, stale CSRF or another browser's token returns 403.
No Host, forwarding, Referer or HTMX header substitutes for these checks.

`BrowserSecurity::protect(router)` adds Origin checks for unsafe methods, a 16 KiB
body limit and sensitive-response headers. **It does not automatically validate
CSRF**: each future form handler must call `authorize_post` or `take_confirmation`.
Ordinary and HTMX requests use the same code. Responses include no-store,
no-referrer, nosniff, DENY framing protection and a restrictive CSP with only
same-origin form submission. Vary includes HX-Request. No scripts, third-party
assets or raw-HTML bypasses are needed by the supplied templates.

## Verification contexts and clean redirects

The future link GET can use this sequence:

1. Begin a transaction and call `ensure_preauth`.
2. Call `prepare_verification` with the emailed token. It locks account then action,
   validates purpose/email/security version/status/deadlines and creates a context.
   It never consumes the token or activates an account.
3. Confirm commit, apply a newly issued pre-auth cookie if needed, and 303 to
   `PreparedConfirmation::clean_path()`. The path contains only a non-secret UUID
   selector, never the emailed token or form credential. Redact the original link
   query from access logs; the library does not install access logging.
4. On the clean GET, obtain the same pre-auth browser and call `load_confirmation`
   with the selector. It rechecks current bindings and reconstructs that browser's
   context credential. A selector alone cannot load another browser's form.
5. Render `confirmation_page` with action-specific CSRF and context credentials in
   hidden POST fields. No email token or account identity is rendered.

The context credential is domain-separated HMAC-SHA256 over its random UUID using
the browser's server-held secret; only its SHA-256 verifier is stored. This lets the
server rebuild the form after redirect without persisting raw credentials or the
emailed token. Contexts expire at the earliest of five minutes, pre-auth expiry,
verification-token expiry and pending-account expiry. Repeated GETs do not renew
an existing context or spend the underlying action. Scanner-created contexts in
another browser do not transfer browser or app-flow binding.

The future protected POST calls `take_confirmation`. It validates CSRF/Origin,
locks account, action and context, then rechecks all bindings and deadlines after
lock waits. It spends that context once and returns an internal action/account
reference. **This primitive does not consume the action or activate the account.**
The later handler must implement action consumption/activation in the SAME
transaction and commit both together; failure requires rollback so the context
remains usable. Never accept a client-supplied action/account ID as this internal
reference. No handler exposing this intermediate operation ships in this PR.
Resend/action deletion cascades away old contexts. Account expiry and activation
are rechecked independently of asynchronous cleanup.

## Cleanup and evidence

`account-cleanup` now also calls `browser::cleanup_expired`: at most 100 expired
pre-auth rows (with cascading contexts) and 100 independently expired contexts per
poll. Expired state stops authorizing immediately, even before erasure. Cleanup
never acquires account/action locks after pre-auth/context locks. Extend the cleanup
role's schema checks and SELECT/DELETE/row-lock permissions for both new tables;
no SMTP key is needed. The worker does not run migrations; start it after the
application/migration owner has applied migration 0004.

With disposable PostgreSQL configured as documented in the README:

```sh
cargo test --locked --test browser_http
cargo test --locked --test browser_database -- --ignored
```

CI invokes the database suite explicitly. Tests cover escaping, ordinary/HTMX
Origin and CSRF failures, body limits, error headers, cookie fixation/duplicates,
browser/action binding, scanner GETs, the clean-selector hand-off, deadline bounds,
context replay/concurrency/rollback, post-lock-wait expiry, replacement invalidation
and bounded cleanup. The Axum harness uses HTTPS request metadata and explicit
cookies; it is not evidence from a real browser or TLS deployment.
