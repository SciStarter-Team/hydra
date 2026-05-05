CREATE TABLE IF NOT EXISTS entity_list_viewers (
    list_id TEXT NOT NULL REFERENCES entity_lists(list_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    can_edit INTEGER NOT NULL DEFAULT 0,
    added_at TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (list_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_entity_list_viewers_user_id ON entity_list_viewers(user_id);
