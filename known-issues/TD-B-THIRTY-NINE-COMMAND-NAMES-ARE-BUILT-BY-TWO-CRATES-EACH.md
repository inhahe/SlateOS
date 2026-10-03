## TD-B-THIRTY-NINE-COMMAND-NAMES-ARE-BUILT-BY-TWO-CRATES-EACH (lane B, 2026-09-10)

**In short:** thirty-nine command names are produced by two different crates in
this tree. `free`, `ps`, `df`, `sed`, `tar`, `wc`, `who`, `uname` and thirty-one
others each exist twice -- once as `userspace/coreutils/src/bin/<name>.rs` and
once as `userspace/<name>/`.

**Derived, not listed:** the set of `coreutils` bin targets intersected with the
`name =` of every other `userspace/*/Cargo.toml`. 84 coreutils binaries, 2762
other userspace crates, 39 names in both.

**It does not cut one way, which is why this is a question and not a chore.**
Two measured examples:

* **`free`** -- **SETTLED 2026-09-12: `userspace/free` is deleted.** The
  prediction below was written from the sources and was, for once, right --
  but it was confirmed by measurement before anything was removed, not
  instead of it. `scripts/free-diff.sh` pins `/proc/meminfo` inside a private
  mount namespace and compares both against procps-ng 4.0.4:
  **coreutils 48 passed / 0 differed; `userspace/free` 0 passed / 48
  differed.** The most lopsided pair yet measured.

  The original reasoning, kept because it is the only one of the five that
  the harness upheld: `coreutils`'s is a 1876-line transcription of procps-ng
  4.0.4, and its own module doc lists the defects of "the implementation this
  replaces": invented flags, wrong header widths, `shared` hardcoded to zero,
  and `used = total - free - buffers - cached` where upstream's is
  `MemTotal - MemAvailable`. `userspace/free` still had every one of them.

  **What the loser knew that the winner did not: nothing.** Its whole unique
  surface was one invented flag, `--json`, which procps-ng does not have and
  which nothing in this tree consumed. It was deliberately not ported --
  adding a flag upstream lacks would break the bug-for-bug property
  `free-diff.sh` asserts, which is the thing that made the pair decidable at
  all. Every *real* option it lacked, coreutils has: `--peta`, `--pebi`,
  `--si`, `--line`, `--committed`, `--version`. It also sat in
  `argv-utf8-baseline.txt` as `argv-as-string`.
* **`ps`** -- **SETTLED 2026-09-12: coreutils' wins, 12 to 0.** The entry
  below is kept because it was right when written and stopped being right four
  hours later, which is the more useful record.

      coreutils ps    12 passed,  0 differed, 15 differ on purpose
      userspace/ps     0 passed, 12 differed, 15 differ on purpose

  Same harness, same cases, opposite results. The reversal is not a change in
  the standalone: it is that coreutils' `ps` was given procps' column set and
  a parser that refuses what it cannot honour, both of which it was missing
  this morning. **A pair verdict is a statement about two implementations on a
  given day, and the losing half of this one was two fixable defects away from
  winning.**

  **The standalone scored 0 XPASS**, which is the sharper half of the result.
  It implements `-l`, `-u`, `-o`, `-p` and `--no-header` -- five real procps
  options coreutils' refuses -- and not one of them produced procps' output.
  Every case declared "not implemented here" for coreutils' *also* differed
  for the implementation that has it. This is `free` again: 23 options
  advertised against 2 is not 21 options that agree.

  **RETIRED 2026-09-12. `userspace/ps` is deleted; coreutils' `ps` is the one
  `ps`.** Final measurement before the delete: **22 passed, 0 differed, 10
  differ on purpose** for coreutils', against **0 passed, 12 differed** for the
  standalone on the same cases.

  **What the loser knew, measured option by option rather than counted.** Its
  nine options split three ways:

  | | |
  |---|---|
  | ported in first | `-p`, `--no-header`, `-u` |
  | real procps options, still missing here | `-l`, `-o`, `-t`, `--sort` |
  | **inventions** | `--reverse`, `--json` |

  `--reverse` and `--json` both draw `error: unknown gnu long option` from
  procps — checked, not assumed, because `free`'s standalone advertised
  `--json` too and it was the one thing that looked like a feature. The four
  real ones are genuine capability and are **not** ported from that crate:
  measured, its implementations of them do not match procps either (0 XPASS),
  so porting the code would import a second wrong rendering. They are written
  fresh against measurements, one at a time, and each is declared by name in
  `ps-diff.sh`. `todo.txt` carries the entry.

  `DIFF_PKG=ps bash scripts/ps-diff.sh` now fails loudly with "has no
  Cargo.toml anywhere under … — is DIFF_PKG right?", which is correct: there
  is no second `ps` to measure.

  **The original entry, from before the fixes:**

  `scripts/ps-diff.sh` pins the process table in a PID
  namespace with its own `/proc` and compares both against procps-ng:

      coreutils ps     0 passed, 26 differed
      userspace/ps     0 passed, 26 differed

  A tally says "tie". The content says otherwise, and this is why a harness
  that only counts is not enough:

      procps      UID          PID    PPID  C STIME TTY          TIME CMD
                  root           1       0  0 13:39 ?        00:00:00 ps -f
      standalone    UID      PID     PPID    C       STIME  TTY   TIME  CMD
                      0        1        0    0       13:36  ?  00:00:00  ps
      coreutils     UID   PID  PPID  STAT   TIME     CMD
                      0     1     0  R      00:00:00 ps -f

  The standalone has **the right column set** and gets the widths, the UID
  resolution (`0` where procps prints `root`) and the `CMD` content wrong.
  coreutils' has **the wrong columns entirely** -- no `C`, no `STIME`, no
  `TTY`, and a `STAT` column procps does not put there. One is a formatting
  problem; the other is a different report. So the old "the standalone plainly
  wins" verdict survives measurement, which is notable given it was reached
  from line counts that were themselves wrong by a factor of five.

  **Do not read "0 vs 0" as "neither is worth keeping."** Both fail against
  procps; only one of them is failing at the last step.

* **`ps` also ignored every option it did not know, and `check-argv-ignored`
  could not see it — FIXED 2026-09-12.** Measured: `ps -X` printed the default listing at exit 0
  where procps exits 1, and `ps --help` prints the process table. The parser
  strips one `-` and then walks the string a character at a time:

      match c { 'e' | 'A' => all_procs = true, 'f' => full = true, _ => {} }

  So `--help` is read as `-h -e -l -p`, the `e` matches, and any long option
  containing `e`, `A` or `f` silently turns that flag on. `--full` would set
  `-f` by accident and `--version` would set `-e`.

  `scripts/check-argv-ignored.py` reports **0 bins ignore argv** with an empty
  baseline, and it is not lying: `ps` *does* read `argv` and *does* honour
  `-e` and `-f`. The gate catches a program that ignores its command line
  **entirely**, which is the defect `uptime` had. It does not catch one that
  honours the options it knows and silently discards the rest — which is the
  same class of hole, one notch finer, and is the shape §1006 is about: a
  program that cannot do what it was asked should say so.

  **The fix:** long options are matched whole, an unknown short option returns
  `error: unsupported SysV option` and an unknown long one
  `error: unknown gnu long option`, both at exit 1 — procps' own messages,
  measured. `--help` prints procps' 170-byte help text, captured with
  `cat -A` rather than retyped, because `uptime`'s help was missing a leading
  blank line and a trailing reference line for exactly the reason that those
  are invisible when you retype instead of measure.

  **A test was holding the defect in place.** It was called
  `parse_unknown_silently_ignored`, and its comment read "Preserves previous
  behaviour — no error, no panic." It preserved `ps -Q` printing the process
  table at exit 0. That is the third time this week a test has certified a bug
  — after `uptime`'s six asserting an unmeasured `up …` format, and
  `ctest-hostname`'s asserting that a personality name was extracted correctly
  when nothing consumed it. **A test and a specification are the same artifact
  right up until someone measures the reference**, and nothing in the test
  itself says which one it is.

  **The gap in `check-argv-ignored.py` is left open deliberately.** Extending
  it from "reads argv at all" to "refuses what it cannot honour" is a much
  harder static question — it would have to know each program's option set —
  and the differential harnesses answer it directly for every program that has
  one. Recorded here rather than filed as a checker change, because the
  cheaper instrument already exists.

* **`uptime`** — **RETIRED 2026-09-12. coreutils 40 passed / 0 differed
  against the standalone's 20 / 20**, same cases, `scripts/uptime-diff.sh`.

  The standalone is the best-performing loser of the seven: it passes half the
  cases rather than none. What it fails is a pair of clusters, and both are the
  kind a from-scratch implementation gets wrong — every uptime at or past a day
  boundary (86400, 86460, 172800, 259200, 604800), and every user count except
  one (0, 2 and 12 all wrong, 1 right). Those are exactly the two places
  procps' own rules are counter-intuitive: `up 1 day, 0 min` rather than
  `1 day, 00:00`, and `0 user` SINGULAR.

  Its three unique options — `-r`, `--raw`, `--json` — are all inventions.
  Measured, not taken from the note that already said so: procps answers
  `invalid option -- 'r'` and `unrecognized option '--raw'` / `'--json'`.
  Nothing to port.

* **`logger` had a separate bug, fixed 2026-09-12, independent of B-Q14.**
  Its parser ended in `_ => message_parts.push(arg)`, so an unrecognised
  option **became the message**: `logger -Q` logged the string `-Q` and
  exited 0 where util-linux prints `logger: invalid option -- 'Q'` and
  exits 1. Worse than `ps` discarding one, because the wrong thing is not
  dropped — it is written to the system log and kept. Refusing an option
  you do not implement is right under either answer to B-Q14.

  **The sweep that found it, kept because it is worth re-running and is
  not wired anywhere.** After fixing the same shape in `ps`, every
  coreutils bin was run with an option no utility has:

  ```bash
  cargo build -p coreutils --target x86_64-pc-windows-gnu
  B=target/x86_64-pc-windows-gnu/debug
  for n in $(ls userspace/coreutils/src/bin/*.rs | sed 's|.*/||; s|\.rs$||'); do
    [ -x "$B/$n.exe" ] || continue
    "$B/$n.exe" --no-such-option-xyzzy >/dev/null 2>&1 </dev/null || continue
    echo "$n accepted it"
  done
  ```

  83 tested, 6 accepted. Five are CORRECT and were checked against the
  real binaries rather than assumed — `echo`, `expr`, `printf`, `test` and
  `true` all treat it as text or an operand and exit 0, and ours agree.
  `logger` was the only defect.

  **Deliberately not made into a gate.** It needs all 83 binaries built,
  which is too heavy for `pre-push`, and the only place that already
  builds them is `scripts/boot-test.sh`, which is lane A's file. Writing
  a checker nobody runs is the shape this tree spent 2026-09-12 digging
  out of — `check-diff-preamble-order.py` sat unwired and red-treed
  `main`. A recipe that works is better than a gate that does not run.

  A *pattern* sweep would not have found it: `grep "_ => {}"` matches 25
  files in that directory and most are state machines. Running the
  program answers the question the pattern only approximates.

* **`logger`'s pair question** — **not a measurement question at all, and a
  harness cannot settle it.** `dup-bins-survey` lists it as "no harness — write one", which is
  the wrong instrument here. coreutils' `logger` writes its message to
  **stdout**; `userspace/logger` sends it to the `/dev/log` socket or appends
  to a file, the way util-linux does, with 23 options against 2. A
  differential harness would report that they disagree about everything, which
  is already known and is the *premise* rather than the finding.

  The stdout behaviour is deliberate — the module doc cites `CLAUDE.md`'s "No
  binary logs. Text-based (JSON-lines) structured logging." I think that
  misreads the rule: it governs the **format** a log is written in, not
  **where** the log lives, and writing to stdout does not make a log textual,
  it means there is no log. But it is a user-visible behaviour change either
  way, so it is **`open-questions.md` B-Q14** rather than a judgment call.
  Do not write the harness and do not delete either side until that is
  answered.

So the answer is per command, and for some pairs it is "merge", not "pick" —
and for at least one it is neither, because the two implementations are
answering different questions.

**Nothing collides today**, because `scripts/create-ext4-rootfs.sh` stages
neither -- no `userspace/` binary reaches `/bin` yet. The collision is latent
and arrives whole on the day they are staged, which is the worst time to
discover thirty-nine of them.

**Why this is not a lane-B decision to make alone.** Deleting a working program
is user-visible, and doing it thirty-nine times on my own judgement is a policy
rather than a fix. What lane B can do without asking is the evidence: for each
pair, which is the measured port and which is the invention. `free` and `ps`
above are two of thirty-nine; the rest are unexamined.

**Related but distinct** from the `/proc` entry above. That one is two parsers
of one *file*; this is two implementations of one *command*, and a pair can be
guilty of both -- the two `free`s are.

### The evidence, gathered 2026-09-10

The entry above said lane B could supply the evidence without deciding
anything. Here it is. Four signals per side, all mechanical: lines, `#[test]`
count, **fidelity markers** (mentions of `GNU coreutils`, `procps-ng`,
`util-linux`, "measured against", "transcription", "byte-exact", "upstream"),
and stub markers.

The fidelity count is the one that separates the groups, and it separates them
sharply: on the `coreutils` side it runs to 76 (`df`), 71 (`free`), 69 (`sed`),
67 (`cal`); on the standalone side it is **0 for every one of the 39**. A file
that cites upstream seventy times was written against upstream.

Classified by a stated rule -- `coreutils` fidelity >= 10 means it is the
measured port; otherwise a standalone more than 1.5x longer *and* with more
tests means it is the substantial one; otherwise it needs a human reading.

| | count | commands |
|---|---|---|
| `coreutils` is the measured port | **27** | cal, chown, cmp, comm, cut, dd, df, du, expand, fold, free, head, join, nl, paste, sed, seq, split, stat, strings, tar, tee, tr, tsort, uniq, wc, xargs |
| the standalone is the substantial one | **6** | date, logger, patch, sha256sum, uptime, who |
| needs reading | **6** | diff, env, hostname, kill, ps, uname |

Two spot-checks, because a rule that classifies without being checked is a
different kind of guess:

* `coreutils`'s `date` opens *"No timezone support yet -- always UTC"* against a
  standalone with strftime, RFC 5322/3339, ISO 8601 and parsing. Group 2 is
  right.
* `coreutils`'s `stat` is a real port, but the standalone is **larger** (2840
  against 2118) with twice the tests. Group 1's rule fires on the fidelity
  markers and the evidence is genuinely mixed. Which turned up the complication
  below.

### Eleven of the thirty-nine are not pairs

**`userspace/stat` is one binary that answers to six names** -- `stat`, `ln`,
`mkfifo`, `readlink`, `realpath`, `touch` -- dispatched on `argv[0]`. Eleven of
the thirty-nine standalone crates do this. "Retire the standalone" is therefore
not a per-command decision for those eleven; it removes whatever else rides
along.

Checked rather than assumed: all five of `stat`'s riders **do** have
`coreutils` counterparts, so that one is safe. One command is not:

> **`ncal` exists only inside `userspace/cal`.** `coreutils/src/bin/cal.rs`
> does not contain the string at all. Retiring `userspace/cal` deletes `ncal`
> from the system.

That is the whole of the loss across the eleven, and it is one command -- which
is worth knowing precisely, because "some of them provide other commands too"
would have been enough to stall the decision indefinitely.

### One thing this exercise found about lane B's own recent work

`userspace/ps` had **zero tests** -- verified by running them, not by counting
`#[test]`. It was converted to `procinfo` two ticks ago and no test was added,
while `coreutils`'s `ps` was converted the next tick *and* given four. Same
lane, same week, same kind of change, two standards.

**Given eight**, since the gap was mine and closing it does not prejudge which
`ps` survives. One of the eight is not an assertion about `ps` at all but about
the pair:

> **The two `ps` implementations render the same `tty_nr` differently.**
> `userspace/ps` prints `tty{n}` from the raw number; `coreutils/src/bin/ps.rs`
> prints `pts/{n & 0xff}`. For `tty_nr = 34816` they print `tty34816` and
> `pts/0`. Neither is obviously right -- 34816 is `(136 << 8) | 0`, so `pts/0`
> reads a real Linux device number correctly and `tty34816` names a device that
> does not exist -- but they are two answers to one question, measured, in one
> tree. It is the concrete form of everything above.

### What is still not decided, and by whom

Nothing above chooses. The 27 in group 1 look like deletions and the 6 in
group 2 look like the reverse, but *acting* on 39 commands is a user-visible
policy and stays the operator's. What has changed is that it can now be decided
from a table instead of from thirty-nine readings.
