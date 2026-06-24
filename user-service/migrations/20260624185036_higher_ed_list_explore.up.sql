-- Track the Higher Education "list" and "explore" steps separately, matching
-- orrery's HigherEdState machine (initial → tutorial → list → explore, where
-- reaching explore grants the badge). Previously these orrery states were
-- collapsed onto tutorial_completed_at/badge_granted_at, which lost the list and
-- explore progress.
ALTER TABLE higher_ed_progress ADD COLUMN list_completed_at TEXT;
ALTER TABLE higher_ed_progress ADD COLUMN explore_completed_at TEXT;
