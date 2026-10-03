## [B] FIXED — `split` implemented three of its fourteen options, and corrupted the file while doing it (2026-08-18)

**In short:** `split` cuts a file into numbered pieces. Ours understood `-l`,
`-b` and `-a` and nothing else — no `-n`, no `-C`, no long options at all, not
even `--help` — and the three it did understand it got wrong for any input that
was not plain ASCII text: it read the input as *lines of UTF-8* and wrote them
back with `writeln!`, so a file with CRLF endings came back with LF, a file
whose last line had no newline gained one, and a file containing a byte that is
not valid UTF-8 — that is, any binary file, the main thing people split —
failed outright with `read error`. It is rewritten against GNU coreutils 9.4,
and `scripts/split-diff.sh` now agrees with GNU on **207 of 207** behavioural
cases, with five declared divergences.

### What was missing

| | |
|---|---|
| absent entirely | `-n`/`--number` in all four forms (`N`, `K/N`, `l/N`, `l/K/N`, `r/N`, `r/K/N`), `-C`/`--line-bytes`, `-d`/`--numeric-suffixes`, `-x`/`--hex-suffixes`, `-t`/`--separator`, `--filter`, `--additional-suffix`, `-e`/`--elide-empty-files`, `-u`/`--unbuffered`, `--verbose`, `--help`, `--version` |
| long options | **none** — `--lines=5` was parsed as a file name |
| suffix widening | none. Where GNU grows `x` `y` `z` into `zaaa`…, ours exited `output file suffixes exhausted` |
| `-b`'s number grammar | `k`/`m`/`g` only, and `n * multiplier` unchecked, so a large count with a suffix panicked in a debug build. GNU reads the full `xstrtoumax` grammar — `K`/`KB`/`KiB` through `Q`, `b` = 512, and `NxM` products |
| non-UTF-8 argv | `env::args()`, which **panics** on an argument that is not valid Unicode. A file name is bytes; see the repo rule on OS-boundary data |

### The four faults in what it did implement

| | what it did | what GNU does |
|---|---|---|
| line endings | `BufRead::lines()` (drops `\n` *and* a preceding `\r`) then `writeln!` | copies bytes; CRLF stays CRLF, and a file whose last line has no newline comes back without one |
| binary input | `lines()` yields `Err(InvalidData)` → `split: read error` and exit 1 | splits it; `split -l` on a binary file is ordinary usage |
| `-a 0` | rejected — `invalid suffix length: 0`, exit 1 | accepted: upstream tests the width for *truth*, so `0` reads as "not given" and the default 2 applies. Measured: `split -a 0 -l 2` writes `xaa xab xac` |
| diagnostics | `unknown option: --lines`, `invalid line count: 0` | `unrecognized option '--lines'`, `invalid number of lines: '0'` — quoted, and worded by `getopt_long` |

### How the rewrite was verified

`scripts/split-diff.sh`, 210 cases, on the `csplit-diff.sh` pattern: it compares
a **manifest of the files left behind** (`od -An -c` per file, in name order),
not stdout, because `split`'s stdout is empty in almost every mode — a `split`
that wrote the wrong bytes into the right names would pass a stdout-only
comparison, and that is most of what there is to get wrong here.

**Five cases are declared expected-failures**, so the harness reports `207
passed, 0 differed, 5 differ on purpose` rather than quietly covering 205:

| | why |
|---|---|
| `--help`, `--version` | text we do not promise to reproduce verbatim |
| three `--hex-suffixes=` cases | GNU 9.4 reads out of bounds there; see `TD-SPLIT-GNU-HEX-SUFFIX-START-READS-OUT-OF-BOUNDS` above and design-decisions.md §336 §3 |

### The part that could not be settled by probing

`-n l/N` — "split into N files without splitting a record" — was implemented
four different ways against 45 measured GNU invocations, and each formula fit
some values of N and failed others. The behaviour is genuinely not inferable
from the outside, because the rule is not "fill each piece up to `size/N`": the
file is cut into the *same* partitions as `-n N`, a record belongs to the
partition its **first byte** lands in, and a record that overruns a partition
leaves that partition **an empty file in its place in the sequence** — not
skipped, and not moved to the end. Three records into `-n l/5` gives record,
record, *empty*, record, *empty*.

That was settled by reading `coreutils-9.4/src/split.c` rather than by more
probing, and reading it also corrected three *other* rules that were wrong or
underspecified: when the suffix field is widened for `-n` (only when the start
value parses as decimal **and** is smaller than the chunk count), how a start
value is validated (`strspn` against the alphabet, with leading zeros stripped
*before* the width is checked, and two different message wordings for `-d` and
`-x`), and that `-n`'s argument skips whitespace *before* the `l/`/`r/` prefix
is looked for, so `-n ' l/3'` is `l/3`. The lesson generalises: a differential
harness proves agreement on the cases you thought of, and cannot tell you the
*shape* of a rule you have not guessed.

### Also in this change

`sh -c` moved into `userspace/coreutils/src/shell.rs`, shared by `split
--filter`, `awk`'s `system()` and two pipe forms, and `sh`'s `$(…)`
substitution. Four copies remain outside the crate — see
`TD-SH-C-IS-SPELLED-FOUR-MORE-TIMES-OUTSIDE-THE-COREUTILS-CRATE` above.
