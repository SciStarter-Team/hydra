-- Cache tables for fast completion/participation lookups
-- Presence of a record indicates completion/participation has occurred

-- Articulate quiz completion cache
-- Tracks which users have completed which Articulate Storyline activities
CREATE TABLE IF NOT EXISTS "articulate_completions_cache" (
    "user_id" TEXT NOT NULL,
    "activity" TEXT NOT NULL,
    PRIMARY KEY ("user_id", "activity")
) STRICT;

-- Project participation cache
-- Tracks which users have participated in which affiliate projects
CREATE TABLE IF NOT EXISTS "project_participations_cache" (
    "user_id" TEXT NOT NULL,
    "project_id" TEXT NOT NULL,
    PRIMARY KEY ("user_id", "project_id")
) STRICT;

-- Indexes for performance
CREATE INDEX IF NOT EXISTS "idx_articulate_completions_cache_user_id"
   ON "articulate_completions_cache" ("user_id");

CREATE INDEX IF NOT EXISTS "idx_articulate_completions_cache_activity"
   ON "articulate_completions_cache" ("activity");

CREATE INDEX IF NOT EXISTS "idx_project_participations_cache_user_id"
   ON "project_participations_cache" ("user_id");

CREATE INDEX IF NOT EXISTS "idx_project_participations_cache_project_id"
   ON "project_participations_cache" ("project_id");
