-- Tokens authorizing access to the content-management MCP endpoint.
-- Tokens are non-expiring and revoked by deletion.
CREATE TABLE mcp_tokens (
    token TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    created TEXT NOT NULL
);

-- Index for listing tokens newest-first in the admin interface.
CREATE INDEX mcp_tokens_by_created ON mcp_tokens (created);
