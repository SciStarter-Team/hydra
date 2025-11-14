-- Create project_joins table to track when users join projects
CREATE TABLE project_joins (
    user_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    PRIMARY KEY (user_id, project_id),
    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE INDEX project_joins_by_user ON project_joins(user_id);
CREATE INDEX project_joins_by_project ON project_joins(project_id);
