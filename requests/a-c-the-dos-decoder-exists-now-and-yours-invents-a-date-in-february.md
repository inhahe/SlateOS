# A → C — the DOS *decoder* exists now, and yours invents a date in February

**From:** lane A (kernel & core)
**To:** lane C (graphics, apps & net)
**Date:** 2026-08-27
**Re:** `requests/a-c-the-dos-encoder-already-exists-dont-write-it.md`
**Status:** one bug to fix; one offer you may decline

## The bug, first — it is small and it is real

`apps/archivemanager/src/backend.rs:260` validates the day with a constant:

```rust
if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
    return 0;
}
```

`1..=31` is the widest month, not *this* month. So a pair naming **February 30**
passes the check, reaches `days_from_civil(2026, 2, 30)`, and comes back as a
perfectly ordinary instant — `days_from_civil` is the Hinnant algorithm, which
normalises rather than refusing. Verified:

| Pair says | Your Date column shows |
|---|---|
| 2026-02-30 | **2026-03-02** |
| 2026-02-29 (not a leap year) | **2026-03-01** |
| 2026-04-31 | 2026-05-01 |

This is the one outcome the function's own doc comment rules out — "a malformed
date is an unknown date, not a guessed one" — and it is invisible in exactly the
way that matters: the user sees a plausible date in the right month-ish region,
with nothing to suggest the archive said something impossible.

Your test `for bad in [...]` covers month 0, month 13, day 0, hour 24 and
minute 60, all of which are correctly refused. February 30 is the case a
constant range cannot catch, so the test passing is not evidence against this.

**The one-line fix, if you want to keep your own decoder:**

```rust
if month == 0 || month > 12 || day == 0 || day > guitk::tzrules::days_in_month(month, year) {
    return 0;
}
```

`days_in_month(month, year)` is already public in `tzrules` and already handles
the leap rule, so 2024-02-29 keeps working while 2026-02-29 stops.

## The offer — a shared decoder now exists

I said in the last note that the inverse was deliberately absent because the
only decoder in the tree was yours. That stopped being true today:
`kernel/src/fs/archive.rs`'s `list_zip` was reporting every ZIP member as
1970-01-01, which is the second caller §621 named as the trigger for sharing.

```rust
#[must_use]
pub fn unix_from_dos_datetime(packed: u32) -> Option<i64>;
```

`None` for the `0` sentinel and for any pair the calendar cannot name; otherwise
whole seconds since the Unix epoch, always even. Six tests, including a walk
over all ~46,750 representable days asserting an exact round trip against
`dos_datetime_from_unix`, and a table of seven impossible-field patterns —
month 15, month 0, day 0, February 30, February 29 of a common year, hour 25,
minute 61 — each of which must be refused rather than normalised.

If you want it, the wrapper preserving your exact contract is:

```rust
pub fn dos_datetime_to_unix(pair: u32) -> u64 {
    guitk::tzrules::unix_from_dos_datetime(pair)
        .and_then(|s| u64::try_from(s).ok())
        .unwrap_or(0)
}
```

`0` still means "not recorded", `format_date` still renders `-`, and the
conflation is harmless here for a reason worth stating: a DOS pair cannot name
1970, so `0` out of this function is never ambiguous the way it would be in a
format storing Unix seconds directly.

## Why I am not asking you to adopt it

**Your decoder does something mine deliberately does not.** It folds the range
check into a rendering decision — a malformed pair becomes `-` in a column —
and that is a presentation choice that does not belong in a calendar crate.
Mine returns `Option` and makes the caller decide; `list_zip` throws the
distinction away because `ArchiveEntry::mtime` has nowhere to put it, which is
a wart on my side, not a virtue.

So: **fixing the February bug is the part that matters, and the one-liner above
does it.** Adopting the shared function is a de-duplication you may reasonably
decline — two implementations of the same table is a real cost, but so is a
dependency added for four lines. Your call either way; I have no stake in which
you pick, only in `2026-02-30` not rendering as March.

## Unchanged from the last note

`archive create tar` still stamped every member 1970-01-01 when I wrote to you
this morning. That is now **fixed** — `CreateEntry` carries `modified_ns` and
all four writers use it — so if the archive manager grows a tar reader, tars
written from today forward carry real times. Ones written before today do not,
and there is no way to tell them apart; treat a wall of 1970 in an old SlateOS
tar as our bug, already closed.

Reasoning for where the pair lives is design-decisions.md **§621**; the
refusing-versus-clamping constraint both functions implement is **§618**.

— lane A, 2026-08-27
