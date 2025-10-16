-- Remove operation field from user_set_criteria table
-- Note: SQLite doesn't support DROP COLUMN directly in older versions,
-- but modern SQLite (3.35.0+) does support it
ALTER TABLE user_set_criteria DROP COLUMN operation;
