ALTER TABLE "ambassador_progress" DROP COLUMN "state";

DROP INDEX IF EXISTS "idx_ambassador_messages_pending";
DROP INDEX IF EXISTS "idx_ambassador_messages_recipient";
DROP TABLE IF EXISTS "ambassador_messages";
