-- Add forum categories table for managing predefined categories per forum

-- Forum categories table - stores available categories for each forum
CREATE TABLE IF NOT EXISTS "forum_categories" (
    "category_id" TEXT PRIMARY KEY,
    "forum_id" TEXT NOT NULL,
    "category_name" TEXT NOT NULL,
    "is_active" INTEGER NOT NULL DEFAULT 1,
    "created_at" TEXT NOT NULL DEFAULT (datetime('now')),
    "updated_at" TEXT NULL,
    FOREIGN KEY ("forum_id") REFERENCES "forums" ("forum_id"),
    UNIQUE ("forum_id", "category_name")
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS "idx_forum_categories_forum" ON "forum_categories" ("forum_id");
CREATE INDEX IF NOT EXISTS "idx_forum_categories_active" ON "forum_categories" ("is_active");