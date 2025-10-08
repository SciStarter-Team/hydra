-- Add created timestamp to entities table
ALTER TABLE entities ADD COLUMN created TEXT NOT NULL DEFAULT '';

-- Populate created timestamp from target tables for existing entities

-- Update from projects
UPDATE entities
SET created = (
    SELECT p.created
    FROM projects p
    WHERE p.project_id = entities.target
)
WHERE kind = 'Project';

-- Update from events
UPDATE entities
SET created = (
    SELECT e.created
    FROM events e
    WHERE e.event_id = entities.target
)
WHERE kind = 'Event';

-- For any remaining entities without a created timestamp, use current time
UPDATE entities
SET created = datetime('now')
WHERE created = '';

-- Create index for sorting by created
CREATE INDEX idx_entities_created ON entities (created DESC);
