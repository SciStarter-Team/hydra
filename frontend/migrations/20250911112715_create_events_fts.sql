-- Create FTS5 virtual table for full-text search on events

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
    content='events',
    content_rowid='rowid'
);

-- Populate the FTS table with existing data including tags
INSERT INTO events_fts(event_id, name, description, presenter, location_name, location_description, address, city, event_type, tags_text)
SELECT event_id, name, description, presenter, location_name, location_description, address, city, event_type, tags_text
FROM events;

-- Create triggers to keep FTS table in sync with events table
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