-- Add additional_data column to send_queue table to store custom merge fields
ALTER TABLE send_queue ADD COLUMN additional_data TEXT NOT NULL DEFAULT '[]';
