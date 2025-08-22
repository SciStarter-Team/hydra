-- Add forum-specific moderators table

-- Forum moderators table - tracks users who can moderate specific forums
CREATE TABLE IF NOT EXISTS "forum_moderators" (
    "forum_id" TEXT NOT NULL,
    "user_id" TEXT NOT NULL,
    "created_at" TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY ("forum_id", "user_id"),
    FOREIGN KEY ("forum_id") REFERENCES "forums" ("forum_id")
);

-- Index for performance when looking up moderators by user
CREATE INDEX IF NOT EXISTS "idx_forum_moderators_user" ON "forum_moderators" ("user_id");