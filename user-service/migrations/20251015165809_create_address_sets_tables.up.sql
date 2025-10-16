-- Address sets table
CREATE TABLE address_sets (
    slug TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Address set members table (junction table for email addresses)
CREATE TABLE address_set_members (
    address_set_slug TEXT NOT NULL,
    email_address TEXT NOT NULL,
    PRIMARY KEY (address_set_slug, email_address),
    FOREIGN KEY (address_set_slug) REFERENCES address_sets(slug) ON DELETE CASCADE
);

-- Index for faster lookups
CREATE INDEX idx_address_set_members_set ON address_set_members(address_set_slug);
