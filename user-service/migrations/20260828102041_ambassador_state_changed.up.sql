-- When the ambassador state machine last moved this user.
--
-- orrery's `AmbassadorState` carries a Django `auto_now` field of the same
-- name, and the staff funnel exports report it as the person's "since" date —
-- for the many who finish Champion training and stop there, it is the date
-- they finished. hydra recorded the state but not when it was reached, so the
-- exports had no such column to report.
--
-- Rows that predate this column have no transition on record. Rather than
-- inventing one, they are seeded from whatever the progress row already dates:
-- the badge award, else the tutorial completion, else left empty, which the
-- exports render as an unknown date.
ALTER TABLE "ambassador_progress" ADD COLUMN "state_changed" TEXT NOT NULL DEFAULT '';

UPDATE "ambassador_progress"
   SET "state_changed" = COALESCE("badge_granted_at", "tutorial_completed_at", '')
 WHERE "state_changed" = '';
