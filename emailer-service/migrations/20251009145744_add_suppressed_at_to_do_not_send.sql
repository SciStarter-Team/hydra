-- Add suppressed_at column to do_not_send table
ALTER TABLE do_not_send ADD COLUMN suppressed_at TEXT NOT NULL DEFAULT '';
