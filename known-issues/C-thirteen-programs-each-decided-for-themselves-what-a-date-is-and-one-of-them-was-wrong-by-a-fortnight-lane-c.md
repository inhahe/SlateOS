## Thirteen programs each decided for themselves what a date is, and one of them was wrong by a fortnight (lane C)

**Found:** 2026-08-21 · **Status:** FIXED · **Lane:** C
**Commits:** `60eec1ba2`, `12cc4ce18`, and the migration commits that follow
**Design decision:** §491

Round 4 of the `apps/**` / `gui/**` sweep. Round 3 swept *durations* (a length
of time) and deliberately left *instants* (a point in time) alone. This is that
problem, and it turned out to hold the two worst individual bugs the sweep has
found.

### The same object, eight surfaces

A file's modification time, a backup run, a restore point — all one kind of
thing, "when did this happen". For the instant 2026-08-18 16:30:45 UTC:

| Program | Rendered |
|---|---|
| `apps/explorer` | `2026-08-18 16:30` |
| `apps/archivemanager` | `2026-08-18 16:30` |
| `apps/taskscheduler` | `2026-08-18 16:30` |
| `apps/rssreader` | `2026-08-18 16:30` |
| `apps/backup` | `2026-08-18 16:30:45` |
| `apps/undelete` | **`2026-09-04 16:30`** |
| `apps/systemrestore` | **`D20683`** |
| `gui/desktop/backup_settings` | **`Day 20683 16:30`** |

### The individual bugs

| Where | Bug | Effect |
|---|---|---|
| `apps/undelete:3571` | year computed as `days / 365`, month as `remaining / 30` | **14 days wrong in 2026, growing ~5 days/year.** A file deleted 18 Aug listed as deleted 4 Sep. The deletion date is the column a user reads to tell two recoverable copies apart. |
| `apps/systemrestore:4249` | rendered `D{days_since_epoch}` | A restore point — the most consequence-laden thing in the system to pick by date — labelled `D20683`. |
| `gui/desktop/backup_settings:376` | rendered `Day {days} HH:MM` | The settings page and the backup application showed a user their backup history and disagreed about what a backup's date is. |
| `gui/desktop/main.rs` taskbar clock | `secs % 86_400`, no zone | **Five hours wrong out of the box** (shipped default zone `America/New_York`). `show_seconds`, `show_day_of_week` and `show_date` are each documented as applying "in the taskbar clock" and reached nothing. `calendar::ClockDisplay` — a complete, tested taskbar clock — had **no callers in the tree** (same defect as §469's two snap implementations). |
| `gui/desktop/datetime_settings.rs` | timezone picker had 20 entries, none named UTC | The zone the shell falls back to when the configured one is unresolvable was one the user could not deliberately select. |
| `apps/taskscheduler:2561` | decomposed the instant **twice** — `decompose_timestamp` for the time, `days_to_ymd(ts / 86400)` for the date | Two halves of one string derived by two routes with nothing making them agree. The shape a timezone would have broken first. |
| `apps/screenrecorder:709` | held the timestamp as `(u16, u8, u8, u8, u8, u8)` | Month 13 at hour 25 was representable, and the formatter printed whatever it was given. |
| `apps/indexer:1812` | relative-time ladder stopped at days | An index untouched for two years read `730 days ago`. |
| all thirteen | no timezone applied | Every one was UTC, and none of them recorded that as a decision. |

### Why the tests did not catch it

Every one had a passing test. See §491's table — the assertions were
`!is_empty()`, `contains('-')`, `starts_with("2023-")`, and in two cases an
exactly-correct assertion that the day counter was a day counter. **They were
not weak tests; they were right about the wrong thing.** All six now assert a
rendered value, and `guitk::datetime` carries a regression test that runs the
old `days / 365` arithmetic beside the new for three instants and asserts they
differ.

### The fix

`guitk::datetime` (`DateTime` + nine renderings), with the zone a **required
argument that is never defaulted**, so a program with no zone to offer writes
`Tz::utc()` — which `rg` can find, and which `secs % 86_400` could not be
distinguished from. `tzrules` is re-exported through `guitk` so a caller can
name the type without its crate growing a dependency.

### Still open

- **`apps/backup` keeps its own `days_to_ymd`.** It is a headless CLI archiver
  that deliberately does not link `guitk`, and the right home for the inverse of
  `days_from_civil` is `tzrules`, which has no public one. Tracked by
  `requests/c-b-year-of-day-computes-the-month-and-day-and-throws-them-away.md`;
  the function's doc comment says "when that lands, delete this function".
- **No per-process zone plumbing exists** (`TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`),
  so every migrated application still renders UTC — but now says so. The list of
  surfaces still to fix is `rg 'Tz::utc' apps/ gui/`.
- **`show_date` and `show_day_of_week` still do not reach the taskbar.**
  `ClockDisplay::format_date` emits weekday and date together with no way to ask
  for one, so honouring the two flags independently needs an API change. The
  zone and `show_seconds` do reach it now.

### Related

Round 4 followed round 3's closing advice — *start from objects displayed on
more than one surface, not from functions that look alike* — and that is
precisely what surfaced the undelete bug: it was found by asking "who else
renders a file's mtime", not by noticing that its code looked odd. Its code did
not look odd. It looked like the other four.
