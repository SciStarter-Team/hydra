# Participation Mechanisms

There are three different options that affiliate projects can select
among to credit participation to a specific SciStarter member.

## Email Hashing

Using the *email hashing* mechanism, the affiliate project performs a
one-way cryptographic operation on the participant’s email address,
which results in an alphanumeric code called a “hash”. That operation
can only be run in one direction (you can go from an email address to a
hash, but you can’t go from a hash to an email address).

We’ve already calculated the same hash codes for every SciStarter
account, so when we receive a report containing the hash, we check
whether the hash matches one of our accounts. If it does, we know who to
credit the participation to. If it does not, we have no way of running
the operation backwards to retrieve the email address, so non-members’
privacy is mathematically guaranteed.

We actually store hashes for three variations of each member’s email
address:

1. Exactly as the user entered it
2. All lowercase
3. All lowercase except the first letter, which is capitalized

That allows us to be a bit flexible about capitalization when the user
enters their email address into the affiliate partner.

We can’t accommodate arbitrary combinations of capital and lowercase,
though: the table of hashes for a single email address would be larger
than the storage space of a server, and we have a quarter million email
addresses and counting to deal with.

This is by far the most common mechanism for affiliates to use.

## OpenID Connect

Using the *OpenID Connect* mechanism, the affiliate project has the user
explicitly grant them permission to access data from their SciStarter
account, and record participation to it. This is impossible if the user
does not have a SciStarter account, and with the user giving explicit
permission to the project there are essentially no privacy concerns.

However, this is distinctly more difficult for the affiliate partner to
implement. Only a few have bothered.

## Link Tokens

Using *Link Tokens* allows the user to create a token which the
affiliate then uses to report that user’s participation. It’s the
simplest mechanism for the affiliate to implement, but because it puts a
bit more work on the user (who has to issue the token on their dashboard
page and then copy-and-paste it into a field on the affiliate), it is
rarely used.