-- Articulate workflow progress tables
-- Each table tracks a user's progress through a specific training workflow

-- Foundations of Citizen Science workflow
CREATE TABLE foundations_progress (
    user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    hour1_completed_at TEXT,  -- Completed first hour tutorial
    squirrel_completed_at TEXT,  -- Completed squirrel mapper project
    stall_completed_at TEXT,  -- Completed stall catchers project
    debris_completed_at TEXT,  -- Completed disk detector project
    badge_granted_at TEXT  -- When badge was earned
) STRICT;

-- Libraries as Community Hubs workflow
CREATE TABLE libraries_hubs_progress (
    user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    hour2_completed_at TEXT,  -- Completed second hour tutorial
    supplemental_signup_at TEXT,  -- Signed up for library network
    badge_granted_at TEXT  -- When badge was earned
) STRICT;

-- Data Literacy workflow (single step)
CREATE TABLE data_literacy_progress (
    user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    tutorial_completed_at TEXT,  -- Completed tutorial (grants badge immediately)
    badge_granted_at TEXT  -- When badge was earned
) STRICT;

-- Higher Education workflow (single step)
CREATE TABLE higher_ed_progress (
    user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    tutorial_completed_at TEXT,  -- Completed tutorial
    badge_granted_at TEXT  -- When badge was earned
) STRICT;

-- Data Ethics workflow (single step)
CREATE TABLE data_ethics_progress (
    user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    tutorial_completed_at TEXT,  -- Completed tutorial (grants badge immediately)
    badge_granted_at TEXT  -- When badge was earned
) STRICT;

-- IDEAL workflow (single step)
CREATE TABLE ideal_progress (
    user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    tutorial_completed_at TEXT,  -- Completed tutorial (grants badge immediately)
    badge_granted_at TEXT  -- When badge was earned
) STRICT;

-- Ambassador program workflow
CREATE TABLE ambassador_progress (
    user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    tutorial_completed_at TEXT,  -- Completed tutorial
    badge_granted_at TEXT  -- When badge was earned
) STRICT;

-- One Million Acts of Science 2024 workflow
CREATE TABLE one_million_2024_progress (
    user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    completed_at TEXT,  -- Completed participation
    badge_granted_at TEXT  -- When badge was earned
) STRICT;

-- One Million Acts of Science 2025 workflow
CREATE TABLE one_million_2025_progress (
    user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    completed_at TEXT,  -- Completed participation
    badge_granted_at TEXT  -- When badge was earned
) STRICT;
