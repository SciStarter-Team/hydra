-- Add ORCID iD field to users table
-- ORCID iDs are persistent digital identifiers for researchers
-- Format: XXXX-XXXX-XXXX-XXXX (16 digits with 3 dashes)
ALTER TABLE "users" ADD COLUMN "orcid" TEXT;
