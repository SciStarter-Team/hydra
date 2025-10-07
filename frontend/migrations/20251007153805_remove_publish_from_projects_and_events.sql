-- Remove publish column and indexes from projects table
-- (publish is now managed at the entity level in entities.publish)
DROP INDEX IF EXISTS idx_projects_publish;
DROP INDEX IF EXISTS idx_projects_status_publish_created;
ALTER TABLE projects DROP COLUMN publish;

-- Remove publish column from events table
-- (publish is now managed at the entity level in entities.publish)
ALTER TABLE events DROP COLUMN publish;
