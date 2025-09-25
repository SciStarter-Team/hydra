-- Add case-insensitive index for go_urls slugs to enable efficient case-insensitive lookups
-- This allows us to preserve the original case in storage while doing case-insensitive uniqueness checks

CREATE UNIQUE INDEX go_urls_slug_lower_unique ON go_urls (LOWER(slug));