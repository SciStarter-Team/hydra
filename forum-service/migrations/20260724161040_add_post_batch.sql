-- Bulk send: one message sent to many recipients creates one post per
-- recipient, all sharing a `batch` id so staff can review (approve/reject)
-- every copy together and reviewers are notified once. NULL for ordinary
-- one-recipient sends and replies.
ALTER TABLE "posts" ADD COLUMN "batch" TEXT NULL;

-- The review queue groups pending posts by batch; index the lookup.
CREATE INDEX "posts_by_batch" ON "posts" ("batch") WHERE "batch" IS NOT NULL;
