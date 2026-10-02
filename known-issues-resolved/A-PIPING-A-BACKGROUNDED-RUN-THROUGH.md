### A-PIPING-A-BACKGROUNDED-RUN-THROUGH-`tail`-REPORTS-SOMEBODY-ELSE'S-EXIT-STATUS — 2026-09-02 — **Status: ✅ FIXED 2026-09-02** (lane A; a working-practice fix, no code change)

**In short:** a boot test that *failed* was reported to me as "completed, exit
code 0", and I was one step from closing a bug and merging to `main` on the
strength of it. The cause was not the harness. It was that I had started the
run as `./scripts/boot-test.sh … | tail -80`, and a shell pipeline exits with
the status of its **last** command — `tail`, which succeeds at tailing the
output of a program that just died. The failure was sitting in the text I did
have; the status that would have made me read it said everything was fine.

**What it looked like.**

```
ERROR: refusing to build.  1 tooling test suite(s) failed:
    test-check-design-decisions-bands.py
[run-timeout] child exited: FAIL (exit 1), 3242s elapsed
```

…delivered under a completion notice reading `completed (exit code 0)`.

**Why this is worth an entry rather than a shrug.** The truncation half of this
trap is already recorded immediately above — "capture the run's output to a
file in full rather than reading a truncated background-task tail". That advice
is right and it is not sufficient, because it is about *losing* information. The
sharper defect is that the pipe does not merely hide the verdict, it **replaces
the verdict with a different program's success**. Lost output announces itself
the moment you go looking; a fabricated `0` does not, and it is trusted
precisely at the moment a decision hangs on it — "is the tree green enough to
merge?".

This is the shape this file already names twice from other directions:
`A-GATES-SILENTLY-STOPPED-CHECKING` (a gate parsing a fraction of the tree and
reporting "clean") and the `[ -f ]` guard in `A-KILL-QEMU-PRINTS-A-BARE-NO-SUCH-
FILE` — *a guard that reports a fact it has not checked costs more than the
finding does*. Here the "guard" is an exit status, and the fact it had not
checked was whether the thing I actually ran succeeded. Arriving from a third
direction is the argument for writing it down.

**A third instance, and the mildest: `git push`'s own printed range can
UNDERSTATE what landed.** Observed twice on 2026-09-12. The log said

```
793ae7fcc..c29318365  lane-b -> lane-b
PUSH_EXIT=0
```

and `git ls-remote` immediately afterwards reported `3963ae04d` — one commit
further on, made while the 32 pre-push gates were still running. One
`lane-b -> lane-b` line in the log, so it was one push, not two. The mechanism
is not asserted here because it was not measured; what was measured is that
**the range git prints is not proof of what is on the server.** The direction
is safe — more landed than was reported, never less — but it means a push log
cannot answer "did my last commit go up?". `git ls-remote` can, and is the
instrument that cannot answer from a local cache.

**The rule.** Never pipe a command whose exit status you intend to believe.

**The sharper form, from lane A on 2026-09-12 — piping is dangerous even when
you do not want the status, because the status is the only thing separating
"no matches" from "no input".** They held a merge for forty minutes on this:

```bash
git show origin/main:userspace/procinfo/src/lib.rs | grep -c trim_comm
# 0
```

`procinfo` is a top-level crate; `userspace/procinfo` does not exist. `git
show` failed with `fatal: …does not exist` on **stderr**, the pipe discarded
it, `rc=128` went unread, and `grep -c` faithfully counted zero matches in an
empty stream. The output was a truthful answer to a question that had not been
asked, and it was indistinguishable from the true answer to the intended one.

So the failure is not confined to `$?`. **Two zeros with different meanings
arrive down the same pipe and only one of them is visible.** The check had
been run *specifically to be careful*, which is the recurring part: every
instance of this family is someone verifying something.

**Why that is not a coincidence, and the counter-habit that follows.** A
casual command has nothing to pipe into — you run `git show` and read it. The
pipe appears the moment you start **filtering, counting, extracting**, which
is what verification *is*. So the construct that destroys the distinction
between "no matches" and "no input" is introduced by the act of being
rigorous, and most reliably by whoever is being most rigorous. Every instance
so far was someone building an instrument rather than cutting a corner.

So the rule is not "avoid pipes", which would forbid most checks. It is:

> **When a check reduces something to a number, ask what that number does
> when the input is ABSENT rather than empty.**

`grep -c` cannot tell you. `wc -l` cannot. `| head -1` cannot. All three are
the natural last stage of a careful check, and all three report the same value
for "I looked and found nothing" as for "I never looked at all". Where the
difference matters, check the thing exists first, or read the status — not
because the status is interesting, but because it is the only surviving
witness that the input was real.

For a backgrounded run, redirect instead, and read the status explicitly:

```bash
# WRONG — notification reports tail's status, not the build's
python scripts/run-timeout.py 28800 ./scripts/boot-test.sh 2>&1 | tail -80

# RIGHT — full output kept, status preserved and printed
python scripts/run-timeout.py 28800 ./scripts/boot-test.sh > /tmp/boot.log 2>&1
echo "EXIT=$?"
```

`tee` is the exception worth knowing, since it is the reason the advice above
reaches for it: `cmd | tee f` has the same defect, but `set -o pipefail` or
`${PIPESTATUS[0]}` recovers the real status, and `run-timeout.py` also prints
`child exited: …` in its own output regardless of how the pipeline is wired.
The cheapest habit is still redirect-then-read.

**Cost this time:** 54 minutes of wall clock on a run that refused to build in
its first phase, plus a near-miss on closing
`A-NO-CROSS-BACKEND-METADATA-CONFORMANCE-TEST` against a boot that never
happened — an entry whose own text warns, in as many words, that "every
assertion here is boot-time, so an unrun harness has demonstrated nothing."
The gate that caught the underlying problem (`check-design-decisions-bands`,
on a §675 entry missing its `**Lane:** A` field) worked exactly as designed.
The only thing that failed was my reading of whether it had run.

**Recurrence, 2026-09-12, in the `&&` position rather than the report.** Same
defect, a shape this entry did not name:

```bash
# WRONG -- `&&` reads tail's status, so a FAILED push runs the next command
git push origin lane-b 2>&1 | tail -5 && <merge to main>
```

The push was refused by the tooling-suite gate. `tail` exited 0, the chain
continued, the merge found nothing new and said "Everything up-to-date", and
the whole thing exited 0. I read that as success and reported it as such.
Nothing had landed; two commits sat unpushed while I believed they were on
`main`.

What is worth adding is not the rule -- the rule was already here, correct,
with a worked example -- but that **having written this entry did not stop me
writing the shape.** That now holds for three separate rules in this file: this
one, the `quote-names` gate (three pushes refused for hand-written `'{}'`), and
the crate-level allow this session's gate 33 exists for. The pattern in all
three is that the rule is *known* and the shape is *fluent*, so it arrives
faster than the recollection does. The remedy that has actually worked is not
better recall; it is a mechanical check -- `quote-names` catches its case every
time, and gate 33 will catch its own. **A rule I keep breaking is a rule that
wants a gate, and this one does not have one yet.** The obstacle is that no
checker can see a pipeline typed into a terminal; what it could see is a
pipeline inside a committed script, which is a narrower target and probably
still worth having.
