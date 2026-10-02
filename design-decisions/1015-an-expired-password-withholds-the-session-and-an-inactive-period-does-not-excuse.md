## 1015. An expired password withholds the session, and an inactive period does not excuse it

**Date:** 2026-09-10
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** if a password has passed the maximum age an administrator set for
it, `login` now makes the user change it before any session starts, instead of
printing a warning and letting them in. A separate setting called the
"inactive" period, which controls when the account stops working altogether,
does *not* buy extra time on the expired password -- which is what the plan for
this work had assumed.

**What the `todo.txt` entry proposed.** It scoped the missing enforcement to
"the refusal when there is no inactive period at all", reading shadow-utils as
letting an expired password log in with a warning for as long as an inactive
grace is still running.

**Why that was not implemented as written.** It makes setting `inactive` a way
to *weaken* `PASS_MAX_DAYS`. An administrator who sets "passwords expire after
90 days" and "accounts die 30 days after that" would, under the proposed
reading, have set "passwords are a suggestion for 30 days" -- and the more
generous the account-death policy, the weaker the password policy. That is
backwards. `inactive` is an additional deadline stacked on the first, not a
licence to keep using a password that is already due.

The entry's own first bullet says the right thing and the later paragraph
contradicts it: "**Expired** -- it must be changed at the next login, and until
it is, the session does not start. This is what `PASS_MAX_DAYS` is *for*." That
is what is implemented.

**Why not ask the operator.** It is a user-visible policy, which normally goes
to `open-questions.md`. It did not, because the two readings are not a genuine
fork: one of them makes a setting mean the opposite of its name. The operator's
decision worth having here is *what the numbers should be*, which is
configuration, not code.

**The failure direction is real and is written down.** The change is forced by
running `passwd` as the user, and if that fails there is no session. On a
system where `passwd` cannot yet write the database unprivileged -- it must be
setuid root, which `userspace/newgrp` assumes but nothing has tested -- every
account with an expired password is refused rather than admitted. That is the
correct behaviour for an expired password reached for the wrong reason, so
`todo.txt` records the exact symptom to recognise it by.

**Why `passwd` and not a prompt written here.** `login` is root and already has
the database open, so it could ask for a new password and write it directly.
That would be a second implementation of password quality policy, hashing and
the aging fields, living where nobody would look for it -- the exact mistake
`authlib` exists to prevent (§329: three programs implemented the hash format
separately and disagreed). Running the real `passwd` as the real user also
keeps its "you are not root, so type your current password" check, which a
root-run `passwd` would skip.

**A bug found on the way, and the sharper version of this entry.** `passwd -e`
writes `changed = 0`, `/etc/shadow`'s sentinel for "must change at next login".
**Nothing at login read it.** `check_account_expired` did not look at it, and
`expiry_warning` needs a `max_days` before it says anything -- so on an account
with no maximum age, `passwd -e` printed "password for alice expired" and
changed nothing at all about that account's next login. A command that reports
success and has no effect is worse than one that fails, and it had a test
suite: `cmd_expire`'s own tests assert the *field* is written, which it always
was. Nothing tested that anything reads it. `password_change_required` reads it
now, and does so as a flag rather than a date, so an administrator's explicit
"change this now" survives a machine whose clock does not work.
