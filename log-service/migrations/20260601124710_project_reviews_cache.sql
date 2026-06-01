-- Project reviews cache
-- One row per (project, user), mirroring orrery's rate_review_review. `private`
-- reviews are hidden from the public list and count (matching orrery, which only
-- shows private = false). `checked` is orrery's staff-vetted flag, carried for
-- completeness. Reviews always have a user (no anonymous reviews).
CREATE TABLE IF NOT EXISTS "project_reviews_cache" (
    "project_id" TEXT NOT NULL,
    "user_id" TEXT NOT NULL,
    "text" TEXT NOT NULL,
    "private" INTEGER NOT NULL DEFAULT 0,
    "checked" INTEGER NOT NULL DEFAULT 0,
    "when" TEXT NOT NULL,
    PRIMARY KEY ("project_id", "user_id")
) STRICT;

-- Public-review lookups filter by project and privacy.
CREATE INDEX IF NOT EXISTS "idx_project_reviews_cache_project"
   ON "project_reviews_cache" ("project_id", "private");
