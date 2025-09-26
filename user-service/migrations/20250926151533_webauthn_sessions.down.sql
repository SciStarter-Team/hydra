-- Drop WebAuthn sessions table and indexes
DROP INDEX IF EXISTS webauthn_sessions_by_expires;
DROP TABLE IF EXISTS webauthn_sessions;
