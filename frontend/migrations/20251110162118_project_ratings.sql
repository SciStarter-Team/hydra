-- Project ratings/votes shared across main site and exchanges
-- Users can vote once per project (regardless of where they found it)

CREATE TABLE IF NOT EXISTS project_ratings (
    project_id TEXT NOT NULL,  -- ULID
    user_id TEXT NOT NULL,     -- ULID
    rated_at TEXT NOT NULL,    -- TimeString
    PRIMARY KEY (project_id, user_id)
) STRICT;

CREATE INDEX IF NOT EXISTS project_ratings_by_project
    ON project_ratings(project_id);

CREATE INDEX IF NOT EXISTS project_ratings_by_user
    ON project_ratings(user_id);
