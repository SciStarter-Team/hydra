-- Add case-insensitive index for entity slugs to enable efficient case-insensitive lookups
-- This allows us to preserve the original case in storage while doing case-insensitive uniqueness checks

-- First drop the existing unique index
DROP INDEX "entities_by_slug";

-- Create a new unique case-insensitive index
CREATE UNIQUE INDEX "entities_by_slug_lower" ON "entities" (LOWER("slug"));

-- Create a regular case-sensitive index for exact lookups (if needed for performance)
CREATE INDEX "entities_by_slug_exact" ON "entities" ("slug");