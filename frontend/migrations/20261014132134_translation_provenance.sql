-- Who wrote each translation: a human, or a machine translation engine.
--
-- A machine translation is displayed like any other, but it is still work
-- for a translator: the admin lists it as needing review, and it becomes
-- 'human' only when someone edits it or approves it as it stands. Anything
-- that fills translations in bulk from a machine must write 'machine' and
-- must only touch rows that are blank or already 'machine', so it can be
-- re-run without disturbing what a human has written.
--
-- Every translation before this column was written or seeded by a human,
-- which is what the default records. A blank translation has no author, and
-- its provenance means nothing.

ALTER TABLE translation
    ADD COLUMN provenance TEXT NOT NULL DEFAULT 'human'
        CHECK (provenance IN ('human', 'machine'));

-- Whether a translator still has something to do here: nothing has been
-- written, or a machine wrote it. Defined once, here, for every query that
-- sorts finished work from outstanding.
ALTER TABLE translation
    ADD COLUMN needs_work INTEGER NOT NULL
        GENERATED ALWAYS AS (translated = '' OR provenance = 'machine') VIRTUAL;
