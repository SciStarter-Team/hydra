-- Create archive_entries table
CREATE TABLE archive_entries (
    archive_id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    notes TEXT,
    file_id TEXT,
    snapshot_html TEXT,
    snapshot_url TEXT,
    created TEXT NOT NULL
);

-- Create FTS table for searching archive entries
CREATE VIRTUAL TABLE archive_entries_fts USING fts5(
    archive_id UNINDEXED,
    name,
    notes,
    content=archive_entries,
    content_rowid=rowid
);

-- Create triggers to keep FTS in sync
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
