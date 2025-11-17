-- Rollback workflow progress backfill
-- Remove workflow records that were created from badge grants

-- Only delete records where badge_granted_at matches a user_badges.when timestamp
-- This ensures we only remove backfilled records, not organically created ones

DELETE FROM foundations_progress
WHERE EXISTS (
    SELECT 1 FROM user_badges
    WHERE user_badges.user_id = foundations_progress.user_id
    AND user_badges.badgeclass = '6Jg_IwmfQTaJ2kUNePsO5Q'
    AND user_badges."when" = foundations_progress.badge_granted_at
    AND foundations_progress.hour1_completed_at = foundations_progress.badge_granted_at
    AND foundations_progress.squirrel_completed_at = foundations_progress.badge_granted_at
);

DELETE FROM libraries_hubs_progress
WHERE EXISTS (
    SELECT 1 FROM user_badges
    WHERE user_badges.user_id = libraries_hubs_progress.user_id
    AND user_badges.badgeclass = 'CWZrsNMPSZSHqKJoz8pLMQ'
    AND user_badges."when" = libraries_hubs_progress.badge_granted_at
    AND libraries_hubs_progress.hour2_completed_at = libraries_hubs_progress.badge_granted_at
);

DELETE FROM data_literacy_progress
WHERE EXISTS (
    SELECT 1 FROM user_badges
    WHERE user_badges.user_id = data_literacy_progress.user_id
    AND user_badges.badgeclass = 'e_t41dudSc6dXymoEa8iNw'
    AND user_badges."when" = data_literacy_progress.badge_granted_at
);

DELETE FROM higher_ed_progress
WHERE EXISTS (
    SELECT 1 FROM user_badges
    WHERE user_badges.user_id = higher_ed_progress.user_id
    AND user_badges.badgeclass = 'OP_wrFm7QauZgaXrZY-DhA'
    AND user_badges."when" = higher_ed_progress.badge_granted_at
);

DELETE FROM data_ethics_progress
WHERE EXISTS (
    SELECT 1 FROM user_badges
    WHERE user_badges.user_id = data_ethics_progress.user_id
    AND user_badges.badgeclass = '1OeTfgY1Q-exWqz---kPAQ'
    AND user_badges."when" = data_ethics_progress.badge_granted_at
);

DELETE FROM ideal_progress
WHERE EXISTS (
    SELECT 1 FROM user_badges
    WHERE user_badges.user_id = ideal_progress.user_id
    AND user_badges.badgeclass = 'a8cFJfeBTRec05glGM3_9w'
    AND user_badges."when" = ideal_progress.badge_granted_at
);

DELETE FROM ambassador_progress
WHERE EXISTS (
    SELECT 1 FROM user_badges
    WHERE user_badges.user_id = ambassador_progress.user_id
    AND user_badges.badgeclass = '9csPkdhzQLSZuk-OWREaKg'
    AND user_badges."when" = ambassador_progress.badge_granted_at
);

DELETE FROM one_million_2024_progress
WHERE EXISTS (
    SELECT 1 FROM user_badges
    WHERE user_badges.user_id = one_million_2024_progress.user_id
    AND user_badges.badgeclass = 'QLLs6Nm9TfqIj3_722yFQg'
    AND user_badges."when" = one_million_2024_progress.badge_granted_at
);

DELETE FROM one_million_2025_progress
WHERE EXISTS (
    SELECT 1 FROM user_badges
    WHERE user_badges.user_id = one_million_2025_progress.user_id
    AND user_badges.badgeclass = 'U431wPFaTaKwUQO0WhQJRg'
    AND user_badges."when" = one_million_2025_progress.badge_granted_at
);
