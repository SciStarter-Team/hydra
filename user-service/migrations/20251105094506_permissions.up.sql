-- Generic permissions system for resources (exchanges, forums, etc.)
CREATE TABLE IF NOT EXISTS permissions (
    user_id TEXT NOT NULL,
    resource_type TEXT NOT NULL,  -- 'exchange', 'forum', etc.
    resource_id TEXT NOT NULL,    -- ULID of the resource
    permission TEXT NOT NULL,      -- 'admin', 'moderator', etc.
    granted_by TEXT NOT NULL,      -- user_id ULID
    granted_at TEXT NOT NULL,      -- TimeString
    PRIMARY KEY (user_id, resource_type, resource_id)
) STRICT;

CREATE INDEX IF NOT EXISTS permissions_by_user ON permissions(user_id);
CREATE INDEX IF NOT EXISTS permissions_by_resource ON permissions(resource_type, resource_id);
