## `A-KSHELL-DIFF-BLAMES-THE-TRAILING-NEWLINE-FOR-ANY-DIFFERENCE-IT-CANNOT-SEE` (lane A, 2026-08-25) — ✅ **FIXED** (`6d8a057d7`, `fc1670be6`, 2026-08-25)

**Where.** `kernel/src/kshell.rs` — `cmd_diff` (~98986), specifically the decode
at ~99024 and the fallback message at ~99215.

**What.** `diff` establishes that the files differ by comparing their *bytes*:

```rust
if data1 == data2 { return; }          // exit 0, identical
```

and then does the actual work on a *lossy decode* of those same bytes:

```rust
let text1 = String::from_utf8_lossy(&data1);
let lines1: Vec<&str> = text1.lines().collect();
```

The two views disagree, and every case where they disagree ends at one line:

```rust
if hunks.is_empty() {
    // Should not happen since data1 != data2, but could if only trailing
    // newline differs. Show a minimal note.
    shell_println!("(files differ only in trailing newline)");
}
```

That message is a **guess about the cause, printed as a finding**. The comment
above it says "should not happen", which is the tell: the author knew the byte
compare and the line compare could disagree, could not enumerate the ways, and
picked the one benign explanation. Exit is 1, correctly — the files do differ.
What is wrong is the sentence, which names a cause nobody checked.

**The all-ASCII path, which needs no exotic input at all.** `str::lines` splits
on `\n` *and strips a trailing `\r`*. So for a DOS file against its Unix
twin — the single most common reason to reach for `diff` — every line decodes
identically, no hunk is produced, and the user is told:

```
$ diff dos.txt unix.txt
(files differ only in trailing newline)
```

They differ on *every* line, in the line ending, which is exactly what the user
was trying to find out. This is the same `str::lines` defect just fixed in
`sed` (`A-KSHELL-SED-I-TRUNCATES-A-FILE-IT-CANNOT-DECODE`), reached through a
different door.

**The undecodable path is worse, because it corrupts a diff that does print.**
`from_utf8_lossy` maps *every* invalid byte to the same U+FFFD. So `\xff` and
`\xfe` become the same character, and two lines that differ in nothing else
compare **equal** in the LCS. The result is not a missing hunk but a wrong
one: the differing lines are classified `Edit::Keep` and printed with a leading
space, i.e. reported as *context that both files share*. A caller reading the
hunk is told the opposite of the truth about those lines, and the surrounding
line numbers in the `@@` header are computed from the same wrong classification.

**Why the byte compare does not save it.** It only ever produces the exit
status. Once past it, nothing re-checks; the printed diff comes entirely from
the decoded view. So the status is right and the output is wrong — which is a
worse combination than both being wrong, because the status is what a script
tests and the output is what a human reads.

**Why it survived.** Same reason as sed's: every `diff` test in the suite is
ASCII with LF endings, the one shape in which the decoded view and the byte
view agree exactly.

**What the proper fix looks like.** The same shape as the sed fix, and it can
reuse the helper that fix introduced:

- Split both files with `sed_lines` (or a shared rename of it) — `&[u8]`
  slices, split on `\n` alone, `\r` kept. That alone fixes the CRLF case and
  the U+FFFD collision together, since byte slices compare byte-wise.
- `lines1`/`lines2` become `Vec<&[u8]>`; the LCS table and backtrack are
  unchanged (`==` on `&[u8]` is what is wanted), and hunk printing goes through
  `shell_write_bytes` for the line body with the prefix written separately.
- Delete the trailing-newline guess. With byte lines, `data1 != data2` and zero
  hunks can still happen for exactly one reason — a difference in the *final*
  newline, which `sed_lines` does not represent — so the message becomes true
  rather than a guess, and should be derived (`data1.last() != data2.last()`)
  rather than assumed.
- Consider GNU's `Binary files X and Y differ` for files containing a NUL: a
  correct byte-level line diff of a binary file is correct but unreadable, and
  floods a serial console. GNU's rule is a NUL in the first buffer. This is a
  separate judgement from the correctness fix and should not be smuggled into
  it. ✅ **DONE 2026-08-25**, in its own commit as this note asked. Two
  departures from GNU, both argued at the function: the test is a NUL
  *anywhere* in the file (GNU's first-buffer rule is an artefact of streaming;
  we already hold the whole file, so the answer becomes a property of the files
  rather than of a buffer size), and the check runs *after* the byte-equality
  early return, so two identical binary files stay silent with status 0 —
  they do not differ, and saying they do would be a false positive no flag
  could switch off. `-a`/`--text` restores GNU's line diff exactly.

**`comm` has the identical defect** (~119460): `from_utf8_lossy` on both files,
then `lines()`, then equality on the decoded lines. Two lines differing only in
undecodable bytes are reported in the "common to both" column — a wrong answer
with exit 0. It should be converted in the same pass, since it is the same three
lines of code.

**…and `comm` has a second one that the decode causes rather than shares.**
`comm` is a merge of two streams and is only correct on **sorted** input — it
advances whichever side compares Less and never looks back. `sort` orders by
bytes. `comm` orders by the *decoded* line, and `from_utf8_lossy` does not
preserve that order: a `\xff` byte becomes U+FFFD, which is the three bytes
`EF BF BD`, so it sorts *below* `\xfe` where the raw byte sorted above it. A
file that `sort` produced can therefore look unsorted to `comm`, at which point
the merge silently emits lines in the wrong columns — not because the lines
compare wrongly one at a time, but because the algorithm's precondition has
been broken underneath it.

Converting to byte lines fixes this as a side effect, because `<[u8]>::cmp` is
the ordering `sort` used. Worth noting separately anyway, because it is the
case where the two commands *disagree with each other* rather than either being
wrong alone — and because `comm` never checks its precondition at all. GNU
prints `comm: file 1 is not in sorted order` and exits 1; this one has no such
check, so even after the conversion an unsorted input is still answered
confidently. That check is a small separate change and should be its own.

**`column` shares the decode** (~14868, ~14908) but not the severity: it is a
display formatter writing only to stdout, so a mangled character is visible as
mangled. It is still worth converting for consistency, last.

**Severity.** Wrong output with a correct exit status, on ordinary text files
(the CRLF case) with no unusual input required. Below sed's data destruction,
above the option-wording items.

### Fixed

`6d8a057d7`, with the assertion repair in `fc1670be6`.

| | was | is |
|---|---|---|
| `diff dos.txt unix.txt` | "(files differ only in trailing newline)" | every line shown, `-` carrying the `\r` |
| `diff` on lines differing only in undecodable bytes | printed as shared context, with a leading space | shown as `-`/`+`, bytes intact |
| `diff a b` where only the final newline differs | "(files differ only in trailing newline)" | names which of the two files has it |
| `comm -12` on lines differing only in undecodable bytes | reported common to both | reported as neither's |
| `comm` on a byte-sorted file | merge precondition broken, lines in wrong columns | `<[u8]>::cmp`, the ordering `sort` used |

`sed_lines` was renamed `split_lines` and is now the one splitter for all
three commands, since all three held the same two bugs.

**The fallback is derived, not assumed**, which is the part worth keeping in
mind if this code is touched again. "Same lines, different bytes" has exactly
one cause under a byte-exact splitter, and the argument is written at the call
site: `split_lines` discards nothing except whether the input ended in `\n`, so
the pair (lines, final-newline) reconstructs the file, and therefore equal lines
with unequal bytes force that flag to differ. The other branch is unreachable by
that argument; it is kept rather than made an `unreachable!()` because the
alternative to being wrong must not be panicking a shell, and it deliberately
names **no** cause — if the splitter ever grows a second thing it discards, it
prints instead of lying.

**Two of the pinning assertions had to be repaired before they meant anything**,
which is worth recording because it is the same defect one level up. `comm -12`
forbade `\xfe\n` and `\xff\n` in the common column — but the decode being
removed turned *both* into U+FFFD, so the broken build emitted `\xef\xbf\xbd`
and neither forbidden string could ever appear. The assertion would have passed
against the bug it named. It is now equality on the whole output. `diff`'s
undecodable case had the same hole (` \xff\n`) and now forbids `\xef\xbf\xbd`
anywhere, which states the real invariant: every byte of a diff is copied from
one of the two files, and none is invented by a decoder.

**Still open, deliberately left out of this change:**

- ~~`comm` never checks that its input is sorted.~~ ✅ **DONE, 2026-08-25.**
  It refuses now: three diagnostic lines naming the file, the line number and
  the remedy, exit 1, and — unlike GNU, which warns and carries on — **nothing
  of the merge is printed**, because a prefix of a merge that went wrong is
  indistinguishable from a short but complete one. It also checks less than
  "are both files sorted": only the adjacent pairs up to and including the index
  the merge stopped at, so disorder confined to a tail (where every remaining
  line goes to the same column regardless) is not refused. That boundary is not
  a nicety — the pair *at* the exit index is the one an in-merge check cannot
  see, and it is the one that misfiles a line. Rationale in
  **design-decisions.md §295**; pinned by self-test rung 56, whose third case
  exists to fail if the check is ever widened to whole files.
- ~~`diff` has no `Binary files X and Y differ`.~~ ✅ **FIXED 2026-08-25.** A
  byte-exact line diff of a binary file was *correct* but unreadable and
  flooded a serial console. `diff` now reports `Binary files X and Y differ`
  with status 1 when either file contains a NUL, and gained `-a`/`--text` to
  force the line diff back on. Our test is a NUL anywhere in the file rather
  than GNU's first-buffer rule, and it runs after the byte-equality early
  return so identical binaries stay silent — see the two bullets above.
- `column` still decodes lossily (~14868, ~14908). It is a display formatter
  writing only to stdout, so a mangled character is visible as mangled rather
  than mistaken for data — the lowest severity of the four and the last to do.

---

### Lesson 47: an app that keeps time but never receives the clock (lane C, 2026-08-25)

**In short:** five lane-C programs measured time, and none of them was given
any. A stopwatch that sat at 00:00.00, a metronome that never beat, a typing
tutor whose every speed reading was zero, toasts that never left the screen,
and a speed test that finished in a single frame. All five had a correct,
well-tested function to advance the clock. Nothing in production ever called
it. Every one of them still laid out, still repainted, still answered the
keyboard — they just showed a plausible zero.

A GUI program's clock arrives as one event:

```rust
Event::Tick { elapsed_ms }
```

`oswindow` computes `now - this window's previous tick` and sends that
interval to the window. An app that ages anything has to route that event to
whatever advances its state. If `handle_event` does not name `Event::Tick`,
the event lands in the `_ => {}` arm and the state is frozen for the life of
the process.

**Why this is lesson 45 wearing a disguise the compiler cannot see through.**
Lesson 45 is "a feature with no production caller is a feature that does not
exist," and `dead_code` is the lint that finds it. `dead_code` cannot find
this one, because the function *is* called — by the tests. And a test can
always reach it, because a test passes the timestamp in by hand:

```rust
#[test]
fn test_auto_dismiss_on_timeout() {
    toasts.tick(3001);          // the daemon never does this
    assert!(toasts[0].dismissing);
}
```

That test passed for the entire time notifications were broken. It asserts
something true about `tick`. It asserts nothing whatever about the program.

**The rule that follows: test a wiring through the entry point, not the
target.** Every fix here got a test that goes in through `handle_event` and
was then *falsified* — delete the match arm, confirm that test and only that
test fails, restore, re-run green. That is the only construction that can
distinguish a wired app from an unwired one.

**The five, and what each was actually showing the user.**

| App | Symptom |
|---|---|
| `apps/stopwatch` | 00:00.00, running. The countdown never counted either. |
| `apps/metronome` | No beat, and `T` (tap tempo) was an empty match arm whose comment read "in a real app this would use system time". |
| `apps/typingtutor` | Every WPM figure and every duration read zero — on the live screen, on the results screen, and in the saved history. |
| `gui/notifications` | Toasts never aged, so they never left the screen. |
| `apps/speedtest` | Start ran latency, download and upload inside one call and returned `Complete`. The live graph arrived full; the phase strip's running-highlight and Escape's cancel were unreachable code. |

Four of the five were found by hand in one afternoon. Four for four is not a
coincidence — it is the default outcome of an event enum with a `_ =>` arm.

**The gate: `scripts/check-tick-wiring.py`.** It reports a file when all three
hold: it defines `fn handle_event`; it defines a function taking a named time
parameter (`delta_ms`, `elapsed_ms`, `current_ms`, `delta_secs`, …); and it
never mentions `Event::Tick` **in production code**. `--self-test` runs 13
fixture cases; exit status is 1 on findings, so it can be run as a gate. It
found `apps/speedtest`, which the hand search had missed.

**Two things about the gate that are worth more than the gate.**

- **The three conditions are tight on purpose.** Flagging every file with a
  `_ms` constant would report dozens of non-problems, and a gate that cries
  wolf is a gate that gets commented out. A `format_time(total_ms)` helper is
  not asking to be driven; a parameter called `delta_ms` is.
- **"In production code" is the whole difference between a gate and a
  decoration, and the first draft did not have it.** Comments and
  `#[cfg(test)]` items are blanked before the search, because every file this
  check causes to be fixed acquires a comment explaining the fix and a test
  constructing an `Event::Tick`. If either counted as evidence of wiring, the
  file would be permanently exempt from the check that found it — delete the
  arm again and the test written to catch exactly that would still hold the
  file green. Caught by falsifying the first draft against the live tree:
  removing `apps/stopwatch`'s arm produced no finding.

That second point generalises past this check. **A static gate must be
falsified against the tree it guards, not only against its own fixtures.** A
fixture proves the gate can see; only a live falsification proves it is still
looking at the thing you think it is. This is the same failure lane A logged
in `A-GATES-SILENTLY-STOPPED-CHECKING` — four gates parsing a fraction of the
tree and reporting "clean" — arrived at from the opposite direction: not a
gate that stopped reading the files, but a gate that read them and was talked
out of its finding by the evidence of its own success.

**Running tally (appended 2026-08-27, lane C).** The five above were the ones
the gate and the hand search found in one afternoon. Two more have turned up
since, both while wiring a simulation `main` to a real window, and neither was
visible to `check-tick-wiring.py` — because an app with no `handle_event` at
all fails the gate's first condition and is therefore never reported:

| # | App | Symptom |
|---|---|---|
| 6 | `apps/maze` | The advertised "Timer tracking" never ticked: `elapsed_secs` was set to zero in two places, read only by `format_time`, and incremented nowhere — and `format_time` was itself never called by `render`, so even a working clock would not have reached the screen. |
| 7 | `apps/magnifier` | No `tick_interval`, so the smoothing that `smooth_edges` promised could not have eased anything. The field was settable from the keyboard and changed nothing observable. |
| 8 | `apps/life` | Conway's Game of Life could not advance one generation. `tick_accum`, `speed_ms` and the catch-up loop were all present and all correct; nothing ever handed them a millisecond. |
| 9 | `apps/mixer` | The peak meters had no clock *and no notion of time*: `update_peak_meters()` took no elapsed argument at all, applying attack 0.6 / decay 0.15 / silence 0.85 **per call**. `main` called it ten times in a loop and exited. |

**#9 is the mirror image of #8, and the pair is the point.** In `apps/life` the
timekeeping was perfect and only the wire was missing; in `apps/mixer` the wire
was missing *and* there was nothing at the far end of it to wire to — no
elapsed-time parameter, no accumulator, no interval, so the ballistics were
tied to whatever the frame rate happened to be. Both scored clean against
`check-tick-wiring.py` for the same reason (no `handle_event` at all), and both
survive a reading: nobody looking at `update_peak_meters()` asks what unit its
`0.6` is *per*, because a rate with no denominator does not read as a rate at
all. It reads as a smoothing constant, which is what it would have been if
anything had been calling it at a fixed cadence. The fix is the same shape in
both cases — `tick(&mut self, elapsed_ms: u64)` with a bank, a catch-up cap and
a dropped (not banked) backlog — but only #8's could honestly be described as
connecting up code that already existed.

The addition to the lesson: **an unwired `main` hides an unwired clock.** The
gate looks for an app that receives events and ignores the time one; an app
that receives no events at all is a strictly worse case and scores clean. Any
app still on the window-wiring backlog should be assumed to have this fault
until its clock is checked, and the check belongs in the wiring work rather
than in a separate sweep — which is how all three of these were found.

**#8 is the sub-case worth naming separately: the timekeeping was already
right.** In the first seven, the clock's absence and the timekeeping's quality
were separate questions that happened to have the same answer — nobody had
written the arm, and often nobody had written a correct `tick` either
(`apps/metronome`'s tap tempo was an empty arm whose comment read "in a real
app this would use system time"). `apps/life` is the case where every line of
the time handling was correct, was covered, and would survive a reading, and
the program was still a still photograph. That is worth stating because it is
the version of this fault that gets through a code review: a reviewer looking
at `tick_accum += elapsed_ms; while accum >= interval { step() }` finds
nothing to object to, and the entire fault is one match arm that is not in the
function being read. The only reading that catches it starts at the window and
works inwards — *what does `oswindow` call, and does that path arrive here?* —
which is the same direction the rule above already prescribes for tests
("test a wiring through the entry point, not the target"), and it applies to
reading code exactly as much as to testing it.

---

### Lesson 48: a gate must be measured on the tree it will gate, not the tree it was written on (lane A, 2026-08-25)

**In short:** lane C wrote a pre-build check and measured it at "about a
second". Lane A timed the same script, unmodified, on the tree it was about to
be wired into: **5 minutes 44 seconds**. Nothing was wrong with either
measurement. They were different trees, and the check's cost depended on a
property of the source that differs wildly between them. A gate that slow does
not fail loudly — it gets commented out six months later by someone who never
reads why it was added.

**The mechanism, because it is a trap and not a typo.** The two regexes that
find a Rust `fn` began `^\s*`. Under `re.M` the `^` already anchors to a line
start, so the `\s*` was there only to skip indentation — but `\s` matches a
newline, so at a blank line it runs on through every following blank line and
every following line's indentation, and then hands the whole run back one
character at a time, retrying `pub`/`fn` at each step. That is O(w²) in the
length of a whitespace run.

In ordinary source that costs nothing, because whitespace runs are a few
characters. What made it fire here is a *deliberate* feature of the checker:
it blanks comments and `#[cfg(test)]` items **to spaces** rather than deleting
them, so that reported line numbers still point at the file the reader will
open. A file whose test module is a third of its bulk therefore hands the
regex one whitespace run a quarter of a megabyte long. On
`gui/compositor/src/lib.rs` — 733 KB, and the largest file in lane C's tree —
finding its 243 `fn`s took **93 seconds** by itself.

So the cost scaled with *the size of the largest file's test module*, which is
exactly the kind of property no author thinks to hold constant between trees.
`^[ \t]*` fixes it; the whole gate now runs in 10.9 seconds, most of that
Python startup and reading 372 files.

**What to take from it.**

- **Time a gate where it will run, before wiring it.** Lane C's §6 listed four
  kinds of verification — fixtures, whole-lane run, live falsification, and
  executing the shell block — and every one of them was real work honestly
  done. None of them was a measurement on the tree that would pay the cost.
- **A performance bug in a gate is a correctness bug with a delay.** The build
  still goes green; the gate is simply gone by the time it would have caught
  something. This is the slow-motion form of `A-GATES-SILENTLY-STOPPED-CHECKING`.
- **`^\s*` in a line-oriented regex is almost always a bug** — `^` has already
  done the anchoring, so the only thing `\s`'s newline adds is the ability to
  match across lines, which line-oriented patterns do not want. Here it also
  produced a wrong answer, quietly: the match could *start* on an earlier blank
  line, and the finding is reported at `m.start()`, so a `fn` preceded by a
  blank line pointed the reader at the blank line rather than at the `fn`.
- **When you change another lane's script, prove equivalence on their tree, not
  on the fixtures.** The 13 fixture cases passing says the rewrite did not break
  what the author thought to write down. Comparing old and new `inspect()` on
  all 372 `.rs` files under the gate's roots — full per-file results, not the
  summary line — says it did not break what they did not.

---

### Lesson 49: a filter that names one severity cannot see the other (lane A, 2026-08-25)

**In short:** the kernel carries ~18k pedantic-level clippy warnings as known
debt, so "did my change add any?" cannot be answered by the exit code — a clean
change and a dirty one both leave the count in the eighteen-thousands. The
established method is to `git stash`, re-measure, and compare. The pipeline used
to do that was

```bash
cargo clippy -p kernel --message-format short 2>&1 | grep -E "^kernel" | grep warning
```

and that last `grep warning` is the whole lesson. **`clippy::all` is
deny-level in this workspace, so a violation of it is emitted as `error:`, not
as `warning:`.** The filter that made the comparison tractable was precisely
the filter that made it blind to the only class of diagnostic that actually
stops the build. The comparison came back "identical multiset — zero new
warnings", which was *true* and useless: the change had introduced a deny-level
`clippy::type_complexity` error, and the boot test refused to build 203 seconds
later, before QEMU ever started.

**The second half of the trap.** `scripts/boot-test.sh` runs its gates *before*
booting, so a gate failure leaves `build/serial-test.txt` untouched — still
holding the previous run's output, ending in `kshell::self_test PASSED`. The
standing verification recipe (zero `!! ` lines, self-test passed, no unexpected
faults) therefore reported **green on a run that never booted**. The recipe is
not wrong; it just has no way to notice it is reading a file from an hour ago.

**What to take from it.**

- **Check the gate's own exit code before starting a run that will check it for
  you.** `cargo clippy -p kernel; echo $?` costs one command and answers the
  deny-level question exactly, with no filtering at all. Do that first; use the
  stash-and-compare only for the warning-level delta it is actually for.
- **When comparing diagnostics, filter by *file*, never by severity.** `grep -E
  "^kernel"` alone is the right filter — it keeps errors and warnings both, and
  the severity word is part of the text being compared rather than a
  precondition for being compared.
- **`rm` the artifact you are about to verify.** A verification that reads a
  file the run may not have written is a verification that can pass without the
  run happening. Deleting `build/serial-test.txt` first turns "stale pass" into
  "file not found", which is unmistakable.
- **Always read the tail of the harness log, not only the greps.** Every fact
  needed to catch this was in `/tmp/bt-*.log`: no `BOOT_OK`, no
  `=== Boot test PASSED ===`, and an explicit `ERROR: refusing to build`. Two
  of those were already in the recipe as *positive* checks — their absence is
  what carried the signal, and absence is easy to skim past when the other four
  greps look right.

---

### Lesson 50: an outer timeout equal to the inner one can never let the inner one fire (lane A, 2026-08-25)

**In short:** the standing recipe for a boot test was

```bash
python scripts/run-timeout.py --poll 30 900 ./scripts/boot-test.sh
```

and `scripts/boot-test.sh` runs QEMU under **its own 900 s timeout**. The outer
budget has to cover the pre-build gates and the kernel build as well, so it is
strictly the smaller of the two windows — the outer kill always wins. On
2026-08-25 the gates plus build took 530 s (a full clippy recompile, because
several `git stash` comparisons had invalidated the cache), leaving 370 s for a
boot that reaches `BOOT_OK` at 370–405 s. `run-timeout` killed the tree at 900 s
while the guest was running post-self-test diagnostics, perfectly healthy.

**Why this is worse than one wasted cycle.** The inner timeout is not a
duplicate of the outer one — it is the *diagnostic* one. When `boot-test.sh`'s
own QEMU timeout expires it reports `SYSTEM HANG`, dumps the guest state, and
(since `49496d935` made the HMP monitor on by default) reads back the faulting
RIP. `run-timeout`'s expiry produces exit 124 and a killed process tree: no RIP,
no task table, no serial marker saying where it stopped. So an outer budget at
or below the inner one silently converts every genuine boot hang — the
`B-FORKEXEC-BOOT-HANG` class, which is intermittent and expensive to catch —
from a diagnosed fault into an anonymous kill. The one run where the
instrumentation matters most is the run where it is guaranteed not to speak.

**The rule.** The outer budget must be **inner QEMU timeout + worst-case gates
and build**, not the inner timeout itself. Gates plus a cold-cache build have
been observed at 530 s here, so:

```bash
python scripts/run-timeout.py --poll 30 1500 ./scripts/boot-test.sh
```

1500 s = 900 s inner + 600 s headroom. This does not weaken the hang protection
`run-timeout` exists to provide: `boot-test.sh` bounds the guest itself, and the
outer wrapper's real job is the one only it can do — killing the *whole process
tree*, grandchildren included, if the harness or QEMU orphans something. That
job is unaffected by the budget being generous.

**Generalisation.** Whenever two timeouts nest, the outer one must exceed the
inner one by the cost of everything the outer covers and the inner does not. Two
equal timeouts are not belt-and-braces; they are one timeout, and it is the one
with the less useful failure message.

**Addendum, 2026-08-30: 530 s was not the worst case, and 1500 s is not enough.**
The `deflate` LZ77 repair was boot-tested the same day and the run took
**4292 s**, of which **3711 s was gates and build** and only 581 s was the boot
itself. That is **seven times** the figure this lesson prescribes a budget from.
The 1500 s recipe above would have been killed deep in the *build* phase, having
never started QEMU — the same anonymous exit-124 this lesson exists to prevent,
with an even less informative cause, because there would not even be a serial log
to read.

**What the original measurement missed is not cache warmth — it is graph
position.** The 530 s run had a cold clippy cache but the change was *in* the
kernel crate. `deflate` sits below it: `kernel/src/fs/compress.rs` links it, so
touching it invalidates the kernel crate and every dependent, for clippy **and**
the build, on every target the gate covers. Cold-cache and bottom-of-graph are
different multipliers and they compose.

**The rule, restated.** Size the outer budget from where the change sits in the
dependency graph, not only from whether the cache is warm:

| Change is… | Observed gates+build | Outer budget |
|---|---:|---:|
| inside one leaf crate nothing links | seconds–minutes | 1500 s |
| inside `kernel`, cold clippy cache | 530 s | 1500 s |
| in a crate `kernel` links (`deflate`, `ziparchive`, …) | **3711 s** | **5400 s** |

When in doubt use 5400 s. A generous budget costs nothing — `run-timeout`'s real
job is tearing down the process tree, and that is independent of the number —
whereas a tight one costs the whole run *and* the diagnosis of why it died.

**Second, smaller lesson from the same run: do not pipe a backgrounded
long-runner through `tail`.** It was launched as
`… ./scripts/boot-test.sh 2>&1 | tail -60`, and a pipe into `tail` buffers
everything until the pipeline ends, so the task's output file stayed empty for
the entire 71 minutes. Progress had to be reconstructed from `tasklist` RSS
readings and from `build/serial-test.txt` directly. Redirect to a file and read
the tail of *that* instead; the whole point of backgrounding is being able to
watch it.
