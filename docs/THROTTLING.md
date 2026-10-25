# Shared registration and resend throttling

`RegistrationThrottle` is the second increment of issue #21. It supplies admission
checks now used by the hosted registration/resend handlers. Both endpoints use
the same operation and namespace.
Recovery will reuse this account-entry budget when that endpoint is implemented.
Login/exchange policies are separate future work.

## Budgets and counting

| Subject | Shared rolling-hour budget |
| --- | --- |
| Canonical email | 5 admitted email attempts |
| Transport peer IP | 20 attempts admitted by the IP check |

A rolling hour means only timestamps older than or equal to one hour are removed;
a clock-hour boundary does not reset active attempts. Database time is read after
row-lock waits. Each admitted IP attempt counts even if its email budget denies it
or subsequent validation/account/mail work fails. Email-denied attempts do not
extend the email window. Full IP budgets deny before allocating any email state;
fully denied IP requests do not extend their window. Invalid email input can pass
None to count only its IP before returning validation errors.

No account lookup is involved. Known/unknown/active/deleted email addresses have
identical limiter behavior. Email canonicalization uses the existing Email type;
provider dots/aliases are not collapsed. Peer ports are ignored; IPv4-mapped IPv6
addresses share their IPv4 budget. Use the actual transport SocketAddr. Never
substitute X-Forwarded-For, Forwarded, submitted fields or a browser cookie. Behind
a proxy, requests share that proxy's transport-IP budget until explicit trusted-peer
handling is implemented/configured; deployments must account for this before exposure.

## Integration order

Construct `RegistrationThrottle::new(pool.clone())` on each instance. It needs no
additional environment key, dependency or storage service. Each instance shares
the same PostgreSQL table, so restarts do not reset active budgets.

The hosted handlers follow this order:

1. Check the signed-cookie CSRF token and Origin using the expected form action.
2. Parse/canonicalize email; preserve an invalid result long enough to charge IP.
3. Call `check(transport_peer, optional_email)` before hashing passwords or beginning
   an account/action/outbox transaction. Proceed only after it returns Ok.
4. Perform validation and account work. Its later rollback must not undo admission.

The limiter owns its transaction. It locks IP first, then email, and commits any
IP debit even when email is denied. A DB error rolls both changes back and fails
closed. Never call it while holding an account/action transaction or retry account
work after an uncertain limiter commit. An uncertain commit may have consumed a
budget slot, but is always reported as unavailable.

ThrottleError implements a generic HTML response: 429 with a positive, ceiling-
rounded Retry-After in seconds, or 503 without an allow fallback on storage/timeout
failure. It reveals no subject or which budget failed. Retry-After is the observed
blocking budget's next release, not a reservation or guarantee that a later request
will succeed; other requests or the second budget can still block it.
The browser response helpers retain no-store/no-referrer/framing/CSP protections.

The whole admission operation has a five-second timeout, with transaction-local
two-second lock and three-second statement limits. Timeouts/errors never grant
admission. Validate budgets/timeouts against target deployment measurements before
public exposure; the broader release gates remain open.

## One bounded counter table

Migration 0004 creates registration_limits:

| Column | Purpose |
| --- | --- |
| kind | 0 for IP, 1 for email; enforces the respective array bound |
| subject | Domain-separated SHA-256 digest of canonical email or binary IP |
| attempts | Accepted timestamps, capped at 20 per IP or 5 per email |
| expires_at | Last admitted timestamp plus one hour, for idle-row cleanup |

The primary key is (kind, subject). Raw emails/IPs are not stored in this table or
limiter errors/logs. Digests are pseudonymous, not anonymization against offline
guessing; restrict database access and retain no unnecessary expired state.
Limits have no account foreign key, so deleting/re-registering an account cannot
reset an active email budget. Row size and timestamps per subject remain bounded.

The upsert locks or creates a bucket atomically even when cleanup competes with it.
Fresh timestamps are pruned and charged under that lock. No account/action locks
are acquired by the limiter. A failed email debit rolls back the preceding IP debit.

`account-cleanup` also deletes at most 100 idle expired buckets per poll, skipping
locked rows. Expiry/admission works without cleanup; cleanup controls storage size.
Run migrations before restarting cleanup and grant its identity the required
SELECT/DELETE/row-lock privileges on registration_limits. Handler identities need
SELECT/INSERT/UPDATE on it. No signing/mail key or new worker process is required.

## Reproducible checks

With disposable PostgreSQL 17 and TEST_DATABASE_URL configured as in the README:

```sh
cargo test --locked --test throttle_database -- --ignored
```

CI explicitly runs this suite. Tests cover canonical email/IP budgets, malformed
email IP-only counting, no denial-driven window extension, rolling expiry, two
independent instances, concurrent requests, rollback, post-lock-wait expiry,
lock-timeout 503, bounded/concurrent cleanup and the real cleanup binary. An Axum
harness verifies signed-cookie/CSRF ordering, ordinary/HTMX response behavior,
Retry-After, varied forged forwarding headers and redacted fail-closed responses.
The hosted_database suite also exercises these checks through the real hosted
router. No production capacity or load-test readiness is claimed.
