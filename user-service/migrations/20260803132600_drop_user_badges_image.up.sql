-- The badge image column carried a Badgr-hosted URL through the orrery sync.
-- Badge images are now derived from the badge class, so nothing writes or
-- reads it, and orrery has stopped sending the field.
ALTER TABLE "user_badges" DROP COLUMN "image";
