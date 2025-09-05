-- Remove performance indexes

DROP INDEX IF EXISTS "users_admin_search";
DROP INDEX IF EXISTS "users_by_first_name";
DROP INDEX IF EXISTS "users_by_last_name";
DROP INDEX IF EXISTS "users_by_handle";
DROP INDEX IF EXISTS "emails_by_user_verified";
DROP INDEX IF EXISTS "users_by_date_joined";
