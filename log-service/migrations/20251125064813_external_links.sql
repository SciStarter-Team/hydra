-- External links table
-- Maps URLs to ULIDs for tracking external link clicks

CREATE TABLE external_links (
    link_id TEXT PRIMARY KEY NOT NULL,  -- ULID
    url TEXT UNIQUE NOT NULL,            -- The external URL
    created_at TEXT NOT NULL             -- When this link was first registered
) STRICT;

-- Index for fast URL lookups
CREATE INDEX external_links_by_url ON external_links(url);
