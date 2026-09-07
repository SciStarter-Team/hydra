-- A one-time URL that performs an action for whoever accepts it, once they
-- are signed in. orrery mints the same kind of link; the two systems keep
-- their own rows, but a row is plain data -- a handler name and its
-- arguments -- rather than serialized code, so neither has to be able to run
-- the other's implementation to describe the same invitation.

create table "invitations" (
       "invitation_id" text not null,
       "kind" text not null,
       "payload" text not null, -- JSON, shaped by the kind
       "created" text not null
);

create unique index "invitations_by_invitation_id" on "invitations" ("invitation_id");
