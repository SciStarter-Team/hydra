-- The Data Ethics badge under the names its Spanish and Portuguese
-- tutorials go by.
--
-- The badge was seeded as "Ética de datos para profesionales" and "Ética de
-- Dados para Profissionais", literal renderings of "for Practitioners". It
-- is now called what the tutorial that earns it is called, so an earner
-- reads one name on the tutorial, on the badge page and on the artwork.
--
-- The name is a phrase of its own, and is also quoted in the badge's
-- description and criteria narrative, so all three phrases are edited.
-- Matched on the exact wording, so a translation someone has since revised
-- is left alone rather than half-edited.
UPDATE translation
SET translated = replace(translated, 'Ética de datos para profesionales', 'Ética de Datos para las Ciencias Participativas'),
    updated = datetime('now')
WHERE phrase_id IN (
        '09207dfcf577b034ae9edf8dec9139e007424249110be9a4cb5ea1ae57f8e66e',
        'fcdda57298a93f116fb1b2d4d745605dd1aacc2030493695faa04b372c54d346',
        '32cad0f22f18cb5d57d8a4d9c3d2f4dd53f6574340504932ca46e24c442f7e75'
    )
  AND lang = 'es'
  AND translated LIKE '%Ética de datos para profesionales%';

UPDATE translation
SET translated = replace(translated, 'Ética de Dados para Profissionais', 'Ética de Dados para Ciência Cidadã'),
    updated = datetime('now')
WHERE phrase_id IN (
        '09207dfcf577b034ae9edf8dec9139e007424249110be9a4cb5ea1ae57f8e66e',
        'fcdda57298a93f116fb1b2d4d745605dd1aacc2030493695faa04b372c54d346',
        '32cad0f22f18cb5d57d8a4d9c3d2f4dd53f6574340504932ca46e24c442f7e75'
    )
  AND lang = 'pt'
  AND translated LIKE '%Ética de Dados para Profissionais%';
