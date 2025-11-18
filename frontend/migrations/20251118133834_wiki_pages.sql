-- Wiki pages table for admin wiki functionality
CREATE TABLE wiki_pages (
    title TEXT PRIMARY KEY,
    content TEXT NOT NULL
) STRICT;
