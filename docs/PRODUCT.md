# Crow Auth

Crow Auth is an API-first identity and subscription service for multiple apps,
including apps built by external developers. One Crow account works across apps.
Every integrating application must have a backend; browser-only applications
are outside the initial scope. All integrating apps use Crow-hosted login and
registration; users enter their Crow passwords only on Crow pages.
Clerk and Google sign-in are references, not feature-parity requirements.

This document defines agreed product behavior. [ARCHITECTURE.md](ARCHITECTURE.md) records technical
and confirmed security decisions; [README](../README.md) owns working setup and check commands.

## Current status

Only the Rust/Axum HTTP service, SQLx PostgreSQL connection pool, startup
`SELECT 1` check, and `GET /health` returning `200 ok` exist. Everything below
is planned; authentication, frontend, Redis integration, and Stripe are not implemented.

## Accounts and connected apps

- Crow owns an immutable shared user UUID, unique username, password hash, and
  subscription status. No email address is required initially.
- Apps store their own business data against the Crow UUID. Separate local
  account registration or passwords are unnecessary; Crow cannot control an
  external app's database structure.
- Apps receive the authenticated user's UUID and username. Subscriber status
  comes from an authenticated subscription API, not a JWT claim. An application backend supplying client credentials and its user's
  access token may query the current user's subscription status; the user is
  derived from the validated token, not a caller-supplied user ID. Sharing identity
  does not expose another app's data or developer configuration.
- Any signed-in developer can register an app, without manual approval. Each app
  has an owner, name, public client ID, and exact registered callback URLs.
- Only the owner manages the app. Application backends authenticate with a client
  secret; browser code never receives that secret. Domain verification is not
  initially required, and registration does not establish app trustworthiness.

## Permissions

Authentication as an application and authentication as a user are separate.
A client secret identifies an application; it does not grant account-owner or
developer permissions. All permissions below describe planned behavior.

| Caller | Allowed operations | Boundaries |
| --- | --- | --- |
| Unauthenticated visitor | Create a Crow account and begin login or registration on Crow-hosted pages | Cannot read private account data or manage applications |
| Application backend authenticated with its client secret | Exchange hosted authorization codes for its own client | Must also supply a valid code with PKCE; the secret alone grants no user access |
| Application backend supplying client credentials and a user access token | Access the identity information released to that app and query their own subscription status | Crow authenticates the client, verifies the token and matching audience and derives the user from it; no other user's data, portal management, or other app's data |
| User authenticated with a Crow account/developer session | View their own account, register applications, and manage applications they own through pages or APIs | Ownership is checked on every application operation; no access to another developer's configuration or secrets |
| Holder of a valid refresh token | Refresh or revoke the corresponding session through the supported protocol | Bound to its user, client, and session family; cannot manage accounts or applications |

Application access tokens cannot be used as Crow account/developer sessions.
Account creation grants no access to existing accounts. Subscription status is
read-only for users and integrating applications. Public APIs do not allow
callers to assign themselves subscriber status. Profile edits, password changes,
account deletion, and billing mutations need explicit contracts before exposure.
Browser code never receives client secrets. Detailed endpoint contracts must
specify the required caller credentials and authorization checks.

## Login flows

### Crow account login

Open `/login` without application parameters, enter username/password, and reach
`/account`. The account/developer session lasts up to 8 hours. A signed-out visit
to a protected Crow page requires login.

### Hosted application login

1. The app opens `/login` with its client ID, registered callback, request state,
   and PKCE challenge.
2. Crow validates the app/callback and shows login or registration.
3. The user enters credentials or completes registration and approves any required
   consent; Crow redirects to the registered callback with
   a short-lived, single-use authorization code and state, never tokens.
4. The application backend validates state and exchanges the code using its
   PKCE verifier and client secret.
5. Its API validates the JWT and uses the shared Crow UUID to load its own data.

Invalid or incomplete application parameters produce an error, not a fallback or
an unvalidated redirect. There is **no SSO**: each new application authorization
requires credentials, even with an existing Crow account session. Refreshing an
existing app session is separate from cross-app sign-in.

### Integration boundary

Hosted login is the only supported application sign-in path, including for
first-party apps. Apps may display a “Continue with Crow” button that redirects
to Crow, but must not collect or forward Crow usernames/passwords for login or
registration. No application-facing password login or registration API is offered.
Crow's hosted pages submit credentials to Crow's own authentication backend.

Application APIs support code exchange, refresh, and permitted identity,
subscription, and management operations under the permissions above. The
application backend keeps its client secret private. Button branding is optional.

## Hosted pages

The frontend uses Next.js.

| Page | Purpose |
| --- | --- |
| `/login` | Crow account login or a validated application login |
| `/register` | Create one Crow account and continue the pending login flow |
| `/account` | Show UUID, username, subscriber status, and session logout controls |
| `/applications` | List owned apps and register an app |
| `/applications/{id}` | Manage name, callbacks, and backend client secrets; show client ID |

Completing registration in a valid pending hosted application flow proceeds to
consent, then issues an authorization code after approval; a second login is unnecessary. Registration without
application parameters creates an account session and continues to `/account`.
Hosted registration requests and responses still require an explicit API contract.

Profile edits and billing screens are not yet defined. No separate developer
identity, embedded components, or broad admin dashboard is required.

## Service responsibilities

Rust/Axum owns the account, application, and authentication API, including
refresh-session creation, rotation, revocation, and signing initial and refreshed
JWT access tokens. PostgreSQL stores accounts, application configuration,
consent, portal sessions and authorization flows/codes. Redis stores application
refresh-session families, token verifiers and reuse-detection state. Go is not
part of the architecture. JWT access tokens are validated cryptographically.
See ARCHITECTURE.md for storage boundaries and
remaining technical decisions. Authentication beyond the existing Rust skeleton
is planned.

## Sessions and subscriptions

JWT access tokens last 10 minutes and are restricted to their application's
audience. Refresh sessions expire after 7 days idle or 30 days total, rotate on
use, and revoke the session family on reuse. Refresh-session state lives in
Redis; unavailable storage fails closed. Lost refresh-session records require
login again; recovery must not restore old tokens as usable sessions.
Normal logout revokes the current refresh session family and clears the local
session. Existing JWTs expire naturally, within 10 minutes. Other application
sessions and devices remain signed in. Portal logout clears the current Crow
account/developer session. Global logout is outside the initial scope.

Before Stripe integration, the authenticated subscription endpoint returns a
default non-subscriber status for every user. The first authentication milestone
includes hosted application login and registration, including the hosted pages
needed for that flow. Direct password login by integrating apps is outside scope.

Stripe subscriptions belong to Crow Auth and assign a subscriber role automatically
from verified events once eligible states are defined. All features stay unlocked.
Cancellation retains status through the paid period. Failed payments retain
existing status for a 7-day grace period; unresolved failure then removes it.
Duplicate events do not restart grace or grant status to a non-subscriber.

## Scope and remaining decisions

No initial SSO, social/passwordless login, MFA, password recovery, organizations,
teams, app-specific business roles, paid feature gating, or full Clerk/Google
compatibility. These require separate scope.

Before exposing additional features, define profile edits and review the detailed API contracts. Consent, secret provisioning, session storage, and signing-key policy are confirmed below. Confirmed technical decisions are recorded in ARCHITECTURE.md; they are not yet implemented. API_CONTRACT.md proposes endpoint shapes for review. Billing still needs purchaser/customer ownership, eligible states,
pricing/plans/trials, interfaces, refunds, payment recovery, overlapping lifecycle
rules, and reconciliation. Do not invent answers during implementation.




## Confirmed implementation decisions

These decisions supersede earlier open questions. See [Architecture](ARCHITECTURE.md)
for enforcement and [API contract](API_CONTRACT.md) for proposed wire formats.

- Normalize usernames by trimming surrounding whitespace and lowercasing ASCII
  uppercase letters. Then require 3–32 characters matching `[a-z0-9_]+`. Enforce
  uniqueness in PostgreSQL. Username changes are unavailable initially.
- Passwords contain 15–128 Unicode characters; spaces, Unicode and paste are
  allowed. No composition rules. Never trim, normalize, lowercase or truncate
  passwords. Reject common/compromised passwords and store Argon2id hashes.
- Application backends store both Crow tokens; browsers receive an application
  session cookie containing a random handle. Refreshes are serialized across
  backend instances. Crow portal sessions are separate.
- Application login/registration creates no portal session. Direct portal login
  or registration creates a portal session, with an absolute eight-hour expiry.
- Authorization codes expire 60 seconds after issuance. PKCE S256 and client
  authentication are required. Pending browser-bound flows expire after 15 minutes.
- Code exchange consumes the code once before creating its Redis session.
  Failure after consumption, an uncertain write outcome, or a lost successful
  response requires a fresh hosted login. Consumed codes are never restored.
- Require HTTPS callbacks except HTTP on exact `localhost` or `127.0.0.1` hosts.
  Match complete registered URLs exactly, including path, query and port; reject
  fragments and wildcards. The actual production domain remains undecided.
- First use requires explicit consent to share UUID, username and subscription
  status. Show the application name and destination domain without implying Crow
  verification or endorsement. Remember consent; ask again if permissions expand.
  Authentication is still required for each new authorization. Issue codes only
  after consent. Declining consumes the flow and redirects to the validated
  callback with `error=access_denied` and original state, without codes or tokens.
  A newly registered account remains created. Invalid callbacks never receive redirects.
- Generate high-entropy client secrets, show once to the owner, and store only
  verifiers. Permit two active secrets per app, identified by ID and creation time.
  Rotation permits overlap until explicit revocation. Revoking a client secret
  does not itself revoke user sessions.
- Owners can disable and re-enable applications; permanent deletion is deferred.
  Disablement blocks new authorizations, code exchanges, refresh and app-token
  access to Crow APIs, and revokes existing refresh families. Re-enabling cannot
  restore revoked sessions. Existing JWTs can remain usable at app APIs until expiry.
- Every refresh authenticates client credentials and reads current app enabled
  state directly from PostgreSQL, without a Redis app-status cache. If this
  check is unavailable, refresh fails temporarily and returns no tokens.
- Identity and subscription APIs require client credentials plus the user's JWT,
  with audience equal to that authenticated client's ID. Derive the user from `sub`.
- Use temporary account-and-IP throttling, not permanent account lockout. Configure
  separate registration, code-exchange and refresh limits before public deployment.
- The initial integration protocol is Crow-specific. Do not claim OAuth 2.0 or
  OpenID Connect compliance. Standards compatibility is a separate milestone.

Billing, profile edits, password changes/deletion, recovery, MFA and future standards
compatibility remain separate work. Forgotten-password recovery is unavailable and
must be disclosed during registration. All features remain unlocked regardless of
subscription status. Before Stripe, subscription responses are non-subscriber.
