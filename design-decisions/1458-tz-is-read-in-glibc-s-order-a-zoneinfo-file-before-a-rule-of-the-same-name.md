## 1458. `TZ` is read in glibc's order: a zoneinfo file before a rule of the same name

**Date:** 2026-10-01 &middot; **Decided by:** Claude (autonomous), correcting
§875, whose table said "glibc's order" and gave a different one &middot;
**Lane:** C, with B and D

**In short:** A program here could show a different hour from the same
program on Linux. Setting the time zone to `EST5EDT` -- a name that is both
a file of United States history and a rule describing today -- gave the rule
(08:00 summer time on 20 March 1990) where Linux gives the file (07:00
standard time; daylight saving began in April then). The shared code that
decides what `TZ` means (`tzrules`) now decides as glibc does, step for step,
and the desktop clock follows it. The C library moves to it next (lane D).

**What changed:** `tzrules::tz_plan` replaces `tz_source` (kept, unchanged,
until the libc moves). It answers a plan -- the zoneinfo file to try, and the
text to read as a rule if there is none -- rather than one source, because
"a file first" depends on whether the file can be read, which only the
caller can find out.

| Question | Before (§875) | Now (glibc 2.39, measured in WSL) |
|---|---|---|
| `EST5EDT`: a file and a rule | the rule | the file; the rule only without it |
| A leading `:` | "a file, never a rule" | dropped, and nothing more: `:EST5EDT` with no file is the rule |
| `::EST5EDT` | a file named `:EST5EDT` | the same file, then UTC (no rule starts with `:`) |
| Empty `TZ` | UTC, reading nothing | the file `Universal`, then UTC |
| `TZ=/etc/localtime` | that path, else UTC | the same as unset |
| A name with `..` or a NUL | refused: UTC | never opened; the text goes on to the rule, as a missing file's does -- which no such name parses as, so UTC |
| A file that parses but states no rule past its records | the desktop read it as UTC | the state its last record left (glibc's reading) |

**Not changed, deliberately:** this crate's rule engine still takes a rule
whole or not at all, and gives a rule with no dates the United States' rules
since 2007. glibc keeps half a rule (`TZ=Foo/Bar` is UTC named `Foo`) and
gives a dateless rule the history of the file `posixrules` where one exists.
Both are the engine's behaviour, not the order's: `TzPlan::rule` hands the
text to an engine that ports glibc's (lane B's `localtime`), and whether the
libc's engine should is
`requests/b-d-the-libc-reads-tz-unlike-glibc-and-now-unlike-date.md`'s
question. SlateOS ships no zoneinfo tree, so neither difference shows today.
