ALTER TABLE "ambassador_progress" ADD COLUMN "state" TEXT NOT NULL DEFAULT 'initial';
ALTER TABLE "ambassador_progress" ADD COLUMN "state_changed" TEXT NOT NULL DEFAULT '';

-- Recover what the stored mirror would have held, from the milestones it was
-- meant to mirror. This is the derivation the code now does at read time.
UPDATE "ambassador_progress"
   SET "state" = COALESCE(
        (SELECT CASE
                  WHEN a."app_status" = 5 THEN 'tutorial'
                  WHEN a."app_status" = 2 THEN 'rejected'
                  WHEN a."screening_status" = 4 THEN 'international'
                  WHEN a."screening_status" = 3 THEN 'failed'
                  WHEN a."verified" IS NOT NULL AND a."screening_status" = 1
                       AND (SELECT u."ambassador" FROM "users" u
                             WHERE u."user_id" = "ambassador_progress"."user_id") IS NOT NULL
                       THEN 'passed'
                  WHEN a."verified" IS NOT NULL THEN 'verified'
                  WHEN a."draft" = 0 AND a."submitted" IS NOT NULL THEN 'application'
                END
           FROM "ambassador_applications" a
          WHERE a."user_id" = "ambassador_progress"."user_id"),
        CASE WHEN "tutorial_completed_at" IS NOT NULL THEN 'tutorial' ELSE 'initial' END
       ),
       "state_changed" = COALESCE("badge_granted_at", "tutorial_completed_at", '');

ALTER TABLE "ambassador_progress" DROP COLUMN "rejected_at";
ALTER TABLE "ambassador_progress" DROP COLUMN "international_at";
ALTER TABLE "ambassador_progress" DROP COLUMN "screening_failed_at";
