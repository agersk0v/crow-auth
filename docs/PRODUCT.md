# Crow Auth product

Crow Auth is a shared identity provider for first-party and external developers'
apps. One Crow account works across apps. Crow implements its own identity-provider
engine using maintained protocol and cryptographic libraries. Apps use Crow-hosted login through
OAuth 2.0 Authorization Code Flow with PKCE S256 and OpenID Connect (OIDC).
Every integrating app initially needs a backend that can keep a client secret.

**Accepted:** planned product behavior unless labeled otherwise. See
[AGENTS.md](../AGENTS.md) for document ownership and decision labels.

## Release goal

Operate one shared Crow account service for participating apps. The first release
covers account registration, sign-in, email verification, recovery and deletion;
a shared Crow browser session with explicit logout boundaries; backend-app OIDC
integration with PKCE S256; and owner-managed app registration, callbacks and
credentials. Each app owns its business data, permissions and local sessions.

The first integration milestone is one user signing into two independently
registered example apps in the same browser after entering Crow credentials only
once. It proves the core SSO journey, not completion of the whole release.
[Implementation milestones](IMPLEMENTATION.md#first-integration-milestone-two-example-apps)
own its acceptance evidence.

## Accounts

Crow owns an immutable internal user UUID, username, password hash and subscription
status. Each app receives a stable pairwise opaque subject for its sector and associates its own
business data with that subject; the internal UUID is not exposed to integrating apps. Shared identity
gives no access to another app's data or developer configuration. Registration
requires an email address for verification and recovery. Email is private account
data and is not shared with apps in the initial scope.

Trim username input and lowercase ASCII uppercase letters, then require 3–32
characters matching `[a-z0-9_]+`. Usernames are unique and initially unchangeable.
Passwords contain 15–128 Unicode code points; allow spaces, Unicode and paste,
with no composition rules. Never trim, normalize, lowercase or truncate passwords.
Reject common/compromised passwords.

### Email verification and recovery

Create a pending account at registration and send a single-use expiring verification
link. Until verification succeeds, allow verification/resend and logout, but no
app authorization or developer management. Resending is throttled. Verification
must prove control of the account's pending address, not a browser-submitted user ID.
Do not auto-merge accounts by email or allow an existing address to be claimed again.

Recovery accepts an email address and always shows the same acknowledgement,
whether an account exists or not. Send a single-use expiring reset link only to
the account's verified address. A successful reset replaces the password and
requires a new sign-in; it does not automatically create a session. Recovery is
part of the first release, although bounded internal prototypes may precede it.
Never trust an unverified address or attach one during a recovery request.

**Unresolved before email implementation:** address normalization and uniqueness,
verification/reset token lifetimes, resend invalidation, delivery provider,
throttling and handling of previously created prototype accounts. Define secure
address-change behavior before enabling address changes; changes are deferred.

Before public production, support authenticated password changes as well as
verified-email recovery. A change requires fresh verification of the current
password; a reset requires a valid single-use recovery credential. Apply the same
password policy to both. Successful replacement invalidates all Crow browser
sessions, unfinished authenticated flows, outstanding authorization codes, grants
and refresh families for the account. Require a new login, including for the
requesting browser. Existing app-local sessions cannot be remotely cleared under
the initial integration contract. Define endpoint/UI shapes before implementation.

Subjects must not reveal or permit derivation of the internal UUID; usernames and
email addresses are not subjects. The [API contract](API_CONTRACT.md#pairwise-subject-sectors)
defines sectors; [Architecture](ARCHITECTURE.md#accounts-identifiers-and-secrets)
owns subject mapping and storage.

### Account deletion

Deletion is part of the first release. Require a signed-in user, fresh password
verification, CSRF protection and explicit confirmation describing the consequences.
Immediately disable the account and invalidate all its Crow sessions, pending
flows, codes, grants and any refresh families. Disable owned apps and revoke their
grants so deletion cannot leave usable developer credentials behind.

Remove account profile data, password/recovery credentials, consent and subject
mappings through a documented deletion process. Never reuse deleted subjects for
another account. Explain that deletion cannot erase data or end existing local
sessions inside independent apps; users must contact those apps separately.

**Unresolved before deletion implementation:** retention and purge deadlines,
minimal security records, purging disabled owned-app records, backup expiry and restoration
suppression. These must be disclosed before confirmation. The immediate access
revocation is mandatory regardless of the later purge schedule.

## Login and consent

1. The app backend starts authorization at `/oauth/authorize`, binding state,
   nonce and PKCE to its initiating browser session.
2. Crow validates the request and shows hosted login or registration.
3. A valid Crow browser session, fresh credentials or completed registration authenticates
   the flow only for a verified account. Crow obtains required
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
without another password entry. Direct portal login and hosted app login after verification
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

Direct login for a verified account creates the shared Crow browser session and
goes to /account. Registration goes to verification first. Hosted app login creates that session but continues to the app callback.
Protected pages require login. Axum renders templates with HTMX enhancements
for these pages:

| Page | Purpose |
| --- | --- |
| `/login` | Portal login or hosted UI for a validated app flow |
| `/register` | Create and verify an account, preserving any valid bound app flow |
| `/account` | Account status, username, email verification, recovery guidance, deletion and logout |
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

**Deferred:** external/social/passwordless login, MFA, profile changes,
organizations/teams, app business roles, paid feature gating,
embedded components and a broad admin dashboard are deferred. Define their
contracts before exposure. Password recovery and password changes are required
before public production as specified above. Clerk/Google feature parity is not a requirement.

## Implementation

[Implementation milestones](IMPLEMENTATION.md) define build order and completion
evidence. Security controls belong with each feature, not in a final hardening pass.
