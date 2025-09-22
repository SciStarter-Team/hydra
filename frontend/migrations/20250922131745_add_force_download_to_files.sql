-- Add force_download column to uploaded_files table
ALTER TABLE uploaded_files ADD COLUMN force_download BOOLEAN NOT NULL DEFAULT FALSE;
