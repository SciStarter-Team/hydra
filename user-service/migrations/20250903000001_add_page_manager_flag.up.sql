-- Add page manager flag to users table
ALTER TABLE "users" ADD COLUMN "is_page_manager" integer NOT NULL DEFAULT 0;