-- Concierge chat conversations, synchronized to all frontends via
-- LoadStorePeered. Expired locally on each instance after 48 hours.
CREATE TABLE "concierge_conversations" (
    "conversation_id" TEXT PRIMARY KEY,
    "created"         TEXT NOT NULL,
    "updated"         TEXT NOT NULL,
    "user_id"         TEXT,
    "turns"           INTEGER NOT NULL DEFAULT 0,
    "tokens_in"       INTEGER NOT NULL DEFAULT 0,
    "tokens_out"      INTEGER NOT NULL DEFAULT 0,
    "transcript"      TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX "idx_concierge_conversations_updated"
    ON "concierge_conversations" ("updated");
