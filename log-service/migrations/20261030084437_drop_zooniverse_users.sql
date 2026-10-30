-- The Panoptes account each SciStarter account's Zooniverse login names is
-- held on the account, in user-service's users.zooniverse_id, and the sweep
-- reads it from user.zooniverse.list_links. This copy, fed whole from
-- orrery, held every classifier of every affiliate project, claimed or not.
--
-- Until this release the bridge matched classifications through it, so
-- which SciStarter accounts it matched, and from when, is kept. An account
-- it matched is covered as it always was. Any other account with an id was
-- matched from this release on, and orrery's copy of what came before that
-- is the one that stands.
CREATE TABLE zooniverse_matched_by_mapping (
    user_id TEXT NOT NULL,
    zooniverse_id INTEGER NOT NULL,
    PRIMARY KEY (user_id, zooniverse_id)
);

INSERT OR IGNORE INTO zooniverse_matched_by_mapping (user_id, zooniverse_id)
SELECT user_id, zooniverse_id FROM zooniverse_users WHERE user_id IS NOT NULL;

CREATE TABLE zooniverse_matching_by_id (
    since TEXT NOT NULL
);

INSERT INTO zooniverse_matching_by_id (since)
VALUES (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

DROP TABLE IF EXISTS zooniverse_users_staging;
DROP TABLE IF EXISTS zooniverse_users;
