# Crow Auth product definition

This document owns Crow Auth's product scope and intended behavior. It separates
the implemented baseline, agreed requirements, and unresolved decisions. An
agreed requirement is planned work until its implementation is merged and
verified; this document does not imply that the described features already exist.

[README](../README.md) owns implemented setup, configuration, and verification
commands. Detailed technical contracts and security rules will belong in
`docs/ARCHITECTURE.md` and `docs/SECURITY.md` when those documents are created.

## Purpose

Crow Auth is a shared authentication service for multiple applications, including
applications built by external developers. Clerk is a reference for the product
direction: applications can use a common identity service and hosted account
pages instead of building their own authentication experience. Feature parity
with Clerk is not an initial requirement.

Crow Auth is API-first. Hosted pages provide a convenient interface to the service;
they must not be the only way to perform the supported authentication flows.

## Implemented baseline

The repository currently contains a Rust HTTP service using Axum and Tokio, a
SQLx PostgreSQL connection pool, and a startup `SELECT 1` connectivity check.
`GET /health` returns `200` with the body `ok` and is an HTTP liveness check.

Users, application registration, authentication, JWT issuance, hosted pages,
subscriptions, the Go refresh-token service, and Redis integration are not
implemented yet. PostgreSQL connectivity alone does not provide user storage.

## Shared identity

A person has one Crow identity across connected applications rather than a
separate Crow account for every application. Initial sign-in uses a username and
password. Users must be able to create an account, sign in, sign out, and access
their account/profile through the supported interfaces.

A shared identity does not by itself grant an application access to all user data
or to another application's configuration. Application permissions and the user
information exposed to applications still need definition.

## Developers and applications

External developers with access to the Crow Auth API or frontend can register
and configure connected applications. Integration is not limited to the Crow
project owner or first-party applications. No separate manual developer approval
requirement has been agreed.

Applications can choose either a hosted authentication experience or their own
interface backed entirely by Crow Auth's API. Developer access, application
ownership, credentials, and permitted account operations must be defined before
application management is implemented. Public access to a page must not be
treated as permission to manage another developer's applications.

## Authentication interfaces

### Hosted authentication

An application can send a user to Crow Auth's hosted login UI, supplying a return
URL in the request URL. After successful authentication, Crow Auth returns the
user to that application so it can establish authenticated access.

JWT access tokens are an agreed requirement. The redirect handoff remains
unresolved: the original request was to include the JWT in the return URL. The
recommended alternative is a short-lived, single-use authorization code that the
application exchanges for tokens, with PKCE, request state, and registered
callback URLs. This recommendation has not yet been approved. Do not implement
the redirect protocol until the handoff and callback validation rules are agreed.

### Direct API authentication

Applications must be able to perform authentication entirely through HTTP API
endpoints without using Crow Auth's hosted pages. This includes the initial
account and authentication flows, with JWT access tokens returned in response
bodies rather than requiring a browser redirect.

Exact endpoints, payloads, application authentication, token audiences, and error
contracts remain to be defined. API availability does not imply that untrusted
clients receive developer credentials or unrestricted account access.

### Hosted account pages

Crow Auth provides a hosted account/profile page. The initial frontend consists
of hosted login and account pages, with the account-creation entry point needed
for the agreed registration flow. Embedded UI components are outside the initial
scope.

The API is the primary product interface. Profile fields, permitted edits, and
any developer-facing frontend screens remain to be specified; a broad admin
dashboard is not an agreed initial requirement.

## Tokens and sessions

JWT access tokens represent authenticated access for connected applications.
A Go microservice, kept in this repository, handles refresh tokens using Redis
for refresh-token state. The main authentication API uses Rust and Axum.

The exact division of responsibilities between these services remains a design
task. Access-token lifetime, refresh rotation and expiry, session persistence,
logout scope, revocation, and behavior during Redis outages must be agreed
before the token flows are implemented.

Sharing an identity does not yet establish automatic single sign-on between
applications or immediate global logout. Those behaviors require explicit
session and token rules.

## Subscriptions and roles

Stripe provides subscriptions tied to Crow Auth rather than subscriptions owned
separately by each connected application. The initial subscription concept is a
subscriber role associated with the Crow identity.

All features remain unlocked initially. The subscriber role does not introduce
paid feature restrictions, API limits, or a plan-based entitlement system.

The following are unresolved:

- Who purchases a subscription and how the Stripe customer maps to a Crow account.
- Whether the subscriber role is assigned from Stripe automatically, manually,
  or through both methods.
- Pricing, plans, trials, and the purchase and billing-management interfaces.
- The effect of cancellation, failed payments, refunds, and subscription expiry.
- How applications can read the role and how quickly role changes become visible.

Stripe Checkout and Customer Portal are possible implementation choices, not
approved requirements. Do not infer billing lifecycle or role transitions from
this document.

## Agreed technology constraints

| Component | Agreed choice |
| --- | --- |
| Main HTTP API | Rust and Axum |
| Persistent user storage | PostgreSQL, accessed by the Rust service through SQLx |
| Access tokens | JWT |
| Refresh-token handling | Go microservice in this repository |
| Refresh-token state | Redis |
| Subscriptions | Stripe |

These choices constrain the implementation. Schemas, service boundaries,
communication protocols, signing keys, password hashing, and deployment details
belong in the architecture and security documents.

## Outside the initial scope

- Password recovery and reset; recovery email requirements are not defined.
- Social login, passwordless login, and multi-factor authentication.
- Embedded authentication UI components.
- Organizations, teams, and application-specific authorization roles.
- Paid feature gating and a full plan/entitlement system.
- Full Clerk compatibility or feature parity.

These exclusions describe the initial scope, not permanent product restrictions.
They require separate agreement before implementation.

## Decisions needed before implementation

Resolve decisions for the affected feature before implementing it; independent,
agreed work can continue without inventing answers for unrelated features.

1. Define username rules, uniqueness, password requirements, and initial profile
   fields, including whether any email address is collected.
2. Define developer access, application ownership, application credentials, and
   the user information each application may receive.
3. Approve the hosted redirect handoff and callback validation rules.
4. Define JWT validation and audiences, session behavior, refresh-token handling,
   and sign-out/revocation across applications.
5. Define subscription ownership, subscriber-role assignment, billing interfaces,
   and lifecycle behavior before implementing billing.

Keep approved plans separate from implemented behavior as the service grows.
Update this document when product decisions change; keep setup commands in the
README and detailed technical contracts in their dedicated documents.
