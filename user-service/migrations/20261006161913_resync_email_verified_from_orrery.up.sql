-- Bring the addresses of the accounts mirrored from orrery back into step
-- with the accounts themselves.
--
-- The user sync wrote `users.verified` from orrery on every pass, but wrote
-- `emails.verified` only when an address first arrived. An account confirmed
-- in orrery after that was verified with no verified address, and one whose
-- mail had since bounced was unverified with an address that still said
-- otherwise. Whatever then re-derived the account's standing from its
-- addresses -- an admin edit, adding or removing an address, a bounce --
-- reached the opposite answer to orrery's.
--
-- The sync now applies this same rule on every pass: an unverified account
-- has no verified address, and a verified one's primary address is verified.
-- Its other addresses are left as they are; nothing in orrery speaks for
-- them one way or the other.
--
-- Accounts created here have no orrery record and are not touched.
update "emails"
set "verified" = (
    select u."verified" and (u."primary_email" = "emails"."email" or "emails"."verified")
    from "users" u
    where u."user_id" = "emails"."user_id"
)
where "user_id" in (select "user_id" from "users" where "legacy_v3_id" is not null);
