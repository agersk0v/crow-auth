# Product

Crow Auth aims to let users reuse one Crow account across independent apps.
Developers integrate **Sign in with Crow** through OpenID Connect.

## Basic scope

- Hosted account registration and sign-in.
- A shared Crow browser session.
- Developer registration and management of apps.
- Standard sign-in integration for apps with a backend.

Each app manages its own data, permissions and local sessions.
Keep the product small enough for one maintainer to operate.

These are product goals. Account storage and password-security foundations exist,
but registration, verification, authentication and app management are not available.

The [first milestone specification](docs/PRODUCT.md) defines the accepted planned
account lifecycle, shared sign-in and app management behavior. The
[implementation checklist and acceptance criteria](docs/IMPLEMENTATION.md) track
the two-app milestone and later features separately.
