-- Add datasets column to stored_emails table
-- Datasets are stored as JSON array of dataset names
ALTER TABLE stored_emails ADD COLUMN datasets TEXT NOT NULL DEFAULT '[]';
