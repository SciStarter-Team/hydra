-- Project ratings ("likes") cache
-- One row per (project, user). `amount` mirrors orrery's rate_review_rating:
-- > 0 means a like (thumbs up), <= 0 means no like. The displayed like count
-- is COUNT(*) WHERE amount > 0. Anonymous orrery ratings (no user) are stored
-- with a synthetic "anon:<legacy_id>" user_id so each counts once and never
-- matches an authenticated user's like check.
CREATE TABLE IF NOT EXISTS "project_ratings_cache" (
    "project_id" TEXT NOT NULL,
    "user_id" TEXT NOT NULL,
    "amount" INTEGER NOT NULL DEFAULT 0,
    "when" TEXT NOT NULL,
    PRIMARY KEY ("project_id", "user_id")
) STRICT;

-- Like-count lookups filter by project and amount.
CREATE INDEX IF NOT EXISTS "idx_project_ratings_cache_project"
   ON "project_ratings_cache" ("project_id", "amount");
