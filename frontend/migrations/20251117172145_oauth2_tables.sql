-- OAuth2 / OpenID Connect Provider Tables

-- OAuth2 Clients (applications that can authenticate users)
CREATE TABLE oauth2_clients (
    client_id TEXT PRIMARY KEY,
    client_secret TEXT NOT NULL,
    client_name TEXT NOT NULL,
    redirect_uris TEXT NOT NULL,  -- Newline-separated list of allowed redirect URIs
    allowed_scopes TEXT NOT NULL, -- Space-separated list of allowed scopes
    created TEXT NOT NULL,         -- TimeString
    owner_user_id TEXT             -- ULID of creator (optional)
) STRICT;

CREATE INDEX oauth2_clients_by_owner ON oauth2_clients(owner_user_id);

-- Authorization Codes (short-lived, one-time use)
CREATE TABLE oauth2_authorization_codes (
    code TEXT PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES oauth2_clients(client_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,         -- ULID
    redirect_uri TEXT NOT NULL,
    scope TEXT NOT NULL,           -- Space-separated scopes
    expires_at TEXT NOT NULL,      -- TimeString
    code_challenge TEXT,           -- PKCE code challenge
    code_challenge_method TEXT,    -- "S256" or "plain"
    created TEXT NOT NULL          -- TimeString
) STRICT;

CREATE INDEX oauth2_codes_by_client ON oauth2_authorization_codes(client_id);
CREATE INDEX oauth2_codes_by_user ON oauth2_authorization_codes(user_id);
CREATE INDEX oauth2_codes_expires_at ON oauth2_authorization_codes(expires_at);

-- Access Tokens (used to access protected resources)
CREATE TABLE oauth2_access_tokens (
    token TEXT PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES oauth2_clients(client_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,         -- ULID
    scope TEXT NOT NULL,           -- Space-separated scopes
    expires_at TEXT NOT NULL,      -- TimeString
    created TEXT NOT NULL          -- TimeString
) STRICT;

CREATE INDEX oauth2_tokens_by_client ON oauth2_access_tokens(client_id);
CREATE INDEX oauth2_tokens_by_user ON oauth2_access_tokens(user_id);
CREATE INDEX oauth2_tokens_expires_at ON oauth2_access_tokens(expires_at);

-- Refresh Tokens (used to obtain new access tokens)
CREATE TABLE oauth2_refresh_tokens (
    token TEXT PRIMARY KEY,
    access_token TEXT NOT NULL,
    client_id TEXT NOT NULL REFERENCES oauth2_clients(client_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,         -- ULID
    expires_at TEXT NOT NULL,      -- TimeString
    created TEXT NOT NULL          -- TimeString
) STRICT;

CREATE INDEX oauth2_refresh_by_access ON oauth2_refresh_tokens(access_token);
CREATE INDEX oauth2_refresh_by_client ON oauth2_refresh_tokens(client_id);
CREATE INDEX oauth2_refresh_by_user ON oauth2_refresh_tokens(user_id);
CREATE INDEX oauth2_refresh_expires_at ON oauth2_refresh_tokens(expires_at);
