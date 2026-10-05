## §491 — An instant is rendered where the zone is known, and the zone is an argument that is never defaulted

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** All over Slate, programs show you *when* something happened — when
a file was last written, when a backup ran, when a restore point was taken.
Thirteen of them each worked out for themselves how to turn a number of seconds
into a date, and none of them applied a timezone, so all thirteen quietly showed
UTC. One of them, the file-recovery tool, was also plain **wrong**: it listed a
file deleted on 18 August as deleted on 4 September, and the error grows by
about five days every year. Two others didn't show a date at all — the system
restore tool labelled restore points `D20683` and the backup settings page
listed runs as `Day 20683 16:30`, both of which are the number of days since
1970. The decision here is that there is now one module (`guitk::datetime`) that
does the conversion, and that **the timezone is a required argument to it** —
a program with no zone to offer has to write `Tz::utc()` and say why, which
leaves something a search can find later.

### The zone is the part that matters

The taskbar clock had the same defect and it is the clearest illustration. Its
whole body was:

```rust
let secs = SystemTime::now()…as_secs();
let h = (secs % 86_400) / 3600;
let m = (secs % 3600) / 60;
format!("{h:02}:{m:02}")
```

That is UTC. The shipped default zone is `America/New_York`, so out of the box
the corner of the screen was five hours wrong — and `show_seconds`,
`show_day_of_week` and `show_date`, whose doc comments each say they apply *"in
the taskbar clock"*, reached nothing at all. Meanwhile `calendar::ClockDisplay`
— a complete taskbar clock, with zone handling, a seconds switch, a 12/24-hour
switch and its own passing tests — had **no callers anywhere in the tree**.

That is §469's defect exactly (`snap.rs` was dead while the shell computed its
own snapping), and it is the sweep's recurring shape: *the tree held one correct
answer that callers could not reach, and grew wrong copies of it.*

The reason `secs % 86_400` spread is that it is **UTC by accident and
indistinguishable from arithmetic that meant it**. No reviewer reading those
four lines can tell whether the author considered zones and chose UTC, or never
thought about it. Thirteen programs shipped a UTC clock and not one of them
recorded the decision, because the code has no place to record it.

Hence the rule: `Tz` is a parameter, on every function, with no default and no
`_utc` convenience shortcut on the free functions. A caller with nothing to
offer writes `Tz::utc()` at the call site. That is one token longer and it is
greppable, which is the entire point — `rg 'Tz::utc'` now lists every surface in
the tree that still shows the wrong hour, and the list shrinks as zone plumbing
arrives.

**Cost, stated plainly:** every call site is longer, and a caller that genuinely
wants UTC has to say so twice (once in code, once in a comment). Fifteen call
sites paid that cost. The alternative — a `stamp(secs)` that means UTC —
would have made the migration a one-line change per site and would have
reproduced the exact condition that caused the bug.

### Why a new module rather than `guitk::date`

`date::Date` says, in its own module docs, that it is a *civil* date and that
converting an instant to one "is the caller's business, because only the caller
knows which zone the user meant". That boundary is right about **which zone**
and wrong to leave **the conversion** with the caller too, which is how thirteen
callers came to own a calendar each. `datetime` is where the caller's business
gets done once, with the zone still explicitly the caller's to supply.

`DateTime` is a `Date` plus a seconds-into-the-day count, not six integers. The
screen recorder held `(u16, u8, u8, u8, u8, u8)` and nothing stopped it holding
month 13 at hour 25; the new type has no such state. It decomposes with
`div_euclid`, so an instant before 1970 lands on the day that *contains* it
rather than an hour before the epoch — the failure truncating division gives.

### What the shapes are, and what is deliberately not one of them

`stamp` (`2026-08-18 16:30`) is the default because five of the thirteen had
already converged on it, and because it sorts lexicographically in the order it
sorts chronologically — so a table column that sorts by rendered text is in
chronological order without a special case. Then `stamp_secs`, `iso_date`,
`clock`, `clock_secs`, `clock12`, `clock12_secs`, `long_date`, `medium_date`.

The module deliberately does **not** decide between 12- and 24-hour, and does
not decide whether seconds show. Those are *settings*, and a setting belongs
with whatever owns it — the taskbar's `ClockDisplay`, the alarm clock's format
enum. A shared formatter that owned them would have to be handed the settings,
which is a wider coupling than handing it a zone.

### Relative time is a different question, and stays rare

The indexer renders "how long ago was this index built", and that stays
relative (`textfmt::duration::relative`) rather than becoming a date. The
reader's question there is *is this stale*, not *on what date*, and a date makes
them do the subtraction. That is the only surface in the tree where it is the
right shape; everything else that renders an instant renders it absolutely. The
distinction is worth stating because "show a relative time, it's friendlier" is
a plausible-sounding rule that would have hidden the undelete bug completely —
`14 days ago` is wrong by a fortnight in a way nobody can see.

### Alternatives rejected

1. **Put it in `textfmt`, so headless programs reach it too.** `textfmt` is
   zero-dependency by construction and a timestamp formatter needs `tzrules`.
   The two headless offenders (`apps/backup`, `apps/indexer`) are handled
   separately: the indexer needed a *duration*, which `textfmt` already has, and
   `apps/backup` keeps its own calendar with a comment saying so and a request
   open with lane B for `tzrules::civil_from_days`. Breaking `textfmt`'s
   zero-dependency property to serve one caller is the wrong trade.
2. **Make the zone optional, defaulting to UTC.** Rejected above: it recreates
   the condition that produced the bug.
3. **Thread a zone through to every app now.** There is no per-process zone
   plumbing yet (known-issues `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`), and
   inventing one as part of a formatting sweep would be a much larger change
   made for the wrong reason. `Tz::utc()` at the call sites is the honest
   interim, and it is honest *because* it is visible.
4. **Leave the differences as house style.** Three of the thirteen were not
   style. One was fourteen days wrong; two showed a day counter where a date
   belongs.

### Where the tests were

Every one of the thirteen had a passing test. They passed because of what they
did not ask:

| Program | The assertion | What it could not notice |
|---|---|---|
| undelete | `!result.is_empty()`, `result != "Unknown"` | any date at all |
| archive manager | `contains('-')`, `contains(':')` | `2026-13-40 25:99` |
| task scheduler | `starts_with("2023-")` | any of that year's 365 days |
| RSS reader | `starts_with("2024-01-01")` | the time of day, derived separately |
| system restore | `== "D0"`, `== "D1"`, `== "D100"` | *correct* — it proved the day counter was a day counter |
| backup settings | `contains("01:01")` | satisfied by `"Day 1 01:01"` |

The last two are the instructive ones. They were not weak tests; they were
*right about the wrong thing*. This is §486 again from another angle — a test
that asserts what the code does, rather than what the user needs, is a test that
locks the defect in. All six now assert a rendered value.
