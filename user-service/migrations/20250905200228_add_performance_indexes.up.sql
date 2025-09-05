-- Add performance indexes for common query patterns

-- Admin search optimization - composite index for common filters
CREATE INDEX "users_admin_search" ON "users" ("is_staff", "is_page_manager", "verified", "date_joined" DESC);

-- Individual indexes for LIKE searches in admin search (SQLite can use these for prefix matches)
CREATE INDEX "users_by_first_name" ON "users" ("first_name");
CREATE INDEX "users_by_last_name" ON "users" ("last_name"); 
CREATE INDEX "users_by_handle" ON "users" ("handle");

-- Email verification queries optimization
CREATE INDEX "emails_by_user_verified" ON "emails" ("user_id", "verified");

-- User list pagination optimization  
CREATE INDEX "users_by_date_joined" ON "users" ("date_joined" DESC);
