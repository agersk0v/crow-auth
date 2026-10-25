-- Stores unauthenticated browser/CSRF state and short-lived verification form contexts.
CREATE TABLE preauth_browsers (
    id uuid PRIMARY KEY,
    cookie_verifier bytea NOT NULL UNIQUE CHECK (octet_length(cookie_verifier) = 32),
    csrf_secret bytea NOT NULL CHECK (octet_length(csrf_secret) = 32),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL CHECK (expires_at = created_at + interval '30 minutes')
);
CREATE INDEX preauth_expiry ON preauth_browsers(expires_at);

CREATE TABLE verification_contexts (
    id uuid PRIMARY KEY,
    context_verifier bytea NOT NULL UNIQUE CHECK (octet_length(context_verifier) = 32),
    browser_id uuid NOT NULL REFERENCES preauth_browsers(id) ON DELETE CASCADE,
    action_id uuid NOT NULL REFERENCES verification_actions(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL
        CHECK (expires_at > created_at AND expires_at <= created_at + interval '5 minutes')
);
CREATE INDEX verification_context_expiry ON verification_contexts(expires_at);
CREATE INDEX verification_context_browser ON verification_contexts(browser_id);
CREATE INDEX verification_context_action ON verification_contexts(action_id);
