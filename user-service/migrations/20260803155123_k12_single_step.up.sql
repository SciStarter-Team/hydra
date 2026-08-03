-- The K-12 training collapsed to a single tracked step: completing the
-- tutorial now grants the badge, and the Education Hub visit and list-building
-- steps are gone from the training entirely.

-- Anyone who reached the tutorial under the old rules has now met the whole
-- requirement, so grant the badge to those still short of it.
UPDATE k12_progress
   SET badge_granted_at = tutorial_completed_at
 WHERE tutorial_completed_at IS NOT NULL
   AND badge_granted_at IS NULL;

ALTER TABLE k12_progress DROP COLUMN education_completed_at;
ALTER TABLE k12_progress DROP COLUMN list_completed_at;
