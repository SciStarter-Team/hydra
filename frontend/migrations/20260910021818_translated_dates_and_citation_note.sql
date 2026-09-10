-- Dates in the reader's own language, and the citation note the translated
-- badge pages no longer carry.
--
-- `strftime`'s `%B` names months in English whatever the page is rendered in,
-- so `i18n::format_date` translates the month like any other phrase and puts
-- the whole date through a pattern -- Spanish and Portuguese join the parts
-- with "de", so the word order is not the only difference. The site would
-- discover these phrases for itself the first time a badge page is read, but
-- they would sit untranslated until someone filled them in, so they are
-- seeded here with the rest of the badge-page wording.
--
-- The months carry the `month` context because several of their English
-- spellings are ordinary words as well ("May", "March"): a translator seeing
-- them bare cannot tell which sense is meant.
--
-- Then: Roland's translations introduced a sentence the English original
-- never had, saying the bibliography is quoted in its published language.
-- Daniel asked for it to go, which also puts the three pages back to saying
-- the same things as each other.
--
-- The ids are sha256(context || 0x1f || source), the same derivation
-- i18n::phrase_id performs, so these rows are the phrases the site discovers
-- for itself. Seeding fills blanks only: a phrase already carrying a
-- translation is left alone.

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('93715e365496f4621dcbccf7ca5bc2c59fbd39836fb119c3bbd32751f34bf6b7', 'date', '{day} {month} {year}', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('93715e365496f4621dcbccf7ca5bc2c59fbd39836fb119c3bbd32751f34bf6b7', 'es', '{day} de {month} de {year}', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('93715e365496f4621dcbccf7ca5bc2c59fbd39836fb119c3bbd32751f34bf6b7', 'pt', '{day} de {month} de {year}', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('aa81d6eab504d14d1e2cc89c3662c2c72e4ec0bbb4bdf319f1a6ec7c7c004569', 'month', 'January', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('aa81d6eab504d14d1e2cc89c3662c2c72e4ec0bbb4bdf319f1a6ec7c7c004569', 'es', 'enero', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('aa81d6eab504d14d1e2cc89c3662c2c72e4ec0bbb4bdf319f1a6ec7c7c004569', 'pt', 'janeiro', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('6b985dd0038bae35ea1cb06fdf125728f86edb71e126f4b542a835b59c8b7977', 'month', 'February', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('6b985dd0038bae35ea1cb06fdf125728f86edb71e126f4b542a835b59c8b7977', 'es', 'febrero', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('6b985dd0038bae35ea1cb06fdf125728f86edb71e126f4b542a835b59c8b7977', 'pt', 'fevereiro', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('4401d7f2780273a0c456ba32310bfa973be7fe97b88896297559b9c3ffca6137', 'month', 'March', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('4401d7f2780273a0c456ba32310bfa973be7fe97b88896297559b9c3ffca6137', 'es', 'marzo', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('4401d7f2780273a0c456ba32310bfa973be7fe97b88896297559b9c3ffca6137', 'pt', 'março', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('f7479d145ac5a9f6a3731c9391af4da85c90b8f46bc028685afed999f39ed08c', 'month', 'April', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('f7479d145ac5a9f6a3731c9391af4da85c90b8f46bc028685afed999f39ed08c', 'es', 'abril', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('f7479d145ac5a9f6a3731c9391af4da85c90b8f46bc028685afed999f39ed08c', 'pt', 'abril', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('d8eb72f7880a4e4c6fefa56c47d4b970fa1f2dd01184b2918cd16bd00c19c00f', 'month', 'May', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('d8eb72f7880a4e4c6fefa56c47d4b970fa1f2dd01184b2918cd16bd00c19c00f', 'es', 'mayo', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('d8eb72f7880a4e4c6fefa56c47d4b970fa1f2dd01184b2918cd16bd00c19c00f', 'pt', 'maio', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('2b957183e8343ec4525ccb9fb9854db47d22045a8a08b48d41f0df9b0c6c68e9', 'month', 'June', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('2b957183e8343ec4525ccb9fb9854db47d22045a8a08b48d41f0df9b0c6c68e9', 'es', 'junio', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('2b957183e8343ec4525ccb9fb9854db47d22045a8a08b48d41f0df9b0c6c68e9', 'pt', 'junho', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('a732a3f0bf670315510c9f2257bbbcb271b308b9abf846b44d6065252bb40b72', 'month', 'July', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a732a3f0bf670315510c9f2257bbbcb271b308b9abf846b44d6065252bb40b72', 'es', 'julio', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a732a3f0bf670315510c9f2257bbbcb271b308b9abf846b44d6065252bb40b72', 'pt', 'julho', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('db51487f19c0618711fc1e1f6a3412ac39ed32814b55080ff76860ae43403f56', 'month', 'August', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('db51487f19c0618711fc1e1f6a3412ac39ed32814b55080ff76860ae43403f56', 'es', 'agosto', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('db51487f19c0618711fc1e1f6a3412ac39ed32814b55080ff76860ae43403f56', 'pt', 'agosto', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('00f4626b1991cef406e631a68e4d3dc6cd7e0ca68bd2c661e02f45eba2ecbbe6', 'month', 'September', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('00f4626b1991cef406e631a68e4d3dc6cd7e0ca68bd2c661e02f45eba2ecbbe6', 'es', 'septiembre', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('00f4626b1991cef406e631a68e4d3dc6cd7e0ca68bd2c661e02f45eba2ecbbe6', 'pt', 'setembro', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('059ea4c0ec05c80128f80227c6a07bb5a933b2cf985366349ca30c0dd4d69cea', 'month', 'October', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('059ea4c0ec05c80128f80227c6a07bb5a933b2cf985366349ca30c0dd4d69cea', 'es', 'octubre', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('059ea4c0ec05c80128f80227c6a07bb5a933b2cf985366349ca30c0dd4d69cea', 'pt', 'outubro', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('dc408a3a1aa491f2f8e25757a3b5ee0577ded3d62178d8e8754762ce97828251', 'month', 'November', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('dc408a3a1aa491f2f8e25757a3b5ee0577ded3d62178d8e8754762ce97828251', 'es', 'noviembre', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('dc408a3a1aa491f2f8e25757a3b5ee0577ded3d62178d8e8754762ce97828251', 'pt', 'novembro', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('3fbaa79f82fef624bc69b3dabcc6e01118eb2634df7e91b1e8d05f09d9e43a87', 'month', 'December', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('3fbaa79f82fef624bc69b3dabcc6e01118eb2634df7e91b1e8d05f09d9e43a87', 'es', 'diciembre', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('3fbaa79f82fef624bc69b3dabcc6e01118eb2634df7e91b1e8d05f09d9e43a87', 'pt', 'dezembro', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

-- Removed rather than reworded: the English criteria narrative has no such
-- sentence, so dropping it is what makes the three languages agree. Matched
-- on the exact wording, so a translation someone has since revised is left
-- alone rather than half-edited.
UPDATE translation
SET translated = replace(translated, ' Las referencias se citan en su idioma original.', ''),
    updated = datetime('now')
WHERE phrase_id = '32cad0f22f18cb5d57d8a4d9c3d2f4dd53f6574340504932ca46e24c442f7e75'
  AND lang = 'es'
  AND translated LIKE '%Las referencias se citan en su idioma original.%';

UPDATE translation
SET translated = replace(translated, ' As referências são citadas no idioma original.', ''),
    updated = datetime('now')
WHERE phrase_id = '32cad0f22f18cb5d57d8a4d9c3d2f4dd53f6574340504932ca46e24c442f7e75'
  AND lang = 'pt'
  AND translated LIKE '%As referências são citadas no idioma original.%';
