-- Add Wysiwyg, DressedHtml, and RawHtml page content to entities_fts
-- This ensures that only pagecontent attached to entities is searchable
-- Also adds publish filtering for projects and events

-- Step 1: Clear existing entries and repopulate with publish filter
DELETE FROM entities_fts WHERE entity_type IN ('Project', 'Event');

-- Repopulate projects (only published)
INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
SELECT
    e.slug,
    'Project' as entity_type,
    p.project_id,
    p.name as title,
    p.name || ' by ' || p.presenter || ': ' ||
    CASE
        WHEN LENGTH(p.description) > 200 THEN SUBSTR(p.description, 1, 197) || '...'
        ELSE p.description
    END ||
    CASE
        WHEN LENGTH(p.tags_text) > 0 THEN '. Tags: ' || p.tags_text
        ELSE ''
    END as summary
FROM entities e
JOIN projects p ON e.target = p.project_id
WHERE e.kind = 'Project' AND p.publish = 1;

-- Repopulate events (only published)
INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
SELECT
    e.slug,
    'Event' as entity_type,
    ev.event_id,
    ev.name as title,
    ev.name || ' at ' ||
    CASE WHEN ev.entirely_online THEN 'Online' ELSE ev.location_name END || ': ' ||
    CASE
        WHEN LENGTH(ev.description) > 180 THEN SUBSTR(ev.description, 1, 177) || '...'
        ELSE ev.description
    END ||
    CASE
        WHEN LENGTH(ev.tags_text) > 0 THEN '. Tags: ' || ev.tags_text
        ELSE ''
    END as summary
FROM entities e
JOIN events ev ON e.target = ev.event_id
WHERE e.kind = 'Event' AND ev.publish = 1;

-- Step 2: Populate entities_fts with existing Wysiwyg content
INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
SELECT
    e.slug,
    'Wysiwyg' as entity_type,
    pc.pagecontent_id,
    pc.name as title,
    pc.name || ': ' ||
    CASE
        WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
        ELSE pc.content
    END as summary
FROM entities e
JOIN pagecontent pc ON e.target = pc.pagecontent_id
WHERE e.kind = 'Wysiwyg' AND pc.kind = 'Wysiwyg';

-- Populate entities_fts with existing DressedHtml content
INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
SELECT
    e.slug,
    'DressedHtml' as entity_type,
    pc.pagecontent_id,
    pc.name as title,
    pc.name || ': ' ||
    CASE
        WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
        ELSE pc.content
    END as summary
FROM entities e
JOIN pagecontent pc ON e.target = pc.pagecontent_id
WHERE e.kind = 'DressedHtml' AND pc.kind = 'DressedHtml';

-- Populate entities_fts with existing RawHtml content
INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
SELECT
    e.slug,
    'RawHtml' as entity_type,
    pc.pagecontent_id,
    pc.name as title,
    pc.name || ': ' ||
    CASE
        WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
        ELSE pc.content
    END as summary
FROM entities e
JOIN pagecontent pc ON e.target = pc.pagecontent_id
WHERE e.kind = 'RawHtml' AND pc.kind = 'RawHtml';

-- Step 2: Recreate entity INSERT trigger with pagecontent types
DROP TRIGGER IF EXISTS entities_fts_entity_insert;
CREATE TRIGGER entities_fts_entity_insert AFTER INSERT ON entities
BEGIN
    -- Handle project entities (only published projects)
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'Project', p.project_id, p.name,
        p.name || ' by ' || p.presenter || ': ' ||
        CASE
            WHEN LENGTH(p.description) > 200 THEN SUBSTR(p.description, 1, 197) || '...'
            ELSE p.description
        END ||
        CASE
            WHEN LENGTH(p.tags_text) > 0 THEN '. Tags: ' || p.tags_text
            ELSE ''
        END
    FROM projects p
    WHERE NEW.kind = 'Project' AND p.project_id = NEW.target AND p.publish = 1;

    -- Handle event entities (only published events)
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'Event', e.event_id, e.name,
        e.name || ' at ' ||
        CASE WHEN e.entirely_online THEN 'Online' ELSE e.location_name END || ': ' ||
        CASE
            WHEN LENGTH(e.description) > 180 THEN SUBSTR(e.description, 1, 177) || '...'
            ELSE e.description
        END ||
        CASE
            WHEN LENGTH(e.tags_text) > 0 THEN '. Tags: ' || e.tags_text
            ELSE ''
        END
    FROM events e
    WHERE NEW.kind = 'Event' AND e.event_id = NEW.target AND e.publish = 1;

    -- Handle Wysiwyg entities
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'Wysiwyg', pc.pagecontent_id, pc.name,
        pc.name || ': ' ||
        CASE
            WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
            ELSE pc.content
        END
    FROM pagecontent pc
    WHERE NEW.kind = 'Wysiwyg' AND pc.pagecontent_id = NEW.target AND pc.kind = 'Wysiwyg';

    -- Handle DressedHtml entities
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'DressedHtml', pc.pagecontent_id, pc.name,
        pc.name || ': ' ||
        CASE
            WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
            ELSE pc.content
        END
    FROM pagecontent pc
    WHERE NEW.kind = 'DressedHtml' AND pc.pagecontent_id = NEW.target AND pc.kind = 'DressedHtml';

    -- Handle RawHtml entities
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'RawHtml', pc.pagecontent_id, pc.name,
        pc.name || ': ' ||
        CASE
            WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
            ELSE pc.content
        END
    FROM pagecontent pc
    WHERE NEW.kind = 'RawHtml' AND pc.pagecontent_id = NEW.target AND pc.kind = 'RawHtml';
END;

-- Step 3: Recreate entity UPDATE trigger with pagecontent types
DROP TRIGGER IF EXISTS entities_fts_entity_update;
CREATE TRIGGER entities_fts_entity_update AFTER UPDATE ON entities
BEGIN
    DELETE FROM entities_fts WHERE slug = OLD.slug;

    -- Re-insert projects (only published projects)
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'Project', p.project_id, p.name,
        p.name || ' by ' || p.presenter || ': ' ||
        CASE
            WHEN LENGTH(p.description) > 200 THEN SUBSTR(p.description, 1, 197) || '...'
            ELSE p.description
        END ||
        CASE
            WHEN LENGTH(p.tags_text) > 0 THEN '. Tags: ' || p.tags_text
            ELSE ''
        END
    FROM projects p
    WHERE NEW.kind = 'Project' AND p.project_id = NEW.target AND p.publish = 1;

    -- Re-insert events (only published events)
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'Event', e.event_id, e.name,
        e.name || ' at ' ||
        CASE WHEN e.entirely_online THEN 'Online' ELSE e.location_name END || ': ' ||
        CASE
            WHEN LENGTH(e.description) > 180 THEN SUBSTR(e.description, 1, 177) || '...'
            ELSE e.description
        END ||
        CASE
            WHEN LENGTH(e.tags_text) > 0 THEN '. Tags: ' || e.tags_text
            ELSE ''
        END
    FROM events e
    WHERE NEW.kind = 'Event' AND e.target = NEW.target AND e.publish = 1;

    -- Re-insert Wysiwyg
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'Wysiwyg', pc.pagecontent_id, pc.name,
        pc.name || ': ' ||
        CASE
            WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
            ELSE pc.content
        END
    FROM pagecontent pc
    WHERE NEW.kind = 'Wysiwyg' AND pc.pagecontent_id = NEW.target AND pc.kind = 'Wysiwyg';

    -- Re-insert DressedHtml
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'DressedHtml', pc.pagecontent_id, pc.name,
        pc.name || ': ' ||
        CASE
            WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
            ELSE pc.content
        END
    FROM pagecontent pc
    WHERE NEW.kind = 'DressedHtml' AND pc.pagecontent_id = NEW.target AND pc.kind = 'DressedHtml';

    -- Re-insert RawHtml
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'RawHtml', pc.pagecontent_id, pc.name,
        pc.name || ': ' ||
        CASE
            WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
            ELSE pc.content
        END
    FROM pagecontent pc
    WHERE NEW.kind = 'RawHtml' AND pc.pagecontent_id = NEW.target AND pc.kind = 'RawHtml';
END;

-- Step 4: Create pagecontent update trigger
CREATE TRIGGER IF NOT EXISTS entities_fts_pagecontent_update
AFTER UPDATE OF name, content ON pagecontent
BEGIN
    -- Delete all FTS entries for this pagecontent
    DELETE FROM entities_fts
    WHERE entity_id = NEW.pagecontent_id
      AND entity_type IN ('Wysiwyg', 'DressedHtml', 'RawHtml');

    -- Re-insert based on pagecontent kind
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        e.slug,
        NEW.kind as entity_type,
        NEW.pagecontent_id,
        NEW.name,
        NEW.name || ': ' ||
        CASE
            WHEN LENGTH(NEW.content) > 200 THEN SUBSTR(NEW.content, 1, 197) || '...'
            ELSE NEW.content
        END
    FROM entities e
    WHERE e.target = NEW.pagecontent_id
      AND e.kind = NEW.kind
      AND NEW.kind IN ('Wysiwyg', 'DressedHtml', 'RawHtml');
END;
