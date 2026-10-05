## B-TAR-GAVE-UP-ON-A-DAMAGED-ARCHIVE-AT-THE-FIRST-BAD-BLOCK (lane B, 2026-10-03)

**Status:** FIXED 2026-10-03 (lane B); boot-confirmed on main at 8be413362 (the boot of 583c2a823 passed).

**In short:** When one block in the middle of an archive is damaged, GNU tar
says `Skipping to next header` and carries on: the members after the damage
are still listed and extracted. Ours stopped at the damaged block and then
reported every member after it as `Not found in archive` -- so a backup with
one bad sector gave back nothing past it. Three smaller divergences in the
same code were fixed with it; all four were found by measuring GNU tar 1.35
while restructuring how an extraction ends.

### What was wrong

| | ours | GNU |
|---|---|---|
| a block of noise between two members | stops; later members `Not found in archive` | `Skipping to next header`, then the later members |
| an archive that is noise from the start | `This does not look like a tar archive` | that, and `Skipping to next header` |
| one zero block left after members with data | `A lone zero block at 4` | `A lone zero block at 6` -- the count includes data blocks |
| a damaged ending and a missing operand | `nosuch: Not found in archive`, then `A lone zero block at …` | the other way round |
| a fatal error part-way through `-x` (a `-C` that cannot be entered, a truncated archive) | directories already extracted keep the mode and time they were made with; delayed symlinks are never made | GNU's `fatal_exit_hook` runs `extract_finish` first: modes, times and links are applied |
| an archive out of the usual order (`d/`, `d/a`, `f`, `d/b`) | `d` gets its stored mtime | `d` is stamped when the extraction leaves it, at `f`, and `d/b` then changes it |

### What it does now

- **`walk`** is GNU's `read_and`: it reports on the archive as it reads it
  (`This does not look like a tar archive`, `Skipping to next header`,
  `A lone zero block at N`, `Unexpected EOF in archive`, `Cannot read`), at the
  point where it finds the problem, and a block that is not a header is
  skipped, with only the first of a run reported. It counts every block that
  passes through it (`Counting`), data blocks included.
- **Callers** then print `Not found in archive`, which GNU's `names_notfound`
  does after its read loop, and an extraction finishes.
- **`DelayedDir` / `apply_nonancestor` / `finish_extraction`** are GNU's
  `delay_set_stat`, `apply_nonancestor_delayed_set_stat` and `extract_finish`:
  a directory's metadata is applied before the first member that is not inside
  it, `.` and the directory a delayed link is made in wait for the links
  (`mark_after_links`), and everything left is applied at the end -- including
  after a fatal error, before `Error is not recoverable`.

### Evidence

`scripts/tar-diff.sh` section 13, twelve new cases; 336 of 336 agree, with
three known divergences. One of those is new and deliberate: GNU credits a
member only to the *first* operand that covers it, so `tar -tf a.tar d d/b`
reports `d/b: Not found in archive` (status 2) although `d/b` was listed. Ours
credits every covering operand, as `Selector::wants` already documented; the
harness now carries the case as an expected failure so it stays visible.
Seven new unit tests (162 tar tests pass under Linux, 147 on the host).

### Not done here

- GNU retries a read error in the middle of an archive up to ten times
  (`Too many errors, quitting`); ours treats the first as fatal.
- GNU's dedupe of a repeated directory member compares the name alone, so
  `-C d1 x/ -C d2 x/` leaves `d1/x` unstamped; ours keys on the name and the
  `-C` destination. Deliberate, and noted at `delay_dir`.
