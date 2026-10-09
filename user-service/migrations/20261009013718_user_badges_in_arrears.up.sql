-- A badge awarded late, to an account that earned it under an earlier rule
-- (orrery's UserBadge.in_arrears). Its date is when it was awarded, not when
-- it was earned, so the sync does not replay it as the event that schedules
-- follow-up nudges.
ALTER TABLE "user_badges" ADD COLUMN "in_arrears" INTEGER NOT NULL DEFAULT 0;

-- orrery began awarding badges in arrears on 2026-10-09, before the sync
-- carried the flag. Read the badges since again so that those rows get it.
UPDATE "misc" SET "value" = 0
 WHERE "key" = 'userbadge-sync-last-id'
   AND EXISTS (
       SELECT 1 FROM "misc"
        WHERE "key" = 'userbadge-sync-bookmark'
          AND "value" > '2026-10-08T00:00:00Z'
   );

UPDATE "misc" SET "value" = '2026-10-08T00:00:00Z'
 WHERE "key" = 'userbadge-sync-bookmark'
   AND "value" > '2026-10-08T00:00:00Z';
