-- Add featured_entity column to schedules table
-- This references the entity slug of what is being featured (separate from target which is where it appears)
ALTER TABLE schedules ADD COLUMN featured_entity TEXT;

-- Index for looking up schedules by featured entity
CREATE INDEX idx_schedules_featured_entity ON schedules(featured_entity);
