-- Create bookmarks table
CREATE TABLE bookmarks (
    user_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    created TEXT NOT NULL DEFAULT (datetime('now')),
    activity TEXT NOT NULL DEFAULT (datetime('now')),
    contacted TEXT,
    active BOOLEAN NOT NULL DEFAULT 1,
    PRIMARY KEY (user_id, project_id),
    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE INDEX bookmarks_by_user ON bookmarks(user_id);
CREATE INDEX bookmarks_by_project ON bookmarks(project_id);
CREATE INDEX bookmarks_by_activity ON bookmarks(activity);
