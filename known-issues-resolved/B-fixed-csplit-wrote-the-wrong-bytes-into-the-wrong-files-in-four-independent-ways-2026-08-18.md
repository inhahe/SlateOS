## [B] FIXED — `csplit` wrote the wrong bytes into the wrong files, in four independent ways (2026-08-18)

**In short:** `csplit` splits a file into pieces at places you name — a line
number, or a line matching a pattern. Ours got four separate things wrong, any
one of which silently corrupts the output: it could produce **10,003 files**
where GNU produces 8, it threw away the line a `%…%` pattern matched, it
rewrote every line ending, and its "patterns" were plain substring searches, so
`/^5$/` and `/5/` meant the same thing. All four are fixed, and
`scripts/csplit-diff.sh` now agrees with GNU on **76 of 76** behavioural cases.

**How they were found.** By writing the differential harness first and then
running it — `scripts/csplit-diff.sh`, which is unlike the other 24 harnesses
in this tree in that it compares a **manifest of the files left behind**, not
just stdout. `csplit`'s stdout is only a list of byte counts; a `csplit` that
printed the right numbers and wrote the wrong bytes would pass a stdout-only
comparison, and that is most of what there was to get wrong here. The first run
appeared to *hang*; it was not hanging, it was writing ten thousand files.

### The four faults

| | what it did | what GNU does |
|---|---|---|
| `{*}` | cloned the pattern 10000 times and re-applied each line number **absolutely**, so the cursor never advanced — `csplit f 3 '{*}'` left 10,003 files | expands the repeat at execution time; 8 files, then `'3': line number out of range on repetition 6` |
| `%RE%` | set the cursor to `match + 1`, **discarding the matching line** | keeps the matching line as the first line of the next section |
| line endings | read with `BufRead::lines`, which drops the terminator, then re-terminated every piece with `\n` | copies bytes; a file whose last line has no newline comes back out without one |
| patterns | `str::contains` — a substring search | a POSIX basic regular expression, so `/^5$/` matches only the line that is exactly `5` |

The first is the only one that is loud. The other three are silent corruption:
the byte counts on stdout are *self-consistently wrong*, so nothing short of
comparing the files reveals them.

### The two semantics that were not obvious, and are now pinned by cases

Both were established by measuring GNU rather than by reading the manual, which
does not describe either.

- **There are two cursors, not one.** `emit` is the first line not yet written;
  `search` is the first line the next pattern examines. A regex match sets
  `search = m + 1` while a line number sets `search = emit`. This is why
  `csplit f 2 '/2/'` reports `match not found` — the search starts *after* the
  matched line, not at the start of the new section — and why
  `csplit f '%MARK%' '/MARK/'` finds the *second* `MARK`.
- **A line number one past the end is an error, but only after the whole file
  has been written and counted.** `csplit f 21` on 20 lines prints `51` and
  *then* fails with `'21': line number out of range`. GNU's own source comments
  the check: "ensure that the line number specified is not 1 greater than the
  number of lines in the file". An implementation that range-checks before
  writing gets the exit status right and the byte counts wrong.

### What the rewrite also gained

`-b/--suffix-format` (a validated single-conversion printf), `-z/--elide-empty-files`
(whose index is **not** consumed by an elided file, so `csplit -z f 1` leaves a
single `xx00` holding everything), `--suppress-matched` (which applies to line
numbers too, not only to the pattern kind whose name it carries), and the long
spellings `--prefix`, `--digits`, `--keep-files`, `--quiet`, `--silent`. Every
one of those was an `xfail` row in the harness when it was written and is a
passing `run_case` now.

### The two divergences that remain, both deliberate

`--help` omits the GNU project's own bug-report URLs, and `--version` names
SlateOS. They stay as `xfail` rows.

### The one behavioural divergence that is *not* covered

Reading a **directory** as input. GNU streams, so it creates `xx00`, prints its
count of `0`, and only then reports `read error: Is a directory`; we read the
whole input before creating anything, so we report the same message with no
count and no file. Matching it would mean streaming the input, which buys
nothing else — every other case is identical — and costs the single-pass
structure that makes the two cursors above tractable. Left as-is deliberately;
if `csplit` ever needs to handle an input too large to hold in memory, that is
the change that fixes both at once.
