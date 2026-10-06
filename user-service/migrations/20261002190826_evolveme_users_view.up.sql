-- The accounts that came through EvolveMe, an external rewards platform whose
-- tasks (the Foundations lesson quiz among them) send people to sign up.
-- Growth reports leave them out, and the Foundations report shows them apart,
-- so the rule for recognizing one lives here once, for every query that needs
-- it. It is the same rule as orrery's `evolveme_user` view.
--
-- An account is EvolveMe's if it was tagged so at signup, carries its origin,
-- or arrived from one of its task pages without getting the tag: thousands
-- did, in Apr-May 2024 and again from Jul 2025. SQLite's LIKE is
-- case-insensitive for ASCII, as orrery's ILIKE is.
CREATE VIEW "evolveme_users" AS
SELECT u."user_id"
  FROM "users" u
 WHERE u."origin" LIKE 'evolveme'
    OR u."referer" LIKE 'http://scistarter.org/evolveme/%'
    OR u."referer" LIKE 'https://scistarter.org/evolveme/%'
    OR u."referer" LIKE 'http://www.scistarter.org/evolveme/%'
    OR u."referer" LIKE 'https://www.scistarter.org/evolveme/%'
    OR EXISTS (
        SELECT 1 FROM "user_tags" t
         WHERE t."user_id" = u."user_id" AND t."tag" = 'evolveme'
    );
