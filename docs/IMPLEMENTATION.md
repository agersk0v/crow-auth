# First authentication milestone: implementation and evidence

## Status and readiness

Account/password foundations from issue #13 are implemented: versioned migrations,
canonical account identifiers and unique reservations, pending/active schema state,
security versions, Argon2id with bounded workers/queue, and local blocklist
provisioning. Startup still exposes only GET /health; it now applies migrations.
Account/password and verification/mail foundations are implemented: initial and
replacement verification actions, atomic activation, encrypted transactional mail,
a separate SMTP worker, and expired pending-account cleanup. No registration or
verification HTTP routes, browser sessions, OAuth endpoints or example apps exist.
**Next: hosted registration and verification**, including browser protections and
shared throttling. The verified-account/two-app milestone remains pending.

Foundation verification (2026-10-25): all required Rust checks passed; five unit
tests cover input rules, Unicode/password-byte preservation, salts/costs,
blocklist failure, saturation/cancellation and async progress. Two startup tests
check safe configuration errors. The explicitly invoked PostgreSQL 17 suite
passed migration reruns, canonical constraints, immutable identities/security
versions, concurrent username/email conflicts, transaction rollback and safe
storage failures. CI runs it with TEST_DATABASE_URL instead of counting the default
ignored test as verification. An actual service process passed startup/migration
and HTTP health smoke checks; invalid database authentication failed safely.
The pinned dataset provisioner downloaded and checksum-verified the reviewed list.
OSV queries for argon2 0.5.3, password-hash 0.5.0 and uuid 1.29.0 reported no
advisories on this date; this is not a full dependency-graph audit or a conformance
claim. Production hashing benchmarks and full lifecycle/SSO acceptance remain
outstanding.

The first milestone is the entire verified-account/two-backend-app experience,
including recovery, deletion and negative cases. It is ready for implementation
against the contracts below, not ready for use or deployment. No product/protocol
blocking decisions remain; deployment measurements and actual verification remain.
[Product](PRODUCT.md) owns milestone behavior, [Architecture](ARCHITECTURE.md) owns
security enforcement, and [API contract](API_CONTRACT.md) owns protocol details.
This document owns build order and evidence. **Accepted** means chosen planned
behavior; **Unresolved** marks a future decision; **Deferred** is outside this
milestone. None implies implementation. Follow [AGENTS.md](../AGENTS.md) for checks.

## Verification-action increment (part of issue #17)

Implemented: versioned verification-action storage, 256-bit credentials with hashed
verifiers and redacted Debug output, initial issuance, and account-first atomic
consumption/activation without a session. Initial issuance rejects repeat issuance;
the resend/cleanup increment below completes invalidation and the remaining
verification-and-mail foundations. Hosted registration/verification is next.

Verification commands (2026-10-25): `cargo fmt --check`, `cargo check --locked`,
`cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`,
`cargo test --locked --test accounts_database -- --ignored`, and
`cargo test --locked --test verification_database -- --ignored`. Database suites
require `TEST_DATABASE_URL` pointing to disposable PostgreSQL 17 and run explicitly
in CI. All checks passed using Rust 1.99.1 and disposable PostgreSQL 17; six
unit tests, two startup tests, both explicitly invoked database suites and the
startup/HTTP health smoke test passed. The verification suite exercised migration
reruns, malformed/replaced/consumed credentials, account/email/version bindings,
expiry and post-lock-wait expiry, concurrent consumption and failure rollback.
No browser or SMTP result is implied by primitive tests.

## Encrypted mail increment (part of issue #17)

Implemented: transactional initial verification-mail queueing, authenticated-encrypted
recipient/link storage, configured sender/origin/key, a separate mandatory-STARTTLS
SMTP worker with bounded send/retries, payload erasure, and an isolated Mailpit
capture inbox. The worker does not apply migrations or expose browser endpoints.
See [mail operation](MAIL.md) for reproducible configuration and worker commands.
The resend/cleanup increment below completes the remaining internal primitives.
Hosted registration and verification are still unimplemented.

Validation uses the required Rust checks plus explicitly invoked account,
verification and mail suites against disposable PostgreSQL 17 and Mailpit v1.27.0.
The mail suite covers rollback/outbox failure, encrypted storage and wrong keys,
concurrent workers, captured-link activation, bounded retries and exhaustion,
STARTTLS refusal against a plaintext-only relay, expiry/invalidation erasure,
SMTP acceptance followed by DB failure, and the standalone worker process.
On 2026-10-25, all required checks passed with Rust 1.99.1: eight unit tests,
three startup tests, all three explicitly invoked database suites, and the
PostgreSQL startup/HTTP health smoke test. Reproduce the mail suite with
`TEST_DATABASE_URL`, `TEST_SMTP_PORT` and `TEST_MAILPIT_URL` set as described in
[mail operation](MAIL.md), then run
`cargo test --locked --test mail_database -- --ignored`. Formatting, compilation,
Clippy, documentation links and `git diff --check` also passed.
No production SMTP or browser-flow result is implied.

## Resend and cleanup completion (issue #17)

Implemented: `mail::queue_resend` replaces old action rows and unsent encrypted
payloads atomically, preserves passwords/security versions and the original 24-hour
reservation, and refuses active/missing/expired accounts. The separate
`account-cleanup` process removes only expired pending accounts, cancels dependent
actions/mail, and releases canonical username/email reservations on confirmed commit.
All paths serialize under account-first locks; errors roll back replacements and
deletions. No new migration or dependency was required for this increment.

Verification (2026-10-25, Rust 1.99.1, PostgreSQL 17, Mailpit v1.27.0): required
format/check/Clippy/test commands passed, including eight unit tests, three startup
tests, all four explicitly invoked database suites, and startup/HTTP health smoke.
The lifecycle suite exercises replacement of delivered and unsent links, unchanged
password/version/deadline, concurrent resends, both resend/consumption orders,
delivery/resend serialization, post-lock-wait expiry, cleanup rollback/competition,
reservation reuse, active-account survival, and the standalone cleanup process.
Run it with the disposable setup and variables in [mail operation](MAIL.md):
`cargo test --locked --test lifecycle_database -- --ignored`. Existing mail tests
continue to verify retries, SMTP capture and activation from the captured link.
Documentation links and `git diff --check` pass.

The verification-and-mail primitives checklist item is complete based on these
internal integration checks. Hosted forms, pre-auth CSRF/Origin enforcement,
durable browser throttling and scanner-GET/confirmation-POST behavior are the next
increment. These checks do not establish a usable registration flow, login, SSO,
production SMTP delivery, hashing benchmarks or release readiness.

## Milestone decisions

| Accepted behavior or boundary | Authoritative source |
| --- | --- |
| Verified email, recovery, password change and deletion are required | [Accounts](PRODUCT.md#accounts) and [browser contracts](API_CONTRACT.md#account-lifecycle-browser-contracts) |
| Registration creates no SSO session; verification must be followed by password login | [Accounts](PRODUCT.md#accounts) |
| Browser/CSRF checks, library responsibilities and transaction ordering are specified | [Architecture](ARCHITECTURE.md) |
| Example app sessions expire after eight hours; logout has no global propagation | [Session effects](PRODUCT.md#sessions-and-failure-behavior) |
| Callback removal and app disablement serialize with completion/exchange; revoked grants stay revoked | [Transactions](ARCHITECTURE.md#transactions-and-failures) |
| Refresh/offline_access and the revocation endpoint are deferred and unadvertised | [Refresh policy](ARCHITECTURE.md#refresh-and-revocation) |

Standards requirements remain mandatory; raise any discovered incompatibility
with its source and recommended resolution before changing behavior.

## Ordered implementation checklist

1. **Foundations and registration:** deliver the following increments in order.
   Account/app locks, security-version checks and expiry enforcement belong with
   the operations that need them; add configured issuer/signing keys before OIDC
   issuance. Review maintained libraries as each increment adds them.

   - [x] **Account/password foundations:** versioned account migrations, canonical
     identifiers and unique reservations, pending/active schema state, security
     versions, bounded Argon2id and local blocklist provisioning. Evidence is
     recorded above; a schema version column alone does not enforce revocation.
   - [x] **Verification and mail primitives:** purpose-bound, hashed,
     single-use verification tokens; encrypted transactional mail outbox;
     configured sender/link origin and encryption key; delivery retries, resend
     invalidation and expired pending-account cleanup. Serialize account changes
     and token consumption under the specified lock order. Test expiry, concurrent
     consumption/resend/cleanup, rollback and mail retry against disposable PostgreSQL
     and a local SMTP capture inbox.
   - [ ] **Hosted registration and verification (next):** escaped forms, pre-auth CSRF
     and Origin checks, durable shared throttling and redacted logs; initialize
     the password service with the provisioned blocklist. Commit account, action
     and outbox together; use generic email-conflict responses. Confirm activation
     only by POST and create no authenticated session. Demonstrate registration
     through captured mail to activation, including scanner GETs, expired/replaced
     links, duplicate submissions and dependency failures. This does not complete
     the account lifecycle or SSO milestone.

2. **Remaining account lifecycle:** implement password login alongside the Crow
   session increment below, then recovery, password change and deletion. Reuse
   the browser/action/delivery protections above. Test concurrent replacement/deletion,
   credential invalidation, failure rollback and backup deletion replay before
   marking complete. Implement original-browser flow continuation with authorization.
3. **Crow session and portal:** implement rotation, eight-hour absolute expiry,
   logout, per-flow browser binding and sensitive-action reauthentication. Test
   fixation, account changes, tab isolation and logout/flow races.
4. **Owned apps:** implement ownership enforcement, callback policy, once-only
   secrets, two-secret rotation, revocation, disable/re-enable and deletion effects.
   Exercise owner/non-owner paths and concurrent edit/exchange/disable operations.
5. **Authorization:** implement request validation, hosted login/consent, stored
   state machine, SSO, prompt/max_age, silent errors, flow expiry and atomic code
   creation. Registration/verification must resume only the original valid flow.
6. **OIDC issuance:** discovery/JWKS, atomic Basic-authenticated PKCE exchange,
   pairwise subjects, RS256 ID tokens, opaque UserInfo access tokens and replay
   revocation. Exercise invalid bindings, lost responses, rollback, key rotation,
   cross-client substitution and standard protocol/conformance checks.
7. **Two example backends:** create separate client registrations and cookie/session
   stores, use a maintained OIDC client, implement callback checks and local logout,
   then run the acceptance procedure and matrix below. Record commands, versions,
   timestamps and redacted results. Never mark SSO verified from unit tests alone.
8. **Release gate:** verify rate-limit budgets/hash cost on target hardware, SMTP
   delivery, trusted proxy configuration if needed, HTTPS/cookie settings, database
   readiness/timeouts, backup restore/deletion ledger and key incident procedures.
   Publish operator configuration and run instructions before claiming reproducibility
   of a running release. Documentation changes alone close none of these evidence gates.

## Reproducible end-to-end acceptance procedure

This is a specified future test fixture, not runnable in the current implementation.
Implementation must supply a compose/test harness implementing these exact setup
inputs and actions; no production credentials or provider account is required.

Use local HTTPS with a trusted development CA: issuer `https://crow.localhost:3000`,
app A `https://a.localhost:3001`, app B `https://b.localhost:3002`. Route these names
to loopback; use separate host-only cookies. PostgreSQL starts empty. Use a local
SMTP capture inbox with no outbound delivery and a test signing key never deployed
elsewhere. Allow a controllable clock in the test harness only. Run Crow and both
backends with the exact configured issuer and normal TLS validation. App sessions
are eight hours absolute, tokens never enter browser storage. Reset the fixture
between independent test cases; do not add test bypass routes to production.

1. In browser P, visit Crow /register. Create developer username `owner_one`,
   email `owner@example.test`, and a unique policy-compliant test password. Expect
   verification instructions and no authenticated session. Attempt app sign-in
   before verification: no code may be issued.
2. Read the captured mail. Open verification link, confirm the POST and sign in
   with password. Expect /account and a rotated Crow cookie. Register app A and
   B under this owner with only `https://a.localhost:3001/callback` and
   `https://b.localhost:3002/callback` respectively. Copy each once-only client
   secret into its own backend configuration. Retain client IDs, never secrets
   in evidence. Confirm each backend discovers Crow and retrieves its JWKS.
3. Crow logout. In a fresh browser Q, visit A and click Sign in with Crow. A stores
   independent random state/nonce, a 32-random-byte base64url PKCE verifier, S256
   challenge, initiating time and a server-held single-use ten-minute transaction.
   Redirect to /oauth/authorize with code response, exact callback, `openid profile`,
   state, nonce and challenge. Crow shows login. Select Register and create
   `alice_one`, `alice@example.test`, a new compliant password. No code yet.
4. Open Alice's verification link from the capture inbox in Q, confirm, then sign
   in with Alice's password. If the 15-minute Crow flow expired, restart from A;
   no fallback portal login or unchecked callback is allowed. Consent screen
   identifies A, callback domain and the two permissions. Approve.
5. A receives code/state/iss. It verifies exact issuer and session-bound state,
   consumes its local transaction, exchanges code with Basic client credentials,
   exact redirect_uri and verifier, validates ID signature/issuer/audience/nonce/
   times and requests UserInfo. It requires identical sub. Expect an A local
   session and preferred_username=alice_one. Record auth_time and subject A.
6. In the same Q browser visit B and click Sign in. B creates its own transaction
   and redirects to Crow. Expect B consent, **no username/password prompt**.
   Approve; B completes the same checks. auth_time equals A's original value;
   Crow's eight-hour deadline did not extend. B's subject differs from A's because
   hostnames differ, but Crow associates both with Alice. Repeating authorization
   for the same app preserves its subject. No tokens appear in browser URLs
   except the short-lived authorization code.
7. Log out of A using its CSRF-protected POST. A local access ends; B remains
   signed in. Sign into A again: SSO still works without credentials or repeated
   consent for unchanged scopes. Then Crow logout: B's existing local session
   remains, but a new authorization in Q requires password login. Confirm a
   separate browser/device's Crow session was unaffected.

Backends must reject unsolicited callbacks, missing/wrong/used state, missing or
wrong iss, duplicate response parameters and local transaction expiry before
exchange. On OAuth error, validate state/iss, consume the local transaction and
show a safe retry link. On exchange timeout/lost response/503, discard transaction
and restart authorization; do not retry the code. Only set an app cookie after
all ID-token and optional UserInfo checks succeed; rotate its handle on login.

## Requirement and negative-case acceptance matrix

Each row is required evidence, in addition to the happy-path procedure above.
Owners of behavior are Product, API and Architecture; this matrix defines checks.

| ID / requirement | Exercise | Expected result |
| --- | --- | --- |
| AC01 registration | Duplicate canonical username/email, Unicode password bounds, compromised password, pending expiry | Username conflict safe field error; email generic confirmation; policy enforced; no duplicate identities; expired pending reservation reusable |
| AC02 verification | No verification, wrong purpose, expired/resend/used link, mail-scanner GET, open in another browser | No app code until active; no GET mutation; one POST activation; older links rejected; no login or transfer of original flow binding |
| AC03 sign-in | Wrong/unknown/deleted credentials; throttling; session fixation | Generic failure; 429 budget; no session; successful verified login rotates cookie and preserves correct user |
| AC04 SSO | A then B; prompt=none before/after consent; max_age=0; prompt=login/consent | No second password with normal SSO; silent login_required before session, consent_required before consent; fresh/forced UI when requested |
| AC05 app logout/Crow logout | Procedure step 7; pending consent during logout; eight-hour deadline | Local logout affects A only; Crow logout ends current SSO and unfinished bound authenticated flows; no extension via SSO; existing B remains |
| AC06 recovery/change | Unknown/pending email; bad/used/expired link; two concurrent POSTs; old-password login; concurrent exchange | Generic request result; invalid links no mutation; one winner; new password only; all prior Crow sessions/codes/grants fail; external app cookies remain |
| AC07 deletion | Wrong/stale reauth; valid delete; concurrent exchange; owner with apps; retry; re-register same email | No deletion without checks; irreversible removal; no surviving Crow credentials; owned apps permanently disabled for all users; new account has new subjects; external app data retained independently |
| AC08 ownership | Non-owner reads/mutations and tokens used for management | 404 for other apps; app tokens never portal authority; no configuration or secret leak |
| AC09 callback policy | Wildcard/fragment/userinfo/HTTP production URL; mismatched port/query/path; unknown client; removed callback | Registration rejected or local authorization 400; no navigation to invalid target; removed callback exchange invalid_grant |
| AC10 secrets/apps | Rotate to two secrets, third, revoke old, disable then re-enable | Third rejected; revoked secret 401 invalid_client; active new secret works; disable rejects operations; old grants remain invalid after re-enable |
| AC11 failed flows | Deny, 15-minute expiry, wrong browser, tab/account replacement, double completion | Denial returns access_denied only to valid callback; other invalid flows local 400/restart; at most one code; no cross-account continuation |
| AC12 exchange | 60-second code expiry; wrong client/verifier/redirect; duplicate/code replay; concurrent exchange | invalid_grant; at most one issuance; fully bound replay revokes grant; invalid bindings cannot revoke victim grants |
| AC13 validation | Wrong iss/aud/nonce/alg/kid; expired/future/malformed times; stale max_age; UserInfo sub mismatch | Backend refuses session; RS256 allowlist and configured issuer enforced; ID token rejected as Bearer API credential |
| AC14 resources | Missing/expired/revoked access token, wrong audience/scope, disabled app | RFC 6750 status/challenge; no user data; valid token returns only scoped claims |
| AC15 browser security | Missing Origin/CSRF, forged HTMX, XSS content, GET logout/delete, URL leakage | No mutation; escaped rendering; no framing or credential caching/logging; form and HTMX policies identical |
| AC16 dependency/races | DB/signing/mail/limiter failure, uncertain commit, key rotation, restore | Fail closed; rollback leaves no partial issuance; no code retry after ambiguity; outbox retries; valid key overlap; deleted/revoked state never resurrected |
| AC17 scope/discovery | Request offline_access, subscription, unsupported grants/features | Standard errors; discovery advertises only verified code/S256/Basic/pairwise/RS256/openid/profile; no refresh/revocation/end-session endpoint advertised |

## Outstanding limitations and later work

Account/password and verification/mail foundations (including resend and pending
cleanup) have implementation evidence recorded above; account lifecycle and OIDC/SSO integration evidence remain outstanding. Example
backends/harness and deployment runbooks must still be built. Library
versions, Argon2 cost above the specified minimum, mail provider, connection/worker
budgets and infrastructure timeouts need validation on the target deployment; they
do not change the accepted behavior. Do not expose publicly before release gates pass.

Deferred: refresh/offline access and its duplicate/retry/theft policy; subscriptions
and billing; social/passwordless login; MFA; email/username changes; internationalized
email; organizations/ownership transfer; public SPA/native clients; dynamic
registration; global/front/back-channel/RP-initiated logout and remote app deletion.
Pairwise sectors cannot change without a future migration policy. Account recovery
has only the verified-email channel; loss of email means no support bypass. Existing
external app sessions remain valid within app policy after Crow logout/reset/deletion.
These are explicit limitations, not unresolved first-milestone implementation decisions.
