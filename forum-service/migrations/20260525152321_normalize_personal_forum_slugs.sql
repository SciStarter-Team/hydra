-- Standardize personal-forum slugs on the `user-{user_id}` pattern.
-- Historically two code paths created personal forums with different slug
-- prefixes (`user-` from forums::get_personal_forum, `inbox-` from
-- messaging::ensure_personal_forum). Whichever path ran first won; the
-- inconsistency leaked `inbox-*` rows into list_forums (which only excludes
-- `user-%`) and made slug-based gating unreliable.
--
-- For every `inbox-{user_id}` row that does not yet have a `user-{user_id}`
-- twin, rename the slug. If a twin already exists (forum_id is the same
-- because forum_id == user_id), delete the redundant `inbox-` row.

DELETE FROM forums
WHERE slug LIKE 'inbox-%'
  AND EXISTS (
    SELECT 1 FROM forums other
    WHERE other.forum_id = forums.forum_id
      AND other.slug = 'user-' || substr(forums.slug, length('inbox-') + 1)
  );

UPDATE forums
SET slug = 'user-' || substr(slug, length('inbox-') + 1)
WHERE slug LIKE 'inbox-%';
