-- Remove the created column from archive_entries table

-- Drop the existing triggers
DROP TRIGGER IF EXISTS archive_entries_fts_insert;
DROP TRIGGER IF EXISTS archive_entries_fts_delete;
DROP TRIGGER IF EXISTS archive_entries_fts_update;

-- Drop the FTS table
DROP TABLE IF EXISTS archive_entries_fts;

-- Create new table without the created column
CREATE TABLE archive_entries_new (
    archive_id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    notes TEXT,
    file_id TEXT,
    snapshot_html TEXT,
    snapshot_url TEXT
);

-- Copy data from old table to new table (excluding created column)
INSERT INTO archive_entries_new (archive_id, name, timestamp, notes, file_id, snapshot_html, snapshot_url)
SELECT archive_id, name, timestamp, notes, file_id, snapshot_html, snapshot_url
FROM archive_entries;

-- Drop old table
DROP TABLE archive_entries;

-- Rename new table to original name
ALTER TABLE archive_entries_new RENAME TO archive_entries;

-- Recreate FTS table for searching archive entries
CREATE VIRTUAL TABLE archive_entries_fts USING fts5(
    archive_id UNINDEXED,
    name,
    notes,
    content=archive_entries,
    content_rowid=rowid
);

-- Populate the FTS table with existing data
INSERT INTO archive_entries_fts(rowid, archive_id, name, notes)
SELECT rowid, archive_id, name, COALESCE(notes, '')
FROM archive_entries;

-- Recreate triggers to keep FTS in sync
CREATE TRIGGER archive_entries_fts_insert AFTER INSERT ON archive_entries BEGIN
    INSERT INTO archive_entries_fts(rowid, archive_id, name, notes)
    VALUES (new.rowid, new.archive_id, new.name, COALESCE(new.notes, ''));
END;

CREATE TRIGGER archive_entries_fts_delete AFTER DELETE ON archive_entries BEGIN
    DELETE FROM archive_entries_fts WHERE rowid = old.rowid;
END;

CREATE TRIGGER archive_entries_fts_update AFTER UPDATE ON archive_entries BEGIN
    DELETE FROM archive_entries_fts WHERE rowid = old.rowid;
    INSERT INTO archive_entries_fts(rowid, archive_id, name, notes)
    VALUES (new.rowid, new.archive_id, new.name, COALESCE(new.notes, ''));
END;
