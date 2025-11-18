-- Add scope column to refresh tokens to preserve granted scopes across token refresh
ALTER TABLE oauth2_refresh_tokens ADD COLUMN scope TEXT NOT NULL DEFAULT 'openid';

-- Add composite indexes for common queries
CREATE INDEX oauth2_access_tokens_by_client_user ON oauth2_access_tokens(client_id, user_id);
CREATE INDEX oauth2_refresh_tokens_by_client_user ON oauth2_refresh_tokens(client_id, user_id);
