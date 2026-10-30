-- What entities_fts indexes for each published entity is defined once, by the
-- entities_fts_documents view. Every trigger that maintains the index deletes
-- the affected rows and re-inserts them from that view, so changing what is
-- searchable means changing the view and nothing else.
--
-- Projects are searched by name (title), and by presenter, the full plain text
-- of the description, goal, task, search terms, tags and the host of the
-- project's URL (summary). The host names the platform a project runs on
-- (zooniverse.org, inaturalist.ala.org.au), which its own text often never
-- mentions. Events add presenter and the full description to what they had.
-- Pages keep their name and a 200-character excerpt.

DROP TRIGGER IF EXISTS entities_fts_entity_insert;
DROP TRIGGER IF EXISTS entities_fts_entity_update;
DROP TRIGGER IF EXISTS entities_fts_entity_delete;
DROP TRIGGER IF EXISTS entities_fts_project_update;
DROP TRIGGER IF EXISTS entities_fts_event_update;
DROP TRIGGER IF EXISTS entities_fts_pagecontent_update;

-- The plain text of a rich-text description. The detail-page editor stores
-- Quill Delta JSON, whose text is the "insert" strings of its ops; anything
-- else (plain text, Markdown, HTML) is indexed as stored. The CASE tests
-- validity first because the JSON functions raise errors on malformed JSON.
CREATE VIEW entities_fts_descriptions (entity_type, target, text) AS
SELECT
    entity_type,
    target,
    CASE
        WHEN NOT json_valid(content) THEN content
        WHEN json_type(content, '$.ops') = 'array' THEN COALESCE(
            (SELECT group_concat(json_extract(op.value, '$.insert'), '')
             FROM json_each(content, '$.ops') AS op
             WHERE json_type(op.value, '$.insert') = 'text'),
            '')
        ELSE content
    END
FROM (
    SELECT 'Project' AS entity_type, project_id AS target, description AS content FROM projects
    UNION ALL
    SELECT 'Event', event_id, description FROM events
);

-- The host of a project's URL: what lies between "://" (or the start, when
-- there is no scheme) and the first "/", "?" or "#".
CREATE VIEW entities_fts_project_hosts (project_id, host) AS
SELECT
    project_id,
    CASE WHEN instr(rest, '/') > 0 THEN substr(rest, 1, instr(rest, '/') - 1) ELSE rest END
FROM (
    SELECT
        project_id,
        replace(replace(
            CASE WHEN instr(url, '://') > 0 THEN substr(url, instr(url, '://') + 3) ELSE url END,
            '?', '/'), '#', '/') AS rest
    FROM projects
);

-- Each arm repeats the condition of the partial index entities_by_target,
-- which SQLite does not infer from the kind test, so that the triggers'
-- lookups by project, event or page id use that index.
CREATE VIEW entities_fts_documents (slug, entity_type, entity_id, title, summary) AS
SELECT
    e.slug,
    'Project',
    p.project_id,
    p.name,
    p.presenter || char(10) ||
    (SELECT d.text FROM entities_fts_descriptions d
     WHERE d.entity_type = 'Project' AND d.target = p.project_id) || char(10) ||
    p.goal || char(10) ||
    p.task || char(10) ||
    p.search_terms || char(10) ||
    p.tags_text || char(10) ||
    (SELECT h.host FROM entities_fts_project_hosts h WHERE h.project_id = p.project_id)
FROM entities e
JOIN projects p ON p.project_id = e.target
WHERE e.kind = 'Project' AND e.publish = 1 AND e.kind NOT IN ('Redirect', 'WordPress', 'Placeholder')

UNION ALL

SELECT
    e.slug,
    'Event',
    ev.event_id,
    ev.name,
    ev.presenter || char(10) ||
    CASE WHEN ev.entirely_online THEN 'Online' ELSE ev.location_name END || char(10) ||
    (SELECT d.text FROM entities_fts_descriptions d
     WHERE d.entity_type = 'Event' AND d.target = ev.event_id) || char(10) ||
    ev.tags_text
FROM entities e
JOIN events ev ON ev.event_id = e.target
WHERE e.kind = 'Event' AND e.publish = 1 AND e.kind NOT IN ('Redirect', 'WordPress', 'Placeholder')

UNION ALL

SELECT
    e.slug,
    pc.kind,
    pc.pagecontent_id,
    pc.name,
    pc.name || ': ' ||
    CASE
        WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
        ELSE pc.content
    END
FROM entities e
JOIN pagecontent pc ON pc.pagecontent_id = e.target AND pc.kind = e.kind
WHERE e.kind IN ('Wysiwyg', 'DressedHtml', 'RawHtml') AND e.publish = 1 AND e.kind NOT IN ('Redirect', 'WordPress', 'Placeholder');

-- Rebuild from the view. This also drops the duplicate and orphaned rows
-- earlier repopulations left behind.
DELETE FROM entities_fts;
INSERT INTO entities_fts (slug, entity_type, entity_id, title, summary)
SELECT slug, entity_type, entity_id, title, summary FROM entities_fts_documents;

CREATE TRIGGER entities_fts_entity_insert AFTER INSERT ON entities
BEGIN
    INSERT INTO entities_fts (slug, entity_type, entity_id, title, summary)
    SELECT slug, entity_type, entity_id, title, summary
    FROM entities_fts_documents WHERE slug = NEW.slug;
END;

CREATE TRIGGER entities_fts_entity_update AFTER UPDATE ON entities
BEGIN
    DELETE FROM entities_fts WHERE slug = OLD.slug;
    INSERT INTO entities_fts (slug, entity_type, entity_id, title, summary)
    SELECT slug, entity_type, entity_id, title, summary
    FROM entities_fts_documents WHERE slug = NEW.slug;
END;

CREATE TRIGGER entities_fts_entity_delete AFTER DELETE ON entities
BEGIN
    DELETE FROM entities_fts WHERE slug = OLD.slug;
END;

-- A project, event or page may be written before or after the entity that
-- publishes it, so each content table refreshes its entries on every insert,
-- update and delete. Updates carry no column list: any column the view reads
-- must refresh the entry, and the view is the only place that says which.

CREATE TRIGGER entities_fts_project_insert AFTER INSERT ON projects
BEGIN
    DELETE FROM entities_fts WHERE entity_type = 'Project' AND entity_id = NEW.project_id;
    INSERT INTO entities_fts (slug, entity_type, entity_id, title, summary)
    SELECT slug, entity_type, entity_id, title, summary
    FROM entities_fts_documents WHERE entity_type = 'Project' AND entity_id = NEW.project_id;
END;

CREATE TRIGGER entities_fts_project_update AFTER UPDATE ON projects
BEGIN
    DELETE FROM entities_fts WHERE entity_type = 'Project' AND entity_id = OLD.project_id;
    INSERT INTO entities_fts (slug, entity_type, entity_id, title, summary)
    SELECT slug, entity_type, entity_id, title, summary
    FROM entities_fts_documents WHERE entity_type = 'Project' AND entity_id = NEW.project_id;
END;

CREATE TRIGGER entities_fts_project_delete AFTER DELETE ON projects
BEGIN
    DELETE FROM entities_fts WHERE entity_type = 'Project' AND entity_id = OLD.project_id;
END;

CREATE TRIGGER entities_fts_event_insert AFTER INSERT ON events
BEGIN
    DELETE FROM entities_fts WHERE entity_type = 'Event' AND entity_id = NEW.event_id;
    INSERT INTO entities_fts (slug, entity_type, entity_id, title, summary)
    SELECT slug, entity_type, entity_id, title, summary
    FROM entities_fts_documents WHERE entity_type = 'Event' AND entity_id = NEW.event_id;
END;

CREATE TRIGGER entities_fts_event_update AFTER UPDATE ON events
BEGIN
    DELETE FROM entities_fts WHERE entity_type = 'Event' AND entity_id = OLD.event_id;
    INSERT INTO entities_fts (slug, entity_type, entity_id, title, summary)
    SELECT slug, entity_type, entity_id, title, summary
    FROM entities_fts_documents WHERE entity_type = 'Event' AND entity_id = NEW.event_id;
END;

CREATE TRIGGER entities_fts_event_delete AFTER DELETE ON events
BEGIN
    DELETE FROM entities_fts WHERE entity_type = 'Event' AND entity_id = OLD.event_id;
END;

CREATE TRIGGER entities_fts_pagecontent_insert AFTER INSERT ON pagecontent
BEGIN
    DELETE FROM entities_fts
    WHERE entity_type IN ('Wysiwyg', 'DressedHtml', 'RawHtml') AND entity_id = NEW.pagecontent_id;
    INSERT INTO entities_fts (slug, entity_type, entity_id, title, summary)
    SELECT slug, entity_type, entity_id, title, summary
    FROM entities_fts_documents
    WHERE entity_type IN ('Wysiwyg', 'DressedHtml', 'RawHtml') AND entity_id = NEW.pagecontent_id;
END;

CREATE TRIGGER entities_fts_pagecontent_update AFTER UPDATE ON pagecontent
BEGIN
    DELETE FROM entities_fts
    WHERE entity_type IN ('Wysiwyg', 'DressedHtml', 'RawHtml') AND entity_id = OLD.pagecontent_id;
    INSERT INTO entities_fts (slug, entity_type, entity_id, title, summary)
    SELECT slug, entity_type, entity_id, title, summary
    FROM entities_fts_documents
    WHERE entity_type IN ('Wysiwyg', 'DressedHtml', 'RawHtml') AND entity_id = NEW.pagecontent_id;
END;

CREATE TRIGGER entities_fts_pagecontent_delete AFTER DELETE ON pagecontent
BEGIN
    DELETE FROM entities_fts
    WHERE entity_type IN ('Wysiwyg', 'DressedHtml', 'RawHtml') AND entity_id = OLD.pagecontent_id;
END;
