-- Add operation field to user_set_criteria table
-- Values: 'union' (default, OR) or 'intersect' (AND)
ALTER TABLE user_set_criteria ADD COLUMN operation TEXT NOT NULL DEFAULT 'union';
