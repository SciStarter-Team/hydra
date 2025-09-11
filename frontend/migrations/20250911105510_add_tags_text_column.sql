-- Add tags_text column to projects table for FTS search
-- SQLite generated columns can't use subqueries, so we use triggers instead

ALTER TABLE projects ADD COLUMN tags_text TEXT NOT NULL DEFAULT '';

-- Function to update tags_text for a project
-- This will be called from triggers
-- We'll create a view to help with the concatenation

-- Update all existing projects with their current tags
UPDATE projects 
SET tags_text = (
    SELECT GROUP_CONCAT(tag, ' ') 
    FROM project_tags 
    WHERE project_tags.project_id = projects.project_id
) 
WHERE project_id IN (SELECT DISTINCT project_id FROM project_tags);

-- Triggers to keep tags_text in sync when project_tags change

-- When a tag is added to a project
CREATE TRIGGER IF NOT EXISTS project_tags_insert_update_fts 
AFTER INSERT ON project_tags
BEGIN
    UPDATE projects 
    SET tags_text = (
        SELECT GROUP_CONCAT(tag, ' ') 
        FROM project_tags 
        WHERE project_tags.project_id = NEW.project_id
    ) 
    WHERE project_id = NEW.project_id;
END;

-- When a tag is removed from a project  
CREATE TRIGGER IF NOT EXISTS project_tags_delete_update_fts
AFTER DELETE ON project_tags
BEGIN
    UPDATE projects 
    SET tags_text = COALESCE(
        (SELECT GROUP_CONCAT(tag, ' ') 
         FROM project_tags 
         WHERE project_tags.project_id = OLD.project_id), 
        ''
    ) 
    WHERE project_id = OLD.project_id;
END;

-- When a tag is updated (changed)
CREATE TRIGGER IF NOT EXISTS project_tags_update_update_fts
AFTER UPDATE ON project_tags
BEGIN
    UPDATE projects 
    SET tags_text = (
        SELECT GROUP_CONCAT(tag, ' ') 
        FROM project_tags 
        WHERE project_tags.project_id = NEW.project_id
    ) 
    WHERE project_id = NEW.project_id;
END;