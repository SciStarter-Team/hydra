-- Staff review queue: a message (post) is not delivered to its recipient
-- until a staff member approves it. `approved = 0` is pending, `1` is
-- approved; a rejected message reuses `is_deleted = 1` (already filtered out
-- everywhere). New posts are written pending by send_message/reply_to_message;
-- the orrery→hydra sync writes approved. All pre-existing posts were already
-- delivered, so grandfather them to approved.

ALTER TABLE "posts" ADD COLUMN "approved" INTEGER NOT NULL DEFAULT 0;

UPDATE "posts" SET "approved" = 1;
