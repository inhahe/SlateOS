## §352 — Password entries written by the old, broken hashers stay refused; nobody is let in one last time

**Date:** 2026-08-21
**Decided by:** Operator (option A; Claude recommended this option)

**In short:** `/etc/shadow` stores one scrambled password per user. It used to be
written by three programs that disagreed about *how* to scramble, so a password
set with `passwd` could not be used to log in at all. That is fixed — all three
now share one implementation (§329). The leftover question was what to do about
entries the old code already wrote. They are refused: those accounts cannot log
in until root runs `passwd <user>`, and `login` says so. The alternative was to
accept such an entry once and silently rewrite it during login. We keep
refusing.

**Answers:** `open-questions.md` B-Q3 (deleted from that file by this entry).
**Related:** §329 (the three disagreeing hashers, and the two authentication
bypasses found in `login` while fixing them), §341 (`authlib`).

### What makes this a small decision rather than a risky one

The two populations are distinguishable with certainty, not heuristically. A
correct entry's hash field is exactly 22, 43 or 86 characters depending on the
method and drawn from the standard alphabet; every entry the old code wrote is
exactly 64 characters from a different alphabet. There is no overlap, so a
correct entry can never be mistaken for a broken one or the reverse. Both
options were therefore *implementable*; the choice is about what should happen,
not about what can be detected.

It also only affects test accounts on a development machine. Nothing has
shipped.

### Why A

Option B's entire benefit is saving one `passwd` command per account. It buys
that by keeping code alive in `login` that treats a known non-hash as if it were
a password check — in the one place in the system where a wrong answer is a
break-in — and by adding a rewrite of `/etc/shadow` to the login path, running
as root before privileges are dropped. A rewrite interrupted by power loss
damages the file that gates every login. That is a poor trade for a saved
keystroke.

Option C — an offline converter — cannot work and is recorded only so it is
visibly ruled out rather than quietly overlooked. The old scramble is not
reversible enough to re-derive a correct hash from, so the converter could only
*blank* the entries, which is option A with extra steps and a new tool.

The failure mode of A is "a developer resets a test password." The failure mode
of B is "authentication code that accepts something that is not a hash." Those
are not comparable.

**Revisit if:** this ever recurs *after* the system has real users — i.e. a
future hash-format migration on a machine somebody depends on. The reasoning
above is about a development tree; on a live machine the arithmetic of "one
command per account" changes, and the right answer there is a planned migration
with the accounts offline, not a rewrite inside the login path.
