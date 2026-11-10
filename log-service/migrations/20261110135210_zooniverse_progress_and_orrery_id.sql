-- Separate what the Zooniverse bridge has read of a Panoptes project from
-- whether the project is connected, and mark the connections copied from
-- orrery.
--
-- Classifications are appended to the participation log as they are read,
-- so reading a project's again from the start would credit them twice. The
-- progress therefore outlives the connection: a project disconnected and
-- connected again carries on where the bridge left it.

CREATE TABLE "zooniverse_progress" (
    "zooniverse_id" INTEGER PRIMARY KEY NOT NULL,

    -- The last classification the bridge has read.
    "last_classification_id" INTEGER NOT NULL DEFAULT 0,

    -- The earliest classification the Zooniverse bridge credited to the
    -- project before the August 2026 import policy, or NULL if it credited
    -- none. Until the policy, the bridge credited every connected account,
    -- so for an account connected before the policy, orrery's Zooniverse
    -- participation from then on is already in the log. Filled in by
    -- `participation-repair`.
    "credited_since" TEXT
);

INSERT INTO "zooniverse_progress" ("zooniverse_id", "last_classification_id", "credited_since")
SELECT "zooniverse_id", "last_classification_id", "credited_since" FROM "zooniverse_projects";

-- The Zooniverse projects whose classifications are credited, one row per
-- Panoptes project, and the SciStarter project each is credited to.
--
-- `reader` names which of SciStarter's Zooniverse accounts the project's
-- team made a collaborator, and so the one its classifications are read as.
--
-- `orrery_id` is orrery's row id on a connection the project sync copied
-- from orrery's admin, and NULL on one made in hydra's. Every connection
-- here before now was copied from orrery, but without its id; each is
-- marked by the negation of its Panoptes id, which no orrery id can be,
-- until the sync finds it in orrery's set and records the real one. One the
-- sync doesn't find there has since been deleted on orrery, and is deleted
-- here too.
CREATE TABLE "zooniverse_projects_new" (
    "zooniverse_id" INTEGER PRIMARY KEY NOT NULL CHECK ("zooniverse_id" > 0),
    "entity_id" TEXT NOT NULL,
    "reader" TEXT NOT NULL CHECK ("reader" IN ('legacy', 'scistarter')),
    "orrery_id" INTEGER UNIQUE CHECK (
        "orrery_id" IS NULL OR "orrery_id" > 0 OR "orrery_id" = -"zooniverse_id"
    )
);

INSERT INTO "zooniverse_projects_new" ("zooniverse_id", "entity_id", "reader", "orrery_id")
SELECT "zooniverse_id", "entity_id", "reader", -"zooniverse_id" FROM "zooniverse_projects";

DROP TABLE "zooniverse_projects";

ALTER TABLE "zooniverse_projects_new" RENAME TO "zooniverse_projects";

CREATE INDEX "zooniverse_projects_by_entity" ON "zooniverse_projects" ("entity_id");
