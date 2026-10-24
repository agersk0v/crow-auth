# Crow Auth

Crow Auth is an API-first identity and subscription service for multiple apps,
including apps built by external developers. One Crow account works across apps.
Every integrating application must have a backend; browser-only applications
are outside the initial scope. All integrating apps use Crow-hosted login and
registration; users enter their Crow passwords only on Crow pages.
Clerk and Google sign-in are references, not feature-parity requirements.

This document defines agreed product behavior. [ARCHITECTURE.md](ARCHITECTURE.md) records technical
and security rules and distinguishes proposed defaults from confirmed behavior; [README](../README.md) owns working setup and check commands.

## Current status

Only the Rust/Axum HTTP service, SQLx PostgreSQL connection pool, startup
`SELECT 1` check, and `GET /health` returning `200 ok` exist. Everything below
is planned; authentication, frontend, Go/Redis, and Stripe are not implemented.

## Accounts and connected apps

- Crow owns an immutable shared user UUID, unique username, password hash, and
  subscription status. No email address is required initially.
- Apps store their own business data against the Crow UUID. Separate local
  account registration or passwords are unnecessary; Crow cannot control an
  external app's database structure.
- Apps receive the authenticated user's UUID and username. Subscriber status
  comes from an authenticated account API, not a JWT claim. An application's
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
| User authenticated with an application access token | Access the identity information released to that app and query their own subscription status | Crow verifies the token and expected audience and derives the user from it; no other user's data, portal management, or other app's data |
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
3. The user enters credentials or completes registration; Crow redirects to the registered callback with
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

Completing registration in a valid pending hosted application flow immediately
issues an authorization code; a second login is unnecessary. Registration without
application parameters creates an account session and continues to `/account`.
Hosted registration requests and responses still require an explicit API contract.

Profile edits and billing screens are not yet defined. No separate developer
identity, embedded components, or broad admin dashboard is required.

## Service responsibilities

Rust/Axum owns the main account, application, and authentication API with
PostgreSQL. A Go microservice handles refresh sessions using Redis and issues
refreshed JWT access tokens. Redis holds refresh-session state; JWT access tokens
are validated cryptographically. See ARCHITECTURE.md for trust boundaries and
remaining technical decisions. These services beyond the existing Rust skeleton
are planned.

## Sessions and subscriptions

JWT access tokens last 10 minutes and are restricted to their application's
audience. Refresh sessions expire after 7 days idle or 30 days total, rotate on
use, and revoke the session family on reuse. Redis data loss requires login again.
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

Before implementing affected features, define profile edits/consent, detailed API
contracts, client-secret provisioning, cookie/session storage, and signing-key
management. Proposed technical defaults are recorded in ARCHITECTURE.md and
must not be treated as already implemented. Billing still needs purchaser/customer ownership, eligible states,
pricing/plans/trials, interfaces, refunds, payment recovery, overlapping lifecycle
rules, and reconciliation. Do not invent answers during implementation.


