CREATE TABLE IF NOT EXISTS project_references (
    reference_id TEXT PRIMARY KEY NOT NULL,
    entity_id TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    reference_type TEXT NOT NULL DEFAULT 'Blog',
    url TEXT NOT NULL DEFAULT '',
    citation TEXT NOT NULL DEFAULT '',
    detailed_citation TEXT NOT NULL DEFAULT '',
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_project_references_entity_id ON project_references(entity_id);

CREATE TABLE IF NOT EXISTS project_investigators (
    investigator_id TEXT PRIMARY KEY NOT NULL,
    entity_id TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    affiliation TEXT NOT NULL DEFAULT '',
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_project_investigators_entity_id ON project_investigators(entity_id);
