-- Take Action projects logged from hydra's /girlscouts-action, the last step of
-- the Think Like a Citizen Scientist Journey.
--
-- Orrery's equivalent table recorded only the five form fields: no submitter,
-- no timestamp, and nothing anywhere in orrery ever read it back, not even
-- through the Django admin. Both are recorded here, and the staff queue at
-- /-admin/girlscouts lists them, because a troop's write-up is only useful if
-- staff can tell whose it is and when it arrived.
CREATE TABLE "girlscouts_take_action" (
    "submission_id" TEXT PRIMARY KEY NOT NULL, -- Ulid
    "submitted"     TEXT NOT NULL DEFAULT '',  -- TimeString
    "user_id"       TEXT NOT NULL REFERENCES "users"("user_id") ON DELETE CASCADE,
    "troop"         TEXT NOT NULL DEFAULT '',
    "problem"       TEXT NOT NULL DEFAULT '',
    "solution"      TEXT NOT NULL DEFAULT '',
    "sustainable"   TEXT NOT NULL DEFAULT '',
    -- URL of the uploaded troop photo, empty when none was attached.
    "photo"         TEXT NOT NULL DEFAULT ''
);

-- The queue lists newest first; the id is a Ulid, so it already orders by
-- submission time and no separate index on "submitted" is needed.
CREATE INDEX "girlscouts_take_action_by_user" ON "girlscouts_take_action" ("user_id");
