-- Add retry_count field to track failed attempts
ALTER TABLE send_queue ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
