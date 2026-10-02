## TD-B-DATE-A-SIGNED-RELATIVE-AFTER-A-BARE-TIME-IS-A-ZONE-TO-GNU (lane B, 2026-09-16) — FIXED 2026-09-25

**In short:** `date -d '2021-06-15 12:00:00 +1 day'` works on GNU and is
refused by ours. The `+1` there is not "plus one" — GNU reads a signed number
straight after a time-of-day as a **time-zone offset in hours**, and the bare
word `day` as one day. So the answer is one day later *at UTC+1*, and
`-1 day` in the same position is also one day **later**, at UTC−1. The sign
belongs to the zone and never to the displacement.

**Measured**, GNU date 9.4, base `2021-06-15 12:00:00`:

| operand | GNU | note |
|---|---|---|
| `12:00:00 +1 day` | Jun 16 **11:00** UTC | +1 day, zone +01:00 |
| `12:00:00 -1 day` | Jun 16 **13:00** UTC | +1 day, zone −01:00 — later, not earlier |
| `12:00:00 1 day` | Jun 16 12:00 UTC | unsigned: an ordinary relative |
| `12:00:00 UTC +1 day` | Jun 16 12:00 UTC | zone already given, so `+1` is a relative again |
| `12:00:00 -90 seconds` | *refused* | and ours refuses it too |
| `2021-06-15 +1 day` | Jun 16 00:00 UTC | no time, so an ordinary relative |

**Where:** `userspace/coreutils/src/bin/date.rs`, `take_relative_terms`. It
extracts relative terms in a pass *before* the date/zone parser runs, so it
cannot know whether a zone has already been seen. A signed number after a bare
time is therefore left alone and the operand is refused.

**Why refused rather than approximated.** Before relative forms existed at all
this operand was refused too, so refusing is not a regression; answering would
be. Our reading gives Jun 16 **12:00**, which is a day and an hour from GNU's
— a plausible number, silently wrong, for an operand shaped exactly like the
common `date -d "$stamp +1 day"` idiom. A visible refusal is the better of the
two failures, and it is the one a caller can act on.

**The proper fix:** interleave zone and relative parsing instead of running
them in two passes — decide each token in order, so "has a zone been seen yet"
is available when a signed number is reached. Then `12:00:00 +1 day` takes the
zone branch and `12:00:00 UTC +1 day` takes the relative one. The harness row
`known_bug_case TD-B-DATE-A-SIGNED-RELATIVE-AFTER-A-BARE-TIME-IS-A-ZONE-TO-GNU`
turns green when it is right, and the four neighbouring rows beside it are the
control that the fix did not disturb the forms that already work.

**Severity: low.** One operand shape, refused rather than mis-answered, with
three spellings that do work (`1 day` unsigned, `UTC +1 day`, and any relative
on a date with no time).

**How it was closed.** Not by interleaving the two passes, but by replacing the
hand-written parser with GNU's: `coreutils::parse_datetime` runs the Bison
tables of `parse-datetime.y`, where this is a shift/reduce conflict resolved in
favour of the zone, so every row of the table above now agrees with GNU. The
harness row is a `run_case` again, with its `-1 day` and `UTC +1 day`
neighbours beside it.
