# First authentication milestone: implementation and evidence

## Status and readiness

Documentation defines **Accepted planned behavior**, not shipped functionality.
Reviewed baseline: commit `6fb1518`. `src/main.rs` connects a five-connection SQLx
PostgreSQL pool, runs SELECT 1 and exposes only GET /health on 127.0.0.1:3000.
`tests/startup.rs` checks missing/invalid database configuration and credential
redaction. Cargo dependencies are Axum, Tokio and SQLx. No migrations, account
records, templates, mail, sessions, OAuth endpoints or example apps exist.
This review inspected source; it does not claim a successful live database smoke test.

The first milestone is the entire verified-account/two-backend-app experience,
including recovery, deletion and negative cases. It is ready for implementation
against the contracts below, not ready for use or deployment. No product/protocol
blocking decisions remain; deployment measurements and actual verification remain.
Document ownership and decision labels follow [AGENTS.md](../AGENTS.md).

## Review findings and resolved decisions

| Baseline gap/conflict | Resolution and authoritative source |
| --- | --- |
| Product said no email and deferred recovery/deletion; requested milestone requires all three | Mandatory verified email, recovery/change and deletion included; [Accounts](PRODUCT.md#accounts). User confirmed verification and owned-app deletion policy |
| API said no account deletion; management/browser routes merely proposed | Accepted form contracts, including lifecycle; [API](API_CONTRACT.md#account-lifecycle-browser-contracts) |
| Registration was treated as authenticated login | Pending registration creates no SSO; verification then password login required; [Accounts](PRODUCT.md#accounts) |
| Browser/CSRF mechanics, library choices and transaction ordering were unresolved | Explicit [Architecture](ARCHITECTURE.md) choices; no blank design gate before feature work |
| Local session expiry was unspecified; logout could be confused with global logout | Eight-hour absolute example app sessions; explicit [session effects](PRODUCT.md#sessions-and-failure-behavior), no global propagation |
| Callback removal and disablement races lacked executable policy | Row locks/rechecks and permanent grant revocation; [transactions](ARCHITECTURE.md#transactions-and-failures) |
| No reproducible two-app walkthrough or requirement mapping | Procedure and acceptance matrix below; evidence must be recorded when implemented |
| Refresh retry behavior remains unresolved | Refresh/offline_access and revocation endpoint stay deferred and unadvertised; not a blocker for milestone one |

Earlier Accepted choices are retained except the explicit lifecycle scope changes
above. Proposed endpoint labels have been replaced by accepted contracts. Standards
requirements remain mandatory; raise any discovered incompatibility with its source
and recommended resolution before changing behavior.

## Ordered implementation checklist

1. **Foundations:** add reviewed libraries from Architecture, migrations and configured
   issuer/mail/signing keys; implement account/app lock order, security versions,
   expiry checks, durable shared throttling, encrypted transactional mail outbox
   and redacted logs. Enforce schema uniqueness and credential verifier storage.
2. **Account lifecycle:** build standard forms, pre-auth CSRF/Origin checks,
   registration, verification, resend, login, recovery, password change, deletion
   and pending-account cleanup. Test concurrent consumption/replacement/deletion,
   failure rollback, mail retry and backup deletion replay before marking complete.
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

This is a specified future test fixture, not runnable in the current skeleton.
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

No source implementation or integration evidence is delivered by this documentation
change. Example backends/harness and deployment runbooks must still be built. Library
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
