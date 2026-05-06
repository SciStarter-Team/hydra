-- Mirror the additional fields on orrery's CountedRedirect model so
-- we can import them via the golink_records sync.
ALTER TABLE go_urls ADD COLUMN statemachine TEXT NOT NULL DEFAULT '';
ALTER TABLE go_urls ADD COLUMN transition TEXT NOT NULL DEFAULT '';
ALTER TABLE go_urls ADD COLUMN session_set_key TEXT NOT NULL DEFAULT '';
ALTER TABLE go_urls ADD COLUMN session_set_value TEXT NOT NULL DEFAULT '';
