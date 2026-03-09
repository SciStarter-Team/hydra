CREATE INDEX IF NOT EXISTS idx_users_inat_username ON users(inat_username) WHERE inat_username != '' AND inat_username IS NOT NULL;
