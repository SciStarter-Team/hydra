CREATE TABLE IF NOT EXISTS entity_lists (
    list_id TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL DEFAULT '',
    private INTEGER NOT NULL DEFAULT 0,
    owner_id TEXT NOT NULL,
    delegate_id TEXT,
    name TEXT NOT NULL DEFAULT '',
    note TEXT NOT NULL DEFAULT '',
    sidebar TEXT NOT NULL DEFAULT '',
    slug TEXT UNIQUE,
    hide_identity_form INTEGER NOT NULL DEFAULT 0,
    image TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_entity_lists_owner_id ON entity_lists(owner_id);
CREATE INDEX IF NOT EXISTS idx_entity_lists_slug ON entity_lists(slug);

CREATE TABLE IF NOT EXISTS entity_list_entries (
    entry_id TEXT PRIMARY KEY NOT NULL,
    list_id TEXT NOT NULL REFERENCES entity_lists(list_id) ON DELETE CASCADE,
    priority INTEGER NOT NULL DEFAULT 0,
    added_at TEXT NOT NULL DEFAULT '',
    entity_slug TEXT NOT NULL DEFAULT '',
    note TEXT NOT NULL DEFAULT '',
    link TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_entity_list_entries_list_id ON entity_list_entries(list_id);

-- Mapping table for orrery integer IDs to hydra ULIDs (used during sync)
CREATE TABLE IF NOT EXISTS entity_list_id_map (
    orrery_id INTEGER PRIMARY KEY NOT NULL,
    list_id TEXT NOT NULL
);
