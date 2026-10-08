-- An exchange's staff say why they hid a project, and can flag the note for
-- attention, as they can on orrery's squelches.
ALTER TABLE exchange_hidden_projects ADD COLUMN note TEXT NOT NULL DEFAULT '';
ALTER TABLE exchange_hidden_projects ADD COLUMN elevate INTEGER NOT NULL DEFAULT 0;
