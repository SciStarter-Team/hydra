-- Standardize personal-forum slugs on the `inbox-{id}` pattern, where `id`
-- is the recipient's ULID (a user, project, or event — all addressable the
-- same way). This reverses 20260525152321, which had converged on `user-`;
-- `inbox-` is the chosen convention because it is recipient-kind-neutral.
--
-- Favor existing `inbox-*` rows: where an `inbox-{id}` twin already exists
-- (same forum_id), delete the redundant `user-{id}` row; then rename any
-- remaining lone `user-*` slug to `inbox-*`.

DELETE FROM forums
WHERE slug LIKE 'user-%'
  AND EXISTS (
    SELECT 1 FROM forums other
    WHERE other.forum_id = forums.forum_id
      AND other.slug = 'inbox-' || substr(forums.slug, length('user-') + 1)
  );

UPDATE forums
SET slug = 'inbox-' || substr(slug, length('user-') + 1)
WHERE slug LIKE 'user-%';
