-- Stores purpose-bound verification verifiers; raw links belong only in encrypted delivery payloads.
CREATE TABLE verification_actions (
    id uuid PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    purpose text NOT NULL CHECK (purpose = 'verify_email'),
    email_snapshot text NOT NULL,
    security_version bigint NOT NULL CHECK (security_version >= 0),
    token_verifier bytea NOT NULL UNIQUE CHECK (octet_length(token_verifier) = 32),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    CHECK (expires_at = created_at + interval '30 minutes'),
    CHECK (consumed_at IS NULL OR (consumed_at >= created_at AND consumed_at < expires_at))
);
CREATE INDEX verification_actions_account ON verification_actions(account_id);
