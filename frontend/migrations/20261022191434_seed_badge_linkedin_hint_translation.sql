-- Spanish and Portuguese for the hint beneath the badge page's LinkedIn
-- button, which the page shows the badge's earner. The wording is the
-- translation Roland reviewed for the same hint on scistarter.org, in
-- orrery's `src/badgetext.rs`.
--
-- The id is sha256(context || 0x1f || source), as i18n::phrase_id derives
-- it. Seeding fills a translation that is blank or written by a machine, and
-- leaves one a human has written alone.

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('1a1be97b1a8aadf1c571a51dc072b3bb26ba8084d17612c88f7f7f5e4c7c9fbd', '', 'On a phone, LinkedIn''s app may leave <b>Issuing organization</b> blank. If it does, type <b>SciStarter</b> into that field and pick us from the list.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('1a1be97b1a8aadf1c571a51dc072b3bb26ba8084d17612c88f7f7f5e4c7c9fbd', 'es', 'En el teléfono, la aplicación de LinkedIn puede dejar en blanco el campo <b>Organización emisora</b>. Si ocurre, escribe <b>SciStarter</b> en ese campo y selecciónanos en la lista.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = 'human',
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('1a1be97b1a8aadf1c571a51dc072b3bb26ba8084d17612c88f7f7f5e4c7c9fbd', 'pt', 'No celular, o aplicativo do LinkedIn pode deixar o campo <b>Organização emissora</b> em branco. Se isso acontecer, digite <b>SciStarter</b> nesse campo e selecione-nos na lista.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = 'human',
  updated = excluded.updated
WHERE translation.needs_work;
