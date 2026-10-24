# Crow Auth product

Crow Auth is a shared identity provider for first-party and external developers'
apps. One Crow account works across apps. Crow implements its own identity-provider
engine using maintained protocol and cryptographic libraries. Apps use Crow-hosted login through
OAuth 2.0 Authorization Code Flow with PKCE S256 and OpenID Connect (OIDC).
Every integrating app initially needs a backend that can keep a client secret.

**Accepted:** planned product behavior unless labeled otherwise. See
[AGENTS.md](../AGENTS.md) for document ownership and decision labels.

## Accounts

Crow owns an immutable internal user UUID, username, password hash and subscription
status. Each app receives a stable pairwise opaque subject for its sector and associates its own
business data with that subject; the internal UUID is not exposed to integrating apps. Shared identity
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

Subjects must not reveal or permit derivation of the internal UUID; usernames and
email addresses are not subjects. The [API contract](API_CONTRACT.md#pairwise-subject-sectors)
defines sectors; [Architecture](ARCHITECTURE.md#accounts-identifiers-and-secrets)
owns subject mapping and storage.

## Login and consent

1. The app backend starts authorization at `/oauth/authorize`, binding state,
   nonce and PKCE to its initiating browser session.
2. Crow validates the request and shows hosted login or registration.
3. A valid Crow browser session, fresh credentials or registration authenticates
   the flow. Crow obtains required
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
without another password entry. Direct portal login and hosted app login/registration
create the same Crow browser session, also used for portal access. App-local sessions
and refresh families remain separate. Consent is still required as documented below.
Remembered consent alone never authenticates a user. Invalid or expired app flows
never silently become portal login.

SSO reuse preserves the original authentication time and browser-session deadline.
Apps can request fresh authentication, forced consent or a silent result using
[API prompt and freshness rules](API_CONTRACT.md#authorization-get-oauthauthorize).

Consent shows requested permissions, app name and destination domain without
implying endorsement. Remember ordinary consent by user, app and permission
version; ask again when permissions expand. **Deferred:** offline access requires explicit
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
| Backend with user access token | Read UserInfo and subscription as scopes permit | Crow verifies resource audience, issuing app and scopes, then resolves the subject |
| Backend with client credentials and refresh token | Refresh or revoke its family | Bound to that user, client and family |
| User with portal session | View account, register and manage owned apps | Check ownership on every management operation |

App access and ID tokens cannot manage the portal. UserInfo and subscription
accept access tokens, never ID tokens, and reject disabled issuing apps.
Subscription status is read-only to users/apps; no public API grants it.

## Portal and developer apps

Direct login/registration creates the shared Crow browser session and goes to
/account. Hosted app login creates that session but continues to the app callback.
Protected pages require login. Axum renders templates with HTMX enhancements
for these pages:

| Page | Purpose |
| --- | --- |
| `/login` | Portal login or hosted UI for a validated app flow |
| `/register` | Create an account, preserving any bound app flow |
| `/account` | Internal UUID, username, subscription status and portal logout |
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

App logout clears local state and, once refresh is supported, revokes its current
refresh family. Other apps
and devices remain signed in. Portal logout invalidates the current Crow browser
session, ending SSO in that browser, but does not revoke existing app sessions
or refresh families. App logout does not end the Crow browser session; signing
in to that app again can therefore use SSO.
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

**Deferred:** external/social/passwordless login, MFA, email recovery, profile/password changes,
account deletion, organizations/teams, app business roles, paid feature gating,
embedded components and a broad admin dashboard are deferred. Define their
contracts before exposure. Clerk/Google feature parity is not a requirement.

## Implementation

[Implementation milestones](IMPLEMENTATION.md) define build order and completion
evidence. Security controls belong with each feature, not in a final hardening pass.
