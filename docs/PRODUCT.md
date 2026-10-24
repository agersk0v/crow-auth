# Crow Auth product

Crow Auth is a shared identity provider for first-party and external developers'
apps. One Crow account works across apps. Apps use Crow-hosted login through
OAuth 2.0 Authorization Code Flow with PKCE S256 and OpenID Connect (OIDC).
Every integrating app initially needs a backend that can keep a client secret.

All behavior here is planned. [README](../README.md) owns runnable status and setup;
this document owns product behavior and lifetimes;
[Architecture](ARCHITECTURE.md) owns security enforcement and implementation checks;
[API contract](API_CONTRACT.md) owns protocol details and proposed internal APIs.
OAuth/OIDC compatibility is a target, not an implemented or certified feature.

## Accounts

Crow owns an immutable shared user UUID, username, password hash and subscription
status. Apps associate their own business data with the Crow UUID. Shared identity
gives no access to another app's data or developer configuration. No email address
is required initially.

Trim username input and lowercase ASCII uppercase letters, then require 3–32
characters matching `[a-z0-9_]+`. Usernames are unique and initially unchangeable.
Passwords contain 15–128 Unicode code points; allow spaces, Unicode and paste,
with no composition rules. Never trim, normalize, lowercase or truncate passwords.
Reject common/compromised passwords. Password recovery is unavailable and must
be disclosed during registration.

Email-based password recovery is planned for a later milestone. It may be delayed
during the prototype, but must be designed and implemented before public production
use. Initially no email is collected and recovery remains unavailable. Before recovery
launch, define verified email enrollment for existing and new accounts, secure
address changes, single-use expiring reset links, abuse protection, generic reset
responses and session invalidation after reset. Never trust an unverified address
or silently attach one during a forgotten-password request.

Initially every app receives the same public user UUID. This makes shared identity
simple, but unrelated developers can correlate a user if they compare identifiers.
It does not grant access to each other's data. Keep this initial choice explicit;
review it before onboarding unrelated external developers. Pairwise OIDC subjects
(a different stable identifier per app/sector) are the alternative and would require
an identifier-migration plan. Do not change subject semantics during implementation.

## Login and consent

1. The app backend starts authorization at `/oauth/authorize`, binding state,
   nonce and PKCE to its initiating browser session.
2. Crow validates the request and shows hosted login or registration.
3. A valid Crow browser session, fresh credentials or registration authenticates
   the flow. Crow obtains required
   consent and returns a single-use code and original state to the exact callback.
4. The backend checks state, exchanges the code at `/oauth/token`, validates the
   ID token and uses its `sub` as the Crow UUID for its local account/session.
5. Access tokens authorize Crow APIs; refresh tokens extend the app session
   when offline access was granted. ID tokens never authorize API requests.

Apps must not collect or forward Crow passwords. There is no app-facing password
grant or registration API. “Continue with Crow” branding is optional.

SSO is included: a valid Crow browser session authenticates new app authorizations
without another password entry. Direct portal login and hosted app login/registration
create the same Crow browser session, also used for portal access. App-local sessions
and refresh families remain separate. Consent is still required as documented below.
Remembered consent alone never authenticates a user. Invalid or expired app flows
never silently become portal login.

`prompt=login` and `max_age=0` require fresh authentication. Other `max_age` values
require it when the original authentication is too old. Reusing a session does not
reset its authentication time or eight-hour absolute deadline. `prompt=consent`
forces consent. With `prompt=none`, return a code only if the session, freshness
and all consent requirements are already satisfied; otherwise return login_required
or consent_required without displaying UI. Silent requests never create a new
offline-access grant that requires explicit consent.

Consent shows requested permissions, app name and destination domain without
implying endorsement. Remember ordinary consent by user, app and permission
version; ask again when permissions expand. Offline access requires explicit
consent for each new grant. Decline consumes the flow and returns access_denied
and state only to a validated callback. A newly registered account remains created.

| Scope | Permission |
| --- | --- |
| `openid` | Sign in and receive the shared UUID as `sub` |
| `profile` | Receive username as `preferred_username` |
| `crow.subscription` | Read the current user's subscription status |
| `offline_access` | Receive rotating refresh tokens after explicit consent |

The [API contract](API_CONTRACT.md) defines prompt behavior and supported features.
Google/Discord login options are separate future integrations.

## Permissions

| Caller | Allowed operations | Boundary |
| --- | --- | --- |
| Visitor | Register and begin hosted login | No private data or app management |
| Backend with client credentials and code/PKCE | Exchange its own authorization code | Client credentials alone grant no user access |
| Backend with user access token | Read UserInfo and subscription as scopes permit | Verify Crow resource audience, issuing app and scopes; derive user from `sub` |
| Backend with client credentials and refresh token | Refresh or revoke its family | Bound to that user, client and family |
| User with portal session | View account, register and manage owned apps | Check ownership on every management operation |

App access and ID tokens cannot manage the portal. UserInfo and subscription
accept access tokens, never ID tokens, and reject disabled issuing apps.
Subscription status is read-only to users/apps; no public API grants it.

## Portal and developer apps

Direct login/registration creates the shared Crow browser session and goes to
/account. Hosted app login creates that session but continues to the app callback.
Protected pages require login. Next.js serves these pages:

| Page | Purpose |
| --- | --- |
| `/login` | Portal login or hosted UI for a validated app flow |
| `/register` | Create an account, preserving any bound app flow |
| `/account` | UUID, username, subscription status and portal logout |
| `/applications` | List owned apps and register an app |
| `/applications/{id}` | Manage name, callbacks, client secrets and enabled state |

Any signed-in developer can register an app without manual approval. An app has
an owner, name, public client ID and exact registered callbacks. Only the owner
manages it. Domain verification is initially unnecessary; registration does not
establish trustworthiness.

Require HTTPS callbacks except HTTP on exact localhost or 127.0.0.1 hosts.
Match full registered URLs exactly, including path, query and port. No fragments
or wildcards. Removed callbacks cannot be used for later redirects or exchanges.

Client secrets are displayed once. Up to two can be active, identified by ID and
creation date. Rotation overlaps until explicit revocation. Revoking a secret
does not itself revoke user sessions, but subsequent authenticated requests need
a valid secret. Owners can disable/re-enable apps; permanent deletion is deferred.
Disablement blocks new authorization, exchange, refresh and app-token Crow API
access and revokes existing refresh families. Re-enabling never restores them.
Previously issued access tokens expire naturally; they are not credentials for
an app's own APIs under the initial Crow resource-token contract.

## Sessions and failure behavior

| Credential or flow | Lifetime |
| --- | --- |
| Access JWT | 10 minutes; Crow resource API audience |
| OIDC ID token | 10 minutes; integrating client audience |
| Refresh family | 7 days idle or 30 days total, whichever is first |
| Crow browser session (SSO and portal) | 8 hours absolute |
| Pending hosted flow | 15 minutes |
| Authorization code | 60 seconds after issuance |

Backends retain tokens in protected server storage; browsers receive a random
app-session cookie handle. Refreshes must be serialized across backend instances.
Rotation consumes the old refresh token. Reuse revokes the entire family,
including accidental duplicates; no grace window or response replay exists.

App logout revokes its current refresh family and clears local state. Other apps
and devices remain signed in. Portal logout invalidates the current Crow browser
session, ending SSO in that browser, but does not revoke existing app sessions
or refresh families. App logout does not end the Crow browser session; signing
in to that app again can therefore use SSO.
Global logout and OIDC logout protocols are outside initial scope. Without offline
access there is no refresh family; local app-session expiry must be bounded and
requires fresh authorization when new Crow tokens are needed. ID-token expiry
alone does not define an app's local session duration.

PostgreSQL stores all account/session state. Exchange consumes the code and,
when applicable, creates its family transactionally. Tokens are returned only
after confirmed commit. Unavailable storage grants no tokens. Uncertain commit
outcomes or lost successful exchange/refresh responses require fresh authorization;
never blindly retry. Missing refresh records require login.

## Subscriptions and deferred scope

Before Stripe, subscription reads return non-subscriber for everyone. Subscription
status stays out of tokens and gates no features. Future Stripe subscriptions
belong to Crow; detailed billing behavior is deferred until that milestone.

Before billing implementation, define purchaser/customer ownership, eligible
states, plans/prices/trials, interfaces, refunds, payment recovery, lifecycle
precedence, webhook idempotency and reconciliation. Billing does not block auth.

External/social/passwordless login, MFA, email recovery, profile/password changes,
account deletion, organizations/teams, app business roles, paid feature gating,
embedded components and a broad admin dashboard are deferred. Define their
contracts before exposure. Clerk/Google feature parity is not a requirement.

## Build order

1. Accounts, migrations and shared Crow browser sessions for portal and SSO.
2. Owned app registration, callbacks and secret management.
3. Hosted authorization, registration and consent.
4. Discovery, code exchange, ID/access tokens and UserInfo.
5. Refresh, revocation, disablement and subscription reads.
6. Integration/conformance checks and deployment hardening.

Use [Architecture's acceptance checks](ARCHITECTURE.md#first-milestone-acceptance-checks)
throughout implementation; security controls belong with each feature.
