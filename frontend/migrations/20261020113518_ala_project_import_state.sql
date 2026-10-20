-- Import state of a project brought in from the Atlas of Living Australia's
-- BioCollect catalog (crate::project::ala::AlaImport, peered between frontend
-- instances).
--
-- The project's guid holds its BioCollect projectId. last_updated is the
-- BioCollect `lastUpdated` of the version last imported in full (a
-- TimeString), so a record is imported again only when BioCollect changes it.
-- A project with no row here has never been fully imported. Until hydra is
-- the primary, orrery imports and the project sync carries its
-- bridge.ALAProject rows here.
--
-- No foreign key to projects: ProjectAll saves with INSERT OR REPLACE, which
-- would cascade a delete through it on every save.

CREATE TABLE ala_projects (
    project_id TEXT NOT NULL PRIMARY KEY,
    last_updated TEXT
) STRICT;
