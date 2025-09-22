-- Create go_urls table for managing /go/ redirects
CREATE TABLE go_urls (
    slug TEXT PRIMARY KEY NOT NULL,
    url_type BLOB NOT NULL -- Flexbuffers-encoded GoUrlType
);