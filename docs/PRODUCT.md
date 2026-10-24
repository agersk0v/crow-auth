# Crow Auth

Crow Auth is an API-first identity and subscription service for multiple apps,
including apps built by external developers. One Crow account works across apps.
Clerk and Google sign-in are references, not feature-parity requirements.

This document defines agreed product behavior. `ARCHITECTURE.md` owns technical
and security rules; [README](../README.md) owns working setup and check commands.

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
  comes from an authenticated account API, not a JWT claim. Sharing identity
  does not expose another app's data or developer configuration.
- Any signed-in developer can register an app, without manual approval. Each app
  has an owner, name, public client ID, and exact registered callback URLs.
- Only the owner manages the app. Backend integrations authenticate with a client
  secret; browser apps never receive that secret. Domain verification is not
  initially required, and registration does not establish app trustworthiness.

## Login flows

### Crow account login

Open `/login` without application parameters, enter username/password, and reach
`/account`. The account/developer session lasts up to 8 hours. A signed-out visit
to a protected Crow page requires login.

### Hosted application login

1. The app opens `/login` with its client ID, registered callback, request state,
   and PKCE challenge.
2. Crow validates the app/callback and shows login or registration.
3. The user enters credentials; Crow redirects to the registered callback with
   a short-lived, single-use authorization code and state, never tokens.
4. The app validates state and exchanges the code using its PKCE verifier.
5. Its API validates the JWT and uses the shared Crow UUID to load its own data.

Invalid or incomplete application parameters produce an error, not a fallback or
an unvalidated redirect. There is **no SSO**: each new application authorization
requires credentials, even with an existing Crow account session. Refreshing an
existing app session is separate from cross-app sign-in.

### Direct API login

Authenticated backend apps may submit username/password and application details
and receive tokens as JSON, without callbacks. Browser apps use hosted login
with PKCE. Hosted pages are optional for integrations; permitted account and
application operations remain available through APIs. Button branding is optional.

## Hosted pages

The frontend uses Next.js.

| Page | Purpose |
| --- | --- |
| `/login` | Crow account login or a validated application login |
| `/register` | Create one Crow account and continue the pending login flow |
| `/account` | Show UUID, username, subscriber status, and session logout controls |
| `/applications` | List owned apps and register an app |
| `/applications/{id}` | Manage name, callbacks, and backend client secrets; show client ID |

Profile edits and billing screens are not yet defined. No separate developer
identity, embedded components, or broad admin dashboard is required.

## Sessions and subscriptions

JWT access tokens last 10 minutes and are restricted to their application's
audience. Refresh sessions expire after 7 days idle or 30 days total, rotate on
use, and revoke the session family on reuse. Redis data loss requires login again.
Support current-session and all-session logout; existing JWTs expire naturally.

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
management. Billing still needs purchaser/customer ownership, eligible states,
pricing/plans/trials, interfaces, refunds, payment recovery, overlapping lifecycle
rules, and reconciliation. Do not invent answers during implementation.
