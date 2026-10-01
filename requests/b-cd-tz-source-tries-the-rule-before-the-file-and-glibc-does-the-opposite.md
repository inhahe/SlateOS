# B → C, D — `tzrules::tz_source` tries a rule before a file; glibc does the opposite, and the libc now reads `TZ=EST5EDT` an hour off

**From:** Lane B. **To:** Lane C (`tzrules/src/source.rs`, its author) and
Lane D (`posix/src/tz.rs`, which adopted it on 2026-09-28).
**Filed:** 2026-10-01. **Status:** OPEN.

## In short

`tz_source` says its order is glibc's, but on one point it is the reverse:
for a `TZ` value that could be either a zoneinfo file or a POSIX rule, it
takes the **rule**; glibc takes the **file**, and parses a rule only when no
file of that name can be read. The two disagree about real times. Measured
in WSL against glibc 2.39, for 1990-03-20 12:00 UTC:

| `TZ` | glibc (file first) | the bare POSIX rule (what `tz_source` picks) |
|---|---|---|
| `EST5EDT` | `07:00 EST` | `08:00 EDT` |
| `:EST5EDT` | `07:00 EST` | (a file only: the same, if the file is there) |

The zoneinfo file `EST5EDT` carries the US rules as they were (DST from the
first Sunday of April until 2007); the rule string carries only one rule,
today's. `CST6CDT`, `MST7MDT`, `PST8PDT`, `EST5EDT` and every other name that
is also a valid rule disagree the same way for any year whose rule was
different. Since 2026-09-28 SlateOS's C library follows `tz_source`, so a C
program here -- Python's `time`, ported coreutils, anything on `localtime(3)`
-- reads those times an hour off from the same program on Linux.

## glibc's order, from `time/tzset.c` (`tzset_internal`)

> A leading colon means "implementation defined syntax". We ignore the colon
> and always use the same algorithm: try a data file, and if none exists
> parse the 1003.1 syntax.

1. Unset: the file `TZDEFAULT` (`/etc/localtime`).
2. Empty: the name `Universal` -- a file -- and UTC if it cannot be read.
3. A leading `:` is dropped, and **means nothing more**: `:EST5EDT` with no
   such file is still parsed as a rule. (`tz_source` makes `:` mean "a file,
   never a rule", so `:EST5EDT` without the zoneinfo tree is UTC there and
   five hours west in glibc.)
4. **A file of that name** -- absolute, or under `TZDIR` -- if one can be
   read and parsed.
5. Otherwise, if the name is empty or is `TZDEFAULT` itself: UTC.
6. Otherwise **the POSIX rule**; and a rule with no dates (`EST5EDT`) takes
   its transitions from `TZDIR/posixrules` when that file exists, not from a
   fixed default.

`userspace/localtime` (lane B) is a function-by-function port of this, and
its tests pin it (`userspace/localtime/src/tzset_tests.rs`:
`a_file_is_tried_before_a_rule_of_the_same_name`,
`empty_is_universal_and_a_bare_colon_is_utc`,
`a_rule_with_no_dates_borrows_posixrules_from_the_zone_directory`). It is
what `date`, `ls`, `osh` and the rest of lane B's programs read `TZ` through,
and they are differential-tested against GNU's.

## What would make one decision serve all three

Because "file first" needs to know whether the file can be read, the
decision cannot be the pure "what to read" that `tz_source` returns today
without changing shape. One shape that stays I/O-free:

```rust
pub enum TzPlan<'a> {
    /// Read this file; if it cannot be read, UTC.
    File { file: TzFile<'a> },          // unset (LOCALTIME), and empty ("Universal")
    /// Read this file; if it cannot be read, the rule (with `posixrules`
    /// for its dates when it has none), or UTC if `name` is empty or is
    /// LOCALTIME itself.
    FileThenRule { file: TzFile<'a>, rule_text: &'a [u8] },
    /// Refused (a `..` component, a NUL): UTC.
    Refused,
}
```

-- the caller does the reading, as now, but in glibc's order. The `..`/NUL
refusal can stay as it is: glibc refuses those only in set-user-ID programs,
lane B's `localtime` and the libc both refuse them always, and that
difference is deliberate and documented on both sides.

Lane B does not change `tzrules` itself (`roadmap.md`: a no-lane crate is
changed through the lanes that use it). Lane B's half of
`requests/c-bd-read-tz-through-tzrules-tz-source.md` waits on this: when
`tzrules` states glibc's order, `localtime` takes the decision from it and
keeps its own tests as the check that nothing moved.

## What happens until then

Lane B's programs keep glibc's order; the libc and the desktop clock keep
`tz_source`'s. They agree on every `TZ` that is only a file name
(`America/New_York`) or only a rule (`CET-1CEST,M3.5.0,M10.5.0/3`), and
disagree, by the table above, on names that are both.
