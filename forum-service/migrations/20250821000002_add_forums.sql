-- Add forums table for organizing topics into distinct forums

-- Forums table - represents distinct forum categories with unique slugs
CREATE TABLE IF NOT EXISTS "forums" (
    "forum_id" TEXT PRIMARY KEY,
    "slug" TEXT NOT NULL UNIQUE,
    "name" TEXT NOT NULL,
    "description" TEXT NULL,
    "created_at" TEXT NOT NULL DEFAULT (datetime('now')),
    "updated_at" TEXT NULL,
    "is_active" INTEGER NOT NULL DEFAULT 1,
    "display_order" INTEGER NOT NULL DEFAULT 0
);

-- Add forum_id foreign key to topics table
ALTER TABLE "topics" ADD COLUMN "forum_id" TEXT NULL;

-- Create foreign key constraint (SQLite doesn't support adding FK constraints to existing tables)
-- We'll enforce this in application code for now

-- Indexes for performance
CREATE INDEX IF NOT EXISTS "idx_forums_slug" ON "forums" ("slug");
CREATE INDEX IF NOT EXISTS "idx_forums_active" ON "forums" ("is_active");
CREATE INDEX IF NOT EXISTS "idx_forums_display_order" ON "forums" ("display_order");
CREATE INDEX IF NOT EXISTS "idx_topics_forum" ON "topics" ("forum_id");

-- Create a default general forum for existing topics
INSERT OR IGNORE INTO "forums" ("forum_id", "slug", "name", "description", "display_order")
VALUES ('01K36XNSJ6DNPDJ59WXV03HSJ6', 'general', 'General Discussion', 'General forum for all discussions', 0);

-- Update existing topics to belong to the general forum
UPDATE "topics" SET "forum_id" = '01K36XNSJ6DNPDJ59WXV03HSJ6' WHERE "forum_id" IS NULL;