# Crow Auth product definition

This document owns Crow Auth's product scope and intended behavior. It separates
implemented functionality, agreed plans, and unresolved decisions. Planned
features do not exist merely because they are described here.

[README](../README.md) owns implemented setup, configuration, and check commands.
Technical design and security rules belong in
[ARCHITECTURE](ARCHITECTURE.md). Security remains part of that document rather
than a separate file at this stage.

## Purpose

Crow Auth is an external identity and subscription service shared by multiple
applications, including applications built by external developers. A person
creates one Crow account and can use it to sign into any connected application.
Clerk and signing into different apps with Google are references for the
experience; compatibility or feature parity is not an initial requirement.

Crow Auth owns the immutable UUID user ID, unique username, securely hashed password,
and Crow subscription status. Connected applications own their application data
and associate it with the Crow user ID. They do not need separate local account
registration, usernames, passwords, or an independently generated user identity.
Crow cannot enforce how external developers structure their databases.

Crow Auth is API-first. Every supported account and authentication operation
must be available through its API; hosted pages are an optional integration
experience rather than a requirement for consuming applications. A button labeled
"Continue with Crow" is optional branding, not a required UI component.

## Implemented baseline

The repository contains a Rust HTTP service using Axum and Tokio, a SQLx
PostgreSQL connection pool, and a startup `SELECT 1` check. `GET /health` returns
`200` with body `ok`; it is an HTTP liveness check rather than an ongoing database
readiness check.

User storage, application registration, authentication, JWT issuance, Next.js
pages, subscriptions, the Go refresh-token service, and Redis integration are
not implemented yet. All following feature sections describe planned behavior.

## Shared accounts and application data

Initial registration and sign-in use simple usernames and passwords; no email,
social login, or additional account factors are required initially. Usernames are
3–32 ASCII letters, digits, or underscores, with case-insensitive uniqueness.
Passwords are 15–128 characters, allow spaces, and have no mandatory character
combinations; they are hashed with Argon2id. The same Crow account and stable user ID work across connected applications; visiting a second
application does not require another Crow registration.

Applications obtain the authenticated user's ID through validated tokens or an
authenticated account API. They use that ID to retrieve their own application
records. For example, Crow Fitness stores workout data against a Crow user ID;
another app stores its own data against that same ID. Crow Auth does not store
those applications' business data.

Shared identity does not grant access to another application's data or developer
configuration. Connected apps receive the user UUID and username. Subscriber
status is available through an authenticated account API rather than embedded
in access tokens. Each app may read only the permitted information for its
authenticated user, not arbitrary users or another developer’s configuration.

## Developers and application registration

Any developer with a Crow account can register an application through the API
or frontend. No separate manual approval is required. The signed-in developer
owns the application and is the only developer allowed to manage its initial
configuration; teams and shared ownership are outside the initial scope.

Registration is lightweight:

1. The developer provides an application name and exact allowed callback URLs.
2. Crow assigns a public client ID identifying that application.
3. The developer integrates using either Crow's hosted login or the direct API.

A client ID is public identification, not a secret or proof of ownership.
Application-management requests require authentication and ownership checks.
Direct API authentication also requires a registered application.

For hosted login, Crow validates the requested callback against the URLs
registered for that client using exact matching, not an arbitrary supplied URL,
request origin, or referrer. Production callbacks use HTTPS; controlled localhost
callbacks may be supported for development. Detailed development URL rules
belong in the security contract.

Separate proof of domain ownership, such as a DNS TXT record or hosted verification
file, is not required initially. Callback registration does not prove domain
control or establish that an application is trustworthy.

## Login and registration flows

### Sign into Crow itself

Opening `/login` without application-login parameters signs the user into Crow.
After successful login, the user reaches `/account`. Opening a protected Crow
account or developer page while signed out requires Crow sign-in first.

This is distinct from logging into an external application. Crow's own pages
must not require an external callback URL.

### Hosted application login

An application opens Crow's `/login` page with its client ID, a registered callback
URL, request state, and a PKCE challenge. Callback details remain associated with
the pending authentication flow while the user logs in or registers.

1. Crow validates the application and callback before continuing.
2. The user authenticates with their Crow username and password, or registers
   one Crow account if they do not already have one.
3. Crow redirects to the validated callback with a short-lived, single-use
   authorization code and the request state. It does not put JWTs or refresh
   tokens in the callback URL.
4. The application validates the returned state and exchanges the code using
   its PKCE verifier to obtain tokens.
5. The receiving API validates the JWT, including its intended audience, before
   using the Crow user ID to access application data.

Codes are bound to the requesting application, callback, and PKCE challenge.
A code cannot be redeemed for another application or reused after a successful
exchange. Exact endpoints and protocol details belong in the technical contracts.

Incomplete or invalid application-login parameters produce an error; they must
not silently fall back to Crow's own login or redirect to an unvalidated URL.
For application login, the callback is required. A plain visit to `/login`
without application-login parameters remains the Crow account flow.

### Direct API authentication

Applications can register users and authenticate entirely through HTTP API
requests without using Crow's frontend. Direct login accepts a username,
password, and application identification and returns tokens as JSON. It does
not accept a callback or redirect tokens to a URL.

Applications that collect Crow credentials directly handle those credentials;
Crow's hosted login avoids exposing passwords to a connected application's
login form. Direct username/password login is allowed only for authenticated
backend applications using a client ID and randomly generated client secret.
Browser applications use hosted login with PKCE and must not receive backend
client secrets. Allow up to two active secrets during rotation so the developer
can deploy a replacement before revoking the old secret. A public client ID alone
must not authorize tokens for that client.

The exact endpoint paths, request/response payloads, and error contracts are not
yet defined. `/login` above identifies the hosted page, not a finalized API route.

## Hosted frontend

The frontend uses Next.js. The initial page scope is:

| Page | Purpose |
| --- | --- |
| `/login` | Crow sign-in or hosted application sign-in, depending on validated flow parameters |
| `/register` | Create a shared Crow account and continue the pending flow, or enter the Crow account area |
| `/account` | Show the user's Crow ID, username, and subscriber status |
| `/applications` | List the developer's applications and register an application |
| `/applications/{id}` | Manage an owned application's name and registered callbacks; show its client ID |

Developer pages use the same Crow account, not a separate developer identity.
The frontend calls the service APIs; it must not be the only interface for account
or application management. Profile edits and any additional fields remain to be
defined. Embedded components and a broad administrative dashboard are not initial
requirements. Billing screens await the billing decisions below.

## Tokens, sessions, and service responsibilities

JWT access tokens contain the stable Crow user ID in `sub` and identify their
intended audience in `aud`. In the initial model each connected application has
its own audience. A token issued for one application must not authenticate the
user to another application's API. Receiving APIs must validate Crow's signature,
issuer, expiration, and expected audience. Changing an audience invalidates the
signature.

Connected APIs should be able to verify signatures using Crow's published public
signing keys rather than making a Crow request for every application request.
A protected account API provides permitted user information when needed.

The main authentication API uses Rust and Axum. A Go microservice in this same
repository handles refresh tokens using Redis for refresh-token state.
PostgreSQL stores persistent identity, application, and subscription data.

Access tokens expire 10 minutes after issuance. Refresh sessions expire after
7 days of inactivity or 30 days from their original creation, whichever comes
first. Every successful refresh replaces the refresh token; reuse revokes that
session’s token family. Clients must coordinate concurrent refresh attempts.
Redis data loss invalidates affected refresh sessions and requires login again;
already-issued access tokens expire normally.

Support both signing out of the current session and signing out of all sessions.
Revoke the affected refresh and Crow browser sessions; already-issued access
tokens remain usable until their normal expiry. Global logout does not require
connected APIs to make an online revocation check for every request.

Shared accounts are required, but single sign-on is outside the initial scope.
Each new hosted application authorization requires username/password entry, even
if the user is signed into Crow or another connected application. A Crow account
page session must not silently authenticate the user to a connected app. Refreshing
an existing application session is separate from cross-application single sign-on.
A Crow account/developer-page session lasts up to 8 hours after sign-in. Consent
requirements and the exact browser-session storage remain to be defined.

## Subscriptions and roles

Stripe provides subscriptions tied to Crow Auth, rather than separate subscriptions
owned by each connected application. The initial subscription concept is a
subscriber role associated with the Crow identity. All features remain unlocked;
there is no initial paid-feature gating, plan-based access restriction, or paid
API limit.

Assign the subscriber role automatically from verified Stripe events once the
eligible subscription states have been defined. Cancellation preserves the role
until the paid period ends. A failed payment starts a 7-day grace period during
which subscriber status is retained; an unresolved failure removes the role at
the end of that period. An account without subscriber status does not gain it
merely because a payment failed. Duplicate failure events must not restart grace.
Applications read current locally recorded subscriber status through the
authenticated account API, not a possibly stale JWT claim.

The following remain unresolved:

- Who purchases subscriptions and how Stripe customers map to Crow accounts.
- Eligible Stripe states and precedence between cancellation and payment failure.
- Pricing, plans, trials, and purchase or billing-management interfaces.
- Refunds, reinstatement after payment recovery, and webhook reconciliation.

Stripe Checkout and Customer Portal are options, not agreed requirements. Do not
invent additional subscription lifecycle rules or role transitions during implementation.

## Agreed technology constraints

| Component | Choice |
| --- | --- |
| Main HTTP API | Rust, Axum, and Tokio |
| Hosted frontend | Next.js |
| Public routing | Nginx combines the frontend and backend under one origin |
| Shared user IDs | Immutable UUIDs |
| Persistent storage | PostgreSQL, accessed by the Rust API through SQLx |
| Access tokens | JWT, with application-specific audiences and 10-minute expiry |
| Hosted token handoff | Authorization code with PKCE and registered callbacks |
| Refresh-token handling | Go microservice in this repository |
| Refresh-token state | Redis |
| Subscriptions | Stripe |

Schemas, deployment, password hashing parameters, API contracts, and protocol
implementation belong in the architecture and security documents. This product
scope does not yet claim OAuth or OpenID Connect standards compliance.

## Outside the initial scope

- Password recovery/reset; recovery email requirements are undefined.
- Social login, passwordless login, multi-factor authentication, and single sign-on.
- Embedded authentication UI components.
- Organizations, teams, and application-specific business authorization roles.
- Paid feature gating and a full entitlement system.
- Full Clerk or Google compatibility or feature parity.

These exclusions concern the initial scope and can change through later product
decisions. The subscriber role remains in scope.

## Decisions needed before implementation

Resolve decisions for an affected feature before implementing it. Independent
work within agreed scope can continue.

1. Permitted profile edits; email is not initially required.
2. Client-secret provisioning/revocation interfaces and any consent requirements.
3. Detailed callback rules, authorization-code expiry/contracts, and PKCE/state handling.
4. Crow session storage/cookie details, signing-key management, and cross-service
   failure handling for refresh and logout.
5. Subscription purchasing, eligible Stripe states, remaining lifecycle rules,
   and reconciliation. The cancellation and failed-payment policies above are agreed.

Update this document when product decisions change and when planned features
become implemented. Keep setup instructions in the README and technical details
in their dedicated documents.
