-- Link tokens allow users to create named tokens for API access
-- Used by /api/participation/link-token endpoint
CREATE TABLE link_tokens (
    token TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    label TEXT NOT NULL,     -- User-selected label (e.g., "Zooniverse", "Mobile App")
    created TEXT NOT NULL,   -- TimeString
    expires TEXT             -- Optional expiration TimeString
) STRICT;

CREATE INDEX link_tokens_by_user ON link_tokens(user_id);
