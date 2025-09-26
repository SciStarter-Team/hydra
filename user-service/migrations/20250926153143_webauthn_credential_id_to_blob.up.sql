-- Convert credential_id from TEXT to BLOB
-- Since this is new functionality with no existing data, we can safely recreate the table

DROP TABLE IF EXISTS webauthn_credentials;

CREATE TABLE webauthn_credentials (
    credential_id BLOB PRIMARY KEY,
    user_id TEXT NOT NULL,
    display_name TEXT,
    credential_data TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_used TEXT,
    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

-- Index for faster user credential lookups
CREATE INDEX webauthn_credentials_by_user_id ON webauthn_credentials (user_id);