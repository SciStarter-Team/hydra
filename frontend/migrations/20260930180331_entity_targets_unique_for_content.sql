-- A target is unique only among entities that address content: a piece of
-- content has one canonical address, and the reverse lookups and joins from
-- a project, event or page to its slug rely on that. The other kinds share
-- targets legitimately: many old URLs redirect to one new one, two slugs can
-- serve the same WordPress page, and every placeholder's target is empty.
--
-- The kinds excluded here must match `EntityKind::addresses_content`.
DROP INDEX "entities_by_target";

CREATE UNIQUE INDEX "entities_by_target" ON "entities" ("target")
    WHERE "kind" NOT IN ('Redirect', 'WordPress', 'Placeholder');
