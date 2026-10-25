# Local password blocklist

`password-blocklist.json` records the reviewed SecLists Xato top-100,000 revision,
source URL, SHA-256 and license. Provision it locally with:

```sh
python3 scripts/provision-password-blocklist.py
```

The downloaded list is ignored by Git. The provisioner verifies its checksum and
atomically replaces `data/common-passwords.txt`; failures preserve the old file.
`PasswordService::load` requires a nonempty UTF-8 list at initialization. No user
password is ever submitted to the source or another service. Initialize the future
registration/password-change service with this file; do not substitute the tiny
unit-test fixture. Loading is a synchronous startup operation, not request work.
The current HTTP skeleton has no password endpoints and needs no blocklist to
serve `/health`.

Review upstream monthly: choose a specific revision, review provenance/license,
compute the expected checksum independently, update the manifest's revision, URL,
SHA-256 and review date, provision the data, and rerun password-policy tests. Deploy
updates atomically, then reload/restart password services: an existing service
holds its loaded snapshot. This limited list is prototype breach coverage; review
coverage before public exposure. Match entries case-insensitively for rejection
only; hash and verify the user's unchanged bytes. Production Argon2 benchmarking
remains a release gate.
