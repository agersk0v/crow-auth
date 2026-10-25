-- Holds authenticated-encrypted recipient/link payloads until delivery or expiry.
CREATE TABLE verification_mail_outbox (
    action_id uuid PRIMARY KEY REFERENCES verification_actions(id) ON DELETE CASCADE,
    nonce bytea NOT NULL CHECK (octet_length(nonce) = 24),
    ciphertext bytea NOT NULL CHECK (octet_length(ciphertext) >= 16),
    attempts smallint NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 4),
    next_attempt_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX verification_mail_due ON verification_mail_outbox(next_attempt_at);
