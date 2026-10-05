# Open Questions — Operator Decision Queue

Decisions that genuinely need the human operator: architectural forks,
user-visible policies, and tradeoffs with no obviously-correct answer that
Claude has **deferred** rather than resolved autonomously.

This directory is distinct from:

- **`design-decisions.md`** — decisions already *made* (each marked with who
  decided it). When the operator answers a question here, move it there as a
  `Decided by: Operator` entry and delete it from this file.
- **`known-issues.md`** — bugs and accumulated technical debt.
- **`todo.txt`** — the working scratchpad / judgment-call log.
- **`deferred-questions.md`** — questions that will need the operator *eventually*
  but cannot be answered usefully yet, each with a trigger for promoting it back
  here. Anything whose own text says "ask again later" belongs there, not here:
  this file is a queue, and a padded queue gets skimmed.

### How an answer actually arrives — read this before assuming nobody replied

The operator has answered this queue by writing a plain text file,
**`open-questions-answers.txt`, in the integration tree** (`E:/visual studio projects/os`), one paragraph per question keyed by its ID. As of 2026-09-12 that
file is dated 2026-09-07, holds about two dozen answers spanning all three lanes,
and every one of them has been processed. **The channel works. What does not work
is noticing it.**

- It is **untracked** — not ignored, just never added — so it exists in exactly one
  directory on one machine. It is on no branch, in no lane's worktree, and in no
  clone. Fetching and merging `origin/main`, which is what the start-of-task
  checklist tells you to do, cannot show it to you.
- **Nothing watches it.** No gate, no hook, no script mentions the filename.
- It was found on 2026-09-12 **by accident**, in `git status` output during an
  unrelated merge, five days after it was written.

So: **check it at the start of a task**, alongside the merge. Reading the
integration tree is fine — the rule against touching `os` is about *writing*.

```bash
cat "E:/visual studio projects/os/open-questions-answers.txt"
```

A question sitting at `Status: OPEN` here is **not** evidence that the operator has
not answered it. An entry can be open precisely because the operator *did* reply
and asked for a clearer explanation — which is a reply, and which is invisible
from this file alone.

*Both examples this note originally cited have since closed, which is worth
saying rather than quietly editing: C-Q9 was written up as §841 on 2026-09-13,
and lane B withdrew B-Q8's option (c) as overtaken on 2026-09-14. The point
stands and the examples did not — so if you are checking the claim against them,
check the dates first. Examples naming live entries go stale by being right;
this note now names its examples as history instead. — lane C, 2026-09-14.*

*Recorded by lane A. This describes what has been observed, not a policy the
operator has set; if a different channel is preferred, say so and this goes away.*

Format for each entry — **written for a reader who does not know the
subsystem**, because an entry the operator cannot decide from has failed no
matter how correct it is:

- **`In short:`** — 2–4 sentences, **no jargon**, opening every entry: what is
  wrong now, what a user would actually see, and what the choice is between. If
  a term of art seems unavoidable here, the paragraph is wrong — rewrite it.
- **Question** — the decision to be made, with every term of art glossed in-line
  on first use in ≤ 10 words, even if it is glossed in another entry. Assume
  nothing carries over: the operator reads one entry at a time, months apart.
- **Options** — each with pros, cons, and a one-line **`What changes:`** stated
  as an observable difference ("the clock reads Eastern instead of UTC"), not an
  implementation, so the options can be compared without reading the prose.
- **If never answered** — one line: is today's behaviour safe, is anything
  blocked, does it get worse with time.
- **Claude's recommendation** — if there is a defensible default (and what
  Claude is doing in the meantime).
- **Where it bites** — files/symbols affected, so the resolution can be applied.
- **Status** — `OPEN` until the operator decides.

Keep entries to what a *decision* needs. Detail that only matters after the
answer belongs in `known-issues.md` or the `requests/` file. Prefer a short
table to a paragraph and a concrete example to an abstraction. (The rule is in
`CLAUDE.md` → "Write `open-questions.md` for a reader who does not know the
subsystem".)

**The body of this file holds OPEN questions only.** When the operator answers
one: write it up in `design-decisions.md` as a `Decided by: Operator` entry,
**delete the entry from here**, and add one line to the `

## One file per question

Each open question is its own file, `open-questions/<ID>.md` (`A-Q14.md`), whose
first line is its heading: `## A-Q14 — [A] <the question> — Status: OPEN (raised
<date>)`. **This directory holds open questions only**: `ls open-questions/` is the
queue. Take your lane's next number (`python scripts/check-docs.py --next-question
<lane>`).

**When the operator answers one**, record the decision in `design-decisions/` as a
`Decided by: Operator` entry, `git rm` the question's file, and add a one-line
record to `open-questions-resolved/lane-<x>.md` (your own lane's file, so lanes
never edit the same lines). `scripts/check-docs.py` refuses a file here whose
heading is not an OPEN question with the id its name claims.
