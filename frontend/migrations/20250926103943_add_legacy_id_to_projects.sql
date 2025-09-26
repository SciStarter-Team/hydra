-- Add legacy_id field to projects table and update existing records
ALTER TABLE projects ADD COLUMN legacy_id INTEGER;

-- Copy legacy_v1_project_id values to legacy_id for existing records
UPDATE projects SET legacy_id = legacy_v1_project_id WHERE legacy_v1_project_id IS NOT NULL;

-- Create index on the new legacy_id field
CREATE INDEX projects_by_legacy_id ON projects (legacy_id) WHERE legacy_id IS NOT NULL;

-- Drop the old indexes on legacy_v1_project_id and legacy_v2_project_id
DROP INDEX projects_by_legacy_v1_project_id;
DROP INDEX projects_by_legacy_v2_project_id;
