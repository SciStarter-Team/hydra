-- Add subject column to send_queue table
ALTER TABLE send_queue ADD COLUMN subject TEXT NOT NULL DEFAULT '';
