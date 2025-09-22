-- Create uploaded_files table for tracking file uploads
CREATE TABLE uploaded_files (
    file_id TEXT PRIMARY KEY NOT NULL,
    original_filename TEXT,
    content_type TEXT,
    size INTEGER NOT NULL,
    uploaded_by TEXT NOT NULL,
    access TEXT NOT NULL CHECK (access IN ('tracked', 'untracked')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    variant TEXT, -- For storing processed file variants like 'thumbnail', 'webp_version', etc.
    friendly_url TEXT UNIQUE -- Optional friendly URL slug for tracked files (e.g., 'kittens-and-puppies')
);

-- Index for finding files by user
CREATE INDEX idx_uploaded_files_uploaded_by ON uploaded_files(uploaded_by);

-- Index for finding files by access type
CREATE INDEX idx_uploaded_files_access ON uploaded_files(access);

-- Index for ordering by creation time
CREATE INDEX idx_uploaded_files_created_at ON uploaded_files(created_at);

-- Index for finding variants of original files
CREATE INDEX idx_uploaded_files_variant ON uploaded_files(variant);

-- Composite index for finding specific variant of a file
CREATE INDEX idx_uploaded_files_file_id_variant ON uploaded_files(file_id, variant);

-- Index for friendly URL lookups
CREATE INDEX idx_uploaded_files_friendly_url ON uploaded_files(friendly_url) WHERE friendly_url IS NOT NULL;