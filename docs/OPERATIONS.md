# Operations

## Deployment target

Planned initial deployment: one Crow service, a private durable PostgreSQL database,
HTTPS termination and a transactional email provider. Start with one service
instance. Do not add distributed components without a measured need.

Only `DATABASE_URL` and the loopback listener are implemented today. Issuer,
signing keys, email, security limits, readiness and shutdown settings below are
requirements for future implementation, not existing environment variables.

## Configuration and release

Provision one canonical HTTPS issuer, trusted proxy rules, database credentials,
signing keys, email sender credentials and a separate email-job encryption key.
Validate configuration at startup and fail closed. Keep secrets outside source
control and container images. Set connection/time budgets for the actual host;
the current five-connection pool is not a production capacity claim.

Before a public release:

1. Run the [release gates](IMPLEMENTATION.md#public-release-gates) and record evidence.
2. Back up the database and necessary key material securely before migrations.
3. Run reviewed migrations once; document rollback or forward-repair strategy.
4. Deploy, check readiness, then run registration, delivery, recovery and OIDC smoke
   checks using dedicated test accounts and an independently configured client.
5. Monitor failures and retain a rollback path that preserves schema compatibility.

Implement graceful shutdown and a separate readiness check for required database
and signing configuration. Keep `/health` a liveness check. Email-provider failure
must be visible as delayed jobs without causing probe-induced restart loops.
Do not expose detailed readiness diagnostics publicly.

## Routine maintenance

Monitor login/exchange error rates, latency, database pool pressure, bounded
password work, email backlog/age, throttling and signing-key expiry. Avoid identity
or credential data in metric labels. Define alert destinations and a practical
response plan for a single maintainer.

Before launch, record numeric rate limits, app/callback quotas, body/time limits,
password-hash cost, email retry/backoff bounds, cleanup schedules, audit retention
and pending-account retention. Limits must support recovery without allowing
email bombing or permanent account lockout. Quota enforcement must be atomic.

Review dependencies regularly and prioritize security updates. Rotate provider,
database and email credentials using tested procedures. Follow signing overlap
rules in [Architecture](ARCHITECTURE.md#tokens-and-signing). Check clock sync.

Delete expired flow/session/email records and expired grants on a bounded schedule.
Preserve consumed-code evidence through grant expiry and preserve subject mappings.
Expiry checks always happen at use time even if cleanup is delayed.

## Backups and restore

Encrypt backups, restrict access and define recovery point/time targets before
launch. Back up accounts, app configuration, subject mappings and required key
material. Protect the email-job encryption key separately. Record retention and
test restoration into an isolated environment using non-delivering email settings.

A restored snapshot may resurrect sessions, reset links, consumed codes and revoked
grants. Before serving restored state, invalidate browser sessions, flows, codes,
grants, verification/reset credentials and pending email jobs. Preserve account/app
identity and subject mappings. Reconcile restored password/account state against
post-backup security events; require recovery for affected accounts or all accounts
when the affected set cannot be established. Invalidate restored client secrets
that may have been revoked after the snapshot and require safe replacement.
Do not assume a successful database restore is safe public recovery.

Record a restore drill and verify that credentials from before the restore are
rejected, identities stay stable and newly issued credentials work.

## Incidents

For an abusive or compromised app, disable it durably and investigate credential-
free audit events. Re-enabling must not restore its grants. For account compromise,
use a controlled recovery procedure that advances its security version and
invalidates Crow credentials. There is no cross-app session termination guarantee.

For signing-key compromise, stop issuing with that key, provision a replacement,
remove compromised public keys and communicate the affected interval to integrating
developers. External JWKS caches can continue trusting a compromised key temporarily;
removal cannot guarantee immediate rejection. Document any client-side emergency
rejection guidance. Ordinary overlap rules do not require retaining a compromised
key. Test emergency replacement before launch.

For email credential/encryption-key compromise, stop delivery, rotate affected
secrets, invalidate outstanding email credentials/jobs and resume with newly
issued links. Never use manual unverified email attachment as recovery.

For database or limiter failure, reject affected credential operations. Restore
availability without bypassing callback, session or transaction checks. Publish
safe incident information without exposing user records or credentials.

## Operator support

Use documented restricted procedures for account deletion requests, abuse reports
and privacy obligations; a self-service deletion UI is deferred. Before launch,
define requester verification, retention, deletion effects and support contact.
Do not create a general public admin API to perform occasional maintenance.
