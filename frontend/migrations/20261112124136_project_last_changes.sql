-- When each project was last really updated: the latest PageChange the
-- activity log holds against it, copied here so that a search can order by
-- it. Derived, never authored: page_change::refresh_project_last_changes
-- replaces the whole table from the log, and a project with no change logged
-- has no row.
CREATE TABLE project_last_changes (
    "project_id" TEXT PRIMARY KEY NOT NULL,
    "changed" TEXT NOT NULL
);
