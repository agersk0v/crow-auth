# Verification mail: configuration and operation

The internal `mail::queue_initial` operation creates the verification action and
encrypted delivery job in the caller's pending-account transaction. Roll back on
any error and report success only after confirmed commit. There are no registration,
resend or browser-verification routes yet. Captured links target the specified
future `/verify-email?token=...` confirmation page, which is not implemented.

## Configuration

The HTTP service still applies migrations at startup and exposes only `/health`.
Run it once before the separate worker. The worker checks the outbox schema but
does not apply migrations. Cargo does not load `.env`; export worker settings in
the shell or supply them through your process manager.

| Variable | Requirement |
| --- | --- |
| `DATABASE_URL` | Worker PostgreSQL connection; no credentials in logs |
| `MAIL_FROM` | Valid sender mailbox, configured by the operator |
| `CROW_ORIGIN` | HTTPS origin, with no credentials, path, query or fragment |
| `MAIL_ENCRYPTION_KEY` | Separately provisioned 32 random bytes, unpadded base64url |
| `SMTP_HOST`, `SMTP_PORT` | SMTP submission host and port; port must be nonzero |
| `SMTP_USERNAME`, `SMTP_PASSWORD` | Both present or both absent; never log them |
| `MAIL_LOCAL_CAPTURE` | Defaults to `false`; only literal `true`/`false` accepted |

Production transport requires STARTTLS with certificate/hostname validation and
has a ten-second total send deadline, including connection/TLS work. It cannot
downgrade to plaintext. Local-capture mode permits plaintext only to literal
loopback hosts (`localhost`, `127.0.0.1`, `::1`) and forbids SMTP credentials. Only
that mode permits an HTTP Crow origin, also on loopback. Sender and links never
come from request Host or forwarded headers.

Provision the encryption key once through your secret manager and keep it stable
across restarts. For disposable local use, generate an environment value without
printing it:

```sh
export MAIL_ENCRYPTION_KEY="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
```

The application that queues mail and every worker must use the same key. A wrong
key or corrupted payload fails closed and retains the encrypted job for operator
recovery. This increment has one active key and no key-version/rotation mechanism;
drain the queue before replacing it. Do not discard a key needed by queued mail.
Restrict key/outbox access to the application writer and delivery worker. Use a
dedicated worker database identity without schema-creation privileges: it needs
SELECT and row-lock permissions on accounts/actions and SELECT/UPDATE/DELETE on
the outbox. It does not need password hashes or account/action creation rights.
Provision column grants/roles as part of the deployment; migrations do not create
operator identities. Keep SMTP credentials limited to the worker.

## Local capture and worker

`compose.test.yaml` starts disposable PostgreSQL 17 and Mailpit v1.27.0. The
SMTP port (1025) and inbox UI/API (8025) bind only to host loopback; there is no
outbound forwarding/relay configuration or persistent mail volume. Do not use
real user mail in this fixture. With the test password configured in `.env`:

```sh
docker compose -f compose.test.yaml -p crow-auth-test up -d --wait
# Set TEST_DATABASE_URL using the configured disposable test password.
export TEST_SMTP_PORT=1025
export TEST_MAILPIT_URL=http://127.0.0.1:8025
cargo test --locked --test mail_database -- --ignored
cargo test --locked --test lifecycle_database -- --ignored
```

Open `http://127.0.0.1:8025` to inspect captured fixture messages. Set the worker
variables above, use the test database as `DATABASE_URL`, and run:

```sh
cargo run --locked --bin mail-worker
# Or process at most one due job and exit:
cargo run --locked --bin mail-worker -- --once
```

The library/test suite is currently the only producer. The HTTP service does not
queue mail. `--once` exits nonzero on configuration/database/decryption failures;
SMTP failures that were durably scheduled for retry count as successful processing.
Continuous mode polls once per second and logs only generic outcomes, never
recipients, tokens, message bodies or SMTP error diagnostics. Ctrl-C finishes the
bounded current attempt before exit; forced termination can cause a duplicate.
Stop the test fixture with `docker compose -f compose.test.yaml -p crow-auth-test down`.

## Delivery and failure contract

Outbox recipient/body (including the secret link) are encrypted with
XChaCha20-Poly1305 and a fresh 192-bit nonce. The action UUID is authenticated
associated data, preventing ciphertext swapping between jobs. The key is never
stored in PostgreSQL. Workers lock account, action, then outbox, and recheck status,
email, security version, token consumption and both deadlines after lock waits.
Holding these locks through the bounded SMTP attempt serializes delivery with
activation, resend and cleanup. Busy accounts are skipped.

After a failed initial attempt, retry after 1 minute, then 5 minutes, then 15
minutes, each delay measured from its preceding failure. There are at most four
attempts per committed job history. Never attempt an expired/consumed/invalid
link. Delete the encrypted job after successful delivery, expiry/invalidation or
retry exhaustion. Invalid/expired jobs are eligible for erasure even when their
next retry is in the future; polling is asynchronous. Account deletion cascades
through action rows and cancels queued payloads. Resend and pending-account cleanup are implemented as described below.

SMTP and PostgreSQL cannot commit together. Acceptance followed by a crash or DB
failure can cause duplicate delivery of the same link; a stable Message-ID helps
identify it, but does not promise deduplication. Database failure rolls back queue
state, never invents a new token, and can make actual sends exceed the committed
attempt count. Deadline checks occur before sending; delivery latency can make a
message arrive after expiry. Token validation remains authoritative.

## Resend and expired pending accounts

`mail::queue_resend(tx, account_id, config)` is the internal resend operation.
It locks the pending account, rechecks the original 24-hour deadline after any
lock wait, locks/deletes all earlier verification actions (cascading away their
unsent encrypted payloads), and creates a fresh action and encrypted job. All of
this shares the caller's transaction. Roll back on every error, including a new
outbox write failure, to preserve the original links and queued payloads. Only a
confirmed commit makes replacement successful. Concurrent resends serialize;
only the last committed link remains valid. Consumed/active, missing or expired
accounts cannot receive a resend.

Resend preserves the password, security version, canonical identifiers and original
pending-account deadline. It does not sign in or extend the 24-hour reservation.
The new token lasts at most 30 minutes, and always stops working at the account's
original deadline if that comes first. The email explains this earlier cutoff.
Already delivered mail cannot be recalled, but its replaced link stops working.
A delivery already holding the account lock can finish before resend proceeds;
a replacement committed first prevents the old job from being sent afterward.

There is still no resend HTTP route. Future hosted handlers must look up canonical
email internally, return generic confirmation for absent/active/expired accounts,
and enforce shared throttling, pre-auth CSRF and Origin checks. They must never
replace a password during duplicate registration or expose internal errors as
account-existence signals.

Run cleanup independently of SMTP and its encryption key, after migrations exist:

```sh
cargo run --locked --bin account-cleanup
# Remove at most one expired pending account and exit:
cargo run --locked --bin account-cleanup -- --once
```

It needs only `DATABASE_URL`, checks existing schema without DDL, processes at most
one account per poll and sleeps one second between polls. It also removes expired
pre-auth browsers and verification contexts in bounded batches; see
[browser foundations](BROWSER.md) for the new table permissions and limits. Multiple cleaners skip
locked accounts. In an account-first transaction it checks `now >= pending_expires_at`,
locks dependent actions, rechecks eligibility, then deletes the account; foreign
keys erase actions and encrypted jobs. Only committed removal releases the unique
username/email reservations. Active accounts are never eligible, even after their
former pending deadline. Re-registration gets a new UUID and old tokens cannot
become valid again. Cleanup errors roll back every deletion and log no identifiers.

Provision a separate cleanup database identity with the required SELECT/row-lock
and DELETE privileges on accounts/actions/outbox; it does not need mail secrets or
password hashes. Ctrl-C finishes the current transaction; `--once` exits nonzero
on storage/configuration failures. Polling can delay reservation release; expiry
checks still reject verification, resend and mail independently of cleanup. No
registration behavior is implemented here.
