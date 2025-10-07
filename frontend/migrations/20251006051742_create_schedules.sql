-- Create schedules table for managing featured content, notifications, and advertisements
CREATE TABLE schedules (
    schedule_id TEXT PRIMARY KEY NOT NULL,
    schedule_type TEXT NOT NULL CHECK(schedule_type IN ('featured', 'notification', 'advertisement')),
    target_type TEXT CHECK(target_type IN ('slug', 'global')),
    target TEXT,
    title TEXT NOT NULL,
    content TEXT,
    url TEXT,
    start_date TEXT NOT NULL,
    end_date TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1
);

-- Index for active schedules by type
CREATE INDEX idx_schedules_type_active ON schedules(schedule_type, active);

-- Index for date range queries
CREATE INDEX idx_schedules_dates ON schedules(start_date, end_date);

-- Index for target lookups
CREATE INDEX idx_schedules_target ON schedules(target_type, target);
