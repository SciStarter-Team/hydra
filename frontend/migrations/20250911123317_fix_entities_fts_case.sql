-- Fix entities_fts population and triggers - correct case for entity kinds
-- The previous migration used lowercase but the entities table has 'Project'/'Event'

-- Clear any existing entries (likely none due to the case mismatch)
DELETE FROM entities_fts;

-- Populate entities_fts with existing projects (correct case: 'Project')
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
WHERE e.kind = 'Project';

-- Populate entities_fts with existing events (correct case: 'Event')
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
WHERE e.kind = 'Event';

-- TODO: Add population for new entity types here
-- Example for HTML entities:
-- INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
-- SELECT 
--     e.slug, 'Html', h.html_id, h.title,
--     h.title || ': ' || 
--     CASE 
--         WHEN LENGTH(h.content_text) > 200 THEN SUBSTR(h.content_text, 1, 197) || '...'
--         ELSE h.content_text
--     END ||
--     CASE 
--         WHEN LENGTH(h.tags_text) > 0 THEN '. Tags: ' || h.tags_text
--         ELSE ''
--     END as summary
-- FROM entities e
-- JOIN htmls h ON e.target = h.html_id
-- WHERE e.kind = 'Html';

-- Drop and recreate triggers with correct case
DROP TRIGGER IF EXISTS entities_fts_entity_insert;
DROP TRIGGER IF EXISTS entities_fts_entity_update;

-- Recreate triggers with correct case handling
CREATE TRIGGER IF NOT EXISTS entities_fts_entity_insert AFTER INSERT ON entities
BEGIN
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
    WHERE NEW.kind = 'Project' AND p.project_id = NEW.target;
    
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
    WHERE NEW.kind = 'Event' AND e.event_id = NEW.target;
    
    -- TODO: Add handling for new entity types here
    -- Example for HTML entities:
    -- INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    -- SELECT 
    --     NEW.slug, 'Html', h.html_id, h.title,
    --     h.title || ': ' || SUBSTR(h.content_text, 1, 200) ||
    --     CASE WHEN LENGTH(h.tags_text) > 0 THEN '. Tags: ' || h.tags_text ELSE '' END
    -- FROM htmls h 
    -- WHERE NEW.kind = 'Html' AND h.html_id = NEW.target;
END;

-- When ANY entity field changes, rebuild the entry
CREATE TRIGGER IF NOT EXISTS entities_fts_entity_update AFTER UPDATE ON entities
BEGIN
    DELETE FROM entities_fts WHERE slug = OLD.slug;
    
    -- Re-insert with correct case
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
    WHERE NEW.kind = 'Project' AND p.project_id = NEW.target;
    
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
    WHERE NEW.kind = 'Event' AND e.event_id = NEW.target;
    
    -- TODO: Add handling for new entity types here (same as in INSERT trigger above)
    -- Also remember to add content update triggers for new entity types following this pattern:
    -- 
    -- CREATE TRIGGER IF NOT EXISTS entities_fts_html_update
    -- AFTER UPDATE OF title, content_text, tags_text ON htmls
    -- BEGIN
    --     DELETE FROM entities_fts WHERE entity_type = 'Html' AND entity_id = NEW.html_id;
    --     INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    --     SELECT e.slug, 'Html', NEW.html_id, NEW.title, 
    --            NEW.title || ': ' || SUBSTR(NEW.content_text, 1, 200) ||
    --            CASE WHEN LENGTH(NEW.tags_text) > 0 THEN '. Tags: ' || NEW.tags_text ELSE '' END
    --     FROM entities e WHERE e.kind = 'Html' AND e.target = NEW.html_id;
    -- END;
END;