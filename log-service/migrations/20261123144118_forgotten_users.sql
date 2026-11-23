-- The accounts deleted (common::user_deletions). Their caches here are
-- cleared when they are deleted; the logs keep their rows, so what is built
-- from the logs afresh each time (the project stats snapshot) leaves them out
-- by this list.
CREATE TABLE "forgotten_users" (
    "user_id" TEXT PRIMARY KEY NOT NULL
) WITHOUT ROWID;
