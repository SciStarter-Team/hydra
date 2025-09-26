-- Create table for storing WebAuthn credentials
CREATE TABLE webauthn_credentials (
    credential_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    display_name TEXT,
    -- Store the WebAuthn Passkey data as JSON blob
    credential_data TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_used TEXT,
    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

-- Index for looking up credentials by user
CREATE INDEX webauthn_credentials_by_user ON webauthn_credentials (user_id);

-- Index for lookup by creation date
CREATE INDEX webauthn_credentials_by_created ON webauthn_credentials (created_at);
