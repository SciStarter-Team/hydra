-- Convert existing SQLite datetime format timestamps to ISO format with UTC timezone
-- This assumes existing timestamps are UTC (which is the SQLite default)

-- Update forums table timestamps
UPDATE "forums" 
SET "created_at" = "created_at" || 'Z'
WHERE "created_at" NOT LIKE '%Z' AND "created_at" NOT LIKE '%+%' AND "created_at" NOT LIKE '%-%.%:%';

UPDATE "forums"
SET "updated_at" = "updated_at" || 'Z'
WHERE "updated_at" IS NOT NULL 
  AND "updated_at" NOT LIKE '%Z' 
  AND "updated_at" NOT LIKE '%+%' 
  AND "updated_at" NOT LIKE '%-%.%:%';

-- Update topics table timestamps
UPDATE "topics"
SET "created_at" = "created_at" || 'Z'
WHERE "created_at" NOT LIKE '%Z' AND "created_at" NOT LIKE '%+%' AND "created_at" NOT LIKE '%-%.%:%';

UPDATE "topics"
SET "updated_at" = "updated_at" || 'Z'
WHERE "updated_at" IS NOT NULL 
  AND "updated_at" NOT LIKE '%Z' 
  AND "updated_at" NOT LIKE '%+%' 
  AND "updated_at" NOT LIKE '%-%.%:%';

-- Update posts table timestamps
UPDATE "posts"
SET "created_at" = "created_at" || 'Z'
WHERE "created_at" NOT LIKE '%Z' AND "created_at" NOT LIKE '%+%' AND "created_at" NOT LIKE '%-%.%:%';

UPDATE "posts"
SET "updated_at" = "updated_at" || 'Z'
WHERE "updated_at" IS NOT NULL 
  AND "updated_at" NOT LIKE '%Z' 
  AND "updated_at" NOT LIKE '%+%' 
  AND "updated_at" NOT LIKE '%-%.%:%';

-- Update moderation_log table timestamps
UPDATE "moderation_log"
SET "created_at" = "created_at" || 'Z'
WHERE "created_at" NOT LIKE '%Z' AND "created_at" NOT LIKE '%+%' AND "created_at" NOT LIKE '%-%.%:%';

-- Update bad_words table timestamps
UPDATE "bad_words"
SET "created_at" = "created_at" || 'Z'
WHERE "created_at" NOT LIKE '%Z' AND "created_at" NOT LIKE '%+%' AND "created_at" NOT LIKE '%-%.%:%';