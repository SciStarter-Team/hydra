CREATE TABLE IF NOT EXISTS zooniverse_projects (
    zooniverse_id INTEGER PRIMARY KEY NOT NULL,
    entity_id TEXT NOT NULL,
    last_classification_id INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS zooniverse_users (
    zooniverse_id INTEGER PRIMARY KEY NOT NULL,
    zooniverse_username TEXT NOT NULL,
    user_id TEXT
);

CREATE INDEX IF NOT EXISTS idx_zooniverse_users_username ON zooniverse_users(zooniverse_username);
CREATE INDEX IF NOT EXISTS idx_zooniverse_users_user_id ON zooniverse_users(user_id);
