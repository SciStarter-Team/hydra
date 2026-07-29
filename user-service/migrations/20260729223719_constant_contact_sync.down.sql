DROP INDEX IF EXISTS "users_pending_constant_contact";

ALTER TABLE "users" DROP COLUMN "state";
ALTER TABLE "users" DROP COLUMN "constant_contact_digest";
ALTER TABLE "users" DROP COLUMN "constant_contact_dirty";
