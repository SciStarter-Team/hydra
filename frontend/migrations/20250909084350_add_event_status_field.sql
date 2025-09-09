-- Add status field to events table, defaulting existing events to 'Active'
ALTER TABLE events ADD COLUMN status TEXT NOT NULL DEFAULT 'Active';
