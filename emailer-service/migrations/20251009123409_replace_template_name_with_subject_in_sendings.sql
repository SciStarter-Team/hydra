-- Replace template_name with subject in sendings table
ALTER TABLE sendings RENAME COLUMN template_name TO subject;
