-- What the visitor saw of each turn (their message, the reply, and the cards
-- shown with it), so a conversation can be redrawn when the visitor reopens
-- the chat on another page. The transcript holds what the model saw, which
-- is not the same thing: it carries tool digests rather than cards.
ALTER TABLE "concierge_conversations"
    ADD COLUMN "shown" TEXT NOT NULL DEFAULT '[]';
