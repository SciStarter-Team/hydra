-- Backfill workflow progress records for users who already have Articulate badges
-- This migration creates workflow records based on existing badge grants

-- Foundations of Citizen Science (6Jg_IwmfQTaJ2kUNePsO5Q)
-- When badge is granted, mark all steps as completed
INSERT INTO foundations_progress (user_id, hour1_completed_at, squirrel_completed_at, stall_completed_at, debris_completed_at, badge_granted_at)
SELECT
    user_id,
    "when" as hour1_completed_at,
    "when" as squirrel_completed_at,
    "when" as stall_completed_at,
    "when" as debris_completed_at,
    "when" as badge_granted_at
FROM user_badges
WHERE badgeclass = '6Jg_IwmfQTaJ2kUNePsO5Q'
ON CONFLICT (user_id) DO NOTHING;

-- Libraries as Hubs for Citizen Science (CWZrsNMPSZSHqKJoz8pLMQ)
INSERT INTO libraries_hubs_progress (user_id, hour2_completed_at, supplemental_signup_at, badge_granted_at)
SELECT
    user_id,
    "when" as hour2_completed_at,
    "when" as supplemental_signup_at,
    "when" as badge_granted_at
FROM user_badges
WHERE badgeclass = 'CWZrsNMPSZSHqKJoz8pLMQ'
ON CONFLICT (user_id) DO NOTHING;

-- Building Data Literacy (e_t41dudSc6dXymoEa8iNw)
INSERT INTO data_literacy_progress (user_id, tutorial_completed_at, badge_granted_at)
SELECT
    user_id,
    "when" as tutorial_completed_at,
    "when" as badge_granted_at
FROM user_badges
WHERE badgeclass = 'e_t41dudSc6dXymoEa8iNw'
ON CONFLICT (user_id) DO NOTHING;

-- Teaching in Higher Education (OP_wrFm7QauZgaXrZY-DhA)
INSERT INTO higher_ed_progress (user_id, tutorial_completed_at, badge_granted_at)
SELECT
    user_id,
    "when" as tutorial_completed_at,
    "when" as badge_granted_at
FROM user_badges
WHERE badgeclass = 'OP_wrFm7QauZgaXrZY-DhA'
ON CONFLICT (user_id) DO NOTHING;

-- Data Ethics for Practitioners (1OeTfgY1Q-exWqz---kPAQ)
INSERT INTO data_ethics_progress (user_id, tutorial_completed_at, badge_granted_at)
SELECT
    user_id,
    "when" as tutorial_completed_at,
    "when" as badge_granted_at
FROM user_badges
WHERE badgeclass = '1OeTfgY1Q-exWqz---kPAQ'
ON CONFLICT (user_id) DO NOTHING;

-- IDEAL Participatory Science (a8cFJfeBTRec05glGM3_9w)
INSERT INTO ideal_progress (user_id, tutorial_completed_at, badge_granted_at)
SELECT
    user_id,
    "when" as tutorial_completed_at,
    "when" as badge_granted_at
FROM user_badges
WHERE badgeclass = 'a8cFJfeBTRec05glGM3_9w'
ON CONFLICT (user_id) DO NOTHING;

-- SciStarter Ambassador (9csPkdhzQLSZuk-OWREaKg)
INSERT INTO ambassador_progress (user_id, tutorial_completed_at, badge_granted_at)
SELECT
    user_id,
    "when" as tutorial_completed_at,
    "when" as badge_granted_at
FROM user_badges
WHERE badgeclass = '9csPkdhzQLSZuk-OWREaKg'
ON CONFLICT (user_id) DO NOTHING;

-- One Million Acts of Science 2024 (QLLs6Nm9TfqIj3_722yFQg)
INSERT INTO one_million_2024_progress (user_id, completed_at, badge_granted_at)
SELECT
    user_id,
    "when" as completed_at,
    "when" as badge_granted_at
FROM user_badges
WHERE badgeclass = 'QLLs6Nm9TfqIj3_722yFQg'
ON CONFLICT (user_id) DO NOTHING;

-- One Million Acts of Science 2025 (U431wPFaTaKwUQO0WhQJRg)
INSERT INTO one_million_2025_progress (user_id, completed_at, badge_granted_at)
SELECT
    user_id,
    "when" as completed_at,
    "when" as badge_granted_at
FROM user_badges
WHERE badgeclass = 'U431wPFaTaKwUQO0WhQJRg'
ON CONFLICT (user_id) DO NOTHING;
