-- A reply reference names one message, and a conversation holds many, so the
-- references are keyed on themselves. Keyed on the topic, as they were, only
-- a conversation's first reference could be kept; a reply's was dropped.
CREATE TABLE "message_reply_refs_new" (
    "reply_ref" TEXT NOT NULL PRIMARY KEY,
    "topic_id" TEXT NOT NULL
);

INSERT INTO "message_reply_refs_new" ("reply_ref", "topic_id")
SELECT "reply_ref", "topic_id" FROM "message_reply_refs";

DROP TABLE "message_reply_refs";
ALTER TABLE "message_reply_refs_new" RENAME TO "message_reply_refs";

CREATE INDEX "message_reply_refs_by_topic" ON "message_reply_refs" ("topic_id");

-- The post each of orrery's messages was copied to, and what orrery says of
-- the message that the post doesn't hold: who it went to, what it answers,
-- and whether its recipient has seen or discarded it.
CREATE TABLE "orrery_messages" (
    "orrery_id" INTEGER NOT NULL PRIMARY KEY,
    "post_id" TEXT NOT NULL UNIQUE REFERENCES "posts" ("post_id"),
    "reply_ref" TEXT NOT NULL,
    "parent_reply_ref" TEXT NULL,
    "sender_id" TEXT NOT NULL,
    "recipient_id" TEXT NOT NULL,
    "sent" TEXT NOT NULL,
    "seen" INTEGER NOT NULL,
    "discarded" INTEGER NOT NULL
);

CREATE INDEX "orrery_messages_by_parent" ON "orrery_messages" ("parent_reply_ref")
    WHERE "parent_reply_ref" IS NOT NULL;
