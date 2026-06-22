-- Preserve the human-readable resource name alongside the URL, matching
-- orrery's name -> URL participant_files model (core_multipleurls keyed by
-- name). Existing rows get an empty name, which the UI renders as the URL.
ALTER TABLE project_participant_files ADD COLUMN name TEXT NOT NULL DEFAULT '';
