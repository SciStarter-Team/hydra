-- Spanish and Portuguese for how an earner's badge page presents itself
-- when shared, and how it is confirmed: the section on showing the badge off
-- that the earner sees, the section for confirming its recipient that
-- everyone else sees,
-- the post its share buttons offer, and the words on its social card, which
-- are also its link title and description. They are drafts that
-- no translator has reviewed, the same wording as orrery's
-- `src/badgetext.rs`, and so are seeded as 'machine': displayed, but listed
-- in the admin as needing review.
--
-- The ids are sha256(context || 0x1f || source), as i18n::phrase_id derives
-- them. Seeding fills a translation that is blank or written by a machine,
-- and leaves one a human has written alone.

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('34bf84ed791b220e4cf773afd63344d74501233290e1ecb6694ba1196a7248e6', '', 'I''m happy to share that I''ve earned the {name} certification from @SciStarter. You can too at {url}', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('34bf84ed791b220e4cf773afd63344d74501233290e1ecb6694ba1196a7248e6', 'es', 'Me alegra compartir que obtuve la certificación {name} de @SciStarter. Tú también puedes obtenerla en {url}', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('34bf84ed791b220e4cf773afd63344d74501233290e1ecb6694ba1196a7248e6', 'pt', 'Tenho o prazer de compartilhar que conquistei a certificação {name} da @SciStarter. Você também pode conquistá-la em {url}', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('0e825aaa2270150cd2ec0adf9f2acb00bb6587435a3c6122747694227872cef5', '', 'I''m happy to share that I''ve earned the {name} badge from @SciStarter.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('0e825aaa2270150cd2ec0adf9f2acb00bb6587435a3c6122747694227872cef5', 'es', 'Me alegra compartir que obtuve la insignia {name} de @SciStarter.', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('0e825aaa2270150cd2ec0adf9f2acb00bb6587435a3c6122747694227872cef5', 'pt', 'Tenho o prazer de compartilhar que conquistei o badge {name} da @SciStarter.', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('cfe6a1a77c48168f67d83d6b62be3cf24a0464e1023e76b1ac8ff6cc7b0ff5dd', '', 'Certification earned', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('cfe6a1a77c48168f67d83d6b62be3cf24a0464e1023e76b1ac8ff6cc7b0ff5dd', 'es', 'Certificación obtenida', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('cfe6a1a77c48168f67d83d6b62be3cf24a0464e1023e76b1ac8ff6cc7b0ff5dd', 'pt', 'Certificação conquistada', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('e2a21a69a6ac060f8dd737e426e49922d5e9902734a7b33347e0face718e8f6d', '', 'Badge earned', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('e2a21a69a6ac060f8dd737e426e49922d5e9902734a7b33347e0face718e8f6d', 'es', 'Insignia obtenida', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('e2a21a69a6ac060f8dd737e426e49922d5e9902734a7b33347e0face718e8f6d', 'pt', 'Badge conquistado', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('b39b8d87f934d6362fc13edab53bb6bf6273b942013ab8f768bc8f1a9b2d6d3e', '', 'Earn yours at {url}', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('b39b8d87f934d6362fc13edab53bb6bf6273b942013ab8f768bc8f1a9b2d6d3e', 'es', 'Obtén la tuya en {url}', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('b39b8d87f934d6362fc13edab53bb6bf6273b942013ab8f768bc8f1a9b2d6d3e', 'pt', 'Conquiste a sua em {url}', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('25b7888ebc04e919b0f7c6a6d0be479957e836bfe38761222448d6f6717e7a65', '', 'Showcase Your Achievement', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('25b7888ebc04e919b0f7c6a6d0be479957e836bfe38761222448d6f6717e7a65', 'es', 'Muestra tu logro', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('25b7888ebc04e919b0f7c6a6d0be479957e836bfe38761222448d6f6717e7a65', 'pt', 'Mostre sua conquista', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('bfe03b44a176981f0b0850eb085e89facd936a4290b31e2ea0d6ee657d1b21ef', '', 'Add your verified badge to your professional profile and share it with your network.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('bfe03b44a176981f0b0850eb085e89facd936a4290b31e2ea0d6ee657d1b21ef', 'es', 'Agrega tu insignia verificada a tu perfil profesional y compártela con tu red.', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('bfe03b44a176981f0b0850eb085e89facd936a4290b31e2ea0d6ee657d1b21ef', 'pt', 'Adicione seu badge verificado ao seu perfil profissional e compartilhe-o com sua rede.', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('a244303bba647905be9a223407475f22c839bc0718a396a23c350c78406a5c7b', '', 'Add to LinkedIn Profile', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('a244303bba647905be9a223407475f22c839bc0718a396a23c350c78406a5c7b', 'es', 'Agregar al perfil de LinkedIn', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('a244303bba647905be9a223407475f22c839bc0718a396a23c350c78406a5c7b', 'pt', 'Adicionar ao perfil do LinkedIn', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('f43d107cc5424182114f8b6efe6cee89a548b815794a3cf3fb78ccce3e1fec31', '', 'Permanently showcase your achievement in the Licenses & Certifications section of your LinkedIn profile, where recruiters and connections can see and verify your credential.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('f43d107cc5424182114f8b6efe6cee89a548b815794a3cf3fb78ccce3e1fec31', 'es', 'Muestra tu logro de forma permanente en la sección Licencias y certificaciones de tu perfil de LinkedIn, donde reclutadores y contactos pueden ver y verificar tu credencial.', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('f43d107cc5424182114f8b6efe6cee89a548b815794a3cf3fb78ccce3e1fec31', 'pt', 'Mostre sua conquista permanentemente na seção Licenças e certificados do seu perfil do LinkedIn, onde recrutadores e conexões podem ver e verificar sua credencial.', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('23deedd55ef27729aca7e93bf7a5384884d1166d805101eec670d0471d8ba513', '', 'Share on Social', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('23deedd55ef27729aca7e93bf7a5384884d1166d805101eec670d0471d8ba513', 'es', 'Comparte en redes sociales', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('23deedd55ef27729aca7e93bf7a5384884d1166d805101eec670d0471d8ba513', 'pt', 'Compartilhe nas redes sociais', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('7ba4e2cc439d99daa25bee0798f044e5c803b39690ff39bb234766ba3af32535', '', 'Celebrate your achievement with your network! Share your badge to highlight your citizen science skills and inspire others to get involved.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('7ba4e2cc439d99daa25bee0798f044e5c803b39690ff39bb234766ba3af32535', 'es', '¡Celebra tu logro con tu red! Comparte tu insignia para destacar tus habilidades en ciencia ciudadana e inspirar a otras personas a participar.', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('7ba4e2cc439d99daa25bee0798f044e5c803b39690ff39bb234766ba3af32535', 'pt', 'Comemore sua conquista com sua rede! Compartilhe seu badge para destacar suas habilidades em ciência cidadã e inspirar outras pessoas a participar.', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('0ddb2279c3388be253d8849c0ecd2ebd0cbc066cc7c945c8f762f36d0a7019ed', '', 'Confirm This Badge''s Recipient', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('0ddb2279c3388be253d8849c0ecd2ebd0cbc066cc7c945c8f762f36d0a7019ed', 'es', 'Confirmar la persona destinataria de esta insignia', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('0ddb2279c3388be253d8849c0ecd2ebd0cbc066cc7c945c8f762f36d0a7019ed', 'pt', 'Confirmar a pessoa destinatária deste badge', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('f68d463236a573130ec576d167b81ec7b4360f09eb61203295e81b4139d2c6df', '', 'For employers, schools, and credential reviewers', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('f68d463236a573130ec576d167b81ec7b4360f09eb61203295e81b4139d2c6df', 'es', 'Para empleadores, instituciones educativas y revisores de credenciales', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('f68d463236a573130ec576d167b81ec7b4360f09eb61203295e81b4139d2c6df', 'pt', 'Para empregadores, instituições de ensino e avaliadores de credenciais', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('eb81027721b349ff70e8b12da94db3d989c2d07f3033e7ebc51c4541b24029da', '', 'Want to confirm that this {name} badge, issued by SciStarter, belongs to a particular person? Enter their email address below to confirm whether it was issued to them.', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('eb81027721b349ff70e8b12da94db3d989c2d07f3033e7ebc51c4541b24029da', 'es', '¿Quieres confirmar que esta insignia {name}, emitida por SciStarter, pertenece a una persona en particular? Introduce abajo su correo electrónico para confirmar si se le emitió a ella.', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('eb81027721b349ff70e8b12da94db3d989c2d07f3033e7ebc51c4541b24029da', 'pt', 'Quer confirmar que este badge {name}, emitido pela SciStarter, pertence a uma pessoa específica? Digite abaixo o e-mail dela para confirmar se foi emitido para ela.', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('828e24d1dfa74e66794b4e086b6283ba411e61bd9e612f19dc7140344825c94f', '', 'Confirm Recipient', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('828e24d1dfa74e66794b4e086b6283ba411e61bd9e612f19dc7140344825c94f', 'es', 'Confirmar persona destinataria', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
VALUES ('828e24d1dfa74e66794b4e086b6283ba411e61bd9e612f19dc7140344825c94f', 'pt', 'Confirmar pessoa destinatária', 'machine', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;

-- The earn button's wording moved to title case, which gives it a new phrase
-- id. It keeps the translations the old wording has -- whatever a translator
-- has made of them since they were seeded -- rather than starting over.
INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('979830049bfae3d049c358ad65eb9e6a2f3753383688529f78499f0b42dc181c', '', 'Learn How to Earn This Badge', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, provenance, updated)
SELECT '979830049bfae3d049c358ad65eb9e6a2f3753383688529f78499f0b42dc181c', lang, translated, provenance, datetime('now')
FROM translation
WHERE phrase_id = '1a1f8fcae59f1a0dbac307e8b2fa2da4b5ee24cb22e4eb0a75cacb1c369b9028'
  AND translated <> ''
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  provenance = excluded.provenance,
  updated = excluded.updated
WHERE translation.needs_work;
