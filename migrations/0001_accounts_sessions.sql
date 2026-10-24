CREATE TABLE accounts (
    id uuid PRIMARY KEY,
    username text NOT NULL UNIQUE CHECK (username ~ '^[a-z0-9_]{3,32}$'),
    password_hash text NOT NULL,
    subscriber boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE browser_sessions (
    verifier bytea PRIMARY KEY CHECK (octet_length(verifier) = 32),
    user_id uuid REFERENCES accounts(id),
    authenticated_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    revoked boolean NOT NULL DEFAULT false,
    CHECK ((user_id IS NULL) = (authenticated_at IS NULL)),
    CHECK (expires_at > created_at)
);
CREATE INDEX browser_sessions_user_id ON browser_sessions(user_id);
