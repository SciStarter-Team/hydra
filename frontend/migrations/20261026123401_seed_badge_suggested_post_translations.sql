-- Spanish and Portuguese for the badge page's LinkedIn button and its
-- suggested post: the label over the post, the name of its copy button, and
-- what that button says once it has copied. They are drafts that no
-- translator has reviewed, the same wording as orrery's `src/badgetext.rs`,
-- and so are seeded as 'machine': displayed, but listed in the admin as
-- needing review.
--
-- The ids are sha256(context || 0x1f || source), as i18n::phrase_id derives
-- them. Seeding fills a translation that is blank or written by a machine,
-- and leaves one a human has written alone.

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('f90683fd2b5041b5b89a3d29538df9525a21cd7b51b6981cae678f181d2185a4', '', 'Add to profile', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('f90683fd2b5041b5b89a3d29538df9525a21cd7b51b6981cae678f181d2185a4', 'es', 'Agregar al perfil', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('f90683fd2b5041b5b89a3d29538df9525a21cd7b51b6981cae678f181d2185a4', 'pt', 'Adicionar ao perfil', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('2c74e082826c97ea0c7e7646361b3647c1c8a4ecb2dd5aace68abe72b95e39b0', '', 'Suggested post:', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('2c74e082826c97ea0c7e7646361b3647c1c8a4ecb2dd5aace68abe72b95e39b0', 'es', 'Publicación sugerida:', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('2c74e082826c97ea0c7e7646361b3647c1c8a4ecb2dd5aace68abe72b95e39b0', 'pt', 'Publicação sugerida:', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('313741f35f1858f78a91773c53bf70acc0e87753a3f77d765a07c95557083d62', '', 'Copy the suggested post', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('313741f35f1858f78a91773c53bf70acc0e87753a3f77d765a07c95557083d62', 'es', 'Copiar la publicación sugerida', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('313741f35f1858f78a91773c53bf70acc0e87753a3f77d765a07c95557083d62', 'pt', 'Copiar a publicação sugerida', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('d75ebbaa6e974878a47fe5f5233e8fd591ce46f3cd2af8d269c9dd6f927e09e9', '', 'Copied!', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('d75ebbaa6e974878a47fe5f5233e8fd591ce46f3cd2af8d269c9dd6f927e09e9', 'es', '¡Copiada!', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('d75ebbaa6e974878a47fe5f5233e8fd591ce46f3cd2af8d269c9dd6f927e09e9', 'pt', 'Copiada!', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;
