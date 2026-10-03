## TD-A-THE-OPERATORS-ANSWER-CHANNEL-IS-WATCHED-BY-NOTHING (lane A, 2026-09-12) — FIXED

**In short:** the operator answers our question queue by writing a text file. That
file lives in one directory, on one machine, is not in version control, and no part
of our tooling looks at it. It has worked so far only because someone happened to
glance at the right output. The answers are not being lost; they are being found by
luck.

**What exists.** `open-questions-answers.txt` in the integration tree, dated
2026-09-07, 7,344 bytes, about two dozen answers keyed by question ID and spanning
all three lanes. Every one has been processed into `design-decisions.md` or the
archive, so the channel is real and has been used comprehensively, once.

**Why it is fragile, precisely:**

| property | consequence |
|---|---|
| untracked (not ignored — never `git add`ed) | on no branch; `fetch && merge` cannot reveal it |
| lives only in `E:/visual studio projects/os` | absent from all three lane worktrees |
| no script, gate or hook names the file | nothing reports that it changed |
| lanes are told never to write to `os` | which is easily over-read as never *look* at it |

**How it was found:** by accident, in `git status` output during an unrelated merge,
**five days** after it was written. Nothing about that latency is bounded — it could
as easily have been fifty days, and two lanes currently have questions in the queue
waiting on it.

**The proper fix** is four lines in a gate that already runs on every push: stat the
file, compare its mtime against a recorded high-water mark, and print a line when it
is newer. Not a refusal — a notice. `check-open-questions` is the natural home; it
already parses the queue and already runs.

**Why it is not fixed here.** That script is in `scripts/`, which `which-lane.py`
assigns to no lane — lane A owns only `boot-test.sh`, `run-timeout.py` and
`wedge-soak.sh` there. That is the ownership gap **A-Q11** already asks about, and
the boot test is the wrong cadence for this anyway: a twenty-four-minute job is not
where you learn that a five-day-old message is waiting.

**FIXED 2026-09-12, same day, and the entry's own reasoning was wrong twice.**
`check-open-questions.py` now hashes the file, keeps a **per-branch** high-water
mark under `.git/coordination/`, and names the answered IDs still open on that
branch. It runs in the pre-push hook as well as the boot test, so the cadence is
minutes. It never refuses: a missing file, an unreadable mark or a read-only
coordination directory all mean silence.

On its first real run it found **B-Q8 and C-Q9 answered five days earlier and
still open** -- and I had hand-checked this same channel hours before, confirmed
A-Q1..A-Q7 were processed, and stopped at lane A's questions. The luck this entry
describes had covered my own lane and missed the other two. Both notified.

Wrong twice, and worth recording:

- *Blocked on A-Q11.* It was not. Lane A had already written
  `scripts/check-variant-lists.py`, and shipped `scripts/check-ran-if.py` to main
  the same day. The ownership question is still open; the blockage was not real.
- *Per-lane was not in the original plan.* A single shared mark would let the
  first lane to push swallow the notice for the other two -- this entry's exact
  failure, reproduced inside its own fix.

**The circularity is worth stating outright:** the fix is blocked on A-Q11, A-Q11 is
waiting for an operator answer, and the answer would arrive through the channel this
entry is about. Interim mitigation is documentation only — `open-questions.md` now
carries a section telling every lane to read the file at task start.
