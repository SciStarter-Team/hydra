CREATE TABLE IF NOT EXISTS csm_staff_picks (
    id TEXT PRIMARY KEY NOT NULL,
    project_slug TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    location TEXT NOT NULL DEFAULT '',
    acts_per_hour TEXT NOT NULL DEFAULT '',
    picker_name TEXT NOT NULL,
    picker_image TEXT NOT NULL DEFAULT '',
    picker_quote TEXT NOT NULL DEFAULT '',
    priority INTEGER NOT NULL DEFAULT 0,
    begin_date TEXT NOT NULL,
    end_date TEXT NOT NULL
);

CREATE INDEX idx_csm_staff_picks_dates ON csm_staff_picks(begin_date, end_date);
