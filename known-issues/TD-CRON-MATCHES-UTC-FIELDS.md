### TD-CRON-MATCHES-UTC-FIELDS. A task scheduled for 03:00 runs at 03:00 UTC, wherever the user is (lane C, 2026-08-21)

**In short:** The task scheduler lets you say "run this every day at 3 a.m."
It then runs it at 3 a.m. *UTC* — which in New York is 11 p.m. the previous
evening, and in Berlin is 4 or 5 a.m. depending on the season. Nothing in the
program tells the user this. It is not a rounding error; it is the schedule
being off by whole hours, and by a different number of hours in summer than in
winter.

**Where:** `apps/taskscheduler/src/main.rs`, `decompose_timestamp` — it builds
a `guitk::datetime::DateTime` with an explicit `Tz::utc()` and hands the
resulting minute/hour/day/month/weekday to `CronExpr::matches`.

**Why it is still UTC.** Two reasons, and only the first is about plumbing:

1. There is no per-process zone to read
   (`TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`). Every lane-C program that renders
   an instant is in the same position and says so with an explicit
   `Tz::utc()`.
2. Cron under a zone that observes DST has real semantics to settle, and they
   are not obvious. A daily 02:30 task on the spring-forward day has no 02:30
   to run at; on the fall-back day it has two. Every implementation picks a
   rule and they do not agree — Vixie cron runs the skipped job once at the
   moment the clock jumps and runs a repeated job only once; systemd timers
   with `Persistent=` behave differently again. Picking one silently, in the
   commit that merely stops the file having two calendars in it, would be
   inventing user-visible policy in a cleanup.

**The proper fix,** in order:

1. `calculate_next_run` takes a `&Tz` (four production call sites, seven test
   ones), and the scheduler holds the zone rather than assuming one.
2. Settle the DST rule and write it down. Recommended default: match Vixie —
   a wall-clock time skipped by a forward jump fires once at the jump, and a
   wall-clock time repeated by a backward jump fires once. That is what a user
   who has used cron before will expect. This is the part that wants an
   operator decision, and it should be promoted to `open-questions.md` at the
   point where step 1 is otherwise ready, not before.
3. Interval schedules (`Hourly`, `EveryNMinutes`) must *not* be zone-adjusted:
   "every 15 minutes" is a span, not a wall-clock time, and stays elapsed-time
   arithmetic. Only the `Cron` and `Daily`/`Weekly`/`Monthly` arms are
   wall-clock.

**If never fixed:** schedules keep firing at the wrong hour for every user not
in UTC, and shift by an hour twice a year for users who observe DST. It does
not get worse with time, and it is not a data-loss risk — but it makes the
`Daily` frequency effectively useless for its most common purpose (an
overnight job).

**Related:** `design-decisions.md` §491 (the zone is an argument that is never
defaulted), `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`.
