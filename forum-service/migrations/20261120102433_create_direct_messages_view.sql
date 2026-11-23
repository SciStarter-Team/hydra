-- Every post in a personal forum, as a direct message: who sent it, who it
-- went to, and whether it was delivered (approved by staff, or let through
-- without review, and not refused since).
--
-- A conversation is between the author of its topic and the owner of the
-- inbox it is in, so a post there goes to whichever of the two did not
-- write it. A copy of an orrery message goes to the recipient orrery names,
-- since the conversation the sync placed it in, its thread's, need not be
-- between the same two.
CREATE VIEW "direct_messages" AS
SELECT p."post_id",
       p."topic_id",
       p."author_id" AS "sender_id",
       COALESCE(om."recipient_id",
                CASE WHEN p."author_id" = t."author_id" THEN t."forum_id" ELSE t."author_id" END)
           AS "recipient_id",
       p."approved" = 1 AND p."is_deleted" = 0 AS "delivered"
FROM "posts" p
JOIN "topics" t ON t."topic_id" = p."topic_id"
JOIN "forums" f ON f."forum_id" = t."forum_id"
LEFT JOIN "orrery_messages" om ON om."post_id" = p."post_id"
WHERE f."slug" LIKE 'inbox-%';
