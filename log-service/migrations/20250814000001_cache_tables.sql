-- Cache tables for user tags and email addresses
-- Following the same patterns as user-service

-- User tags cache table
CREATE TABLE IF NOT EXISTS "user_tags_cache" (
    "user_id" TEXT NOT NULL,
    "tag" TEXT NOT NULL,
    "updated_at" TEXT NOT NULL,
    PRIMARY KEY ("user_id", "tag")
) STRICT;

-- User emails cache table  
-- Multiple emails per user_id allowed, but each email must be unique
-- Each user_id/email pair must be unique
CREATE TABLE IF NOT EXISTS "user_emails_cache" (
    "user_id" TEXT NOT NULL,
    "email" TEXT NOT NULL,
    "updated_at" TEXT NOT NULL,
    PRIMARY KEY ("user_id", "email")
) STRICT;

-- Create indexes for performance
CREATE INDEX IF NOT EXISTS "idx_user_tags_cache_user_id" 
   ON "user_tags_cache" ("user_id");

CREATE INDEX IF NOT EXISTS "idx_user_tags_cache_tag" 
   ON "user_tags_cache" ("tag");

-- Email table indexes
CREATE UNIQUE INDEX IF NOT EXISTS "idx_user_emails_cache_email" 
   ON "user_emails_cache" ("email");

CREATE INDEX IF NOT EXISTS "idx_user_emails_cache_user_id" 
   ON "user_emails_cache" ("user_id");