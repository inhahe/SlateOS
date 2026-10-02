## 545. The lock screen asks someone else whether the password was right, and gets a one-shot ticket back

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous), except the passwordless-account policy, which is deferred to the operator (`open-questions.md`)

**In short:** The lock screen used to check the typed password itself, against a
hash it was handed at startup, and then throw the answer away — nothing outside
the screen could find out that the user had got it right, so the desktop never
actually unlocked. That is now fixed, and fixed in the shape the real system
will need: the screen hands the username and password to something else that
answers, and if the answer is "yes" the screen raises a flag that whoever is
driving it collects exactly once. Four smaller calls came with it, listed
below.

The bug being fixed is recorded in `known-issues.md` as
`TD-C-THE-LOCK-SCREEN-THROWS-AWAY-THE-ANSWER-TO-THE-ONLY-QUESTION-IT-ASKS`.
It shipped with a full green test suite over a screen that could not unlock,
which is why the replacement is proved by `scripts/reintro-lockscreen.py`
rather than by the tests passing.

**The answer is an enum, not a `bool`.** `AuthOutcome` is Accepted / Rejected /
Locked / NoPassword / Unusable / RateLimited — deliberately the same six cases
as lane B's `authlib::Outcome` (§341), so the day the real authenticator is
wired in there is nothing to translate. A `bool` cannot distinguish "wrong
password" from "this account is administratively locked" from "there is no
authenticator at all", and the last of those is precisely the failure that
produced the original bug: with a `bool`, a screen with nothing to check
against reports "wrong password" for ever and looks like user error.

- *Alternative:* keep `bool` and add a separate error channel. Rejected —
  two return paths that must be read together is how the first one got
  ignored.

**A missing authenticator is `Unusable`, not `Rejected`.** They are shown the
same words ("Incorrect password"), because telling a stranger at a locked
screen which accounts are administratively disabled is an information leak, and
because there is nothing the person standing there can do differently either
way. But they are separate variants, and `needs_administrator()` sorts them:
one is a typo, the other needs someone with a key. Sharing the *message* is a
presentation choice; sharing the *variant* would destroy the distinction before
anyone could log it.

**An accepted password authorises one unlock, not a mode.** `submit_password`
sets a flag; `take_unlock_request` takes it and clears it. A wrong guess
afterwards clears it, and so does switching to another account — an
authorisation earned by one user must not survive a switch to another. The
alternative, an `unlocked: bool` the caller polls, cannot express "this was
already spent", so a dropped frame or a re-entrant draw could unlock twice, and
a re-lock that raced the collection would unlock immediately.

**Submitting an empty box costs nothing.** It returns `Rejected` without
counting an attempt or starting a lockout, because a stray Enter is not a
guess. Counting it would let anyone at the screen drive a legitimate user into
a lockout without knowing anything about them.

**The passwordless-account policy is not decided here.** Whether an account
with no password should let the holder straight through or be unable to unlock
at all is a user-visible security policy with a real argument on both sides, so
the behaviour was preserved exactly and the question filed in
`open-questions.md`. It is isolated in one function, `LockScreen::unlocks_for`,
so answering it is a one-line change either way.
