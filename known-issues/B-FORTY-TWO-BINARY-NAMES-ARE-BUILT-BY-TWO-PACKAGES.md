## B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES (lane B, 2026-08-22) — harnesses fixed, the duplication itself is open

**Status (lane D, 2026-09-28):** two pairs are left, `kill` and `logger`,
and both are on the image now, so "nothing we *install* is affected" below no
longer holds: built in one cargo invocation, the image's `/bin/kill` was
coreutils' copy, without `killall`. `scripts/create-ext4-rootfs.sh` now builds
the standalone crates in a second invocation, so theirs are the copies left in
`release/`, and refuses to stage coreutils' copy of a name two packages build.
Merging the two pairs is asked of lane B in
`requests/d-b-kill-and-logger-are-built-twice-and-three-image-crates-miss-sysroot-dep.md`.


**In short:** Forty-two of our command-line utilities exist *twice* in this
tree, as two separate programs with the same name — one inside the big
`userspace/coreutils` package, one as its own little `userspace/<name>` crate.
Both get compiled to the same file on disk, so the one you actually run is
whichever was compiled most recently. That is how a test harness spent a day
reporting three bugs in `bc` that the shipped `bc` does not have. Nothing we
*install* is affected yet, because the disk image does not stage these binaries;
what is affected is every measurement taken by running one of them.

**How to see it:**

```
$ cargo build -p tr -p coreutils --bin tr --target x86_64-pc-windows-gnu
warning: output filename collision at …\target\x86_64-pc-windows-gnu\debug\tr.exe
  = note: the bin target `tr` in package `tr` has the same output filename as
          the bin target `tr` in package `coreutils`
  = note: this may become a hard error in the future; see rust-lang/cargo#6313
```

Cargo warns and then picks one. The warning is emitted only when both are asked
for in a single invocation, so `cargo build -p coreutils` alone is silent — and
then a later `cargo build --workspace` quietly replaces the file.

**The full list** as first filed (42 names; every one is `coreutils` vs a
standalone `userspace/<name>`). `bc` is since resolved — one producer,
`userspace/bc` — leaving **41**:

```
awk bc cal chown cmp comm cut date dd df diff du env expand fold free head
hostname join kill logger nl paste patch ps sed seq sha256sum sort split stat
strings tar tee tr tsort uname uniq uptime wc who xargs
```

**It is not simply "the standalone ones are dead."** The fork went both ways:

| | live copy | the other one |
|---|---|---|
| `tr`, `wc`, `uniq`, `nl`, `cut`, `head`, `join`, `sed`, `seq`, `split`, `tsort`, `expand`, `fold`, `comm`, `paste`, `awk`, `sort` | **`coreutils`** — rewritten Aug 2026 for GNU parity, each certified by its own `*-diff.sh` | standalone, frozen 2026-06-13, roughly half the size |
| `bc` | **`userspace/bc`** — Aug 2026 rewrite on `bignum::Decimal`, 200/200 against GNU | `coreutils/src/bin/bc.rs`, June, the one that fails 105 cases — **deleted 2026-08-22, this pair is done** |
| `cal`, `date`, `dd`, `df`, `diff`, `du`, `env`, `free`, `hostname`, `kill`, `logger`, `patch`, `ps`, `stat`, `strings`, `tee`, `uname`, `uptime`, `who`, `xargs`, `chown`, `cmp`, `sha256sum` | **standalone** — 1000–2800 lines each | `coreutils/src/bin/<n>.rs`, a 125–450 line stub |
| `tar` | **`coreutils`** — 7189 lines, 131 tests, certified by `tar-diff.sh` (178/178) | `userspace/tar`, 2724 lines, 72 tests — **moved out of the third row 2026-08-30; see below** |

So resolving it means a per-utility decision, and in the third row it means
moving the real implementation *into* `coreutils` (or deciding `coreutils` is
not the home). It is not a delete-41-directories change.

**`tar` changed rows, and the stale entry cost a session's work (2026-08-30).**
This table put `tar` in the third row — standalone live, `coreutils` a stub —
which was true when it was written on 2026-08-22 and is not true now.
`coreutils/src/bin/tar.rs` has since grown to 7189 lines with 131 tests and is
what `scripts/tar-diff.sh` measures (`DIFF_PKG` defaults to `coreutils`), what
the old-option style, the `LONG_OPTIONS` refusal table, the delayed-symlink
defence and the whole overwrite family live in. `userspace/tar` is referenced by
**nothing**: no script, no image staging, no harness. The only thing that builds
it is the `userspace/*` workspace glob, and when it does it collides with
`coreutils`' `tar.exe` and whichever linked last wins.

Acting on the stale row, four commits of GNU-parity work (`c3844c749`,
`7924a556a`, `b01d77349`, `896d01a2d`, `e161a340c`) went into `userspace/tar` —
correct code, on the copy nobody runs, duplicating behaviour `coreutils` already
had and in one case reaching it by a worse route (`userspace/tar` joins paths
for `-C` and so has to reason about which name to print; `coreutils` chdirs, as
GNU does, and the question does not arise). Only one genuine gap in the shipped
tar came out of it: the once-per-run `Extracting contiguous files as regular
files` notice for type flag `7`, now fixed with a discriminating case in
`tar-diff.sh`.

**The lesson is the general one, not a `tar` one:** the live/dead split in this
table is *not* checkable by reading the table, because the table ages. Check it
the way the harness does — which package does `DIFF_PKG` name, and does anything
outside the workspace glob reference the standalone crate at all. Before editing
any utility in either of the two rows above, confirm which copy is live
**today**.

**Fixed so far (2026-08-22).**

1. The differential harnesses no longer depend on which copy won:
   `scripts/diff-subject.sh` builds the subject from a named package
   immediately before the comparison, and all 27 `*-diff.sh` harnesses use it.
   That removes the way this defect was actually hurting us.
   *(2026-08-25: the mechanism is now `scripts/diff-wsl.sh`, which subsumed
   `diff-subject.sh` when every harness moved into WSL, and it covers all 45
   harnesses rather than 27. Naming the package is unchanged — it is the
   `DIFF_PKG` knob. See design-decisions.md §382.)*
2. **`bc`'s two implementations are merged**, which was the substance of the
   `bc` problem regardless of where the survivor lives: the August rewrite on
   `bignum::Decimal` (200/200 against GNU) is the one that survives, and the
   June implementation that fails 105 cases is gone. `calc-diff.sh` names the
   package explicitly and still reports 200 passed, 0 differed.

**Not settled — which package is the one home.** I wrote
**design-decisions.md §359** saying `coreutils` is, and began implementing it,
before finding that **§8 (2026-06-12) decides the opposite and is an *operator*
decision**: standalone per-tool crates are canonical, `coreutils/src/bin/*`
retires. §359 is therefore **suspended and unimplemented**, and the `bc` move
into `coreutils` has been reverted so the tree matches §8 — `userspace/bc` is
back, carrying the good August code.

The reason this is a question rather than simply "obey §8" is that §8's
deciding argument is false. It retires `coreutils` for being "a busybox-style
multi-call binary that dispatches on `argv[0]`" — one file serving many tools,
which forces one on-disk identity to hold the union of all their capabilities.
`coreutils` is not that: 86 separate bin targets, no `argv[0]` dispatch, no
`src/main.rs` anywhere in its history, and it already contains the shared
library §8 proposed extracting as `coreutils-common` (which was never created —
no part of §8 was ever carried out). The crates that *do* dispatch on `argv[0]`
are standalone ones: `stat` (six tools), `sha256sum` (four), `chown` (two),
`who` (two). Queued for the operator as **`open-questions.md` → B-Q7**, with
the measurements and three options. Until it is answered, §8 governs and no
further consolidation happens in either direction.

**A tool for the remaining 41: `scripts/dup-bins-survey.py`.** It lists every
colliding name with both sides' line counts and the set of command-line options
each side's source mentions, and ranks which side is ahead. It is triage, not a
verdict — every pair is still read before either copy is deleted — but it turns
"42 unknown pairs" into 12 where `coreutils` leads, 24 where the standalone
does, and 6 that need reading.

Its own history is a warning worth keeping. The first version scanned for
`"--name"` string literals, which `coreutils`'s newer parsers do not contain —
they hold long options in a table with the dashes already stripped
(`("equal-width", Long::EqualWidth)`), because that is the form GNU's
abbreviation rule compares against. So it credited every long option to the
standalone side and called `nl`, `split`, `seq`, `comm` and `tr` "standalone
ahead" — five utilities where `coreutils` implements a superset *and* is the
side under a differential harness. Acting on that would have deleted the tested
implementation: the same mistake as the `bc` bug report, one level up. A second
pass over `b'X'` byte literals was needed for the same reason in reverse — a
bare scan credited `tr` with `-A -F -X -Z`, which are `for b in b'A'..=b'Z'`
expanding `[:upper:]`, not flags.

**39 -> 38 (2026-09-11): `fold`.** The first pair retired under §1005, and the
method is worth repeating because reading alone would not have decided it.
The survey called this one "close -- read both", with the standalone 483 lines
against coreutils' 874 and **no option unique to either side**, so an options
diff had nothing to say.

A behavioural differential did. Both binaries were built and run against GNU
coreutils 9.4 over 18 cases: **coreutils agreed 18/18, the standalone 14/18.**
The four are all user-visible and none is a missing option:

| case | GNU and coreutils | standalone |
|---|---|---|
| `-w 0` | refuses, *invalid number of columns* | silently folds to one character per line |
| legacy `-5` | `abcde` / `fghij` | *invalid option -- '5'* |
| multibyte `-w 4` on three `é` | `éé` / `é` | all three on one line |
| a `
` in the line | resets the column | breaks the line |

The `-w 0` row is the one that matters most: a silent wrong answer where GNU
refuses is worse than a missing feature, because nothing downstream can tell.

`userspace/fold` deleted. Two side-effects worth noting: the lint-exemption
list fell 128 -> 127 without anyone fixing a warning, because the crate that
was exempt is gone; and gate 24 stayed green, because `fold` is still a name
the tree produces — it tracks the NAME, not the crate, which is exactly the
distinction that made it worth building.

**One observation about the remaining 38.** Every duplicated pair builds two
binaries with the same file name into the same target directory, so the second
`cargo build` silently overwrites the first. Which implementation you get
depends on build order. That is not a new finding — §1005 describes the tools
"alternating non-deterministically between two implementations" — but it is
worth knowing that it reproduces locally in seconds.

**38 -> 37 (2026-09-11): `cut`.** The survey called this one "close -- read
both" as well, 843 standalone lines against coreutils' 1515. The differential
(`scripts/dup-differential.py`, written for exactly this) ran 36 cases:
**coreutils agreed with GNU 36/36, the standalone 24/36.**

Counted honestly, the 12 split two ways. **Seven are message wording only** --
both refuse, both exit 1, only the text differs -- and are a divergence rather
than a defect. **Five are wrong answers:**

| case | GNU and coreutils | standalone |
|---|---|---|
| `-b 1-3` on `ab` | passes the bytes through | **refuses the whole stream**: "did not contain valid UTF-8" |
| `-c 1` on `éé` | the first byte | the first character |
| `--output-delimiter` with `-b` ranges | `ab-de` | `abde`, delimiter dropped |
| `-f 2` on a CRLF line | keeps the `
` | strips it |
| `-d ''` | works | refuses |

The first is the one that settles it. `cut -b` is BYTE mode, and the standalone
forces UTF-8 on the stream, so the flag whose entire purpose is byte-oriented
work fails on binary input — CLAUDE.md item 7 in the one place it matters most.

`userspace/cut` deleted; three ledgers moved with it (lint exemptions 127 ->
126, argv-utf8 223 -> 222, collisions 38 -> 37) without anyone fixing a
warning.

**37 -> 36 (2026-09-11): `seq`.** One of the five names the survey's FIRST
version ranked backwards, so it is exactly where reading had already proved
unreliable. 41 cases: **coreutils 41/41 against GNU, the standalone 28/41.**

Five of the thirteen are message wording. **Eight are wrong answers**, and two
of those would change what a script does:

| case | GNU and coreutils | standalone |
|---|---|---|
| `seq 5 2` | **nothing** — a descending range with the default `+1` step is empty | counts backwards: `5 4 3 2` |
| `seq -f %03g 1 3` | `001 002 003` | `1 2 3` — the option is accepted and ignored |
| `seq -f %.2f 1 3` | `1.00 2.00 3.00` | `1 2 3` |
| `seq -f '%g%%' 1 2` | `1%` | `1%%` |
| `seq -f %d 1 3` | refuses: not a float format | prints anyway |
| `seq 0x1 0x3` | `1 2 3` | refuses |
| `seq nan` | refuses, exit 1 | exit 0, no output |

`seq 5 2` is the one to read twice. `for i in $(seq $start $end)` is the
commonest use of this program, and with `start > end` GNU runs the loop zero
times while ours runs it *backwards*. Nothing in the script says which it got.

The `-f` rows are §1006's shape in miniature: the option is parsed, stored, and
advertised in `--help` as "use printf-style FORMAT", and does not reach the
output.

`userspace/seq` deleted; lint exemptions 126 -> 125, argv-utf8 222 -> 221,
collisions 37 -> 36.

**The tool grew a bound because of this pair.** `seq 1 inf` and `seq 1 0 5`
are the natural ways to ask a number generator to misbehave, and the first run
hung on them. `dup-differential.py` now caps each case at 5 s and 1 MiB and
records a timeout as its own outcome — so a side that hangs where the other
answers is a visible difference rather than a stuck harness. A differential
that cannot survive the inputs it exists to try is not one.

**36 -> 35 (2026-09-11): `tr`.** Also one of the five the survey first ranked
backwards -- it had credited `tr` with flags `-A -F -X -Z` that were really
`b'A'..=b'Z'` inside a character-class expansion. 46 cases: **coreutils 46/46,
the standalone 38/46.**

Five of the eight are message wording. The other **three are one defect wearing
three hats**: GNU refuses, ours succeeds.

| invocation | GNU and coreutils | standalone |
|---|---|---|
| `tr abc ''` | *when not truncating set1, string2 must be non-empty*, exit 1 | passes `abc` through, exit 0 |
| `tr abc '[:upper:]'` | *misaligned `[:upper:]` construct*, exit 1 | outputs `ABC`, exit 0 |
| `tr a b c` | *extra operand 'c'*, exit 1 | outputs `bbc`, exit 0 — the third operand is ignored |

The third is the one a person hits. An extra operand is a typo, and GNU stops;
ours produces output that looks entirely reasonable. The second is subtler and
worse for portability: `[:upper:]` in SET2 is only valid opposite `[:lower:]`
in SET1, so a script that works here fails on any real system.

**Worth noting against the previous two.** `cut` and `seq` were too STRICT in
places (`cut -b` refusing non-UTF-8) and too lax in others. `tr` is uniformly
too lax: every difference is a malformed invocation accepted. Three pairs in,
the standalone implementations are not wrong in one characteristic way -- they
are wrong in whatever way nobody tested.

`userspace/tr` deleted; lint exemptions 125 -> 124, argv-utf8 221 -> 220,
collisions 36 -> 35.

**35 -> 34 (2026-09-11): `nl`.** The last of the five the survey first ranked
backwards, and the worst pair so far: 43 cases, **coreutils 43/43, the
standalone 21/43** — under half, with the DEFAULT invocation among the
failures.

Seven of the 22 are message wording. The other **fifteen are five distinct
defects**, two of which are total:

| defect | effect |
|---|---|
| unnumbered lines | emits a literal **tab** where GNU pads the field with spaces. Plain `nl` on any file containing a blank line produces different bytes. |
| `-l N` | **no output at all**, at every value tried. The option is accepted and the program prints nothing. |
| section delimiters `\:`, `\:\:`, `\:\:\:` | not recognised. GNU consumes the delimiter line and RESTARTS numbering; ours prints it as a line and keeps counting. |
| a byte that is not UTF-8 | **no output at all** |
| a CRLF line | the `
` is stripped |

`-l` deserves the emphasis. It is not mis-implemented, it is inert *and*
destructive: the program consumes its input, prints nothing, and exits. Any
pipeline using it loses the data silently.

The unnumbered-line defect is the one that would be noticed last and hurt
longest, because the output looks right in a terminal — a tab and seven spaces
land in the same column — and is wrong to `diff`, to `cut -f`, and to anything
counting bytes.

`userspace/nl` deleted; lint exemptions 124 -> 123, argv-utf8 220 -> 219,
collisions 35 -> 34.

**All five of the originally-misranked names are now decided** (`nl`, `split`,
`seq`, `comm`, `tr` — `split` and `comm` by the survey's corrected ranking,
`seq`, `tr` and `nl` by differential). Every one of the three put to a
differential went to coreutils, and none of the deciding differences was a
missing option — which is what the survey measures.

**A correction to the method, found after four retirements (2026-09-11).**
`scripts/` already holds **59 differential harnesses** — `nl-diff.sh`,
`cut-diff.sh`, `uniq-diff.sh` and the rest — and they cover **half the
remaining pairs**. I wrote `dup-differential.py` without looking for them.

They are better at the same job:

* far more cases — `nl-diff.sh` has 222 against my 43, `uniq-diff.sh` 273;
* `od -An -c`, so whitespace is exact. That decided `nl`: a literal tab where
  GNU pads with spaces. Its own comment says a comparison that collapsed
  whitespace "would agree with almost every wrong implementation";
* both sides inside WSL under one `argv[0]`, because the Windows host's
  coreutils are MSYS2's and word every diagnostic differently;
* **a reference built from GNU 9.4 source**, because WSL's installed coreutils
  is Ubuntu's patched `9.4-3ubuntu6.1` — §726's point that a green run against
  it certifies agreement with Debian rather than with GNU;
* and they are parameterised: `OURS=/path/to/binary ./scripts/uniq-diff.sh`,
  which is exactly the question I wrote a new tool to ask.

`dup-differential.py` compares against `wsl -e <name>` — the patched build —
so its numbers are agreement with Ubuntu's coreutils and not with GNU.

**Why the four verdicts still stand.** In each pair the coreutils side scored
100% against that same reference while the standalone scored 52–88%, and every
deciding difference was structural rather than a wording or packaging detail:
`seq` counting backwards where GNU prints nothing, `nl -l` printing nothing at
all, `tr` accepting an extra operand, `cut -b` refusing a non-UTF-8 stream. A
reference that were systematically wrong could not have produced a perfect
score for one side. That is internal consistency, not a built reference, which
is why this is written down rather than waved away.

**The rule going forward:** use `scripts/<name>-diff.sh` where one exists
(awk, cmp, comm, dd, df, du, expand, join, paste, sed, sort, split, tar, tee,
tsort, uniq, wc, xargs) and `dup-differential.py` only for the eighteen that
have none. Writing a real harness for those eighteen would be better than
either.

**34 -> 33 (2026-09-11): `uniq`, and the first decided with the PROPER
harness.** `DIFF_PKG=uniq bash scripts/uniq-diff.sh` — the tree's own
`uniq-diff.sh`, 273 cases, `od -An -c`, and a GNU 9.4 reference built from
source rather than Ubuntu's patched package.

**coreutils 273 passed, 0 differed. The standalone 134 passed, 139 differed** —
more than half the cases.

`DIFF_PKG` is the knob `diff-wsl.sh` grew for exactly this, after
`calc-diff.sh` once "reported 95 passed, 105 differed" and three bugs were
written up against a `bc` nobody intends to ship, because the output-filename
collision let the wrong binary win. Overriding it points a harness written for
coreutils at the standalone instead, which is the whole question §1005 asks.

Four representative defects:

| case | GNU | standalone |
|---|---|---|
| `-z -f1` | `p\nq\0 p\nr\0` | `p\nq\0` — **a record is dropped** |
| `+2` (traditional skip-chars) | works | treated as a FILENAME: *No such file or directory* |
| `--group=both` | one blank line between groups | two |
| `--group=b` | accepts the unambiguous abbreviation | rejects it |

The `-z -f1` row is silent data loss, which is the worst outcome available to a
filter: fewer records out than in, exit 0, nothing said.

**This is the method working as corrected.** The four pairs before it were
decided with `dup-differential.py` against Ubuntu's patched coreutils; this one
used the built GNU reference and far more cases, and reached the same kind of
verdict much more strongly. For the eighteen names that have a `<name>-diff.sh`,
`DIFF_PKG=<name>` is the whole procedure.

**33 -> 32 (2026-09-11): `tee`, and the survey was wrong in the dangerous
direction.** It ranked this pair **"standalone ahead"** — the only such verdict
tested so far — on the strength of three options only the standalone mentions.
Behaviour: **coreutils 71 passed 0 differed; the standalone 34 passed 37
differed.**

That is the failure mode the entry above warns about for the survey's first
version, still live in the current one: an options count can say the standalone
is ahead when it is behind on half the cases, and acting on it deletes the
better implementation.

Of the 37, seventeen are Rust's `io::Error` leaking `(os error 2)` into
diagnostics where GNU prints only the `strerror` text — the file trees match
exactly, so those are wording. The rest are real:

| invocation | GNU | standalone |
|---|---|---|
| `--output-error=exit-n` | accepts the unambiguous abbreviation and writes the file | refuses, **writes nothing** |
| `tee --output-error out` | `--output-error`'s argument is OPTIONAL, so `out` is the file — writes it | consumes `out` as the mode, fails, writes nothing |
| `--output-error=e` | *ambiguous argument* | *invalid* — no ambiguity concept at all |

**The uncomfortable part, and the reason this is worth more than one line.**
Two ticks earlier I brought `userspace/tee` under the lint policy and added six
tests to it. One of the sites I repaired is the `--output-error` argument
handling: I replaced a bound-check-then-index with `let Some(next) =
args.get(i)`, which made it provably non-panicking — and left it *behaviourally
wrong*, because GNU's argument there is optional and ours consumes the next
operand unconditionally. My own tests passed, because I wrote them against the
behaviour rather than against GNU.

A lint pass proves a program cannot crash. It says nothing about whether the
program is right, and it is easy to come away feeling otherwise. The 129-crate
lint programme above is still worth doing; it is not a substitute for a
differential, and two of its three crates so far were duplicates that a
differential then deleted.

**20 -> 19 (2026-09-11): `sort`, which truncates on a bad byte and exits 0.**
`DIFF_PKG=sort bash scripts/sort-diff.sh`:

**coreutils 280 passed, 0 differed. The standalone 113 passed, 167 differed.**

| | cases | defect |
|---|---|---|
| **truncates, complains, succeeds** | **9** | `sort bytes.txt`, five lines of which one holds a high byte: GNU sorts all five. The standalone prints **one line**, writes `read error: stream did not contain valid UTF-8` to stderr — and **exits 0**. Partial output with a success status is the worst combination available: `sort < in > out` in a pipeline silently loses four fifths of the data and nothing downstream can tell. The other standalones merely refuse; this one refuses *halfway through* and calls it success. |
| option unrecognised | 78 | `-i` (ignore nonprinting) and `-g` (general numeric) are whole sort modes, not flags. `-k1,1i` is rejected too — a key with a per-key modifier. |
| **wrong order, exit 0** | **62** | `sort -n` over `+1 -0 0 01 1 1.0 1.00` gives `-0 0 +1 01 1 1.0 1.00`; GNU gives `+1 -0 0 01 1 1.0 1.00`, because GNU's `-n` does not accept a leading `+` and so ranks it as zero. `sort -nr` is not the reverse of the standalone's own ascending answer either. Putting lines in order is the whole program. |
| `+1` read as a filename | 5 | the traditional key syntax `sort +1` and `sort +0.1` become `+1: No such file or directory (os error 2)` — printed to stderr while **exiting 0** and emitting an unsorted file. |
| exits 0 where GNU refuses | 5 | `-t '	'` (`multi-character tab`) and `-k1.0` (`character offset is zero`) are both accepted and acted on. |
| message shape | 7 | `sort -c a b` reports a disorder in `a` where GNU reports `extra operand 'b' not allowed with -c`; the operand error is the real one. |

**Two families here exit 0 after writing a diagnostic** — the truncation and the
`+1`-as-filename. That combination deserves its own note: a caller that checks
the exit status, which is the correct thing to check, is told the run
succeeded.

**21 -> 20 (2026-09-11): `du`, whose every number is wrong.**
`DIFF_PKG=du bash scripts/du-diff.sh`:

**coreutils 188 passed, 0 differed. The standalone 3 passed, 185 differed.**

The whole of it is visible in the first case, `du t`:

| GNU | standalone |
|---|---|
| 4 → `t/empty` | *(absent)* |
| 12 → `t/sub/deep` | 16 → `t/sub/deep` |
| 24 → `t/sub` | 48 → `t/sub` |
| 3036 → `t` | 4112 → `t` |

**An empty directory is missing from the listing**, and **every remaining
number is different** — not by a constant factor either: 12→16, 24→48,
3036→4112. Both exit 0. `du` prints nothing but sizes and paths, so a `du` that
gets the sizes wrong and drops a directory has no correct output left; there is
nothing else in it to be right about.

**10 -> 9 (2026-09-11): `stat`, and one line explains 25 of our own failures.**
`scripts/stat-diff.sh`, written today, 107 cases against a GNU coreutils 9.4
built from source.

**coreutils 75 passed, 30 differed. The standalone 44 passed, 61 differed.**

The standalone lacks `--printf` entirely (12 cases) and long-option
abbreviation, and its default report is wrong in 41 cases. Retired.

**What the surviving half gets wrong — after a correction.** 25 of coreutils'
30 were the default report's `File:` line, which ours quotes and GNU does not. I
filed that as a defect and **it is not one**: it is design decision §371, taken
deliberately because GNU's own quoting there is a `strstr` accident and because
a file name must not be able to forge a line of `stat`'s own output. Those cases
are now `xfail`s naming §371, and the honest score is **75 passed, 13 differed,
19 differ on purpose**. Of those, two more turned out to be the harness
writing to the filesystem it was measuring, so the score is now **77 passed, 11
differed, 19 differ on purpose** — seven `stat -f` cases (one real defect in
`%i`, plus `%t`/`%T` which are a documented consequence of using `statvfs`) and
four missing-operand wordings.

**Two things about the harness worth keeping.**

*Both sides read the SAME files.* `stat` does not mutate, so there is no reason
to give each side its own copy — and a strong reason not to: two copies have two
different inode numbers, so `%i`, `%d` and `%b` could never be compared. Those
are exactly the fields an implementation is most likely to get wrong, because
they are the ones it cannot guess. **Give the subject a private copy only when
it writes** — `patch-diff.sh` and `chown-diff.sh` do, this does not.

*The multicall hazard was checked before the harness was written, not after.*
`userspace/stat` dispatches on `argv[0]` and also serves `touch`, `mkfifo`,
`readlink`, `realpath` and `ln`, so retiring the crate deletes six programs. All
six were confirmed present in `coreutils/src/bin` first. A pair's row in
`dup-bins-survey.py` names one program; the crate behind it may be six.

**`chown` — MEASURED AND NOT DECIDED (2026-09-11).**
`scripts/chown-diff.sh`, written today, 66 cases against a GNU coreutils 9.4
built from source. **`coreutils` 64 passed, 0 differed** — every ownership
change, every refusal, `-R` with each of `-H`/`-L`/`-P`, `-h` on a symlink and
on a dangling one, `--from=`, `--reference=`, `-c`/`-v`/`-f`.

The standalone scored 3 of 64, and **that number is not evidence**: 43 of its 61
failures are `chown syscall unavailable on this platform`, a stub it compiles
only when NOT built for SlateOS. See
`TD-B-A-LINUX-TARGETED-HARNESS-CANNOT-MEASURE-A-SLATEOS-GATED-SUBJECT`. The pair
stays open.

*What the harness does establish* is that `coreutils`' half is correct against
GNU on all 64, which is worth having on its own — and the 18 real differences on
the standalone's side are recorded there.

*Two safety properties of this harness, since `chown` mutates:* it refuses to
run as root, because unprivileged `chown root f` is a refusal and a comparison
while as root it is a change; and it verifies after building its fixtures that
**no symlink resolves outside the fixture tree**, because `chown -R -L` follows
directory symlinks and a link to `/etc` would have made the harness change
something real.

**11 -> 10 (2026-09-11): `strings`, decided by one data-loss case.**
`scripts/strings-diff.sh`, written today, 71 cases against GNU Binutils 2.42.

**coreutils 54 passed, 15 differed. The standalone 46 passed, 23 differed.**

Eight passes apart, which on its own is the kind of margin this file has twice
declined to act on (`patch`, `hostname`). What decides it is that the margin is
**qualitative as well**:

| | coreutils | standalone |
|---|---|---|
| `-e S` (8-bit characters) | correct | **drops data** |
| `-s` / `--output-separator` | present | absent (3 cases) |
| `strings -` (stdin) | refuses as GNU does | exits 0 |

The `-e S` case is the one that matters. That encoding exists to say "high bytes
are printable", and on `café latte`:

    GNU         café latte
    standalone  latte

It drops the high byte **and everything before it**, with exit 0. A tool run to
find strings in a binary silently returns fewer of them, which is the failure
mode that tool cannot have.

**What BOTH halves get wrong, and so survives the retirement** — filed as
`B-STRINGS-HAS-NO-DATA-SECTION-OPTION`:

  * `-d`/`--data` is absent from both. GNU scans only the *initialised, loaded*
    sections with it, which is the option's whole point on an object file.
  * The diagnostics for a bad option *value* differ on 10 of coreutils' 15 —
    `-n 0`, `-n -1`, `-n notanumber`, `-t q`, `-e q` and the bare forms. The
    exit statuses agree; the sentences do not.

**A note on the fixtures.** They are written with `printf` escapes from inside
the harness rather than checked in, because every interesting case here is a
byte that is not text — a run exactly at the length threshold, a NUL, a high
byte, UTF-16. A checked-in fixture is a file some editor has had an opinion
about. The one exception is a copy of `/bin/true`, present so that the default
"scan only the loaded sections" rule is exercised against a real object file;
a fixture that is not an object file hides that rule completely.

**12 -> 11 (2026-09-11): `cal`, where our half is perfect and the other fails
everything.** `scripts/cal-diff.sh`, written today, 105 cases against
util-linux's `cal`.

**coreutils 103 passed, 0 differed. The standalone 0 passed, 101 differed.**

`cal` is pure computation — every byte is a function of the arguments and of a
calendar reform that happened in 1752 — so a disagreement is always a defect in
one side and never an environment difference. The coreutils half gets all of it:
September 1752 missing its eleven days, 1900 not a leap year while 2000 is, the
ISO and US week numberings, `-j` renumbering every cell, `--reform=julian`, the
three-month and whole-year layouts.

**The standalone fails every case, for two causes:**

| | cases | defect |
|---|---|---|
| **no trailing padding** | ~94 | util-linux pads *every* line to the calendar's width. Measured on `cal 1 2021`: its line lengths are `20 20 20 20 20 20 20 20`, ours are `16 20 20 20 20 20 20 2` — the month title is not padded to width and the last week row is two bytes. That padding is exactly what makes `-3` and `-y` line up in columns, so the layout options cannot work without it. |
| **ANSI escapes on a pipe** | 7 | today's cell is wrapped in `ESC[7m` … `ESC[0m` on non-terminal output, in every month that contains today. Same defect as the retired `df`: highlighting must be gated on the output being a terminal. |

Neither is visible to a comparison that trims whitespace, which is why this
harness compares `od -An -c`. In the collapsed report `cal 1 2021` renders
*identically* on both sides and is still a difference.

**Two of the harness's own `xfail`s were wrong, and it said so.** `-h` and
`--help` were written as expected-to-differ on the assumption that help text is
always ours; they came back XPASS, because coreutils' `cal` reproduces
util-linux's help exactly. Corrected to ordinary cases. An exemption that has
stopped being true is worth more as a noisy failure than as a quiet allowance —
which is the argument for counting XPASS at all.

**13 -> 12 (2026-09-11): `env`, and a harness that measured nothing first.**
`scripts/env-diff.sh`, written today, 61 cases against a GNU coreutils 9.4 built
from source.

**coreutils 39 passed, 20 differed. The standalone 18 passed, 41 differed.**

The standalone's largest family is the program's core function: **16 cases where
both sides exit 0 and the environment printed differs** — plain `env`, `env -0`,
`env -u ZETA`. Eleven more leak `(os error N)` into diagnostics. Nine are
options it does not have.

**What the surviving half still gets wrong is the same root cause as `uname`:**
8 of its 20 are `-S`/`--split-string` (absent entirely) and long-option
abbreviations (`--unse`, `--ign`, `--nu`), because `env.rs` is one of the
sixteen coreutils bins that still parse `argv` by hand instead of through
`coreutils::getopt`. Three more exit 0 where GNU refuses — `env -u` with no
argument, `env -C target` with no command.
