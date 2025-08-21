-- Forum service database schema

-- Topics table - represents forum topics/discussions
CREATE TABLE IF NOT EXISTS "topics" (
    "topic_id" TEXT PRIMARY KEY,
    "title" TEXT NOT NULL,
    "content" TEXT NOT NULL,
    "author_id" TEXT NOT NULL,
    "created_at" TEXT NOT NULL DEFAULT (datetime('now')),
    "updated_at" TEXT NULL,
    "category" TEXT NULL,
    "is_pinned" INTEGER NOT NULL DEFAULT 0,
    "is_locked" INTEGER NOT NULL DEFAULT 0,
    "is_deleted" INTEGER NOT NULL DEFAULT 0
);

-- Posts table - represents replies within topics
CREATE TABLE IF NOT EXISTS "posts" (
    "post_id" TEXT PRIMARY KEY,
    "topic_id" TEXT NOT NULL,
    "content" TEXT NOT NULL,
    "author_id" TEXT NOT NULL,
    "parent_post_id" TEXT NULL,
    "created_at" TEXT NOT NULL DEFAULT (datetime('now')),
    "updated_at" TEXT NULL,
    "is_deleted" INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY ("topic_id") REFERENCES "topics" ("topic_id"),
    FOREIGN KEY ("parent_post_id") REFERENCES "posts" ("post_id")
);

-- Topic tags table - many-to-many relationship for topic tags
CREATE TABLE IF NOT EXISTS "topic_tags" (
    "topic_id" TEXT NOT NULL,
    "tag" TEXT NOT NULL,
    PRIMARY KEY ("topic_id", "tag"),
    FOREIGN KEY ("topic_id") REFERENCES "topics" ("topic_id")
);

-- Moderation log table - tracks moderation actions
CREATE TABLE IF NOT EXISTS "moderation_log" (
    "log_id" TEXT PRIMARY KEY,
    "moderator_id" TEXT NOT NULL,
    "target_type" TEXT NOT NULL CHECK ("target_type" IN ('topic', 'post')),
    "target_id" TEXT NOT NULL,
    "action" TEXT NOT NULL CHECK ("action" IN ('delete', 'pin', 'unpin', 'lock', 'unlock', 'flag')),
    "reason" TEXT NULL,
    "created_at" TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Bad words table - stores words/phrases to filter
CREATE TABLE IF NOT EXISTS "bad_words" (
    "word" TEXT PRIMARY KEY,
    "severity" TEXT NOT NULL DEFAULT 'moderate' CHECK ("severity" IN ('mild', 'moderate', 'severe')),
    "created_at" TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS "idx_topics_author" ON "topics" ("author_id");
CREATE INDEX IF NOT EXISTS "idx_topics_category" ON "topics" ("category");
CREATE INDEX IF NOT EXISTS "idx_topics_created" ON "topics" ("created_at");
CREATE INDEX IF NOT EXISTS "idx_topics_updated" ON "topics" ("updated_at");

CREATE INDEX IF NOT EXISTS "idx_posts_topic" ON "posts" ("topic_id");
CREATE INDEX IF NOT EXISTS "idx_posts_author" ON "posts" ("author_id");
CREATE INDEX IF NOT EXISTS "idx_posts_parent" ON "posts" ("parent_post_id");
CREATE INDEX IF NOT EXISTS "idx_posts_created" ON "posts" ("created_at");

CREATE INDEX IF NOT EXISTS "idx_topic_tags_tag" ON "topic_tags" ("tag");

CREATE INDEX IF NOT EXISTS "idx_moderation_log_target" ON "moderation_log" ("target_type", "target_id");
CREATE INDEX IF NOT EXISTS "idx_moderation_log_moderator" ON "moderation_log" ("moderator_id");