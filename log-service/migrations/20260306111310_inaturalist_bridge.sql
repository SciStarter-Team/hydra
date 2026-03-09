CREATE TABLE IF NOT EXISTS inaturalist_projects (
    inat_project_id INTEGER PRIMARY KEY NOT NULL,
    entity_id TEXT NOT NULL,
    entity_slug TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_inaturalist_projects_slug ON inaturalist_projects(entity_slug);
