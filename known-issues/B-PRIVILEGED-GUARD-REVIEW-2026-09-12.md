## B-PRIVILEGED-GUARD-REVIEW-2026-09-12 (lane B) -- COMPLETE, and the note on it is written carefully

**What was done.** Every `-> bool`/`-> Option<bool>` predicate in the programs
that grant a privilege -- `doas`, `sudo`, `su`, `login`, `passwd`, `newgrp`,
`crontab`, `at` -- read against one question:

> **What fact does this actually establish, and is that the fact the caller
> needs?**

Not *what does it do when it fails*. That question had already been asked of
these same files on 2026-09-10 and had found them clean.

**Four defect clusters, all fixed, all in the SUCCESS path:**

| Guard | Established | Caller needed |
|---|---|---|
| `doas::user_in_group` | listed in `/etc/group`'s member field | *is a member* -- the field omits primary members, so `deny :wheel` stopped applying to the likeliest members |
| `sudo::check_timestamp` | recorded number is within N of the clock | *authenticated within N seconds* -- differs whenever the number is in the future or is the invalidation sentinel |
| `doas`/`sudo` command specs | the last path component matches | *this is the command the rule names* -- differs the moment the caller supplies a path |
| `sudo` host/user/runas lists | some entry mentions this | *does this rule apply* -- differs the moment the list holds an exception |

**Five guards the question CONFIRMED**, which is the ordinary outcome and is
worth recording so they are not re-read:

* `doas::persist_valid` -- already refused a future timestamp, in the same tree
  where `sudo` did not. The `sudo` fix was not invented; the correct version
  was one program over.
* `newgrp::user_is_member` -- checks the primary gid explicitly, which is the
  thing `doas` was missing.
* `passwd::has_password` -- both callers use `false` in the safe direction.
* `login::check_account_expired` -- an unreadable clock skips *date* expiry and
  still enforces the lock, with the trade written out in the code.
* `at::atd_may_run` -- requires the daemon to already be the submitting user
  and refuses when it cannot tell, because `atd` cannot change user and
  running the job "would give it authority its submitter did not have".

### The note itself, and why it is worded like this

The 2026-09-10 sweep left this on `sudo::check_timestamp`:

> Checked ... this one is correct as written and is noted so the next sweep
> does not have to re-derive it.

It was accurate about every arm it examined, and it is the sentence that would
have stopped the next reader finding two live defects in the lines below it.
A note that says *checked* invites the reader to skip; a note that says **what
question was asked** lets them decide whether their question is different.

So: the guards above were checked on 2026-09-12 against *what fact does this
establish*. **That is not a statement that they are correct against some other
question**, and a sweep arriving with a different one should read them again.
