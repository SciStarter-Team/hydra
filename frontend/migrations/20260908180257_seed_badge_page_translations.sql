-- Spanish and Portuguese for the badge pages.
--
-- The wording is the translation Roland reviewed for the same pages on
-- scistarter.org, where it lives in orrery's `src/badgetext.rs`: the two
-- sites show the same badge, so they say the same words about it. The five
-- strings hydra's pages have and orrery's do not -- the recipient-confirming
-- form, and the picker's way back to the browser's own preference -- follow
-- the same register, including its decision that Portuguese says "badge"
-- rather than "selo".
--
-- The ids are sha256(context || 0x1f || source), the same derivation
-- i18n::phrase_id performs, so these rows are the same phrases the site
-- discovers for itself. Change a source string in a template and its id
-- changes with it: the seeded translation is then simply unused, and the new
-- wording turns up in the admin as untranslated.
--
-- Seeding fills blanks only. A phrase already carrying a translation is left
-- alone, so re-running this can never overwrite a translator's work.

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('69c15ec96ea0eb33a794604dd1f42164b9a165332c5fe9ad98c541342646c747', '', '{name} Badge', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('69c15ec96ea0eb33a794604dd1f42164b9a165332c5fe9ad98c541342646c747', 'es', 'Insignia: {name}', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('69c15ec96ea0eb33a794604dd1f42164b9a165332c5fe9ad98c541342646c747', 'pt', 'Badge: {name}', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('a634a61ce49e64ee42498386cf08ed05909805aefe699d54aec3bf707ced68a5', '', 'verifiable {name} badge image', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a634a61ce49e64ee42498386cf08ed05909805aefe699d54aec3bf707ced68a5', 'es', 'imagen verificable de la insignia {name}', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a634a61ce49e64ee42498386cf08ed05909805aefe699d54aec3bf707ced68a5', 'pt', 'imagem verificável do badge {name}', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('051b6c9b0a5cb2a047a64e9ead433ffd220ae95003463eea73408ca914f5a773', '', 'example {name} badge image', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('051b6c9b0a5cb2a047a64e9ead433ffd220ae95003463eea73408ca914f5a773', 'es', 'imagen de ejemplo de la insignia {name}', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('051b6c9b0a5cb2a047a64e9ead433ffd220ae95003463eea73408ca914f5a773', 'pt', 'imagem de exemplo do badge {name}', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('3c04e251d0c7be0f179328397aece5444feba52d0edc95817d390852cbb818c1', '', 'Verified', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('3c04e251d0c7be0f179328397aece5444feba52d0edc95817d390852cbb818c1', 'es', 'Verificado', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('3c04e251d0c7be0f179328397aece5444feba52d0edc95817d390852cbb818c1', 'pt', 'Verificado', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('9386312d1e1042c45431d6e6813a260375b2706c1eff500ca00df8756f141290', '', 'Last verified just now by SciStarter', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('9386312d1e1042c45431d6e6813a260375b2706c1eff500ca00df8756f141290', 'es', 'Última verificación realizada hace un momento por SciStarter', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('9386312d1e1042c45431d6e6813a260375b2706c1eff500ca00df8756f141290', 'pt', 'Última verificação realizada agora mesmo pela SciStarter', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('e8182fbeb6f60f4e48371b54df5947c1347061dc046d03c8794c854fc9446b63', '', 'Issued on:', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('e8182fbeb6f60f4e48371b54df5947c1347061dc046d03c8794c854fc9446b63', 'es', 'Emitida el:', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('e8182fbeb6f60f4e48371b54df5947c1347061dc046d03c8794c854fc9446b63', 'pt', 'Emitido em:', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('4335c37b401098602186636d18e7639debb4379a9587b0b17d4db30c44c5a946', '', 'Issued to:', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('4335c37b401098602186636d18e7639debb4379a9587b0b17d4db30c44c5a946', 'es', 'Emitida para:', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('4335c37b401098602186636d18e7639debb4379a9587b0b17d4db30c44c5a946', 'pt', 'Emitido para:', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('4dcf78c934839c4c962d1674efbdee0916f0e7ca93ae5fc7fdf2f7e979c306c5', '', 'NOT', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('4dcf78c934839c4c962d1674efbdee0916f0e7ca93ae5fc7fdf2f7e979c306c5', 'es', 'NO es', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('4dcf78c934839c4c962d1674efbdee0916f0e7ca93ae5fc7fdf2f7e979c306c5', 'pt', 'NÃO é', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('6df273ee1f59f099212ebcb117529cc98c2838afae5ee9b43f8ba5f77fe2e8b9', '', 'Confirm recipient:', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('6df273ee1f59f099212ebcb117529cc98c2838afae5ee9b43f8ba5f77fe2e8b9', 'es', 'Confirmar la persona destinataria:', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('6df273ee1f59f099212ebcb117529cc98c2838afae5ee9b43f8ba5f77fe2e8b9', 'pt', 'Confirmar a pessoa destinatária:', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('6edf5f760064fb9e5d9ec2871914f0ee15354e656677339eaae128887a7b1e63', '', 'Enter email address', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('6edf5f760064fb9e5d9ec2871914f0ee15354e656677339eaae128887a7b1e63', 'es', 'Introduce un correo electrónico', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('6edf5f760064fb9e5d9ec2871914f0ee15354e656677339eaae128887a7b1e63', 'pt', 'Digite um e-mail', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('a134119f6a0bbbc6064200b55a3b9e3c8aebb3581676f6453a2973a6e485ab68', '', 'Check', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a134119f6a0bbbc6064200b55a3b9e3c8aebb3581676f6453a2973a6e485ab68', 'es', 'Comprobar', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a134119f6a0bbbc6064200b55a3b9e3c8aebb3581676f6453a2973a6e485ab68', 'pt', 'Verificar', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('8a9ddc31153904d17033347b2a82b644ff0de697d85d4390b9ac4dd4aa67c146', '', 'You can also use Open Badges tools such as <a target=_blank href="https://badgecheck.io/" class="hyperlink">BadgeCheck</a> to verify this badge.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('8a9ddc31153904d17033347b2a82b644ff0de697d85d4390b9ac4dd4aa67c146', 'es', 'También puedes usar herramientas de Open Badges como <a target=_blank href="https://badgecheck.io/" class="hyperlink">BadgeCheck</a> para verificar esta insignia.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('8a9ddc31153904d17033347b2a82b644ff0de697d85d4390b9ac4dd4aa67c146', 'pt', 'Você também pode usar ferramentas de Open Badges como <a target=_blank href="https://badgecheck.io/" class="hyperlink">BadgeCheck</a> para verificar este badge.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('1a1f8fcae59f1a0dbac307e8b2fa2da4b5ee24cb22e4eb0a75cacb1c369b9028', '', 'learn how to earn this badge', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('1a1f8fcae59f1a0dbac307e8b2fa2da4b5ee24cb22e4eb0a75cacb1c369b9028', 'es', 'Aprende cómo obtener esta insignia', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('1a1f8fcae59f1a0dbac307e8b2fa2da4b5ee24cb22e4eb0a75cacb1c369b9028', 'pt', 'Saiba como obter este badge', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('0f5d3b89f73101583f9c8084c35a7ec105e2c996cf0f797c75657db83fb62cde', '', 'Offered by:', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('0f5d3b89f73101583f9c8084c35a7ec105e2c996cf0f797c75657db83fb62cde', 'es', 'Ofrecida por:', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('0f5d3b89f73101583f9c8084c35a7ec105e2c996cf0f797c75657db83fb62cde', 'pt', 'Oferecido por:', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('8948236bb48a2a58f98669475d2d440c54aea14571af0f2ed560ab715d5e387c', '', 'Earning Criteria', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('8948236bb48a2a58f98669475d2d440c54aea14571af0f2ed560ab715d5e387c', 'es', 'Criterios para obtener la insignia', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('8948236bb48a2a58f98669475d2d440c54aea14571af0f2ed560ab715d5e387c', 'pt', 'Critérios para obtenção', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('f9a997f3f2843ffac99e50a07c1e90450cfb2da7d4f10fdc7c8516a9e28c84bd', '', 'Recipients must complete the earning criteria to earn this badge', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('f9a997f3f2843ffac99e50a07c1e90450cfb2da7d4f10fdc7c8516a9e28c84bd', 'es', 'Las personas destinatarias deben completar los criterios establecidos para obtener esta insignia.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('f9a997f3f2843ffac99e50a07c1e90450cfb2da7d4f10fdc7c8516a9e28c84bd', 'pt', 'As pessoas destinatárias devem cumprir os critérios estabelecidos para obter este badge.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('54eec0d343ac7f3e4a86b7342a6cd15be1c145aa5700680cdde49dc823786d10', '', 'Tags', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('54eec0d343ac7f3e4a86b7342a6cd15be1c145aa5700680cdde49dc823786d10', 'es', 'Etiquetas', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('54eec0d343ac7f3e4a86b7342a6cd15be1c145aa5700680cdde49dc823786d10', 'pt', 'Tags', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('cbb2788e2f70fd8f200dccd21cc8102e2ca10a66a202eef31050ac66cf836b76', '', 'Skills', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('cbb2788e2f70fd8f200dccd21cc8102e2ca10a66a202eef31050ac66cf836b76', 'es', 'Habilidades', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('cbb2788e2f70fd8f200dccd21cc8102e2ca10a66a202eef31050ac66cf836b76', 'pt', 'Habilidades', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('e5aecf142ddd30b6c9e3d063b63e0a6d2df34a6b4c2546b08bef13337b3e0d72', '', 'Recipients demonstrated these job skills', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('e5aecf142ddd30b6c9e3d063b63e0a6d2df34a6b4c2546b08bef13337b3e0d72', 'es', 'Las personas destinatarias demostraron estas habilidades laborales.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('e5aecf142ddd30b6c9e3d063b63e0a6d2df34a6b4c2546b08bef13337b3e0d72', 'pt', 'As pessoas destinatárias demonstraram estas habilidades profissionais.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('9f0cc3e5184111b99525d67014e099f642f75ca9dafe8cf1e540cec363c6c9e9', '', 'Questions? Check out our <a class=hyperlink href="https://scistarter.org/training-faq">FAQ</a>.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('9f0cc3e5184111b99525d67014e099f642f75ca9dafe8cf1e540cec363c6c9e9', 'es', '¿Tienes preguntas? Consulta nuestras <a class=hyperlink href="https://scistarter.org/training-faq">preguntas frecuentes</a>.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('9f0cc3e5184111b99525d67014e099f642f75ca9dafe8cf1e540cec363c6c9e9', 'pt', 'Dúvidas? Consulte nossas <a class=hyperlink href="https://scistarter.org/training-faq">perguntas frequentes</a>.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('a4aede1ad9271b42b58991ba7fea7548700a5caac03d5cbf83c033480d74d1e6', '', 'Language', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a4aede1ad9271b42b58991ba7fea7548700a5caac03d5cbf83c033480d74d1e6', 'es', 'Idioma', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a4aede1ad9271b42b58991ba7fea7548700a5caac03d5cbf83c033480d74d1e6', 'pt', 'Idioma', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('df83b19d342d4a37f6743a2315c4961d348c1d913d1997f5642ef2dc5821c0ba', '', 'Use my browser''s setting', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('df83b19d342d4a37f6743a2315c4961d348c1d913d1997f5642ef2dc5821c0ba', 'es', 'Usar la configuración de mi navegador', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('df83b19d342d4a37f6743a2315c4961d348c1d913d1997f5642ef2dc5821c0ba', 'pt', 'Usar a configuração do meu navegador', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('fdc2694b2020381ff8d0eecc1258ced8909e83fd2a39370edc7d82f14c80236d', '', 'This is a courtesy translation. The official text of this badge, which is what the verifiable record contains, is in English: <a class=hyperlink href="{url}">see the original</a>.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('fdc2694b2020381ff8d0eecc1258ced8909e83fd2a39370edc7d82f14c80236d', 'es', 'Esta es una traducción de cortesía. El texto oficial de esta insignia, que es el que contiene el registro verificable, está en inglés: <a class=hyperlink href="{url}">ver la versión original</a>.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('fdc2694b2020381ff8d0eecc1258ced8909e83fd2a39370edc7d82f14c80236d', 'pt', 'Esta é uma tradução de cortesia. O texto oficial deste badge, que é o que consta no registro verificável, está em inglês: <a class=hyperlink href="{url}">ver a versão original</a>.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';
