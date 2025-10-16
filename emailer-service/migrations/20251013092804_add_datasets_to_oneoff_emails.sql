-- Add datasets column to oneoff_emails table
-- Datasets are stored as JSON array of dataset names
ALTER TABLE oneoff_emails ADD COLUMN datasets TEXT NOT NULL DEFAULT '[]';
