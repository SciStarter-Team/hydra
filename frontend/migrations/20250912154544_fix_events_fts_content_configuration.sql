-- Fix events FTS5 table configuration by removing problematic content mapping
-- This removes content='events' and content_rowid='rowid' which can cause search mapping issues

-- Drop existing FTS table and triggers
DROP TRIGGER IF EXISTS events_fts_insert;
DROP TRIGGER IF EXISTS events_fts_update;
DROP TRIGGER IF EXISTS events_fts_delete;
DROP TABLE IF EXISTS events_fts;

-- Create new FTS5 virtual table without external content configuration
-- This stores both search index and content directly (no mapping issues)
CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
    event_id UNINDEXED,
    name,
    description,
    presenter,
    location_name,
    location_description,
    address,
    city,
    event_type,
    tags_text,
    tokenize='porter'
);

-- Populate the FTS table with existing data
INSERT INTO events_fts(event_id, name, description, presenter, location_name, location_description, address, city, event_type, tags_text)
SELECT event_id, name, description, presenter, location_name, location_description, address, city, event_type, tags_text
FROM events;

-- Create triggers to keep FTS table in sync (same as before)
CREATE TRIGGER IF NOT EXISTS events_fts_insert AFTER INSERT ON events
BEGIN
    INSERT INTO events_fts(event_id, name, description, presenter, location_name, location_description, address, city, event_type, tags_text)
    VALUES (NEW.event_id, NEW.name, NEW.description, NEW.presenter, NEW.location_name, NEW.location_description, NEW.address, NEW.city, NEW.event_type, NEW.tags_text);
END;

CREATE TRIGGER IF NOT EXISTS events_fts_update AFTER UPDATE ON events
BEGIN
    UPDATE events_fts 
    SET name = NEW.name, 
        description = NEW.description, 
        presenter = NEW.presenter,
        location_name = NEW.location_name,
        location_description = NEW.location_description,
        address = NEW.address,
        city = NEW.city,
        event_type = NEW.event_type,
        tags_text = NEW.tags_text
    WHERE event_id = NEW.event_id;
END;

CREATE TRIGGER IF NOT EXISTS events_fts_delete AFTER DELETE ON events
BEGIN
    DELETE FROM events_fts WHERE event_id = OLD.event_id;
END;
