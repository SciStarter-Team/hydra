-- Orrery's counts of clicks on a project's Visit button ("join-click" entity
-- actions), the record of visits from before hydra counted each one itself.
-- A row is one of orrery's counting windows: daily from February to June
-- 2019, and after that a single window per project that never closed, so
-- `as_of` is only the time of the window's latest click. Nothing here says
-- who clicked.
CREATE TABLE orrery_visit_clicks (
    window_id INTEGER PRIMARY KEY,
    project_id TEXT NOT NULL,
    as_of TEXT NOT NULL,
    clicks INTEGER NOT NULL
);

CREATE INDEX orrery_visit_clicks_by_project ON orrery_visit_clicks (project_id);
