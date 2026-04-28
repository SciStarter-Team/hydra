-- Staging table for the daily full-replace download of Zooniverse user
-- mappings. Rows are inserted as they arrive from orrery; once the
-- download completes successfully, the live table is replaced from
-- staging in a single transaction. A failed download leaves the previous
-- mapping intact.

CREATE TABLE zooniverse_users_staging (
    zooniverse_id INTEGER NOT NULL,
    zooniverse_username TEXT NOT NULL,
    user_id TEXT,
    PRIMARY KEY (zooniverse_id, user_id)
);
