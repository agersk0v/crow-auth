# Crow Auth product

Crow Auth is a shared identity provider for first-party and external developers'
apps. One Crow account works across apps. Crow implements its own identity-provider
engine using maintained protocol and cryptographic libraries. Apps use Crow-hosted login through
OAuth 2.0 Authorization Code Flow with PKCE S256 and OpenID Connect (OIDC).
Every integrating app initially needs a backend that can keep a client secret.

This document owns milestone product behavior. **Accepted** means chosen planned
behavior; **Deferred** is outside this milestone. Neither implies implementation.

## Accounts

Crow owns an immutable internal UUID, username, verified email and password hash.
Each app receives a stable pairwise opaque subject for its sector and owns its
business data. Shared identity gives no access to another app's data/configuration.
Email verification, recovery and deletion are required for milestone one.

Trim usernames, lowercase ASCII uppercase letters and require 3–32 characters
matching `[a-z0-9_]+`; enforce uniqueness. Usernames are initially unchangeable.
Passwords contain 15–128 Unicode code points; allow spaces, Unicode and paste,
with no composition rules. Never normalize, trim or truncate passwords. Reject
common/compromised passwords using the Architecture policy.

Require an email at registration. For the initial ASCII-only email policy, trim
outer whitespace, lowercase the whole address, require one @, an ASCII dot-atom
local part and a valid dotted DNS domain, maximum 254 bytes. Do not apply provider
alias/dot rules. Disclose case-insensitive matching; internationalized addresses
and email changes are deferred. Enforce unique canonical email in PostgreSQL.
Registration creates a pending account and sends a verification link. Duplicate
username is a field error; duplicate email returns the same generic confirmation
as a new registration and never changes an existing account or its password.
Pending accounts expire after 24 hours and release username/email reservations.
A duplicate pending registration may request resend but cannot replace its password.

Verification credentials expire after 30 minutes, are single-use and bound to the
account/email and action purpose. Resend invalidates earlier verification links.
Following an emailed GET only renders a confirmation form with hidden verification
and CSRF tokens; POST consumes the link. The token can remain in the initial URL
until POST, with no-store/no-referrer and redacted logs. No separate browser-bound
verification-context record or five-minute form deadline is required. Opening the
email on another device is supported using that browser's own pre-auth cookie/CSRF.
Verification activates the account but does not sign in the link-opening browser.
Require password sign-in after verification. An unverified password sign-in grants
no SSO/portal session; show verification/resend guidance after valid credentials.
Registration creates no authenticated session. An original browser-bound app flow
may continue after verification and password login if still valid; otherwise
restart authorization. Email links never override browser flow binding.

Recovery accepts the canonical email and always shows the same generic result
whether absent, pending, deleted or active. Only active verified accounts receive
a reset link. Reset credentials expire after 15 minutes; a new request invalidates
older reset links. GET renders a form, POST consumes the credential and replaces
the password atomically. A wrong/expired/used link makes no change and shows
“Request a new recovery link.” Delivery failure does not disclose account existence;
queue delivery with bounded retries. No support bypass or security questions.
Authenticated password change requires current password and the same new-password
policy. Successful reset/change invalidates all Crow sessions, authenticated flows,
codes and grants, including the requesting browser; require new password login.
An attacker possessing only an unverified address cannot recover an account.

Deletion requires an active portal session, current-password reauthentication
within five minutes and explicit “Delete account” confirmation. Immediately revoke
all Crow credentials and unfinished flows; permanently disable owned apps and
revoke their credentials/grants, including grants of other users. Owned apps cannot
be restored or transferred in this milestone. Remove username, email, password,
consents and subject mappings at committed deletion; retain a non-identifying
UUID/version tombstone for revocation integrity. Never reuse old subjects or owned
client IDs. Username/email can be registered again as a new account. There is no
undo. Encrypted backups age out within 30 days and are inaccessible to live auth;
restore must reapply deletion tombstones before serving traffic. Notification mail
may be queued to the former verified address at deletion, then erased from the
outbox after delivery/24-hour retry deadline. Security logs contain no username/email.

Crow logout, reset and deletion cannot clear cookies or data held by external apps.
Explain this before deletion. The example apps bound local sessions to eight
hours absolute and consult Crow only during new authorization/UserInfo requests;
reset/deletion therefore do not guarantee immediate logout inside an existing app.
External apps own separate deletion/retention obligations. Global session propagation
is deferred; it is not an implied security guarantee of this milestone.

Subjects must not reveal or permit derivation of the internal UUID; usernames and
email addresses are not subjects. The [API contract](API_CONTRACT.md#pairwise-subject-sectors)
defines sectors; [Architecture](ARCHITECTURE.md#accounts-identifiers-and-secrets)
owns subject mapping and storage.

## Login and consent

1. The app backend starts authorization at `/oauth/authorize`, binding state,
   nonce and PKCE to its initiating browser session.
2. Crow validates the request and shows hosted login or registration.
3. A valid Crow browser session or fresh credentials authenticate
   the flow for an email-verified account. Crow obtains required
   consent and returns a single-use code and original state to the exact callback.
4. The backend checks state, exchanges the code at `/oauth/token`, validates the
   ID token and uses its `sub` as the app-specific identity for its local account/session.
5. Access tokens authorize Crow APIs. ID tokens never authorize API requests.
   **Deferred:** refresh tokens extend access when offline access is introduced.

The first complete integration includes hosted login, consent, discovery, code
exchange, ID tokens and UserInfo. It issues no refresh tokens and does not support
`offline_access`; apps repeat authorization when new Crow tokens are needed.
Refresh and subscription reads are later milestones, independently of billing.

Apps must not collect or forward Crow passwords. There is no app-facing password
grant or registration API. “Continue with Crow” branding is optional.

SSO is included: a valid Crow browser session authenticates new app authorizations
without another password entry. Direct portal login and hosted app password login
create the same Crow browser session, also used for portal access. App-local sessions
and refresh families remain separate. Consent is still required as documented below.
Remembered consent alone never authenticates a user. Invalid or expired app flows
never silently become portal login.

SSO reuse preserves the original authentication time and browser-session deadline.
Apps can request fresh authentication, forced consent or a silent result using
[API prompt and freshness rules](API_CONTRACT.md#authorization-get-oauthauthorize).

Consent shows requested permissions, app name and destination domain without
implying endorsement. Remember ordinary consent by user, app and permission
version; ask again when permissions expand. Initially permission version is 1;
remember the approved scope set, and require consent if requested scopes are not
a subset or the version changes. Authenticate and recheck account/session at the
consent POST; approval and code creation commit together. No app can preapprove
consent or supply its text. **Deferred:** offline access requires explicit
consent for each new grant once supported. Decline consumes the flow and returns access_denied
and state only to a validated callback. A newly registered account remains created.

| Scope | Permission |
| --- | --- |
| `openid` | Sign in and receive a pairwise opaque subject as `sub` |
| `profile` | Receive username as `preferred_username` |
| `crow.subscription` (**Deferred**) | Read the current user's subscription status |
| `offline_access` (**Deferred**) | Receive rotating refresh tokens after explicit consent |

The [API contract](API_CONTRACT.md) defines prompt behavior and supported features.
Google/Discord login options are separate future integrations.

## Permissions

| Caller | Allowed operations | Boundary |
| --- | --- | --- |
| Visitor | Register and begin hosted login | No private data or app management |
| Backend with client credentials and code/PKCE | Exchange its own authorization code | Client credentials alone grant no user access |
| Backend with user access token | Read UserInfo; subscription deferred | Crow verifies resource audience, issuing app and scopes, then resolves the subject |
| Backend with client credentials and refresh token (**Deferred**) | Refresh or revoke its family | Bound to that user, client and family |
| User with portal session | View account, register and manage owned apps | Check ownership on every management operation |

App access and ID tokens cannot manage the portal. UserInfo and subscription
accept access tokens, never ID tokens, and reject disabled issuing apps.
Subscription status is read-only to users/apps; no public API grants it.

## Portal and developer apps

Direct verified-account password login creates the shared Crow browser session and goes to
/account. Hosted app login creates that session but continues to the app callback.
Protected pages require login. Axum renders templates with HTMX enhancements
for these pages:

| Page | Purpose |
| --- | --- |
| `/login` | Portal login or hosted UI for a validated app flow |
| `/register` | Create an account, preserving any bound app flow |
| `/account` | Username, verified email, password change, deletion and Crow logout |
| `/applications` | List owned apps and register an app |
| `/applications/{id}` | Manage name, callbacks, client secrets and enabled state |

Any active verified user with a portal session can register an app without manual approval. An app has
an owner, name, public client ID and exact registered callbacks. Only the owner
manages it. Domain verification is initially unnecessary; registration does not
establish trustworthiness.

Require HTTPS callbacks except HTTP on exact localhost or 127.0.0.1 hosts
in an explicitly enabled development deployment. Reject userinfo, fragments,
wildcards, backslashes, control characters and malformed URL encodings; prohibit
reserved OAuth response query keys code/state/iss/error/error_description/error_uri.
Store accepted absolute URLs without silent rewriting; compare their exact strings.
Require 1–10 callbacks, one sector hostname, and a 1–100 character app display name.
Management mutations require password reauthentication within five minutes for
secret issuance/revocation, app disable/enable and callback edits. Ordinary ownership
failures return 404 without revealing app existence.
Match full registered URLs exactly, including path, query and port. No fragments
or wildcards. Removed callbacks cannot be used for later redirects or exchanges.

Client secrets are displayed once. Up to two can be active, identified by ID and
creation date. Rotation overlaps until explicit revocation. Revoking a secret
does not itself revoke user sessions, but subsequent authenticated requests need
a valid secret. Owners can disable/re-enable apps; permanent deletion is deferred.
Disablement blocks new authorization, exchange, refresh and app-token Crow API
access and permanently revokes existing grants and refresh families. Re-enabling
never restores them; old access tokens remain unusable at Crow APIs even before
expiry. These tokens are not credentials for an app's own APIs under the initial
Crow resource-token contract. Existing app-local sessions are not remotely cleared.

## Sessions and failure behavior

| Credential or flow | Lifetime |
| --- | --- |
| Opaque access token | 10 minutes; Crow resource APIs |
| OIDC ID token | 10 minutes; integrating client audience |
| Refresh family (**Deferred**) | 7 days idle or 30 days total, whichever is first |
| Crow browser session (SSO and portal) | 8 hours absolute |
| Pending hosted flow | 15 minutes |
| Authorization code | 60 seconds after issuance |

Backends retain tokens in protected server storage; browsers receive a random
app-session cookie handle. **Deferred:** refresh tokens rotate;
[Architecture](ARCHITECTURE.md#refresh-and-revocation) owns enforcement and the
unresolved retry policy.

App logout uses the app’s CSRF-protected POST to invalidate its server-side local
session, erase associated stored tokens and expire its cookie; once refresh is supported, it also revokes its current
refresh family. Other apps
and devices remain signed in. Portal logout invalidates the current Crow browser
session, ending SSO in that browser, but does not revoke existing app sessions
or refresh families. App logout does not end the Crow browser session; signing
in to that app again can therefore use SSO.
Crow logout also invalidates unfinished authenticated flows tied to that session,
but leaves already-issued codes and grants intact. Logout is POST with CSRF;
repeated logout clears the cookie and shows the signed-out page.
Global logout and OIDC logout protocols are outside initial scope. Without offline
access there is no refresh family; local app-session expiry must be bounded and
requires fresh authorization when new Crow tokens are needed. ID-token expiry
alone does not define an app's local session duration.

Storage and transaction failure guarantees are defined in
[Architecture](ARCHITECTURE.md#transactions-and-failures).

## Subscriptions and deferred scope

Before Stripe, subscription reads return non-subscriber for everyone. Subscription
status stays out of tokens and gates no features. Future Stripe subscriptions
belong to Crow; detailed billing behavior is deferred until that milestone.

Before billing implementation, define purchaser/customer ownership, eligible
states, plans/prices/trials, interfaces, refunds, payment recovery, lifecycle
precedence, webhook idempotency and reconciliation. Billing does not block auth.

**Deferred:** external/social/passwordless login, MFA, profile changes,
organizations/teams, app business roles, paid feature gating,
embedded components and a broad admin dashboard are deferred. Define their
contracts before exposure. Recovery, verification, deletion and password changes are part of milestone one. Clerk/Google feature parity is not a requirement.

## Implementation

[Implementation milestones](IMPLEMENTATION.md) define build order and completion
evidence. Security controls belong with each feature, not in a final hardening pass.
