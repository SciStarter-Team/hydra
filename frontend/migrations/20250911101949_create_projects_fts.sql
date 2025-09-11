-- Create FTS5 virtual table for full-text search on projects
CREATE VIRTUAL TABLE IF NOT EXISTS projects_fts USING fts5(
    project_id UNINDEXED,
    name,
    description,
    presenter,
    goal,
    task,
    search_terms,
    content='projects',
    content_rowid='rowid'
);

-- Populate the FTS table with existing data
INSERT INTO projects_fts(project_id, name, description, presenter, goal, task, search_terms)
SELECT project_id, name, description, presenter, goal, task, search_terms
FROM projects;

-- Create triggers to keep FTS table in sync
CREATE TRIGGER IF NOT EXISTS projects_fts_insert AFTER INSERT ON projects
BEGIN
    INSERT INTO projects_fts(project_id, name, description, presenter, goal, task, search_terms)
    VALUES (NEW.project_id, NEW.name, NEW.description, NEW.presenter, NEW.goal, NEW.task, NEW.search_terms);
END;

CREATE TRIGGER IF NOT EXISTS projects_fts_update AFTER UPDATE ON projects
BEGIN
    UPDATE projects_fts 
    SET name = NEW.name, 
        description = NEW.description, 
        presenter = NEW.presenter,
        goal = NEW.goal,
        task = NEW.task,
        search_terms = NEW.search_terms
    WHERE project_id = NEW.project_id;
END;

CREATE TRIGGER IF NOT EXISTS projects_fts_delete AFTER DELETE ON projects
BEGIN
    DELETE FROM projects_fts WHERE project_id = OLD.project_id;
END;