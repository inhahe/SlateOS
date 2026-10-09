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

The operator answers this queue by writing a plain text file in the
integration tree (`E:/visual studio projects/os`):
**`open-questions-answers*.txt` at its root, with any suffix** --
`open-questions-answers.txt` (2026-09-07), then `open-questions-answers.2.txt`
(2026-09-27) -- **or `answers*.txt` in this directory**
(`open-questions/answers.txt`, 2026-10-09), one paragraph per question, keyed
by its id or by its title. Or in chat, in one lane's session, which then holds
every lane's answers.

- The file is **untracked**: on no branch, in no lane's worktree. No merge of
  `origin/main` will show it to you.
- **`python scripts/check-lane-signals.py` watches for it.** Every lane runs it at
  the start of a task and on every wakeup. It reports any answers file -- or a
  new version of one, when the operator adds answers to it -- whose content
  hash is not yet in `operator-answers/LEDGER.md`, with the ids in it by lane.
- **What to do then** is in `CLAUDE.md`, "When the operator answers": copy the
  file verbatim into `operator-answers/`, record your own lane's answers, send
  every other lane with answers in it a notice naming its ids, and add the
  file's section to the ledger -- the same day.
- **The operator reads this directory in the integration tree**, so it is only as
  current as that tree. The same script keeps it fast-forwarded to `main`.
  Before it did, the operator answered 36 questions on 2026-09-27, every lane
  recorded its answers that day, and two weeks later the operator still saw
  every one of them listed as open -- in a tree 3,389 commits behind `main`
  (`operator-answers/README.md`).

A question sitting at `Status: OPEN` here is **not** evidence that the operator has
not answered it. An entry can be open precisely because the operator *did* reply
and asked for a clearer explanation -- which is a reply, and which is invisible
from this directory alone: F-Q1's HEIC half, for one, was rewritten on
2026-09-27 around the question the operator asked in answering it.

*History: the first answers file was found on 2026-09-12 by accident, in `git
status` output during an unrelated merge, five days after it was written. This
section then told lanes to `cat` it by name -- an instruction the second file's
`.2` suffix would have walked straight past. The two examples it once cited have
closed (C-Q9 became §841 on 2026-09-13; lane B withdrew B-Q8's option (c) on
2026-09-14). -- lanes A and C. The policy above is the operator's, asked for on
2026-10-09.*

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
