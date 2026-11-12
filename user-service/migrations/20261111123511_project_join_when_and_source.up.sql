-- When each join was made, and what made it (common's `JoinSource`).
--
-- Every row here so far is either a copy of one of orrery's confirmed joins,
-- which carry no date or source yet, or one that hydra made on a visit, a
-- signup or a sign-in, none of which are joins any longer. So the table is
-- emptied and the join sync is sent back to the start of orrery's history,
-- which refills it with orrery's own dates.
DROP TABLE "project_joins";

CREATE TABLE "project_joins" (
    "user_id" TEXT NOT NULL,
    "project_id" TEXT NOT NULL,
    "when" TEXT NOT NULL,
    "source" TEXT NOT NULL CHECK ("source" IN (
        'legacy_orrery', 'explicit', 'participation', 'platform_connect'
    )),
    PRIMARY KEY ("user_id", "project_id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("user_id") ON DELETE CASCADE
);

CREATE INDEX "project_joins_by_user" ON "project_joins" ("user_id");
CREATE INDEX "project_joins_by_project" ON "project_joins" ("project_id");

DELETE FROM "misc" WHERE "key" IN ('join-sync-bookmark', 'join-sync-last-id');
