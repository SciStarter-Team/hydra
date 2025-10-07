-- Populate entities.publish from projects.publish and events.publish
-- This migration copies the publish status from project/event records to their corresponding entities

-- Temporarily disable the FTS trigger that has a bug (will be fixed in next migration)
DROP TRIGGER IF EXISTS entities_fts_entity_update;

-- Update entities with kind='Project' to match their project's publish status
UPDATE entities
SET publish = (
    SELECT COALESCE(p.publish, 1)
    FROM projects p
    WHERE p.project_id = entities.target
)
WHERE entities.kind = 'Project'
  AND EXISTS (
    SELECT 1 FROM projects p2 WHERE p2.project_id = entities.target
  );

-- Update entities with kind='Event' to match their event's publish status
UPDATE entities
SET publish = (
    SELECT ev.publish
    FROM events ev
    WHERE ev.event_id = entities.target
)
WHERE entities.kind = 'Event'
  AND EXISTS (
    SELECT 1 FROM events ev2 WHERE ev2.event_id = entities.target
  );

-- All other entity kinds (Redirect, WordPress, RawHtml, DressedHtml, etc.)
-- keep their default publish=1 value since they don't have corresponding records
