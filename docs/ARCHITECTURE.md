# Architecture and security

## Shape and boundaries

Accepted design: one Rust/Axum/Tokio process and private PostgreSQL. The process
serves hosted pages, the developer portal, OAuth/OIDC and UserInfo on one configured
HTTPS origin. Use server-rendered templates and ordinary forms. No separate SPA,
Redis, message broker, signing service or microservices are required.

A transactional email provider is the only additional product dependency. Persist
email jobs in PostgreSQL and process them in the same service. Multiple instances,
if later needed, must coordinate job claims and limits through durable state.

Crow authenticates its accounts. App backends authenticate their own sessions and
keep Crow credentials server-side. Crow tokens never authorize portal management
or an integrating app's own APIs. Client credentials alone grant no user access.

Before implementation, select maintained libraries and record responsibility for
provider-side request validation, JWT signing, PKCE, secure randomness, password
hashing, templates and email. Never assume a client library implements a provider.

## Durable model

These are logical records, not a prescribed migration or generic repository layer:

| Record | Responsibility |
| --- | --- |
| Accounts | Internal UUID, unique username/email keys, password hash, verification and security version |
| Browser sessions | Token verifier, account, authentication time, expiry and revocation |
| Apps and callbacks | Owner, client ID, name, immutable sector, enabled state, exact callbacks |
| Client secrets | Verifier, app, identifier, creation and revocation |
| Pending flows | Browser binding, request parameters, stage, authenticated session |
| Consents | Account, app and approved scopes |
| Subject mappings | Stable random subject per account and sector |
| Codes | Verifier, bindings, expiry, consumption and resulting grant |
| Grants and access tokens | Account, app, subject, scopes, audience, expiry and revocation |
| Email credentials and jobs | Purpose, account, verifier, expiry, consumption and delivery state |

Enforce uniqueness and foreign keys in PostgreSQL. Preserve subject mappings for
the life of the account; never regenerate them on login, secret or key rotation.
Do not expose the internal UUID. Subject-sector rules belong to the
[API contract](API_CONTRACT.md#subjects).

Use cryptographically random credentials with at least 256 bits of entropy for
sessions, flow bindings, codes, client secrets, access and email tokens. Store
verifiers, not usable bearer values. High-entropy credentials may use a maintained
cryptographic digest; passwords require Argon2id with unique salts and stored cost
parameters. Benchmark hashing and use a bounded blocking pool and queue.

## Browser protection

Crow sessions use host-only Secure, HttpOnly, SameSite=Lax cookies. Rotate the
handle on credential authentication, revoke the old handle and invalidate its
unfinished authenticated flows. SSO never extends the original absolute deadline.
Local HTTP development needs a separate explicit cookie configuration; production
must fail configuration validation if secure cookies or HTTPS are disabled.

Issue a random anonymous browser session and synchronizer CSRF token before
login/registration. Bind authenticated CSRF tokens to their current session.
All form mutations, including login, require both CSRF verification and exact
configured Origin validation; reject absent or unexpected Origin. Rotate CSRF
state with session replacement. Email-link GETs render a confirmation form; only
protected POSTs consume credentials or change account state.

Escape templates, disallow framing, set a restrictive content security policy,
use no-store for sensitive pages/responses and a no-referrer policy for credential
pages. Bound request bodies and execution time. Never use submitted return URLs,
HTMX headers or browser-provided account IDs as authority.

## Flow and transaction rules

A pending authorization request is bound to a separate initiating-browser secret;
a flow ID alone grants no authority. Each tab has its own flow record. Stages are
pending authentication, pending consent, completed, denied and expired. Exactly
one terminal transition is permitted.

Store request values server-side after validation. Bind authentication to a live
Crow session and its account. Recheck session, account security version, consent,
app enabled state and callback registration before completing. Do not silently
switch an authenticated flow to another account. Logout or session replacement
invalidates its authenticated flows. Unauthenticated flows may bind to a newly
authenticated session only after checking their own browser binding.

Code issuance and flow completion are atomic. Exchange atomically consumes a code
and creates its access grant. Prepare signed ID tokens and random credentials
before commit; send them only after confirmed commit. Signing failure rolls back.
Two simultaneous exchanges cannot both succeed.

Retain consumed-code verifiers and grant links until the issued grant expires.
A replay that passes original client authentication, callback and PKCE bindings
revokes the resulting grant and returns `invalid_grant`. Invalid bindings cannot
revoke another grant. Database failures fail closed.

A lost success response or uncertain commit requires a new authorization request.
There is no safe code-exchange retry promise. Cleanup removes expired records;
it is never the mechanism enforcing expiry or revocation.

Before each migration, document transaction isolation and lock order. Serialize
app disablement/callback removal with authorization completion and exchange.
If removal wins, no later redirect or exchange may use that callback. An already
committed response cannot be recalled. Disablement atomically revokes grants;
re-enabling never restores them.

Password replacement atomically changes the hash, advances the account security
version and invalidates the records listed in [Product](PRODUCT.md#accounts).
Credential verification, flow completion, exchange and UserInfo must reject stale
security versions. Serialize these operations with replacement so a concurrent
operation cannot commit a new credential based on the old password. Failure
rolls back replacement and invalidation together.

## Email credentials and delivery

Verification and reset credentials are purpose-bound, single-use and account-bound.
Consume them atomically with their state change, checking expiry and current
account state. Reset credentials require a verified address. Never accept a new
address from a reset request. Resending invalidates older credentials of that
purpose; throttle issuance and delivery by account and source.

A delivery job may need a usable link. Encrypt that payload at rest with a
separately provisioned key, restrict worker access and delete it after delivery
or expiry. Keep only token verifiers in the credential table. Job retries reuse
the same valid link; duplicates never create additional usable credentials.
Do not log message bodies or link queries. Email outages delay verification and
recovery; they never bypass them. Establish pending-account expiry and retention
before registration is exposed.

## Tokens and signing

Access tokens are opaque and accepted only by Crow UserInfo. Every request checks
the stored verifier, expiry, audience, scopes, account security version, enabled
app and live grant. No app/grant cache in the first release.

ID tokens are RS256 JWTs using provisioned RSA keys of at least 3072 bits. Public
keys are exposed through JWKS; private keys never enter source, images or logs.
Claims and client validation belong to the [API contract](API_CONTRACT.md#tokens).

Rotate signing keys at least every 90 days. Publish new keys ten minutes before
activation; keep old public keys fifteen minutes after the final signer stops
using them. JWKS cache max-age is at most five minutes. Unknown-key refresh must
be bounded. Emergency replacement and external cache limitations are covered in
[Operations](OPERATIONS.md#incidents).

## Abuse and failures

Use generic authentication/recovery messages and comparable processing paths.
Apply separate temporary account/source limits to registration, login, email,
client creation, authorization and exchange. Include request-size limits,
bounded password work and trusted-proxy configuration. Store shared budgets in
PostgreSQL when running more than one instance. Reject credential operations if
the required limiter cannot enforce its budget.

Record numeric limits and measured costs before public exposure. Logs may contain
request IDs, operation names and safe outcomes; never passwords, secrets, tokens,
headers, email links, database URLs or sensitive callback queries. Restrict audit
access and retention. Deployment and restore requirements belong to Operations.
