-- Add flagged fields to topics and posts tables for quick moderation queries

-- Add is_flagged field to topics table (checking if it exists first)
ALTER TABLE "topics" ADD COLUMN "is_flagged" INTEGER NOT NULL DEFAULT 0;

-- Add is_flagged field to posts table (checking if it exists first)
ALTER TABLE "posts" ADD COLUMN "is_flagged" INTEGER NOT NULL DEFAULT 0;

-- Create partial indexes for efficient flagged content queries
CREATE INDEX IF NOT EXISTS "idx_topics_flagged" ON "topics" ("is_flagged") WHERE "is_flagged" = 1;
CREATE INDEX IF NOT EXISTS "idx_posts_flagged" ON "posts" ("is_flagged") WHERE "is_flagged" = 1;

-- Create composite partial indexes for flagged content in non-deleted items
CREATE INDEX IF NOT EXISTS "idx_topics_flagged_active" ON "topics" ("is_flagged", "is_deleted") 
    WHERE "is_flagged" = 1 AND "is_deleted" = 0;
CREATE INDEX IF NOT EXISTS "idx_posts_flagged_active" ON "posts" ("is_flagged", "is_deleted") 
    WHERE "is_flagged" = 1 AND "is_deleted" = 0;