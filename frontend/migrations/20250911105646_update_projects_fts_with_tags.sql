-- Update FTS5 virtual table to include tags_text column
-- SQLite FTS5 doesn't support ALTER TABLE, so we need to recreate it

-- Drop existing FTS table and triggers
DROP TRIGGER IF EXISTS projects_fts_insert;
DROP TRIGGER IF EXISTS projects_fts_update;
DROP TRIGGER IF EXISTS projects_fts_delete;
DROP TABLE IF EXISTS projects_fts;

-- Create new FTS5 virtual table with tags_text included
CREATE VIRTUAL TABLE IF NOT EXISTS projects_fts USING fts5(
    project_id UNINDEXED,
    name,
    description,
    presenter,
    goal,
    task,
    search_terms,
    tags_text,
    content='projects',
    content_rowid='rowid'
);

-- Populate the FTS table with existing data including tags
INSERT INTO projects_fts(project_id, name, description, presenter, goal, task, search_terms, tags_text)
SELECT project_id, name, description, presenter, goal, task, search_terms, tags_text
FROM projects;

-- Create updated triggers to keep FTS table in sync with tags_text
CREATE TRIGGER IF NOT EXISTS projects_fts_insert AFTER INSERT ON projects
BEGIN
    INSERT INTO projects_fts(project_id, name, description, presenter, goal, task, search_terms, tags_text)
    VALUES (NEW.project_id, NEW.name, NEW.description, NEW.presenter, NEW.goal, NEW.task, NEW.search_terms, NEW.tags_text);
END;

CREATE TRIGGER IF NOT EXISTS projects_fts_update AFTER UPDATE ON projects
BEGIN
    UPDATE projects_fts 
    SET name = NEW.name, 
        description = NEW.description, 
        presenter = NEW.presenter,
        goal = NEW.goal,
        task = NEW.task,
        search_terms = NEW.search_terms,
        tags_text = NEW.tags_text
    WHERE project_id = NEW.project_id;
END;

CREATE TRIGGER IF NOT EXISTS projects_fts_delete AFTER DELETE ON projects
BEGIN
    DELETE FROM projects_fts WHERE project_id = OLD.project_id;
END;