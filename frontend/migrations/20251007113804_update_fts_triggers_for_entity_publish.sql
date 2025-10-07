-- Update FTS triggers to use entities.publish instead of projects.publish/events.publish
-- Now that publish is on the entity, we can check NEW.publish directly in the trigger

-- Recreate INSERT trigger with entity publish check
DROP TRIGGER IF EXISTS entities_fts_entity_insert;
CREATE TRIGGER entities_fts_entity_insert AFTER INSERT ON entities
BEGIN
    -- Only index published entities
    -- Handle project entities
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
    WHERE NEW.kind = 'Project' AND p.project_id = NEW.target AND NEW.publish = 1;

    -- Handle event entities
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
    WHERE NEW.kind = 'Event' AND e.event_id = NEW.target AND NEW.publish = 1;

    -- Handle Wysiwyg entities (always indexed if entity is published)
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT
        NEW.slug, 'Wysiwyg', pc.pagecontent_id, pc.name,
        pc.name || ': ' ||
        CASE
            WHEN LENGTH(pc.content) > 200 THEN SUBSTR(pc.content, 1, 197) || '...'
            ELSE pc.content
        END
    FROM pagecontent pc
    WHERE NEW.kind = 'Wysiwyg' AND pc.pagecontent_id = NEW.target AND pc.kind = 'Wysiwyg' AND NEW.publish = 1;

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
    WHERE NEW.kind = 'DressedHtml' AND pc.pagecontent_id = NEW.target AND pc.kind = 'DressedHtml' AND NEW.publish = 1;

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
    WHERE NEW.kind = 'RawHtml' AND pc.pagecontent_id = NEW.target AND pc.kind = 'RawHtml' AND NEW.publish = 1;
END;

-- Recreate UPDATE trigger with entity publish check
DROP TRIGGER IF EXISTS entities_fts_entity_update;
CREATE TRIGGER entities_fts_entity_update AFTER UPDATE ON entities
BEGIN
    DELETE FROM entities_fts WHERE slug = OLD.slug;

    -- Re-insert projects (only if entity is published)
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
    WHERE NEW.kind = 'Project' AND p.project_id = NEW.target AND NEW.publish = 1;

    -- Re-insert events (only if entity is published)
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
    WHERE NEW.kind = 'Event' AND e.event_id = NEW.target AND NEW.publish = 1;

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
    WHERE NEW.kind = 'Wysiwyg' AND pc.pagecontent_id = NEW.target AND pc.kind = 'Wysiwyg' AND NEW.publish = 1;

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
    WHERE NEW.kind = 'DressedHtml' AND pc.pagecontent_id = NEW.target AND pc.kind = 'DressedHtml' AND NEW.publish = 1;

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
    WHERE NEW.kind = 'RawHtml' AND pc.pagecontent_id = NEW.target AND pc.kind = 'RawHtml' AND NEW.publish = 1;
END;
