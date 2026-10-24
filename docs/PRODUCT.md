# Product

## Promise and audience

Crow is an independent identity provider: developers add **Sign in with Crow**
to their apps, and users reuse their Crow account. The first release supports
external developers through self-service app registration. It is not limited to
the maintainer's own apps.

The maintenance budget is one person. Deliver one complete, documented OIDC
integration with conventional hosted pages. App data, app roles, memberships,
subscriptions and local user management remain the integrating developer's job.
All behavior below is Accepted planned behavior unless marked otherwise.

## Release boundary

Included: username/password accounts, required verified recovery email, password
change/reset, shared browser login, consent, developer app management, discovery,
authorization code exchange with PKCE, ID tokens and UserInfo.

Only confidential web clients with a backend are supported. Developers use
existing OIDC libraries. Crow never asks integrating apps to collect Crow passwords.

Deferred: refresh/offline access, public browser clients, native apps, dynamic
client registration, organizations, app roles, webhooks, custom domains, custom
branding, embedded components, dedicated SDKs, upstream social/passwordless login,
MFA, self-service account deletion, username/email editing and OIDC logout protocols.
Billing and subscriptions have no fields, scopes, endpoints or milestones in this
release. Future expansion requires a separate product decision.

## Accounts

Registration collects username, email and password. The account remains pending
until email verification. Pending users cannot authorize apps or manage developer
apps. Verification continues the original bound flow only if it is still valid
and belongs to the initiating browser; otherwise ask the user to restart login.
An email link alone never signs a browser in or authenticates an app flow.

Normalize usernames by trimming and lowercasing ASCII uppercase letters; require
3–32 characters matching `[a-z0-9_]+`. Usernames are unique and cannot initially
change. Login accepts username or email. Email is private recovery information;
apps receive no email claim in the first release.

Use one documented email comparison policy for registration, login and recovery:
trim surrounding whitespace, accept ASCII addresses up to 254 characters, compare
case-insensitively, and do not apply provider-specific dot or plus transformations.
Reject addresses that differ only by that comparison key. Deliver to the stored
address and prove control before activation. These are Crow account rules, not
claims about all mail providers' address semantics.

Passwords are 15–128 Unicode code points. Allow spaces, Unicode and paste, with
no composition rules. Never trim, normalize or truncate them. Reject common or
compromised passwords. Login and recovery responses must avoid disclosing whether
an account exists. Repeated attempts are throttled without permanent lockout.

Password change requires the current password; recovery uses a single-use link
sent only to the account's verified address. Both apply the registration password
policy and atomically invalidate all Crow sessions, authenticated pending flows,
authorization codes and access grants for that account. Require login afterward.
Existing sessions inside independent apps cannot be remotely cleared by Crow.

Email verification links last 24 hours; password reset links last 30 minutes.
Issuing a replacement invalidates older links of the same purpose. Reset success
sends a notification without including credentials. Existing users must have a
verified email before the public release; no production no-recovery account mode.

## Sign-in journey

1. The app backend redirects the browser to Crow using a standard OIDC request.
2. Crow validates the app and callback, then offers login or registration.
3. A valid Crow session may skip password entry. Consent still applies.
4. Crow shows the app name, callback domain and requested permissions.
5. Approval returns a short-lived code to the registered callback. The backend
   exchanges it, validates the ID token and creates its own session.

Remember consent per user, app and granted scope set. Ask again for additional
permissions or an explicit consent request. App names are developer supplied;
registration is not endorsement. Declining returns `access_denied` to a validated
callback and does not delete a newly created account.

Only `openid` and optional `profile` are supported. `openid` supplies a stable
opaque identity; `profile` adds `preferred_username`. An identity is keyed by
issuer and subject, never username or email. Subject sectors are specified in the
[API contract](API_CONTRACT.md#subjects).

## Minimal portal

| Page | Purpose |
| --- | --- |
| `/register`, `/verify-email` | Create and activate an account |
| `/login` | Portal or bound app login |
| `/forgot-password`, `/reset-password` | Verified-email recovery |
| `/account` | Username, email verification status, password change and logout |
| `/applications` | List owned apps and create an app |
| `/applications/{id}` | Name, callbacks, credentials and enabled state |

Use server-rendered forms and redirects. JavaScript is not required. There is no
public management API or broad administrator dashboard in this release.

Any verified signed-in user can create an app without manual approval, subject
to abuse limits. Every app has one owner, one name, a public client ID and exact
registered callbacks. Domain verification and ownership transfer are deferred.
Only its owner can read or change its configuration.

Client secrets are shown once. Permit at most two active secrets so developers
can rotate without downtime. Show secret identifiers and creation dates, never
retrieve plaintext. Create a replacement, deploy it, then revoke the old secret.

Disabling an app blocks authorization, exchange and UserInfo access and permanently
revokes its existing grants. Re-enabling permits new grants; old tokens stay
invalid. Secret revocation blocks subsequent client authentication but does not
itself revoke grants. Callback edits follow the API callback policy.

## Sessions and lifetimes

| Item | Lifetime |
| --- | --- |
| Crow browser session | 8 hours absolute |
| Pending authorization flow | 15 minutes |
| Authorization code | 60 seconds |
| Access token | 10 minutes |
| ID token | 10 minutes |

SSO reuses the original authentication time and session deadline. Consent is not
proof of authentication. Invalid app flows require restart and never silently
turn into portal login.

Crow logout ends the current Crow browser session and its unfinished authenticated
flows. App logout ends only that app's local session. Neither operation signs the
user out of other apps. There is no cross-app logout guarantee.

Apps choose a bounded local-session policy. Token expiry does not automatically
end that session. To obtain new Crow tokens, repeat authorization; no refresh
token is issued. Existing Crow login and remembered consent can make that quick.

## Success criteria

A developer can register an app, use an existing OIDC library, sign in a user,
rotate credentials and diagnose integration errors from the documentation.
The maintainer can deploy, restore and rotate keys without operating additional
application services. Public launch requires every release gate in
[Implementation](IMPLEMENTATION.md), not feature parity with Clerk or Google.
