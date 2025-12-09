-- Tracks the source of each project field value for permission-based updates
-- Higher priority sources can overwrite lower priority sources, but not vice versa
-- Priority: Staff (4) > Form (3) > Api (2) > Legacy (1)

CREATE TABLE project_field_sources (
    project_id TEXT NOT NULL,
    field_type TEXT NOT NULL,
    source TEXT NOT NULL,
    PRIMARY KEY (project_id, field_type)
) STRICT;

CREATE INDEX project_field_sources_by_project_id ON project_field_sources(project_id);
