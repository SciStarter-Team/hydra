-- Revert credential_id back to TEXT
DROP TABLE IF EXISTS webauthn_credentials;

CREATE TABLE webauthn_credentials (
    credential_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    display_name TEXT,
    credential_data TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_used TEXT,
    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);