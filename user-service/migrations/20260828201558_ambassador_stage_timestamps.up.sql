-- The Ambassador funnel stage is derived, not stored.
--
-- `ambassador_progress` briefly carried a `state` string mirroring orrery's
-- `AmbassadorState.state`, alone among the workflow tables: every other one
-- represents progression as a timestamp per milestone. The mirror was a fourth
-- copy of what the application row, the directory date and the tutorial stamp
-- already record between them, and being derived-but-stored it drifted — added
-- with `DEFAULT 'initial'` and no backfill, on a table fed by a bookmarked
-- sync that had already walked past every existing row, it left five hundred
-- finished Champions and a hundred and sixty Ambassadors reading as `initial`.
--
-- The stage is now computed from the milestones, so it cannot disagree with
-- them. Three terminal milestones had no timestamp anywhere; they get one here,
-- each doubling as the once-only guard for its entry email, the way
-- `tutorial_completed_at` guards the training email.
ALTER TABLE "ambassador_progress" ADD COLUMN "rejected_at" TEXT;
ALTER TABLE "ambassador_progress" ADD COLUMN "international_at" TEXT;
ALTER TABLE "ambassador_progress" ADD COLUMN "screening_failed_at" TEXT;

-- Seed them from the verdict already recorded on the application, so that a
-- staff re-review of somebody long since turned down does not read as a fresh
-- transition and re-send the email orrery already sent. `updated_at` is when
-- the application last changed, which is the closest date on record; rows with
-- no application row have reached no terminal stage and stay empty.
UPDATE "ambassador_progress"
   SET "rejected_at" = (SELECT a."updated_at" FROM "ambassador_applications" a
                         WHERE a."user_id" = "ambassador_progress"."user_id")
 WHERE EXISTS (SELECT 1 FROM "ambassador_applications" a
                WHERE a."user_id" = "ambassador_progress"."user_id"
                  AND a."app_status" = 2);

UPDATE "ambassador_progress"
   SET "international_at" = (SELECT a."updated_at" FROM "ambassador_applications" a
                              WHERE a."user_id" = "ambassador_progress"."user_id")
 WHERE EXISTS (SELECT 1 FROM "ambassador_applications" a
                WHERE a."user_id" = "ambassador_progress"."user_id"
                  AND a."screening_status" = 4);

UPDATE "ambassador_progress"
   SET "screening_failed_at" = (SELECT a."updated_at" FROM "ambassador_applications" a
                                 WHERE a."user_id" = "ambassador_progress"."user_id")
 WHERE EXISTS (SELECT 1 FROM "ambassador_applications" a
                WHERE a."user_id" = "ambassador_progress"."user_id"
                  AND a."screening_status" = 3);

ALTER TABLE "ambassador_progress" DROP COLUMN "state";
ALTER TABLE "ambassador_progress" DROP COLUMN "state_changed";
