## B-ED-TESTS-READ-THE-REAL-STDIN-AND-HUNG-THE-WHOLE-SUITE (lane B, 2026-09-12) — FIXED (properly, later the same day)

**In short:** `cargo test -p coreutils` stopped after 20 of its 93 test groups
and sat there until killed, twice in one afternoon. The cause was one line in
one `ed` test: it ran the editor's `a` (append) command, which reads its text
from **standard input** — the *test runner's* standard input. When that is a
pipe with nothing on it, the read never returns, and because `Editor::new`
holds `stdin().lock()` the whole binary's other tests queue behind it forever.

**Measured on the built test binary, with no `cargo` involved:**

```text
stdin = /dev/null       73 passed in 0.01s
stdin = pipe, no data   hangs until killed
```

**I diagnosed it wrong twice first, and the way it misled is the part worth
keeping.** The symptom is

```text
test tests::copy_duplicates_a_range_and_may_copy_into_itself has been running
for over 60 seconds
```

printed for *four* tests at once — which is exactly what CPU starvation looks
like. The first occurrence had the machine at 88% with another lane's boot
test running, so I wrote it off as contention, recorded "a starved test and a
hung one are spelled the same way in that output", and moved on. That sentence
was true and it is what stopped me looking. The third occurrence had the CPU at
34% and no QEMU, which is the only reason it got examined.

**The file already knew.** The comment on `ed`'s option table says `a` and `i`
are excluded from that list "because their text comes from stdin". The
knowledge was thirty lines from the loop that used `0a` anyway.

**The fix** was to drop `0a` from
`a_mark_follows_its_line_wherever_the_line_goes`. Its other four shuffles —
`1d`, `1m$`, `1t0`, `1,2j` — are four independent ways of moving text above the
marked line, which is the property under test, so no coverage was lost.
Verified both directions on the same rebuilt binary: hangs before, 73 tests in
0.01s after, identical stdin. `cargo test -p coreutils` now completes at 93
groups, 0 failures.

**The proper fix landed the same day, and it found a bug.** `Editor` now
carries `input: Box<dyn BufRead>`; `new` still measures `file_driven` off the
real stdin and passes `stdin().lock()`, while `with_input(opts, input,
file_driven)` takes both as parameters. Every editor in the tests comes from
`editor_reading`/`editor_driven_by`, so the suite no longer touches the
process's stdin, and `0a` is back in the shuffle list carrying its own input.
Proof the hang is gone rather than avoided: the ed test binary run with an
**open pipe** on stdin — the exact condition that hung it — exits 0 in 0s.

The bug it exposed: `c` with no text — a `c` followed at once by `.` — left
the current line at `lo - 1`, where GNU leaves it at `lo` clamped to the new
length. Measured against GNU ed 1.20.1 on one/two/three, `2c` then `.` gives
current 2 there and gave 1 here. Empty-text `c` is exactly `d`, and `d` in
this same file already had the right formula six hundred lines up. **No test
could reach it**, which is the whole argument for the refactor stated as a
measurement instead of a principle: untestable code is not merely unverified,
it is where the defect actually was.

**Blast radius, checked rather than assumed.** `scripts/hooks/pre-push` runs
`cargo test -p posix --lib`, not coreutils, so a push was never exposed.

**A correction to this entry.** It used to end "a sweep of every built test
binary for the same defect found no second instance." **No finished sweep had
said that when it was written** — the first attempt was killed for a false
positive and the second was stopped for cost, so the sentence recorded an
expectation in the past tense. It has now actually been run, as
`scripts/stdin-hang-sweep.sh`: **84 test binaries across coreutils' bins and
`posix`, 0 stdin hangs, 0 too slow to judge.** `ed` was the only instance.

**Three ways the sweep was wrong before it was right**, all worth keeping
because each is a different species of the same mistake — a check that ran
correctly and answered a narrower question than the one asked:

| Version | What it did | Why it was wrong |
|---|---|---|
| 1 | one 6-second timeout | flagged `apps/automator`, which runs 160 tests in 12.8s. **Slow is not stuck.** A stdin hang is a *difference between two conditions*; a checker that measures one cannot see it. |
| 2 | `( sleep 40 \| timeout 35 "$f" )` over all 1677 binaries | the subshell waits for `sleep` too, so every binary cost a flat 40s whether or not it exited instantly — 8+ hours for a 4-minute question. `< <(sleep 40)` holds the pipe open without being waited on: 0s for a non-reader, a refusal at the deadline for a reader. |
| 3 | newest `name-*.exe` per name | `deps/` holds **both** the test harness and the plain binary under that pattern, so "newest wins" picked the non-harness about half the time — including for `ed`, the binary the sweep exists for, which it duly reported as having *no tests*. Walking candidates newest-first and taking the first that lists some took coverage from 48 names to 84. |

The last of those is the sharpest: the detector had just been shown catching a
planted reader, and then silently skipped its own motivating case. **A green
self-test says the mechanism works, not that it was pointed at anything.** The
sweep now ships that self-test (`--selftest`, which plants one binary that
reads stdin and one that does not, and requires exactly one to be caught) and
still needed a separate check that its *input set* was the intended one.
