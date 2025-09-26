-- Drop WebAuthn credentials table and indexes
DROP INDEX IF EXISTS webauthn_credentials_by_created;
DROP INDEX IF EXISTS webauthn_credentials_by_user;
DROP TABLE IF EXISTS webauthn_credentials;
