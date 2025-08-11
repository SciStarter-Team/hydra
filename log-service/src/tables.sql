-- Activity logging table
CREATE TABLE IF NOT EXISTS activity_log (
    when_timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    action VARCHAR NOT NULL,
    user_id VARCHAR,
    target VARCHAR,
    extra VARCHAR
);

CREATE INDEX IF NOT EXISTS activity_log_by_when ON activity_log (when_timestamp);
CREATE INDEX IF NOT EXISTS activity_log_by_action ON activity_log (action);
CREATE INDEX IF NOT EXISTS activity_log_by_user_id ON activity_log (user_id);
CREATE INDEX IF NOT EXISTS activity_log_by_target ON activity_log (target);

-- Page view logging table
CREATE TABLE IF NOT EXISTS page_views (
    when_timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ip_address VARCHAR NOT NULL,
    session_id VARCHAR NOT NULL,
    interaction_id VARCHAR NOT NULL, -- Unique identifier for this interaction
    user_id VARCHAR, -- Can be null for anonymous users
    path VARCHAR NOT NULL,
    referrer VARCHAR, -- Can be null for direct visits
    query VARCHAR, -- Can be null if no query params
    geolocation VARCHAR NOT NULL -- GeoJSON point string
);

CREATE INDEX IF NOT EXISTS page_views_by_when ON page_views (when_timestamp);
CREATE INDEX IF NOT EXISTS page_views_by_session ON page_views (session_id);
CREATE INDEX IF NOT EXISTS page_views_by_interaction ON page_views (interaction_id);
CREATE INDEX IF NOT EXISTS page_views_by_user ON page_views (user_id);
CREATE INDEX IF NOT EXISTS page_views_by_path ON page_views (path);
CREATE INDEX IF NOT EXISTS page_views_by_ip ON page_views (ip_address);

-- Clickthrough tracking table
CREATE TABLE IF NOT EXISTS clickthroughs (
    when_timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ip_address VARCHAR NOT NULL,
    session_id VARCHAR NOT NULL,
    interaction_id VARCHAR NOT NULL, -- Unique identifier for this interaction
    user_id VARCHAR, -- Can be null for anonymous users
    from_path VARCHAR NOT NULL,
    to_url VARCHAR NOT NULL,
    link_type VARCHAR NOT NULL, -- 'project_page', 'actual_project', etc.
    program_id VARCHAR, -- Can be null for non-program links
    geolocation VARCHAR NOT NULL -- GeoJSON point string
);

CREATE INDEX IF NOT EXISTS clickthroughs_by_when ON clickthroughs (when_timestamp);
CREATE INDEX IF NOT EXISTS clickthroughs_by_session ON clickthroughs (session_id);
CREATE INDEX IF NOT EXISTS clickthroughs_by_interaction ON clickthroughs (interaction_id);
CREATE INDEX IF NOT EXISTS clickthroughs_by_user ON clickthroughs (user_id);
CREATE INDEX IF NOT EXISTS clickthroughs_by_link_type ON clickthroughs (link_type);
CREATE INDEX IF NOT EXISTS clickthroughs_by_program ON clickthroughs (program_id);
CREATE INDEX IF NOT EXISTS clickthroughs_by_from_path ON clickthroughs (from_path);
CREATE INDEX IF NOT EXISTS clickthroughs_by_ip ON clickthroughs (ip_address);

-- User tags cache table (normalized)
CREATE TABLE IF NOT EXISTS user_tags (
    user_id VARCHAR NOT NULL,
    tag VARCHAR NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, tag)
);

CREATE INDEX IF NOT EXISTS user_tags_by_user ON user_tags (user_id);
CREATE INDEX IF NOT EXISTS user_tags_by_tag ON user_tags (tag);
CREATE INDEX IF NOT EXISTS user_tags_by_updated ON user_tags (updated_at);

-- Affiliate events table is created by migration system

