-- Message read/discard tracking for personal forum messaging
CREATE TABLE IF NOT EXISTS "message_status" (
    "user_id" TEXT NOT NULL,
    "topic_id" TEXT NOT NULL,
    "seen" INTEGER NOT NULL DEFAULT 0,
    "discarded" INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY ("user_id", "topic_id")
);

CREATE INDEX IF NOT EXISTS "idx_message_status_user" ON "message_status" ("user_id");

-- Reply reference for orrery message deduplication
-- Stored on topics created via messaging (first message in conversation)
CREATE TABLE IF NOT EXISTS "message_reply_refs" (
    "topic_id" TEXT NOT NULL PRIMARY KEY,
    "reply_ref" TEXT NOT NULL UNIQUE
);
