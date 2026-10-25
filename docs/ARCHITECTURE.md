# Crow Auth architecture and security

## Status and authority

This document owns milestone security enforcement. **Accepted** means chosen
planned behavior; **Unresolved** marks a future decision; **Deferred** is outside
this milestone. None implies implementation.

Before protocol implementation, evaluate library coverage and document which
security responsibilities remain Crow's.

## Services and trust boundaries

One Rust/Axum application owns accounts, app management, browser sessions,
authorization, token issuance and resource APIs. It renders hosted and portal
pages using templates with HTMX enhancements. Use Askama for escaped server-rendered templates; HTMX is optional. Standard forms and redirects must work without HTMX; client-side state
and HTMX headers are never authorization evidence.

Pages, API, OAuth and discovery endpoints share one configured HTTPS origin.
No separate Next.js server, refresh service or internal signing protocol is needed.
PostgreSQL is private, authenticated and authoritative for durable state.
Subscription billing is deferred.

Opaque access tokens suit the initial deployment because Crow is their only
resource server and every resource request already needs durable grant/app
checks. ID tokens remain signed JWTs for OIDC interoperability.

Application integrations and user-visible session boundaries follow
[Product](PRODUCT.md). Enforce app ownership on every management operation;
derive the owner from the browser session, never a supplied owner ID.
App tokens never authorize portal management.

## Accounts, identifiers and secrets

Product owns username/password input rules. Enforce normalized username uniqueness
in PostgreSQL. Hash passwords with Argon2id, unique random salts and stored cost
parameters. Benchmark costs before public deployment. Run hashing on a bounded
blocking-worker pool with bounded queues, away from Tokio async workers.
Request-size limits must permit the full supported UTF-8 password.
Select a common/compromised-password dataset and update process before release;
never send plaintext passwords to a third party.

Keep the internal account UUID separate from external subjects. Store a stable,
cryptographically random opaque subject for each user/sector pair, with uniqueness
constraints on the pair and the subject. Use the same mapping in ID tokens, access
tokens and UserInfo for that client's sector. Resolve it server-side to the internal account
for resource access. Do not regenerate subjects on login or secret rotation.
Preserve mappings in backups; an identifier is not an authentication credential.

Generate high-entropy client secrets and store only verifiers (apart from the encrypted, short-lived once-only delivery
context required by the management API). Product owns
display, active-secret limits and rotation behavior. Never log passwords, tokens,
authorization headers, client secrets, signing keys, database credentials or
sensitive callback queries.

Password replacement must atomically update the hash and invalidate account
sessions, authenticated flows, codes, grants and refresh families as required by
[Product](PRODUCT.md#accounts). Serialize it with authentication, authorization
completion, exchange and refresh so concurrent operations cannot commit usable
credentials based on the previous password/security state. Use the account lock and security-version mechanism in Transactions and failures. Failure rolls back
both the password update and invalidation; return success only after confirmed commit.

## Browser security and sessions

Application backends store tokens server-side; their browsers receive random
session handles in Secure, HttpOnly cookies. Each app owns its cookie, CSRF and
local-session protection.

Store verifiers of Crow browser-session tokens with user UUID, original
authentication time, creation time, absolute expiry and revocation state.
Cookies are host-only, Secure, HttpOnly and SameSite=Lax. Rotate the handle on
credential authentication, revoke the prior handle and establish a new session.
SSO reuse preserves the original authentication time and deadline.
Product owns lifetimes and the effects of portal versus app logout.

Cookie-authenticated mutations, including login/registration, require CSRF tokens
and validated Origin checks. Protect pages against framing, escape untrusted
template content, bound request sizes and prevent sensitive-response caching.
SameSite and CORS are not substitutes for CSRF or authorization. HTMX requests
receive the same checks as ordinary forms. Issue a 256-bit random host-only pre-auth browser cookie and server-side CSRF
secret on the first form GET. Forms carry a synchronizer token bound to that
browser context and action; compare in constant time. Require Origin to equal the
configured Crow origin on every browser POST; reject absent/null/mismatched
Origin with 403. Rotate CSRF state with credential login. Keep the independent
browser flow binding stable across session rotation so unauthenticated tabs may
continue; invalidate already-authenticated flows tied to the replaced session.
Use `__Host-` cookies with Path=/ and no Domain. Never accept flow IDs as CSRF.
A missing/stale CSRF token returns 403 with a restart link and no mutation.

Top-level redirects are the supported SSO path. Hidden iframes and third-party
cookies are not a reliability requirement.

## Authorization state machine

Store validated pending requests in PostgreSQL, bound to an initiating-browser
secret. Retain client, exact callback, state, nonce, scopes and PKCE challenge.
Use opaque flow IDs to support independent tabs; an ID alone grants no authority.
Flow credentials grant no portal permissions.

Stages are pending authentication, pending consent, completed, denied and expired.
Only one terminal transition is allowed. Store authenticated user, authentication
time and associated browser session server-side. Never accept a browser-selected
user ID or silently change a flow's authenticated account.
Logout or replacement of its session invalidates unfinished authenticated flows.

Enforce Product/API freshness and consent requirements. Recheck browser-session
validity and identity before completion, including after credential login.
Code creation and flow completion are atomic. Callback validation and protocol
bindings follow [API contract](API_CONTRACT.md#authorization-get-oauthauthorize).

Recheck enabled app state and current callback registration at completion and
exchange. Serialize callback removal and app disablement with these operations
using a consistent lock order. If removal wins, show a local error; do not redirect
to the removed URL. A response committed before removal cannot be recalled.

## Transactions and failures

Codes, browser sessions, consent, grants and refresh state live in PostgreSQL.
Use PostgreSQL READ COMMITTED with explicit row locks. All account-sensitive
operations lock account first, then affected apps in UUID order, then browser
session, flow, code, grant and one-time action rows in that order. Operations
without an account lock app first; never acquire an account lock after an app
lock. Read token/code metadata to discover account/app IDs, then lock and reread
all authoritative state. All paths, including deletion and app edits, obey this
order; retry deadlocks only before returning any credential.

Schema constraints include unique normalized username/email, unique user/sector
subject, unique subject, unique credential verifier, and one grant per code.
Foreign keys retain consumed-code-to-grant evidence. Store UTC deadlines and
reject at `now >= expires_at`; cleanup is asynchronous and never grants authority.
Account security_version is captured by sessions, authenticated flows, codes and
grants. Under the account lock, compare version and active/verified status before
login, completion, exchange, resource access and management. Password replacement
and deletion increment version and invalidate all account credentials atomically.
Database failures fail closed; never fall back to cached authorization.

Code exchange atomically consumes the code and creates its grant and optional
refresh family. Constraints and locking prevent duplicate consumption/families.
Prepare signed ID tokens and opaque credentials before commit; return them only after
confirmed commit. Signing failures roll back the transaction.

A confirmed rollback leaves a code usable within its original lifetime.
Milestone-one clients must start fresh authorization after any exchange failure;
no safe-retry signal or exchange replay response is implemented.
An uncertain exchange commit or lost success response requires fresh authorization;
a generic error or timeout never proves rollback.

Generate high-entropy opaque access tokens and store only their verifiers with
the issuing client, user/sector subject mapping, scopes, Crow resource audience,
issuance/expiry and grant relationship. Each resource request resolves the
verifier and checks current app/grant state; missing, expired or revoked state
grants no access. Raw tokens are returned only after confirmed commit.

Retain consumed-code verifiers and issued-grant relationships until the grant can
no longer be used. A replay passing original-client authentication, callback and
PKCE bindings revokes its refresh family, if any, and marks its access-token grant
revoked. Invalid bindings must not revoke another grant. Resource APIs check grant
state until token expiry. App-local sessions cannot be remotely cleared this way.
The access-token record links directly to its stored grant.

App disablement atomically marks the app disabled and permanently revokes its
existing grants and refresh families. Serialize it with authorization completion,
exchange and refresh through durable app state and consistent transaction locking.
Re-enabling permits new authorizations but never clears those revocations; old
access tokens remain rejected. Cleanup is not the enforcement mechanism.

## Token signing and keys

[API contract](API_CONTRACT.md#token-claims-and-validation) owns ID-token claims
and protocol validation. Sign ID tokens with a securely provisioned 3072-bit RSA private key.
Never commit private keys or bake them into images.

Rotate keys every 90 days. Publish new public keys at least ten minutes before
activation across instances; retain old public keys for at least fifteen minutes
after all instances stop signing with them. All signers must acknowledge rollout;
a lagging signer extends retention. Set JWKS Cache-Control max-age=300; clients refresh once on unknown kid,
rate-limited to once per minute per issuer, and reject if still unknown.

On suspected private-key compromise, stop issuance, replace the key, remove the
compromised public key, revoke all Crow grants/sessions and instruct integrators
to purge JWKS caches and existing local sessions before resuming. External caches
can delay rejection; a compromised signer can forge trusted tokens. Test this
procedure before public deployment; ordinary key overlap is not safe for compromise.

## Refresh and revocation

**Deferred:** refresh is outside the first complete OIDC integration.

Use high-entropy opaque refresh credentials stored as verifiers with user,
client, family, scopes, creation time, last successful use, absolute deadline and
revocation state. Retain spent-token evidence until family expiry.
Lock family/token state; rotate atomically and return credentials only after
confirmed commit. Never extend the original absolute deadline.

**Unresolved:** duplicate/retry behavior, any grace window, theft response and
recovery after an ambiguous commit or lost response. Do not implement refresh
until the policy is accepted and its protocol outcomes are recorded in the API
contract. The decision must prevent independent successor branches and define
whether spent credentials can produce any response.
If replay is selected, define protected response storage, retention and deletion;
the existing verifier-only model cannot reconstruct a returned refresh token.
Provide a reference backend covering concurrent requests and failures.

Coordinate refresh and disablement through durable app state and transaction
locking. Follow the permanent grant/family revocation rules in
[Transactions and failures](#transactions-and-failures).
Missing, expired or revoked state grants no tokens. Product owns logout effects.

## Operations and deployment gates

Use generic login failures, temporary normalized-account/IP throttling and
separate registration/exchange/refresh limits. Shared deployments need shared
budgets. Initial shared PostgreSQL-backed limits: login 10 attempts per normalized username
and 100 per source IP per 15 minutes; registration/reset/resend 5 per email and
20 per IP per hour; token exchange 120 per client and 300 per IP per minute.
Excess returns 429 with Retry-After; never permanently lock an account. Hash even
unknown-user login attempts using a dummy hash. Limiter failure returns 503.
Trust no forwarding headers initially; deployments behind a proxy must configure
an explicit trusted peer allowlist before exposure. Record event type, time,
internal actor identifier and outcome, never action-link queries; restrict logs
to operators and retain for 30 days. Request bodies are limited to 16 KiB.
Use Argon2id at minimum 19 MiB memory, 2 iterations, parallelism 1; benchmark and
raise cost to the highest sustainable setting before public deployment. Bound
hash workers to 4 and queue to 16 per instance; overload returns 503.
Use a locally provisioned compromised-password list with a recorded version,
updated monthly; missing list blocks registration/password replacement.

Before deployment, define connection budgets, timeouts, graceful shutdown,
migrations, metrics, readiness, durability, backups and restore procedures.
HTTP liveness remains distinct from database readiness.
A restore can resurrect consumed tokens and revoked sessions: invalidate affected
sessions, flows/codes and grants before serving restored state.

Any app/grant cache needs explicit invalidation and maximum staleness. Add caches,
partitioning or services only when measurements justify them.
[Implementation](IMPLEMENTATION.md) assigns verification to each milestone.

## Account actions and delivery

Generate at least 256 random bits for sessions, flow binding, codes, access tokens,
action tokens and client secrets; encode base64url without padding. Store SHA-256
verifiers of these high-entropy credentials; use constant-time comparisons.
Password hashes remain Argon2id. Action rows carry purpose, account, canonical
email snapshot, security_version, created/expiry/consumed times. Never accept a
verification token as a reset token. Under the account lock, check all bindings,
consume once and commit the lifecycle transition. Concurrent reset consumption
has exactly one winner; a new reset request serializes with consumption. On password
replacement revoke all outstanding reset links as well as Crow credentials.
Verification does not increment security_version; it cannot establish a session.

Persist delivery jobs in the same transaction as account/action creation. A worker
sends using configured SMTP with TLS validation, timeout 10 seconds and retries at
1, 5 and 15 minutes only while the action is valid. The queued link contains a
secret: encrypt payloads at rest using a separately provisioned key, restrict worker
access, never log payloads and erase on delivery or expiry. Resend/new reset removes
older unsent payloads. Sender and link origin are deployment configuration, never
request Host/forwarded headers. Account deletion cancels all existing delivery jobs.
Verification/reset confirmations require pre-auth CSRF and the action context,
not possession of a flow ID. Browser-bound action contexts live for at most five
minutes and never extend the original link deadline. Consuming an email-link GET
into a context must not invalidate the link: mail scanners cannot consume it;
only the confirmed POST does. Invalid links expose no account identity.

Deletion locks the account, then all owned apps in sorted order and marks an
irreversible deleted tombstone. Delete personal records and revoke credentials
atomically; remove encrypted mail payloads except the bounded deletion notification.
Retain non-personal revoked grants/codes until their maximum lifetime ends. Subject
mappings for a deleted user are removed; tokens referencing them fail closed.
Owned app metadata is replaced with a disabled tombstone (client ID retained,
name/callbacks/secrets removed); other users' grants for it are revoked. Append a
durable deletion ledger entry preserved independently of restorable backups.
No successful response precedes confirmed commit. Retry after uncertainty can
inspect the tombstone but cannot restore credentials.

## Library responsibility record

**Accepted planned selection:** Askama for escaped templates, argon2 for password
hashing, rand using the OS CSPRNG, sha2 for random-token verifiers, jsonwebtoken
with an explicit RSA backend/RS256 allowlist for ID-token signing and verification,
url for parsing registered redirects, and lettre for SMTP. Pin maintained versions
and audit APIs/advisories at implementation time; a replacement requires updating
this record. Axum/Tokio/SQLx are already dependencies; the other libraries are not
all installed. Argon2 and UUID support are installed for account foundations;
no library choice constitutes verified interoperability.

Crow owns OAuth/OIDC server request validation, PKCE S256 calculation, transaction
bindings, one-use semantics, discovery truthfulness, consent, pairwise mapping,
CSRF, session rotation, ownership, revocation and durable races. Do not claim that
a JWT library implements an OIDC provider. Use a maintained standards-based OIDC
client library in each example backend; its configured checks must cover the
[API validation contract](API_CONTRACT.md#token-claims-and-validation). Protocol
conformance and negative tests remain required regardless of library choice.

## Account foundation implementation

Issue #13 implements only account storage and password primitives. Accounts have
canonical unique ASCII username/email, immutable UUID/username/email, pending or
active status, verification timestamp, monotonic nonnegative security_version,
and a pending reservation deadline exactly 24 hours after creation. Database
constraints complement Rust input types; no provider email alias rules apply.
Active state requires a verification timestamp. There is no activation endpoint, cleanup job or login permission. Internal
verification credentials and activation are implemented as described below.
A future deletion migration must accommodate the specified non-identifying
tombstone/identity removal; the current identity guard deliberately rejects edits.

`create_pending` takes a caller-owned SQLx transaction so account creation and a
future encrypted verification-mail outbox can commit together. It inserts only
pending records. The caller must confirm commit before reporting browser success;
uncertain commits are failures and never justify returning credentials. Email
conflicts are internal storage outcomes, not an HTTP disclosure policy. Expired
reservations remain reserved until the lifecycle cleanup/registration transaction
releases them; no cleanup is falsely advertised by the timestamp alone.

Later login, verification, replacement and deletion must acquire the account row
`FOR UPDATE` first, then follow the authoritative lock order above, reread account
status/security_version, and check deadlines after any lock wait. Version bumps
and dependent credential invalidation must share the transaction; the current
schema's version column alone does not implement invalidation. Keep hashing
outside the account lock, then recheck authoritative credential state under lock
before authenticating. No future session/flow/token tables are created in advance.

The password service uses the accepted minimum Argon2id v19 parameters and OS-backed
salt generation from the maintained argon2/password-hash libraries. It supports
only the explicitly bounded stored PHC costs of this release; extend that reviewed
allowlist when tuning costs while retaining support for existing hashes. Four
worker permits and twenty total admission permits bound execution and waiting.
Construct one password service per server instance and clone it for handlers;
clones share admission, workers and the loaded blocklist.
Queued cancellation frees admission; running cancellation leaves both permits
with the blocking task until it finishes. Future handlers map overload to 503;
distributed throttling and same-cost unknown-user verification remain login work.
The dataset version, checksum and monthly update instructions are in
[data/README.md](../data/README.md); loading occurs at service initialization.
The health-only HTTP server does not construct a password service yet.

## Verification-action primitives

The `verification` module implements initial action issuance and atomic activation
in caller-owned transactions. Callers must roll back on every error, including
storage failures, and report success only after confirmed commit. Initial issuance
refuses an account with any existing verification action: it is not a resend API.
The next increment must integrate issuance with an encrypted transactional outbox
before exposing registration; no delivery payload storage or SMTP worker exists yet.

Credentials contain 256 OS-random bits encoded as unpadded base64url. Only SHA-256
verifiers are stored; raw values have redacted Debug output and an explicit secret
accessor for encrypted delivery integration. The maintained existing rand_core
OsRng supplies randomness, base64 supplies canonical encoding, sha2 computes the
verifier and subtle supplies constant-time verifier comparison. These libraries
do not enforce lifecycle or transaction semantics; Crow does.

Actions bind purpose, account, canonical email and security version, with a
30-minute deadline. Consumption discovers account metadata without locking, locks
the account before the action, then rereads bindings and database time after lock
waits. Both action and pending-account deadlines must remain valid. Consuming the
action and setting active/verified state share the transaction; no security-version
increment or session is created. There is no HTTP endpoint. Future browser handlers
must enforce confirmation POST, pre-auth CSRF/Origin and action-context checks;
mail-scanner GETs must never call consumption.

Resend invalidation, encrypted mail delivery and pending-account cleanup remain
unimplemented. PostgreSQL integration tests cover bindings, deadline rejection,
concurrent consumption, expiry after lock waits, migration reruns and rollback,
including a simulated activation failure after consumption.
