-- Project reviews shared across main site and exchanges
-- Users can review projects they've participated in
-- User display info (handle, image) fetched from user service at render time

CREATE TABLE IF NOT EXISTS project_reviews (
    project_id TEXT NOT NULL,  -- ULID
    user_id TEXT NOT NULL,     -- ULID
    text TEXT NOT NULL,
    reviewed_at TEXT NOT NULL, -- TimeString
    PRIMARY KEY (project_id, user_id)
) STRICT;

CREATE INDEX IF NOT EXISTS project_reviews_by_project
    ON project_reviews(project_id);

CREATE INDEX IF NOT EXISTS project_reviews_by_user
    ON project_reviews(user_id);
