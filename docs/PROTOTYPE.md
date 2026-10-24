# Account/session prototype: current implementation and gaps

This branch began against the earlier no-email account/session specifications.
Main subsequently accepted a verified-account/two-app milestone. The newer
requirements in PRODUCT.md, ARCHITECTURE.md and API_CONTRACT.md remain authoritative;
this prototype does not satisfy them and is not ready for deployment or merge as
completion of that milestone.

## Known differences from the accepted milestone

- Registration collects no email and immediately authenticates. The accepted
  contract requires pending registration, email verification, then password login.
- Recovery, password replacement, account deletion, mail/outbox, shared abuse
  limits and account security versions/locks are absent.
- CSRF is derived from the session handle and shared across form actions. The
  accepted design requires independent server-side, action-bound CSRF state.
- Browser rotation locks the session only; it does not implement the newer
  account-first lock order or independent hosted-flow browser binding.
- Developer apps, hosted authorization, tokens, discovery and two example
  backends are absent. No complete OIDC/SSO milestone is claimed.

Recommended resolution: bring account schema, registration/verification and CSRF
mechanics into line with the current accepted design before continuing the ordered
implementation checklist. Keep this PR draft until that scope is addressed or an
explicitly reviewed prototype scope is accepted. Do not weaken the accepted
requirements to match this implementation.

## Implemented HTTP behavior

GET /register and /login issue/reuse a browser context and render standard forms.
POST /api/hosted/register and /api/hosted/login accept username, password and csrf,
require the cookie and exact APP_ORIGIN, and redirect 303 to /account on success.
GET /account and /api/portal/account render the current account or redirect to
/login. POST /api/portal/logout accepts csrf, revokes the browser session and
clears its cookie before redirecting to /login. Unknown form fields are rejected.
GET login/register query parameters are rejected until hosted flows exist.
Policy/credential errors use 400, browser-control failures 403 and dependency or
hash-admission failures 503. Full-page and HTMX requests use identical responses.
No app tokens are returned.

## Prototype mechanics and library responsibilities


**Prototype implementation (not the accepted milestone design):** Askama provides compile-time HTML
escaping; Axum owns HTTP/form parsing; SQLx owns PostgreSQL transactions and
migrations; RustCrypto Argon2 owns Argon2id hashing; the OS-backed rand generator
owns random bytes; SHA-256 stores high-entropy session verifiers. Crow remains
responsible for input policy, CSRF/Origin enforcement, deadlines, rotation and
transaction boundaries. No OAuth/OIDC compatibility is claimed by these libraries.

The configured `APP_ORIGIN` is an HTTPS origin without a path, query, credentials
or fragment. A TLS reverse proxy can forward to the loopback HTTP listener.
Cookies use the `__Host-crow` name, Path=/, Secure, HttpOnly and SameSite=Lax.
Anonymous browser sessions last fifteen minutes and carry no account authority.
Forms receive a domain-separated SHA-256 CSRF token derived from the random
browser handle; only its session verifier is persisted. Every mutation requires
that token, an exact matching Origin and a live database session. Missing Origin
fails closed. HTMX uses ordinary complete pages and 303 redirects, with the same
controls; no special fragment protocol or external scripts are needed initially.

Successful registration/login locks the initiating session row, verifies that it
is still live, revokes it and inserts a fresh eight-hour session in one READ
COMMITTED transaction. Registration inserts the account in that transaction;
PostgreSQL uniquely constrains normalized usernames. Logout takes the same row
lock. A concurrent submission from another tab using the replaced handle fails;
the user reloads the form. No grace alias restores a revoked handle. Account
creation and cookie issuance happen only after confirmed commit. On uncertain
commit, return a generic unavailable response without a new cookie. A lost
registration response can leave an account created; login recovers access.
Future authenticated flows must lock this same session before binding/completion
and invalidate on rotation/logout; flows are not implemented in this milestone.

Hashing uses Argon2id v19 with 19 MiB, two iterations and one lane, fresh 16-byte
salts, and at most two concurrent blocking workers per process. Admission uses
try-acquire with no waiting queue; overload returns 503. A permit remains held
inside the blocking task even if its HTTP caller disconnects. Unknown usernames
verify a dummy hash with the same costs. Benchmarking and distributed abuse
limits remain public-exposure gates.

The embedded SecLists Xato top-100,000 password dataset is checked locally,
case-insensitively for blocklist matching only; the original password bytes are
hashed unchanged. See ../data/README.md for source, checksum and refresh policy.
This limited common/compromised list is prototype protection, not a claim to
contain every compromised password. Expired/revoked rows remain fail-closed;
cleanup is operational work and never the enforcement mechanism.

## Verification evidence for this prototype only


- `cargo fmt --check`, `cargo check --locked`,
  `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked`
  passed (two password-policy/worker tests and two credential-safe startup tests).
- `TEST_DATABASE_URL=<disposable PostgreSQL URL> cargo test --locked --test browser
  -- --ignored` passed against PostgreSQL 17. The explicit database suite checks
  migration reruns, unique normalized usernames, same-browser and independent
  registration races, session fixation/rotation, original password bytes, ordinary
  and HTMX CSRF/Origin enforcement, cross-session CSRF rejection, absolute expiry,
  expiry after lock waits, logout versus authentication, rollback after injected
  session-insertion failure, persistence across router recreation, body limits and
  fail-closed database outages. Hash cost parameters and fresh salts are checked;
  measured cost tuning before exposure is still outstanding.
- An actual loopback HTTP process using the migrated PostgreSQL database passed
  `/health`, registration, account rendering, logout and stale-cookie rejection.
  The smoke client supplied the configured HTTPS Origin explicitly; TLS termination
  and a browser behind the production proxy have not been verified.
- Templates, session/CSRF mechanics, library responsibilities and transaction
  policy are recorded in
  the prototype mechanics above.
  No hosted-flow races, password replacement or OIDC conformance are claimed.
