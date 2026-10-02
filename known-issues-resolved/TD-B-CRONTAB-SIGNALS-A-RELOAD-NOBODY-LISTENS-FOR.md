## TD-B-CRONTAB-SIGNALS-A-RELOAD-NOBODY-LISTENS-FOR (lane B, 2026-09-11) -- FIXED 2026-09-11

**FIXED by deleting the write,** which is the option this entry named as
the better default. One thing the entry did not know when it was filed, and
which settles it rather than merely favouring it: implementing the watcher
could not have helped either. `crond` sleeps to the next MINUTE BOUNDARY
and reloads when it wakes, and cron's granularity is one minute -- so the
earliest a newly added job can run is exactly the moment the daemon
re-reads the spool. An instant reload would notice the change sooner and
still run nothing sooner.


**In short:** `crontab` writes a file at `/run/crond/reload` after editing a
crontab, meaning "daemon, re-read the spool". `crond` has no reload watcher —
the string `/run/crond/reload` appears nowhere in it. The write goes to a path
nothing reads.

**Why it is harmless today, which is why it is here and not fixed.** `crond`
calls `load_all_crontabs()` on every iteration of its main loop, so an edited
crontab is picked up on the next tick regardless. The signal is redundant
machinery, not a broken path: removing it changes nothing observable, and
implementing a watcher would only shorten a delay that is already bounded by
the loop period.

**The proper fix is one of two, and they point opposite ways.** Either delete
the write in `userspace/crontab` (`RELOAD_SIGNAL_PATH`, and the function that
writes it), because a signal nobody reads is a claim the program does not
honour — §1006's reasoning applied to a side effect rather than a command. Or
have `crond` check for the file and reload early, if the tick latency ever
matters. Deleting is the better default: the machinery has never worked, so
nothing can be relying on it.

**Found** while tracing where `crontab` writes, which is how the spool-path
divergence next to it turned up — `crontab` wrote `/var/spool/cron/alice`
while both daemons read `/var/spool/cron/crontabs/alice`, so jobs never ran.
That one is fixed (`cronspool`, 2026-09-11).
