-- Staff's leave for an unverified account to have its badges handed out.
--
-- A badge is earned whether or not the account is verified, and shows on
-- the earner's profile either way, but the credential itself -- the badge
-- page, the baked image and the assertion -- is withheld until the account
-- is verified or this is set.
--
-- Orrery keeps it as the `badge-allowed` UserFlag, and the user sync
-- carries it here as a column.
ALTER TABLE users ADD COLUMN badge_allowed BOOLEAN NOT NULL DEFAULT 0;
