## TD-B-EVERY-SYSLOG-TIMESTAMP-WE-WRITE-IS-UTC-WHERE-SYSLOG-MEANS-LOCAL-TIME (lane B, 2026-09-16) — FIXED 2026-09-26

**Status: FIXED 2026-09-26**, found 2026-09-16 while bringing `logger`'s frame
in line with util-linux 2.39.3.

**The fix.** The "small shared local-time facility" this entry asked for now
exists -- `userspace/localtime`, glibc's `tzset`/`tzfile` resolution of `TZ`,
`TZDIR` and `/etc/localtime` -- and `logger` is a caller. First the old
program's RFC 3164 TIMESTAMP moved to local time (ee4be2820); then the whole
program was replaced by a port of util-linux 2.39.3's `logger.c` (413e56f1d,
design-decisions §1033), whose clocks are all upstream's: the local and
RFC 3164 headers are `rfc3164_current_time`, `localtime_r` of the current
second formatted `Mmm dd hh:mm:ss` with upstream's English month table, and
the RFC 5424 header is local time with microseconds and its numeric offset,
`2026-09-26T06:37:16.599632-04:00`. The old program's own `--rfc3339` (UTC
with `Z`) went with it: util-linux has no such option and refuses it.
`scripts/logger-diff.sh` compares every header form in seven zones --
including a half-hour zone, a POSIX-rule `TZ` and `/etc/localtime` -- reading
each clock as a number within the time the case took, so a zone error shows
as the hours it is.

**In short:** the time stamped on every line `logger` writes is UTC, but the
syslog format it is writing means *local* time. On a machine four hours behind
UTC the two differ by four hours, so a log line records an hour the event did
not happen at. Nothing crashes and nothing is refused — the damage is done
later, by whoever reads the log, or by anything that lines our entries up
against timestamps from another program that got it right.

### How it shows

Same moment, same format, our build and the reference side by side:

```
ours: <13>Sep 16 11:33:22 mytag: hi
ref : <13>Sep 16 07:33:22 mytag: hi
```

RFC 3164's `TIMESTAMP` is local time with no zone marker at all, which is
exactly why the difference is invisible in the line itself: there is no field
that says which of the two readings is meant, so a wrong one cannot be
detected by looking at it. It took a side-by-side run against a reference on
the same machine to see it.

### Where it lives

`userspace/logger/src/main.rs`, `format_timestamp()` — its own comment says
"simplified UTC" and it is doing exactly what it says. The bug is not that it
lies; it is that the caller frames the result as RFC 3164, which means
something else.

`format_rfc3339()` (used for `--rfc3339`) is *not* affected, and this was
checked rather than assumed: it ends its output with `Z`, so
`<13>2026-09-16T11:34:25Z t: hi` states its zone and is simply true. That is
the whole difference — RFC 3339 has a field for the answer and RFC 3164 does
not, so the same clock is honest in one format and wrong in the other.

(Noted in passing: `--rfc3339` is ours alone. The reference answers
`logger: unrecognized option '--rfc3339'`; util-linux spells these
`--rfc3164` and `--rfc5424`. An extension is not a defect, but it is worth
knowing that `scripts/option-gap.sh` could not have told us — it looks for
options the reference has and we reject, never for options we have and the
reference does not.)

### Why it is not a one-line fix

The function needs a *timezone*, and there is no obvious place to get one:

- there is no `TZ` handling anywhere in this program, and reading `$TZ` alone
  would not be enough — the value is a name like `America/New_York`, which
  needs a zoneinfo database to turn into an offset, and it is not clear the
  image ships one;
- the offset is not constant, so a single number recorded at boot is wrong
  twice a year;
- `logger` is not the only writer. Anything else in the tree that stamps a
  local time has the same question, and answering it once in a shared place is
  worth more than answering it here.

So the proper fix is a small shared "local time" facility with a single
documented source of the offset, and `logger` as its first caller — not a
constant added inside `format_timestamp`.

### Until then

UTC is at least *consistent* and self-consistent across our own programs, and
an hour that is uniformly four hours off is easier to reason about than a mix.
Do not "fix" this by adding a fixed offset: that is correct for half the year
in one place and wrong everywhere else, and it would make the error
intermittent instead of constant, which is strictly harder to notice.
