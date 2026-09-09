-- Spanish and Portuguese for the Data Ethics for Practitioners badge.
--
-- The badge's own words -- its name, description, criteria narrative and
-- tags -- and the address of the tutorial that teaches it. Until this
-- migration they were a static table in the frontend, one struct per badge
-- holding one struct per language, which meant a badge could only be
-- translated by a deploy and only into the languages that table had a field
-- for. They are phrases like any other now, so a translator adds a language
-- to a badge in the admin, and no badge is special.
--
-- Wording follows the published Spanish and Portuguese tutorials
-- (scistarter.org/training-dataethics-es and -pt), translated in
-- collaboration with WCS, the Alianza Aguas Amazonicas, the Rede Brasileira
-- de Ciencia Cidada and the Red Iberoamericana de Ciencia Participativa. The
-- same text is in orrery's `src/badgetext.rs`, which serves the same badge on
-- scistarter.org: the two sites have to say the same thing about the same
-- credential.
--
-- The ids are sha256(context || 0x1f || source), the same derivation
-- i18n::phrase_id performs, so these rows are the same phrases the site
-- discovers for itself when a badge page is rendered. Change one of the
-- badge's strings in `common/src/badge` and its id changes with it: the
-- seeded translation is then unused and the new wording turns up in the
-- admin as untranslated. `badge::text`'s tests check for exactly that.
--
-- Seeding fills blanks only. A phrase already carrying a translation is left
-- alone, so re-running this can never overwrite a translator's work.

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('09207dfcf577b034ae9edf8dec9139e007424249110be9a4cb5ea1ae57f8e66e', '', 'Data Ethics for Practitioners', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('09207dfcf577b034ae9edf8dec9139e007424249110be9a4cb5ea1ae57f8e66e', 'es', 'Ética de datos para profesionales', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('09207dfcf577b034ae9edf8dec9139e007424249110be9a4cb5ea1ae57f8e66e', 'pt', 'Ética de Dados para Profissionais', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('fcdda57298a93f116fb1b2d4d745605dd1aacc2030493695faa04b372c54d346', '', 'The "Data Ethics for Practitioners" badge is awarded to practitioners and leaders of participatory science projects, including citizen science and community science projects, who have demonstrated an ability to "think like an ethicist."', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('fcdda57298a93f116fb1b2d4d745605dd1aacc2030493695faa04b372c54d346', 'es', 'La insignia “Ética de datos para profesionales” se otorga a profesionales y líderes de proyectos de ciencia participativa, incluidos proyectos de ciencia ciudadana y ciencia comunitaria, que han demostrado la capacidad de “pensar como una persona experta en ética”.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('fcdda57298a93f116fb1b2d4d745605dd1aacc2030493695faa04b372c54d346', 'pt', 'O badge “Ética de Dados para Profissionais” é concedido a profissionais e lideranças de projetos de ciência participativa, incluindo projetos de ciência cidadã e ciência comunitária, que demonstraram a capacidade de “pensar como uma pessoa especialista em ética”.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('32cad0f22f18cb5d57d8a4d9c3d2f4dd53f6574340504932ca46e24c442f7e75', '', '# Data Ethics for Practitioners
## Skills and Competencies
The recipient has demonstrated the following data ethics skills and obtained a score of 80% or better via traditional, online assessment:

- Distinguish roles and ethical obligations of project leaders, participants, and partners in participatory science projects.
- Identify how ethical tensions arise in participatory science projects.
- Recognize the obligations associated with ethical principles in the context of participatory science projects.
- Analyze ethical tensions related to data integrity, report-outs, recompense, and report-backs in participatory science projects.
- Recognize how data governance structures can concentrate or diffuse decision-making power.

***
## Research
Additional publications produced as a result of [Grant No. 1835352, “Establishing Norms of Data Ethics in Citizen Science”](https://www.nsf.gov/awardsearch/showAward?AWD_ID=1835352)
- Cooper, Caren B. and Rasmussen, Lisa M. and Jones, Elizabeth D. "Perspective: The Power (Dynamics) of Open Data in Citizen Science" Frontiers in Climate , v.3 , 2021 [https://doi.org/10.3389/fclim.2021.637037](https://doi.org/10.3389/fclim.2021.637037), [Citation Details](http://par.nsf.gov/biblio/10276550)
- de Sherbinin, Alex and Bowser, Anne and Chuang, Tyng-Ruey and Cooper, Caren and Danielsen, Finn and Edmunds, Rorie and Elias, Peter and Faustman, Elaine and Hultquist, Carolynne and Mondardini, Rosy and Popescu, Ioana and Shonowo, Adenike and Sivakumar, K "The Critical Importance of Citizen Science Data" Frontiers in Climate , v.3 , 2021 [https://doi.org/10.3389/fclim.2021.650760](https://doi.org/10.3389/fclim.2021.650760), [Citation Details](http://par.nsf.gov/biblio/10276551)
- Cooper, Caren and Shanley, Lea and Scassa, Teresa and Vayena, Effy "Project Categories to Guide Institutional Oversight of Responsible Conduct of Scientists Leading Citizen Science in the United States" Citizen Science: Theory and Practice , v.4 , 2019 [http://dx.doi.org/10.5334/cstp.202](http://dx.doi.org/10.5334/cstp.202), [Citation Details](http://par.nsf.gov/biblio/10110884)
***
## What is the Association for Advancing Participatory Sciences (AAPS)?

The Association for Advancing Participatory Sciences (AAPS) is a member-driven organization that connects people from a wide range of experiences around one shared purpose: advancing knowledge through research and monitoring done by, for, and with members of the public. 

Citizen science – the most recognizable term for this practice – is expanding the reach, relevance, and impact of science in almost every area of inquiry; in the field and online; through local and global efforts. With increased attention to citizen science, AAPS brings depth to how citizen science is understood both as public engagement and as research, and shines a light on the integrity and complexity of the practice. 

Visit [https://participatorysciences.org/about/](https://participatorysciences.org/about/) to learn more.

****
## Earn the Data Ethics for Practitioners badge
The Data Ethics for Practitioners badge helps you “think like an ethicist” - by identifying relevant ethical obligations, tensions, and issues that may arise in your project, and use them to make decisions.

This material is based upon input from over 100 individuals so that it reflects both areas of broad agreement and areas of divergent priorities and values. The work was supported by the National Science Foundation under Grant No. Cooper & Rasmussen # 1835352, “Establishing Norms of Data Ethics in Citizen Science.” 

Resources were developed and field tested by a team with expertise in instructional design, education, libraries, inclusive practices, digital design, micro accreditation, and, of course, citizen science to produce a series of tutorials, trainings, and accompanying modules to help people from all walks of life discover and engage in authentic science.

Visit [https://scistarter.org/training-dataethics](https://scistarter.org/training-dataethics) to learn more about earning this badge.
****
## What is SciStarter?
SciStarter is a globally acclaimed, online citizen science hub where millions of people connect with thousands of scientific research projects in need of their help.  

SciStarter is supported by the National Science Foundation, NASA, the National Library of Medicine, the Institute for Museum and Library Services, the Gordon and Betty Moore Foundation, Verizon, the Girl Scouts of the USA and others seeking to support and sustain engagement in science, catalyze customized experiences and pathways, track progress and measure collective impact among their communities.

SciStarter is a research affiliate of Arizona State University and North Carolina State University.

Visit [https://scistarter.org/](https://scistarter.org/) to learn more.
****', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('32cad0f22f18cb5d57d8a4d9c3d2f4dd53f6574340504932ca46e24c442f7e75', 'es', '# Ética de datos para profesionales
## Habilidades y competencias
La persona destinataria de esta certificación ha demostrado las siguientes habilidades en ética de datos y ha obtenido una puntuación del 80 % o superior mediante una evaluación tradicional en línea:

- Distinguir los roles y las obligaciones éticas de líderes de proyecto, participantes y socios en proyectos de ciencia participativa.
- Identificar cómo surgen las tensiones éticas en proyectos de ciencia participativa.
- Reconocer las obligaciones asociadas con principios éticos en el contexto de proyectos de ciencia participativa.
- Analizar tensiones éticas relacionadas con la integridad de los datos, la comunicación pública de resultados, la compensación y la devolución de resultados en proyectos de ciencia participativa.
- Reconocer cómo las estructuras de gobernanza de datos pueden concentrar o distribuir el poder de decisión.

***
## Investigación
Publicaciones adicionales producidas como resultado de la [subvención n.º 1835352, “Establecimiento de normas de ética de datos en ciencia ciudadana”](https://www.nsf.gov/awardsearch/showAward?AWD_ID=1835352). Las referencias se citan en su idioma original.
- Cooper, Caren B. and Rasmussen, Lisa M. and Jones, Elizabeth D. "Perspective: The Power (Dynamics) of Open Data in Citizen Science" Frontiers in Climate , v.3 , 2021 [https://doi.org/10.3389/fclim.2021.637037](https://doi.org/10.3389/fclim.2021.637037), [Citation Details](http://par.nsf.gov/biblio/10276550)
- de Sherbinin, Alex and Bowser, Anne and Chuang, Tyng-Ruey and Cooper, Caren and Danielsen, Finn and Edmunds, Rorie and Elias, Peter and Faustman, Elaine and Hultquist, Carolynne and Mondardini, Rosy and Popescu, Ioana and Shonowo, Adenike and Sivakumar, K "The Critical Importance of Citizen Science Data" Frontiers in Climate , v.3 , 2021 [https://doi.org/10.3389/fclim.2021.650760](https://doi.org/10.3389/fclim.2021.650760), [Citation Details](http://par.nsf.gov/biblio/10276551)
- Cooper, Caren and Shanley, Lea and Scassa, Teresa and Vayena, Effy "Project Categories to Guide Institutional Oversight of Responsible Conduct of Scientists Leading Citizen Science in the United States" Citizen Science: Theory and Practice , v.4 , 2019 [http://dx.doi.org/10.5334/cstp.202](http://dx.doi.org/10.5334/cstp.202), [Citation Details](http://par.nsf.gov/biblio/10110884)
***
## ¿Qué es la Association for Advancing Participatory Sciences (AAPS)?

La Association for Advancing Participatory Sciences (AAPS) es una organización impulsada por sus miembros que conecta a personas con una amplia variedad de experiencias en torno a un propósito compartido: avanzar el conocimiento mediante investigación y monitoreo realizados por, para y con integrantes del público.

La ciencia ciudadana —el término más reconocible para esta práctica— está ampliando el alcance, la relevancia y el impacto de la ciencia en casi todas las áreas de investigación; en campo y en línea; mediante esfuerzos locales y globales. Ante la creciente atención hacia la ciencia ciudadana, AAPS aporta profundidad a la manera en que la ciencia ciudadana se entiende tanto como participación pública como investigación, y da visibilidad a la integridad y complejidad de la práctica.

Visita [https://participatorysciences.org/about/](https://participatorysciences.org/about/) para obtener más información.

****
## Obtén la insignia Ética de datos para profesionales
La insignia Ética de datos para profesionales te ayuda a “pensar como una persona experta en ética”, identificando las obligaciones éticas, tensiones y cuestiones relevantes que pueden surgir en tu proyecto y usándolas para orientar la toma de decisiones.

Este material se basa en los aportes de más de 100 personas, de modo que refleja tanto áreas de amplio acuerdo como áreas con prioridades y valores divergentes. El trabajo recibió apoyo de la National Science Foundation mediante la subvención Cooper & Rasmussen n.º 1835352, “Establecimiento de normas de ética de datos en ciencia ciudadana”.

Los recursos fueron desarrollados y probados en campo por un equipo con experiencia en diseño instruccional, educación, bibliotecas, prácticas inclusivas, diseño digital, microacreditación y, por supuesto, ciencia ciudadana, con el fin de producir una serie de tutoriales, formaciones y módulos complementarios que ayuden a personas de distintos contextos a descubrir y participar en ciencia auténtica.

Visita [https://scistarter.org/training-dataethics-es](https://scistarter.org/training-dataethics-es) para obtener más información sobre cómo conseguir esta insignia.
****
## ¿Qué es SciStarter?
SciStarter es un centro en línea de ciencia ciudadana reconocido a nivel mundial, donde millones de personas se conectan con miles de proyectos de investigación científica que necesitan su ayuda.

SciStarter cuenta con el apoyo de la National Science Foundation, NASA, la National Library of Medicine, el Institute for Museum and Library Services, la Gordon and Betty Moore Foundation, Verizon, Girl Scouts of the USA y otras organizaciones que buscan apoyar y sostener la participación en la ciencia, impulsar experiencias y trayectorias personalizadas, hacer seguimiento del progreso y medir el impacto colectivo en sus comunidades.

SciStarter está afiliada como entidad de investigación a Arizona State University y North Carolina State University.

Visita [https://scistarter.org/](https://scistarter.org/) para obtener más información.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('32cad0f22f18cb5d57d8a4d9c3d2f4dd53f6574340504932ca46e24c442f7e75', 'pt', '# Ética de Dados para Profissionais
## Habilidades e competências
A pessoa destinatária desta certificação demonstrou as seguintes habilidades em ética de dados e obteve uma pontuação de 80% ou superior por meio de uma avaliação tradicional on-line:

- Distinguir os papéis e as obrigações éticas de lideranças de projeto, participantes e parceiros em projetos de ciência participativa.
- Identificar como surgem tensões éticas em projetos de ciência participativa.
- Reconhecer as obrigações associadas a princípios éticos no contexto de projetos de ciência participativa.
- Analisar tensões éticas relacionadas à integridade dos dados, à divulgação de resultados, à compensação e à devolutiva de resultados em projetos de ciência participativa.
- Reconhecer como as estruturas de governança de dados podem concentrar ou distribuir o poder de decisão.

***
## Pesquisa
Publicações adicionais produzidas como resultado da [bolsa n.º 1835352, “Estabelecimento de normas de ética de dados em ciência cidadã”](https://www.nsf.gov/awardsearch/showAward?AWD_ID=1835352). As referências são citadas no idioma original.
- Cooper, Caren B. and Rasmussen, Lisa M. and Jones, Elizabeth D. "Perspective: The Power (Dynamics) of Open Data in Citizen Science" Frontiers in Climate , v.3 , 2021 [https://doi.org/10.3389/fclim.2021.637037](https://doi.org/10.3389/fclim.2021.637037), [Citation Details](http://par.nsf.gov/biblio/10276550)
- de Sherbinin, Alex and Bowser, Anne and Chuang, Tyng-Ruey and Cooper, Caren and Danielsen, Finn and Edmunds, Rorie and Elias, Peter and Faustman, Elaine and Hultquist, Carolynne and Mondardini, Rosy and Popescu, Ioana and Shonowo, Adenike and Sivakumar, K "The Critical Importance of Citizen Science Data" Frontiers in Climate , v.3 , 2021 [https://doi.org/10.3389/fclim.2021.650760](https://doi.org/10.3389/fclim.2021.650760), [Citation Details](http://par.nsf.gov/biblio/10276551)
- Cooper, Caren and Shanley, Lea and Scassa, Teresa and Vayena, Effy "Project Categories to Guide Institutional Oversight of Responsible Conduct of Scientists Leading Citizen Science in the United States" Citizen Science: Theory and Practice , v.4 , 2019 [http://dx.doi.org/10.5334/cstp.202](http://dx.doi.org/10.5334/cstp.202), [Citation Details](http://par.nsf.gov/biblio/10110884)
***
## O que é a Association for Advancing Participatory Sciences (AAPS)?

A Association for Advancing Participatory Sciences (AAPS) é uma organização orientada por seus membros que conecta pessoas com uma ampla variedade de experiências em torno de um propósito compartilhado: avançar o conhecimento por meio de pesquisa e monitoramento realizados por, para e com integrantes do público.

A ciência cidadã —o termo mais reconhecido para essa prática— está ampliando o alcance, a relevância e o impacto da ciência em quase todas as áreas de pesquisa; em campo e on-line; por meio de esforços locais e globais. Diante da crescente atenção à ciência cidadã, a AAPS aprofunda a forma como a ciência cidadã é compreendida tanto como engajamento público quanto como pesquisa, e dá visibilidade à integridade e à complexidade da prática.

Visite [https://participatorysciences.org/about/](https://participatorysciences.org/about/) para saber mais.

****
## Obtenha o badge Ética de Dados para Profissionais
O badge Ética de Dados para Profissionais ajuda você a “pensar como uma pessoa especialista em ética”, identificando obrigações éticas, tensões e questões relevantes que podem surgir em seu projeto e usando-as para orientar a tomada de decisões.

Este material se baseia nas contribuições de mais de 100 pessoas, de modo que reflete tanto áreas de amplo acordo quanto áreas com prioridades e valores divergentes. O trabalho recebeu apoio da National Science Foundation por meio da bolsa Cooper & Rasmussen n.º 1835352, “Estabelecimento de normas de ética de dados em ciência cidadã”.

Os recursos foram desenvolvidos e testados em campo por uma equipe com experiência em design instrucional, educação, bibliotecas, práticas inclusivas, design digital, microacreditação e, claro, ciência cidadã, com o objetivo de produzir uma série de tutoriais, formações e módulos complementares para ajudar pessoas de diferentes contextos a descobrir e participar de ciência autêntica.

Visite [https://scistarter.org/training-dataethics-pt](https://scistarter.org/training-dataethics-pt) para saber mais sobre como obter este badge.
****
## O que é a SciStarter?
A SciStarter é um centro on-line de ciência cidadã reconhecido globalmente, onde milhões de pessoas se conectam a milhares de projetos de pesquisa científica que precisam de sua ajuda.

A SciStarter conta com o apoio da National Science Foundation, NASA, National Library of Medicine, Institute for Museum and Library Services, Gordon and Betty Moore Foundation, Verizon, Girl Scouts of the USA e outras organizações que buscam apoiar e sustentar o engajamento em ciência, impulsionar experiências e trajetórias personalizadas, acompanhar o progresso e medir o impacto coletivo em suas comunidades.

A SciStarter é afiliada de pesquisa da Arizona State University e da North Carolina State University.

Visite [https://scistarter.org/](https://scistarter.org/) para saber mais.', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('a91cfcbc3f18426d86064833f87bd0f0b81022ff4dab952d4214b752f46c7d98', '', 'professional development', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a91cfcbc3f18426d86064833f87bd0f0b81022ff4dab952d4214b752f46c7d98', 'es', 'desarrollo profesional', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('a91cfcbc3f18426d86064833f87bd0f0b81022ff4dab952d4214b752f46c7d98', 'pt', 'desenvolvimento profissional', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('ea0c36a6c1a33ac8a3c56fddad032350680f19d383b9526dbe01ba769e66a3a5', '', 'community engagement', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('ea0c36a6c1a33ac8a3c56fddad032350680f19d383b9526dbe01ba769e66a3a5', 'es', 'participación comunitaria', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('ea0c36a6c1a33ac8a3c56fddad032350680f19d383b9526dbe01ba769e66a3a5', 'pt', 'engajamento comunitário', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('7bd7f35a99fe03b1e9ec8d1ed83b85e1106baf3bcce00c0201959044584cef97', '', 'citizen science', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('7bd7f35a99fe03b1e9ec8d1ed83b85e1106baf3bcce00c0201959044584cef97', 'es', 'ciencia ciudadana', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('7bd7f35a99fe03b1e9ec8d1ed83b85e1106baf3bcce00c0201959044584cef97', 'pt', 'ciência cidadã', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation_phrase (phrase_id, context, source, origin, first_seen, last_seen)
VALUES ('b5465f8e106e2ae10a748539e47d8e0cc05996fe9987dd5fefa49296e3361282', 'badge: where to earn it', 'https://scistarter.org/training-dataethics', 'manual', datetime('now'), datetime('now'))
ON CONFLICT (phrase_id) DO NOTHING;

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('b5465f8e106e2ae10a748539e47d8e0cc05996fe9987dd5fefa49296e3361282', 'es', 'https://scistarter.org/training-dataethics-es', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

INSERT INTO translation (phrase_id, lang, translated, updated)
VALUES ('b5465f8e106e2ae10a748539e47d8e0cc05996fe9987dd5fefa49296e3361282', 'pt', 'https://scistarter.org/training-dataethics-pt', datetime('now'))
ON CONFLICT (phrase_id, lang) DO UPDATE SET
  translated = excluded.translated,
  updated = excluded.updated
WHERE translation.translated = '';

