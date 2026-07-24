-- Citizen Science for K-12 Educators workflow (tutorial → education)
CREATE TABLE k12_progress (
    user_id TEXT PRIMARY KEY,
    tutorial_completed_at TEXT,  -- Completed the K-12 Educators tutorial
    education_completed_at TEXT,  -- Visited the SciStarter Education Hub
    badge_granted_at TEXT  -- When badge was earned
) STRICT;
