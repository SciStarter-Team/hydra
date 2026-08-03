-- Building a SciStarter list is now a required K-12 step, and it is the one
-- that grants the badge (previously the Education Hub visit did).
ALTER TABLE k12_progress ADD COLUMN list_completed_at TEXT;  -- Built a SciStarter list

-- Anyone already badged under the old rules reached the end of the training as
-- it then stood, so credit them with the step rather than revoking the badge.
UPDATE k12_progress
   SET list_completed_at = badge_granted_at
 WHERE badge_granted_at IS NOT NULL;
