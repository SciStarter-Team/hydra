-- A link to a contact form or page through which staff can reach the
-- contact, beside or instead of an email address. Orrery added it for
-- events (core 0294), for organizations that publish only a web contact
-- form; a contact here needs an email, this link, or both, so `email` may
-- be empty when `url` is not.
ALTER TABLE event_contacts ADD COLUMN url text;
ALTER TABLE project_contacts ADD COLUMN url text;
