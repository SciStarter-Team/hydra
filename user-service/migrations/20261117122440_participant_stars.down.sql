ALTER TABLE "project_joins" DROP COLUMN "consented";
ALTER TABLE "project_joins" DROP COLUMN "starred";
ALTER TABLE "bookmarks" RENAME COLUMN "starred" TO "active";
