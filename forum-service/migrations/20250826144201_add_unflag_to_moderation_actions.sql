-- Add unflag action to the CHECK constraint in moderation_log table

-- SQLite doesn't support ALTER TABLE to modify CHECK constraints directly
-- We need to recreate the table with the updated constraint

-- Create a temporary table with the updated constraint
CREATE TABLE "moderation_log_new" (
    "log_id" TEXT PRIMARY KEY,
    "moderator_id" TEXT NOT NULL,
    "target_type" TEXT NOT NULL CHECK ("target_type" IN ('topic', 'post')),
    "target_id" TEXT NOT NULL,
    "action" TEXT NOT NULL CHECK ("action" IN ('delete', 'pin', 'unpin', 'lock', 'unlock', 'flag', 'unflag')),
    "reason" TEXT NULL,
    "created_at" TEXT NOT NULL
);

-- Copy all existing data
INSERT INTO "moderation_log_new" 
SELECT "log_id", "moderator_id", "target_type", "target_id", "action", "reason", "created_at"
FROM "moderation_log";

-- Drop the old table
DROP TABLE "moderation_log";

-- Rename the new table
ALTER TABLE "moderation_log_new" RENAME TO "moderation_log";

-- Recreate indexes
CREATE INDEX IF NOT EXISTS "idx_moderation_log_target" ON "moderation_log" ("target_type", "target_id");
CREATE INDEX IF NOT EXISTS "idx_moderation_log_moderator" ON "moderation_log" ("moderator_id");
CREATE INDEX IF NOT EXISTS "idx_moderation_log_created" ON "moderation_log" ("created_at");