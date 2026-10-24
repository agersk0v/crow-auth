# Crow Auth product

Crow Auth is an API-first shared identity and subscription service for multiple
apps, including external developers' apps. One Crow account works across apps.
Every integrating app needs a backend and uses Crow-hosted login/registration.
Clerk and Google sign-in are references, not feature-parity requirements.

This document owns agreed product behavior. [Architecture](ARCHITECTURE.md)
owns technical enforcement; the [API draft](API_CONTRACT.md) owns proposed wire
formats. [README](../README.md) describes what runs today. All behavior below is
planned; see the README for implementation status.

## Accounts

Crow owns an immutable shared user UUID, unique username, password hash and
subscription status. No email is required initially. Apps associate their own
business data with the Crow UUID; they do not need separate passwords. Shared
identity grants no access to another app's data or developer configuration.

Username input is trimmed and ASCII uppercase letters are lowercased, then must
match `[a-z0-9_]+` with 3–32 characters. Usernames are unique and cannot initially
be changed.

Passwords contain 15–128 Unicode code points. Spaces, Unicode and paste are
allowed; there are no composition rules. Never trim, normalize, lowercase or
truncate passwords. Reject common/compromised passwords. Forgotten-password
recovery is unavailable and must be disclosed during registration.

## Permissions

Application authentication and user authentication are separate.

| Caller | Allowed operations | Boundary |
| --- | --- | --- |
| Visitor | Register a Crow account and begin hosted login | No private account data or app management |
| App backend with client credentials and a valid authorization code/PKCE verifier | Exchange a code for that app | Client credentials alone grant no user access |
| App backend with client credentials and a user access token | Read that user's UUID, username and subscription status | Token must belong to the authenticated app; derive user from the token, never a supplied user ID |
| App backend with client credentials and a refresh token | Refresh or revoke its session family | Bound to its user, client and family; no portal permissions |
| User with a Crow portal session | View their account, register apps and manage owned apps | Check ownership on every management operation |

App access tokens cannot authorize portal or developer operations. Subscription
status is read-only to users and apps; no public API can grant subscriber status.

## Hosted login and consent

### Application login

1. The app redirects to Crow with its client ID, registered callback, state and
   PKCE challenge.
2. Crow validates the request and shows hosted login or registration.
3. The user authenticates and approves any required consent.
4. Crow returns a short-lived authorization code and original state to the
   registered callback, never access or refresh tokens.
5. The backend validates state and exchanges the code with its client credentials
   and PKCE verifier. It validates the access JWT and uses the Crow UUID for its
   local data. It calls the identity API when it needs the username.

Hosted login is required for first-party and external apps alike. Apps must not
collect or forward Crow usernames/passwords; no app-facing password grant or
registration API exists. A “Continue with Crow” button is optional branding.

There is no initial SSO: every new app authorization requires credentials, even
with a Crow portal session or remembered consent. Registration authenticates
its pending flow without a second login. App login/registration creates no portal
session. Invalid or incomplete app parameters produce an error, never a fallback
to portal login or an unvalidated redirect.

First-use consent discloses UUID, username and subscription access, showing the
app name and destination domain without implying endorsement. Remember consent
and ask again if permissions expand. Decline consumes the flow and returns
`error=access_denied` with original state only to a validated callback. A newly
registered account remains created. Codes are issued only after required consent.

### Portal login and pages

Direct login or registration without app parameters creates a separate Crow
account/developer session and goes to `/account`. Protected pages require login.
The frontend uses Next.js; no separate developer identity is needed.

| Page | Purpose |
| --- | --- |
| `/login` | Portal login or validated app login |
| `/register` | Account creation, preserving a pending app flow |
| `/account` | UUID, username, subscription status and logout controls |
| `/applications` | List owned apps and register an app |
| `/applications/{id}` | Manage name, callbacks and client secrets; show client ID |

## Developer applications

Any signed-in developer can register an app without manual approval. An app has
an owner, name, public client ID and exact registered callbacks. Only its owner
may manage it. Domain verification is not initially required; registration does
not establish trustworthiness.

Backends keep client secrets private. Secrets are displayed once; up to two can
be active, with IDs and creation dates. Rotation overlaps until explicit
revocation. Revoking a secret does not itself revoke user sessions.

Owners can disable and re-enable apps; permanent deletion is deferred.
Disablement blocks new authorizations, exchanges, refresh and app-token access
to Crow APIs, and invalidates existing refresh families. Re-enabling never
restores them, including when Redis cleanup failed. Previously issued JWTs may
remain usable at the app's own APIs until expiry.
[Architecture](ARCHITECTURE.md#app-disablement) defines the session-version mechanism.

## Sessions and failure behavior

| Credential or flow | Lifetime |
| --- | --- |
| App access JWT | 10 minutes, restricted to one app audience |
| App refresh family | 7 days idle or 30 days total, whichever comes first |
| Crow portal session | 8 hours absolute |
| Pending hosted flow | 15 minutes |
| Authorization code | 60 seconds after issuance |

Refresh tokens rotate on use. Reuse revokes the whole family, including accidental
duplicates; there is no grace window. Backends serialize refreshes across
instances. A lost refresh response requires login rather than a blind retry.

App logout revokes its current refresh family and clears local app state.
Existing JWTs expire naturally; other apps/devices stay signed in. Portal logout
invalidates only the current portal session. Global logout is outside scope.

Exchange consumes its code once before creating a refresh session. Failure after
consumption, an uncertain write outcome or a lost success response requires a
fresh hosted login. Consumed codes are never restored. Unavailable session
storage or app-state checks grant no tokens; lost refresh records require login.

## Subscriptions and deferred scope

Before Stripe, the authenticated subscription API returns non-subscriber for
everyone. Subscription status is fetched from an API, not embedded in JWTs.
All features remain unlocked regardless of status.

Future Stripe subscriptions belong to Crow Auth. Verified events assign status
once eligible states are defined. Cancellation retains status through the paid
period. Failed payment retains existing status for a seven-day grace period;
unresolved failure then removes it. Duplicate events neither restart grace nor
grant status to a non-subscriber.

Billing still needs customer/purchaser ownership, eligible states, plans/prices,
trials, interfaces, refunds, payment recovery, lifecycle precedence, webhook
idempotency and reconciliation. Billing does not block authentication.

Profile edits, password changes, account deletion, recovery, social/passwordless
login, MFA, SSO, organizations/teams, app-specific business roles, paid feature
gating, embedded components and a broad admin dashboard are outside the initial
milestone. Define their contracts before exposure. The initial protocol is
Crow-specific; OAuth 2.0/OpenID Connect compatibility is a separate milestone.

Implementation and deployment tasks are collected in the
[Architecture checklist](ARCHITECTURE.md#milestone-checklist).
