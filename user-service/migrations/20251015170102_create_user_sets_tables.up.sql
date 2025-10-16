-- User sets table
CREATE TABLE user_sets (
    slug TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- User set criteria table
CREATE TABLE user_set_criteria (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_set_slug TEXT NOT NULL,
    criterion_type TEXT NOT NULL,
    criterion_data BLOB NOT NULL,
    FOREIGN KEY (user_set_slug) REFERENCES user_sets(slug) ON DELETE CASCADE
);

-- Index for faster lookups
CREATE INDEX idx_user_set_criteria_set ON user_set_criteria(user_set_slug);
