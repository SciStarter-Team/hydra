-- A file can be uploaded by someone not signed in: a photo for a hosted form
-- that takes submissions from anyone. Its `uploaded_by` is then NULL.

CREATE TABLE "uploaded_files_new" (
    file_id TEXT PRIMARY KEY NOT NULL,
    original_filename TEXT,
    content_type TEXT,
    size INTEGER NOT NULL,
    uploaded_by TEXT,
    access TEXT NOT NULL CHECK (access IN ('tracked', 'untracked')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    variant TEXT, -- For storing processed file variants like 'thumbnail', 'webp_version', etc.
    friendly_url TEXT UNIQUE, -- Optional friendly URL slug for tracked files (e.g., 'kittens-and-puppies')
    force_download BOOLEAN NOT NULL DEFAULT FALSE
);

INSERT INTO "uploaded_files_new"
    (file_id, original_filename, content_type, size, uploaded_by, access, created_at,
     variant, friendly_url, force_download)
SELECT file_id, original_filename, content_type, size, uploaded_by, access, created_at,
       variant, friendly_url, force_download
FROM "uploaded_files";

DROP TABLE "uploaded_files";
ALTER TABLE "uploaded_files_new" RENAME TO "uploaded_files";

CREATE INDEX idx_uploaded_files_uploaded_by ON uploaded_files(uploaded_by);
CREATE INDEX idx_uploaded_files_access ON uploaded_files(access);
CREATE INDEX idx_uploaded_files_created_at ON uploaded_files(created_at);
CREATE INDEX idx_uploaded_files_variant ON uploaded_files(variant);
CREATE INDEX idx_uploaded_files_file_id_variant ON uploaded_files(file_id, variant);
CREATE INDEX idx_uploaded_files_friendly_url ON uploaded_files(friendly_url) WHERE friendly_url IS NOT NULL;
