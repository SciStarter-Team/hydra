-- Create unified entities FTS5 table for cross-type search
-- This complements the existing projects_fts and events_fts tables (hybrid approach)
-- Tags are folded into the summary for simplified "search everything" experience

CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
    slug UNINDEXED,         -- URL path for the entity
    entity_type UNINDEXED,  -- 'project', 'event', 'html', etc.
    entity_id UNINDEXED,    -- target ID (project_id, event_id, etc.)
    title,                  -- unified title field across all entity types
    summary                 -- smart-generated summary including tags
);

-- Populate entities_fts with existing projects
-- Summary format: "Name by Presenter: Description. Tags: tag1 tag2 tag3"
INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
SELECT 
    e.slug,
    'project' as entity_type,
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
WHERE e.kind = 'project';

-- Populate entities_fts with existing events  
-- Summary format: "Name at Location: Description. Tags: tag1 tag2 tag3"
INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
SELECT 
    e.slug,
    'event' as entity_type, 
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
WHERE e.kind = 'event';

-- TODO: Add population for additional entity types here
-- Example patterns for future entity types:
--
-- For HTML entities:
-- INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
-- SELECT e.slug, 'html', h.html_id, h.title, 
--        h.title || ': ' || SUBSTR(h.content_text, 1, 200) ||
--        CASE WHEN LENGTH(h.tags_text) > 0 THEN '. Tags: ' || h.tags_text ELSE '' END
-- FROM entities e JOIN htmls h ON e.target = h.html_id WHERE e.kind = 'html';
--
-- For forum entities:  
-- INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
-- SELECT e.slug, 'forum', f.forum_id, f.name,
--        f.name || ': ' || SUBSTR(f.description, 1, 200) ||
--        CASE WHEN LENGTH(f.tags_text) > 0 THEN '. Tags: ' || f.tags_text ELSE '' END
-- FROM entities e JOIN forums f ON e.target = f.forum_id WHERE e.kind = 'forum';
--
-- For user profile entities:
-- INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary) 
-- SELECT e.slug, 'user', u.user_id, u.display_name,
--        u.display_name || ': ' || SUBSTR(u.bio, 1, 200) ||
--        CASE WHEN LENGTH(u.interests) > 0 THEN '. Interests: ' || u.interests ELSE '' END
-- FROM entities e JOIN users u ON e.target = u.user_id WHERE e.kind = 'user';