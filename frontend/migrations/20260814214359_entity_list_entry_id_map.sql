-- Mapping table for orrery entity list entry ids to hydra ULIDs, mirroring
-- entity_list_id_map. Without it the sync mints a fresh ULID for every entry
-- on every pass, so re-syncing a list duplicates all of its entries.
CREATE TABLE IF NOT EXISTS entity_list_entry_id_map (
    orrery_id INTEGER PRIMARY KEY NOT NULL,
    entry_id TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_entity_list_entry_id_map_entry_id
    ON entity_list_entry_id_map(entry_id);
