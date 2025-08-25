-- Remove SQLite DEFAULT datetime('now') clauses that create timezone-incompatible timestamps
-- The application will handle timestamp generation with proper ISO format

-- Since SQLite doesn't support dropping DEFAULT constraints directly,
-- we need to recreate the tables without the DEFAULT clauses

-- First, create new tables without DEFAULT datetime('now')
CREATE TABLE "topics_new" (
    "topic_id" TEXT PRIMARY KEY,
    "title" TEXT NOT NULL,
    "content" TEXT NOT NULL,
    "author_id" TEXT NOT NULL,
    "created_at" TEXT NOT NULL,
    "updated_at" TEXT NULL,
    "category" TEXT NULL,
    "is_pinned" INTEGER NOT NULL DEFAULT 0,
    "is_locked" INTEGER NOT NULL DEFAULT 0,
    "is_deleted" INTEGER NOT NULL DEFAULT 0,
    "forum_id" TEXT NULL
);

CREATE TABLE "posts_new" (
    "post_id" TEXT PRIMARY KEY,
    "topic_id" TEXT NOT NULL,
    "content" TEXT NOT NULL,
    "author_id" TEXT NOT NULL,
    "parent_post_id" TEXT NULL,
    "created_at" TEXT NOT NULL,
    "updated_at" TEXT NULL,
    "is_deleted" INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY ("topic_id") REFERENCES "topics_new" ("topic_id"),
    FOREIGN KEY ("parent_post_id") REFERENCES "posts_new" ("post_id")
);

CREATE TABLE "moderation_log_new" (
    "log_id" TEXT PRIMARY KEY,
    "moderator_id" TEXT NOT NULL,
    "target_type" TEXT NOT NULL CHECK ("target_type" IN ('topic', 'post')),
    "target_id" TEXT NOT NULL,
    "action" TEXT NOT NULL CHECK ("action" IN ('delete', 'pin', 'unpin', 'lock', 'unlock', 'flag')),
    "reason" TEXT NULL,
    "created_at" TEXT NOT NULL
);

CREATE TABLE "bad_words_new" (
    "word" TEXT PRIMARY KEY,
    "severity" TEXT NOT NULL DEFAULT 'moderate' CHECK ("severity" IN ('mild', 'moderate', 'severe')),
    "created_at" TEXT NOT NULL
);

CREATE TABLE "forums_new" (
    "forum_id" TEXT PRIMARY KEY,
    "slug" TEXT NOT NULL UNIQUE,
    "name" TEXT NOT NULL,
    "description" TEXT NULL,
    "created_at" TEXT NOT NULL,
    "updated_at" TEXT NULL,
    "is_active" INTEGER NOT NULL DEFAULT 1,
    "display_order" INTEGER NOT NULL DEFAULT 0
);

-- Copy data from old tables to new tables
INSERT INTO "topics_new" SELECT * FROM "topics";
INSERT INTO "posts_new" SELECT * FROM "posts";
INSERT INTO "moderation_log_new" SELECT * FROM "moderation_log";
INSERT INTO "bad_words_new" SELECT * FROM "bad_words";
INSERT INTO "forums_new" SELECT * FROM "forums";

-- Drop old tables
DROP TABLE "topics";
DROP TABLE "posts";
DROP TABLE "moderation_log";
DROP TABLE "bad_words";
DROP TABLE "forums";

-- Rename new tables
ALTER TABLE "topics_new" RENAME TO "topics";
ALTER TABLE "posts_new" RENAME TO "posts";
ALTER TABLE "moderation_log_new" RENAME TO "moderation_log";
ALTER TABLE "bad_words_new" RENAME TO "bad_words";
ALTER TABLE "forums_new" RENAME TO "forums";

-- Recreate indexes
CREATE INDEX IF NOT EXISTS "idx_topics_author" ON "topics" ("author_id");
CREATE INDEX IF NOT EXISTS "idx_topics_category" ON "topics" ("category");
CREATE INDEX IF NOT EXISTS "idx_topics_created" ON "topics" ("created_at");
CREATE INDEX IF NOT EXISTS "idx_topics_updated" ON "topics" ("updated_at");
CREATE INDEX IF NOT EXISTS "idx_topics_forum" ON "topics" ("forum_id");

CREATE INDEX IF NOT EXISTS "idx_posts_topic" ON "posts" ("topic_id");
CREATE INDEX IF NOT EXISTS "idx_posts_author" ON "posts" ("author_id");
CREATE INDEX IF NOT EXISTS "idx_posts_parent" ON "posts" ("parent_post_id");
CREATE INDEX IF NOT EXISTS "idx_posts_created" ON "posts" ("created_at");

CREATE INDEX IF NOT EXISTS "idx_topic_tags_tag" ON "topic_tags" ("tag");

CREATE INDEX IF NOT EXISTS "idx_moderation_log_target" ON "moderation_log" ("target_type", "target_id");
CREATE INDEX IF NOT EXISTS "idx_moderation_log_moderator" ON "moderation_log" ("moderator_id");

CREATE INDEX IF NOT EXISTS "idx_forums_slug" ON "forums" ("slug");
CREATE INDEX IF NOT EXISTS "idx_forums_active" ON "forums" ("is_active");
CREATE INDEX IF NOT EXISTS "idx_forums_display_order" ON "forums" ("display_order");