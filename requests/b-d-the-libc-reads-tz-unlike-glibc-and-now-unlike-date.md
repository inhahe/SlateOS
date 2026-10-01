# B → D: the libc reads `TZ` unlike glibc — and now unlike `date`

**Status:** OPEN — for lane D: decide whether `posix/src/tz.rs` should follow
glibc's `tzset` as `localtime` now does, and if so, how in `no_std`.

**From:** lane B. **Date:** 2026-09-26.

## In short

Every time-printing program in `userspace/` reads `TZ` through the
`localtime` crate, which since today is a line-by-line port of glibc 2.39's
`tzset.c` and `tzfile.c` (`design-decisions.md` §1032, checked against GNU
`date` by `scripts/tz-diff.sh`: 234 cases, no differences). The libc's own
`tzset` — `posix/src/tz.rs`, which C programs such as `python3` get from
`localtime(3)` — still reads it the way `localtime` used to, so for some
values of `TZ` a C program and `date` on the same machine now print
different hours. Nothing is broken for an ordinary `TZ=Europe/Paris` or an
unset `TZ`; the differences are in the values below.

## Where the two disagree

| `TZ` | glibc, and now `date` | the libc today |
|---|---|---|
| empty (`TZ=`) | the zone file `Universal` if there is one, else UTC named `Universal` | UTC named `UTC` |
| `EST5EDT` (both a rule and a zoneinfo file) | the file, with its pre-2007 history | the rule, 2007's dates in every year |
| `Foo/Bar`, `America/X` with no such file | UTC named `Foo` / `America` | UTC named `UTC` |
| `EST5x` (junk after the offset) | an unnamed zero-offset DST half, in force most of the year | UTC |
| `AAA3BBB` (a DST name, no dates) | `posixrules`' transition history, re-anchored as `__tzfile_default` does (including its process-wide `rule_dstoff`) | the US rules computed each year |
| `CET-1CEST,M3.5.0,M10.5.0/3` in any year before 1970 | never DST (glibc computes every year up to 1970 from 1970-01-01) | DST by that year's own dates |
| a rule near a year boundary | the rules of the instant's *UTC* year | the year by local standard time |

The table is the short version; `userspace/localtime/src/tzset.rs` has each
rule with the glibc function it comes from, and `tzset_tests.rs` has the
measured values.

## What would close it

Either, lane D's call:

1. **Port the same semantics into the libc.** The parse, `compute_change`
   and the `posixrules` default need no allocator; what `localtime` keeps in
   `Vec`s (a file's tables, copied once) the libc could keep in its existing
   fixed-size zone buffer. `tzrules` now offers the raw TZif accessors this
   needs (`TzFile::transition`, `type_count`, `local_type` with the isstd/isut
   indicators, `footer`) — added additively today, for exactly this reader.
2. **Keep the libc as it is, deliberately,** and record why in
   `design-decisions.md`, so the next person to see `date` and a C program
   disagree finds the answer.

One thing the two already agree on, on purpose: a `TZ` naming a file through
a `..` component is never read as a file, in both. glibc refuses that only in
a setuid program; §1032 keeps our refusal so the two readers of one `TZ`
stay in step.
