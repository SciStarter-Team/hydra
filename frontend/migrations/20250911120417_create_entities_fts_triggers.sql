-- Create triggers to keep entities_fts synchronized with source tables
-- Uses a simplified "rebuild" approach to minimize code duplication

-- =============================================================================
-- ENTITY LIFECYCLE TRIGGERS
-- =============================================================================

-- When an entity is created, populate entities_fts
CREATE TRIGGER IF NOT EXISTS entities_fts_entity_insert AFTER INSERT ON entities
BEGIN
    -- Handle project entities
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT 
        NEW.slug, 'project', p.project_id, p.name,
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
    WHERE NEW.kind = 'project' AND p.project_id = NEW.target;
    
    -- Handle event entities  
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT 
        NEW.slug, 'event', e.event_id, e.name,
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
    WHERE NEW.kind = 'event' AND e.event_id = NEW.target;
    
    -- Add new entity types here following the same pattern
END;

-- When an entity is deleted, remove from entities_fts
CREATE TRIGGER IF NOT EXISTS entities_fts_entity_delete AFTER DELETE ON entities
BEGIN
    DELETE FROM entities_fts WHERE slug = OLD.slug;
END;

-- When ANY entity field changes, rebuild the entry (handles slug, kind, target changes)
CREATE TRIGGER IF NOT EXISTS entities_fts_entity_update AFTER UPDATE ON entities
BEGIN
    DELETE FROM entities_fts WHERE slug = OLD.slug;
    
    -- Re-insert using the same logic as the INSERT trigger
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT 
        NEW.slug, 'project', p.project_id, p.name,
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
    WHERE NEW.kind = 'project' AND p.project_id = NEW.target;
    
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT 
        NEW.slug, 'event', e.event_id, e.name,
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
    WHERE NEW.kind = 'event' AND e.event_id = NEW.target;
END;

-- =============================================================================
-- CONTENT UPDATE TRIGGERS (rebuild approach)
-- =============================================================================

-- When project content changes, rebuild entities_fts entries for this project
CREATE TRIGGER IF NOT EXISTS entities_fts_project_update 
AFTER UPDATE OF name, presenter, description, tags_text ON projects
BEGIN
    DELETE FROM entities_fts WHERE entity_type = 'project' AND entity_id = NEW.project_id;
    
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT 
        e.slug, 'project', NEW.project_id, NEW.name,
        NEW.name || ' by ' || NEW.presenter || ': ' || 
        CASE 
            WHEN LENGTH(NEW.description) > 200 THEN SUBSTR(NEW.description, 1, 197) || '...'
            ELSE NEW.description
        END ||
        CASE 
            WHEN LENGTH(NEW.tags_text) > 0 THEN '. Tags: ' || NEW.tags_text
            ELSE ''
        END
    FROM entities e 
    WHERE e.kind = 'project' AND e.target = NEW.project_id;
END;

-- When event content changes, rebuild entities_fts entries for this event
CREATE TRIGGER IF NOT EXISTS entities_fts_event_update
AFTER UPDATE OF name, location_name, entirely_online, description, tags_text ON events
BEGIN
    DELETE FROM entities_fts WHERE entity_type = 'event' AND entity_id = NEW.event_id;
    
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT 
        e.slug, 'event', NEW.event_id, NEW.name,
        NEW.name || ' at ' || 
        CASE WHEN NEW.entirely_online THEN 'Online' ELSE NEW.location_name END || ': ' ||
        CASE 
            WHEN LENGTH(NEW.description) > 180 THEN SUBSTR(NEW.description, 1, 177) || '...'
            ELSE NEW.description  
        END ||
        CASE 
            WHEN LENGTH(NEW.tags_text) > 0 THEN '. Tags: ' || NEW.tags_text
            ELSE ''
        END
    FROM entities e
    WHERE e.kind = 'event' AND e.target = NEW.event_id;
END;

-- To add a new entity type:
-- 1. Add INSERT logic to entities_fts_entity_insert trigger
-- 2. Add INSERT logic to entities_fts_entity_update trigger  
-- 3. Add a content update trigger following the pattern above