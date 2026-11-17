-- What each signed-in participant has contributed to each project, and when
-- they last did, for its Participant Manager. Part of the daily project
-- stats snapshot (src/project_stats.rs), rebuilt with the rest of it.
CREATE TABLE project_stats_participants (
    project_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    contributions INTEGER NOT NULL,
    last_contribution TEXT NOT NULL,
    PRIMARY KEY (project_id, user_id)
) STRICT;
