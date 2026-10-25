-- Shares bounded rolling-hour registration/resend budgets across service instances.
CREATE TABLE registration_limits (
    kind smallint NOT NULL CHECK (kind IN (0, 1)),
    subject bytea NOT NULL CHECK (octet_length(subject) = 32),
    attempts timestamptz[] NOT NULL DEFAULT '{}',
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (kind, subject),
    CHECK (cardinality(attempts) <= CASE kind WHEN 0 THEN 20 ELSE 5 END),
    CHECK (cardinality(attempts) = 0 OR array_ndims(attempts) = 1),
    CHECK (array_position(attempts, NULL) IS NULL)
);
CREATE INDEX registration_limits_expiry ON registration_limits(expires_at);
