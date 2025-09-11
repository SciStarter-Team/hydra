-- Indexes to optimize admin project queries

-- Primary index for ORDER BY created DESC (main sorting)
CREATE INDEX IF NOT EXISTS idx_projects_created_desc ON projects (created DESC);

-- Individual filter indexes for common WHERE clauses
CREATE INDEX IF NOT EXISTS idx_projects_status ON projects (status);
CREATE INDEX IF NOT EXISTS idx_projects_publish ON projects (publish);

-- Composite index for queries with multiple filters plus sorting
-- This covers: WHERE status=? AND publish=? ORDER BY created DESC
CREATE INDEX IF NOT EXISTS idx_projects_status_publish_created ON projects (status, publish, created DESC);

-- Additional indexes for other admin filtering operations
CREATE INDEX IF NOT EXISTS idx_projects_flagged ON projects (flagged);
CREATE INDEX IF NOT EXISTS idx_projects_origin ON projects (origin);

-- Note: project_id already has unique index "projects_by_project_id" for FTS joins