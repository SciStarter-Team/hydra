-- Add tags_text column to events table for FTS search

ALTER TABLE events ADD COLUMN tags_text TEXT NOT NULL DEFAULT '';

-- Update all existing events with their current tags
UPDATE events 
SET tags_text = (
    SELECT GROUP_CONCAT(tag, ' ') 
    FROM event_tags 
    WHERE event_tags.event_id = events.event_id
) 
WHERE event_id IN (SELECT DISTINCT event_id FROM event_tags);

-- Triggers to keep tags_text in sync when event_tags change

-- When a tag is added to an event
CREATE TRIGGER IF NOT EXISTS event_tags_insert_update_fts 
AFTER INSERT ON event_tags
BEGIN
    UPDATE events 
    SET tags_text = (
        SELECT GROUP_CONCAT(tag, ' ') 
        FROM event_tags 
        WHERE event_tags.event_id = NEW.event_id
    ) 
    WHERE event_id = NEW.event_id;
END;

-- When a tag is removed from an event  
CREATE TRIGGER IF NOT EXISTS event_tags_delete_update_fts
AFTER DELETE ON event_tags
BEGIN
    UPDATE events 
    SET tags_text = COALESCE(
        (SELECT GROUP_CONCAT(tag, ' ') 
         FROM event_tags 
         WHERE event_tags.event_id = OLD.event_id), 
        ''
    ) 
    WHERE event_id = OLD.event_id;
END;

-- When a tag is updated (changed)
CREATE TRIGGER IF NOT EXISTS event_tags_update_update_fts
AFTER UPDATE ON event_tags
BEGIN
    UPDATE events 
    SET tags_text = (
        SELECT GROUP_CONCAT(tag, ' ') 
        FROM event_tags 
        WHERE event_tags.event_id = NEW.event_id
    ) 
    WHERE event_id = NEW.event_id;
END;