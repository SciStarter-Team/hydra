-- Phrases the site displays, and their translations.
--
-- Rows arrive two ways: discovered automatically the first time tr() is
-- called on a phrase, and entered by hand in the admin ahead of the text
-- appearing anywhere. Both land here, distinguished only by `origin`.

CREATE TABLE translation_phrase (
    -- Derived from (context, source) rather than generated, so that two
    -- frontend instances discovering the same phrase agree on its identity
    -- instead of each peering a duplicate.
    phrase_id TEXT NOT NULL PRIMARY KEY,

    -- Empty for almost everything. Set only where the same English wording
    -- needs different translations in different places.
    context TEXT NOT NULL DEFAULT '',

    -- The phrase as written in the source, which is also what is displayed
    -- when no translation exists.
    source TEXT NOT NULL,

    -- 'discovered' | 'content' | 'manual'
    origin TEXT NOT NULL DEFAULT 'discovered',

    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL
) STRICT;

CREATE INDEX translation_phrase_last_seen ON translation_phrase (last_seen);

CREATE TABLE translation (
    phrase_id TEXT NOT NULL REFERENCES translation_phrase (phrase_id) ON DELETE CASCADE,
    lang TEXT NOT NULL,

    -- Empty means "not translated yet": the phrase still displays in its
    -- source language, and the admin lists it as outstanding work.
    translated TEXT NOT NULL DEFAULT '',

    updated TEXT NOT NULL,

    PRIMARY KEY (phrase_id, lang)
) STRICT;
