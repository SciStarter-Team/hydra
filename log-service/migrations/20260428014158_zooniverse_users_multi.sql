-- A single Zooniverse account can be claimed by multiple SciStarter users.
-- Replace the (zooniverse_id) primary key with a (zooniverse_id, user_id) composite
-- so we can store one row per mapped pair, including a single null-user row
-- when no SciStarter account claims the Zooniverse account yet.

CREATE TABLE zooniverse_users_new (
    zooniverse_id INTEGER NOT NULL,
    zooniverse_username TEXT NOT NULL,
    user_id TEXT,
    PRIMARY KEY (zooniverse_id, user_id)
);

INSERT INTO zooniverse_users_new (zooniverse_id, zooniverse_username, user_id)
SELECT zooniverse_id, zooniverse_username, user_id FROM zooniverse_users;

DROP TABLE zooniverse_users;

ALTER TABLE zooniverse_users_new RENAME TO zooniverse_users;

CREATE INDEX idx_zooniverse_users_username ON zooniverse_users(zooniverse_username);
CREATE INDEX idx_zooniverse_users_user_id ON zooniverse_users(user_id);
