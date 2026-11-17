-- Orrery's views of each project, one row per UTC day, carried from
-- /staff/sync/project_views: the record of views from before hydra counted
-- them itself.
CREATE TABLE orrery_project_views (
    project_id TEXT NOT NULL,
    day TEXT NOT NULL,
    views INTEGER NOT NULL,
    PRIMARY KEY (project_id, day)
) STRICT;

-- Orrery's record of the share buttons clicked on an entity's page, carried
-- from /staff/sync/project_shares. `entity_id` may name an event as well as
-- a project.
CREATE TABLE orrery_shares (
    click_id INTEGER PRIMARY KEY,
    entity_id TEXT NOT NULL,
    platform TEXT NOT NULL,
    "when" TEXT NOT NULL
) STRICT;

-- The daily snapshot of every project's stats (src/project_stats.rs). The
-- tables below are rebuilt together, in one transaction, each time.

-- When the snapshot was built, and the last UTC day it counts.
CREATE TABLE project_stats_snapshot (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    built TEXT NOT NULL,
    through TEXT NOT NULL
) STRICT;

-- The projects the snapshot knows.
CREATE TABLE project_stats_projects (
    project_id TEXT PRIMARY KEY,
    published INTEGER NOT NULL
) STRICT;

-- Each project's total of each series over each span ('all_time',
-- 'last_30', 'last_7'). A series is a measure ('views', 'visits', 'saves',
-- 'joins', 'contributions') or 'share:<platform>'. Zero totals are left out.
CREATE TABLE project_stats_totals (
    project_id TEXT NOT NULL,
    series TEXT NOT NULL,
    span TEXT NOT NULL,
    total INTEGER NOT NULL,
    PRIMARY KEY (project_id, series, span)
) STRICT;

-- Each project's counts for each of the last 90 days. Days with nothing to
-- count are left out.
CREATE TABLE project_stats_days (
    project_id TEXT NOT NULL,
    day TEXT NOT NULL,
    views INTEGER NOT NULL,
    visits INTEGER NOT NULL,
    saves INTEGER NOT NULL,
    contributions INTEGER NOT NULL,
    PRIMARY KEY (project_id, day)
) STRICT;

-- What every project that has any of a series has of it over a span,
-- ascending, as a JSON array: what one project's figure is compared with.
-- Feedback figures are the series 'feedback:<figure>' over 'all_time'.
CREATE TABLE project_stats_distributions (
    series TEXT NOT NULL,
    span TEXT NOT NULL,
    sorted_values TEXT NOT NULL,
    PRIMARY KEY (series, span)
) STRICT;

-- How many of the signed-in accounts that went to each project's site have
-- joined it, and how many have not said.
CREATE TABLE project_stats_feedback (
    project_id TEXT PRIMARY KEY,
    joined INTEGER NOT NULL,
    unknown INTEGER NOT NULL
) STRICT;

-- How many times each signed-in account other than staff viewed each
-- project's page.
CREATE TABLE project_stats_viewers (
    project_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    views INTEGER NOT NULL,
    PRIMARY KEY (project_id, user_id)
) STRICT;

CREATE INDEX project_stats_viewers_by_user ON project_stats_viewers (user_id, project_id);

-- The gender and birthday of the viewers that have given one.
CREATE TABLE project_stats_people (
    user_id TEXT PRIMARY KEY,
    gender TEXT NOT NULL,
    birthday TEXT
) STRICT;
