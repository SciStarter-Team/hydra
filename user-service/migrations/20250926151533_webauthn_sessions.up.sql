-- Create table for storing WebAuthn session state
CREATE TABLE webauthn_sessions (
    session_id TEXT PRIMARY KEY,
    session_type TEXT NOT NULL, -- 'registration' or 'authentication'
    user_id TEXT, -- NULL for usernameless authentication
    state_data BLOB NOT NULL, -- Raw bytes from webauthn-rs
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL DEFAULT (datetime('now', '+10 minutes'))
);

-- Index for cleanup of expired sessions
CREATE INDEX webauthn_sessions_by_expires ON webauthn_sessions (expires_at);
