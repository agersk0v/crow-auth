-- Account lifecycle transitions and reservation cleanup are later work.
CREATE TABLE accounts (
    id uuid PRIMARY KEY,
    username text NOT NULL UNIQUE
        CHECK (octet_length(username) = length(username) AND username ~ '^[a-z0-9_]{3,32}$'),
    email text NOT NULL UNIQUE
        CHECK (octet_length(email) = length(email) AND email = lower(email) AND octet_length(email) <= 254)
        CHECK (email ~ '^[a-z0-9!#$%&''*+/=?^_`{|}~-]+([.][a-z0-9!#$%&''*+/=?^_`{|}~-]+)*@[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?([.][a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)+$'),
    password_hash text NOT NULL CHECK (password_hash LIKE '$argon2id$v=19$%'),
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'active')),
    security_version bigint NOT NULL DEFAULT 0 CHECK (security_version >= 0),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    pending_expires_at timestamptz NOT NULL,
    verified_at timestamptz,
    CHECK (pending_expires_at = created_at + interval '24 hours'),
    CHECK ((status = 'pending' AND verified_at IS NULL)
        OR (status = 'active' AND verified_at IS NOT NULL AND verified_at >= created_at))
);
CREATE INDEX accounts_pending_expiry ON accounts(pending_expires_at) WHERE status='pending';

CREATE FUNCTION reject_account_identity_update() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.username IS DISTINCT FROM OLD.username
       OR NEW.email IS DISTINCT FROM OLD.email THEN
        RAISE EXCEPTION 'Account identity fields are immutable' USING ERRCODE = '23514';
    END IF;
    IF NEW.security_version < OLD.security_version THEN
        RAISE EXCEPTION 'Account security version cannot decrease' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER accounts_identity_guard BEFORE UPDATE ON accounts
FOR EACH ROW EXECUTE FUNCTION reject_account_identity_update();
