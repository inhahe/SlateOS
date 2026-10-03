## `yes` ran at 1.6 MiB/s — 1600× slower than GNU — and `yes --help` printed `--help` forever (lane B, 2026-08-24)

**In short:** `yes` prints a line over and over until something stops it. Ours
had three faults. It knew no options at all, so `yes --help` did not print help
— it printed the word `--help` endlessly, and the only way out was Ctrl-C. It
did one `write` system call per line, which is the one thing this particular
program must not do, and measurement put it at **1.6 MiB/s against GNU's 2690
MiB/s**. And it crashed outright on any argument holding a byte that is not
valid UTF-8, which on this system is a legal thing to pass. Rewritten from GNU
coreutils 9.4's `src/yes.c`, checked against the real GNU binary across 39
cases; it now runs at 2634 MiB/s, within 2% of GNU.

### Why the slowness is the whole point of this program and not a side note

Every other utility in this section was judged on what it did. `yes` is judged
on how fast it does it: it exists to feed an endless stream into something
else, so its throughput *is* its behaviour. A `yes` that is correct and slow
has failed at the only thing asked of it.

The old loop was three lines and looked unimprovable:

```rust
let line = format!("{text}\n");
let bytes = line.as_bytes();
loop {
    if out.write_all(bytes).is_err() { break; }
}
```

The defect is invisible unless you know what `io::stdout()` does. It is
`LineWriter`-backed, so a buffer holding a `\n` is flushed immediately — which
means this writes `y\n` and then calls `write(2)`, two bytes at a time, roughly
860,000 syscalls a second. Upstream's `yes.c` does the opposite and says so:
it copies the record into a buffer, **doubles that buffer until it is at least
`BUFSIZ`**, and then writes the whole buffer forever. The doubling is what
keeps every write a whole number of records regardless of the record's length,
so no record is ever split across a call. Ours now does the same, and the
measured 1606× is that one change.

The numbers were measured, not asserted: the old file was rebuilt from git at
`-O` so the comparison is optimised-against-optimised, and all three binaries
were run for three seconds into `dd bs=1M of=/dev/null`, best of three.

| | throughput | vs GNU |
|---|---|---|
| old `yes` | 1.6 MiB/s | 0.06% |
| new `yes` | 2634 MiB/s | 98% |
| GNU 9.4 | 2690 MiB/s | — |

### The other three defects

1. **No option parsing whatsoever.** `--help` and `--version` were operands, so
   `yes --help` printed the string `--help` on every line and never stopped.
   That is worse than an unimplemented option, because the usual way a person
   finds out what a program takes is the thing that hangs their terminal. `-x`
   was likewise a string rather than an error. Both now go through
   `coreutils::getopt` with the two-entry table upstream has, which also brings
   abbreviation (`--h`, `--ver`) and glibc permutation — `yes a --help` prints
   help, because upstream does not pass `+` and so an option keeps its meaning
   after an operand. The harness checks all of that.
2. **`Vec<String>` from `env::args()`, so a non-UTF-8 argument panicked** —
   this is the `argv-utf8` finding that brought the program up for conversion.
   `design.txt` allows every byte but `/` and NUL, so `yes "$(printf 'na\377me')"`
   was a crash before a byte of output. Argv is `OsString` end to end now and
   the bytes are only taken at the point the record is built.
3. **Every write error was swallowed and reported as success.** The
   `.is_err() { break }` above does not distinguish a reader that went away
   from a disk that filled: `yes > /dev/full` exited **0 with no diagnostic**,
   claiming to have done the job. GNU prints `yes: standard output: No space
   left on device` and exits 1. Only `BrokenPipe` is quiet now, which is the
   established convention here — `cut`, `head`, `tail` and `uniq` do the same,
   because SlateOS has no `SIGPIPE` to die of (`design.txt`) and Rust masks it
   anyway, so the case that kills GNU arrives as `EPIPE` instead.

### What the harness had to solve that the others did not

`scripts/yes-diff.sh` is the first in this family whose subject never exits.
Two consequences shaped it:

- **Every running case is bounded by `head -c 40000` and compared on a
  digest.** The bound is deliberately several times the 8 KiB buffer, because
  the failure the buffering could introduce — a record split across a write, or
  a buffer that is not a whole number of records — is invisible in the first
  4 KiB and unmistakable in 40 KB. Records of 3, 5 and 7 bytes, and one of
  20,000, are in the case list for exactly that reason.
- **The exit status could not be compared for those cases**, since both sides
  are killed by the bound rather than finishing. Rather than let that silently
  drop a real difference, the `EPIPE`-vs-`SIGPIPE` divergence is pulled out
  into a single explicit `xfail` — so it is recorded once, in the summary,
  instead of being smeared across every case as an unexamined exemption.

Two further harness defects were found by running it rather than reading it:

- **`PIPESTATUS` is a bashism, and `scripts/all-diff.sh` runs every harness as
  `sh "$h"`.** Under a `sh` that is not bash it reads as empty — and empty
  compares equal to empty, so *every status check in the harness would have
  passed by vacuity* while appearing to run. The status is now carried out of
  the pipeline in a file, and the harness was re-run under `dash` to prove it:
  same 36/0/3.
- **The abbreviation and permutation cases were comparing the full help text**,
  so all six failed on the one difference already recorded as an `xfail`. Those
  cases exist to ask *which* option resolved, not what it printed, so they now
  compare stdout's class — `help` / `version` / `empty` / `stream` — while
  still comparing stderr and status byte for byte. A known difference that
  fails a case it is not about stops being a record and becomes noise.

### A third gate defect: `getopt-ambiguity-check.py` could not see a table that fitted on one line

Checking that the gate really covered the new program — rather than assuming it
did, because the summary said `41 table(s) checked; 0 disagreement(s)` — showed
that it did not. `python scripts/getopt-ambiguity-check.py yes` answered:

```
note: yes has no LONG_OPTIONS table, so there is nothing to compare
      (not yet converted to coreutils::getopt?)
0 table(s) checked; 0 disagreement(s).
```

The table exists. The pattern ended at a literal `\n];`, which made the gate
depend on rustfmt's layout: `yes` has two options, so rustfmt collapses the
table onto one line, there is no `];` at the start of a line, and the
declaration was not found. This is the *second* time this gate has silently
stopped seeing a program — the first was the `digest.c` delegation, recorded
above — and both failures read as reassurance rather than as a warning, because
"no table" is a note the sweep prints legitimately for the bins not yet
converted.

Fixed properly rather than by adding another alternative to the regex: only the
declaration's head is matched by pattern now, and the body is taken by counting
brackets (skipping string literals), so a one-line table and a fifty-line one
parse alike and no future formatting choice can hide one.

The fix was checked by diffing what the old and new parsers find across every
bin, which is the only way to know a looser pattern did not also break
something: **nothing lost, no table's contents changed, and four newly
visible** — `md5sum` and `sha256sum` from the earlier delegation fix, `yes`,
and **`tsort`, which nobody had noticed was uncovered either**. Both newly
reachable tables were then run against their real GNU binaries and agree. The
sweep is now `43 table(s) checked; 0 disagreement(s)`.

### Deliberate divergences, all recorded as `xfail`

Three: `--help` omits the GNU project's `Report bugs to:` block, `--version`
names SlateOS, and the broken-pipe status above. Confirmed discriminating by
running the harness with `OURS=/usr/bin/yes`, which turns all three into
`XPASS` and nothing else — the flip is what proves the harness is testing our
binary rather than agreeing with itself.

Final: **36 passed, 0 differed, 3 differ on purpose**, plus 20 unit tests.
