# Deferred Questions — not answerable yet

Decisions that will eventually need the operator, but **cannot be answered
usefully today** because the evidence or the prerequisite does not exist yet.

They are here rather than in `open-questions.md` because that file is the
operator's *decision queue* — a list of things they can act on now. A question
that says "do not decide this yet" does not belong in a queue; it pads it, and a
padded queue trains the reader to skim.

**Every entry must carry a `Trigger:` line** — the concrete event that makes it
answerable. When that event happens, whoever notices moves the entry back into
`open-questions.md` (refreshed with whatever the evidence turned out to be) and
deletes it here. An entry without a trigger is either a real open question or
dead; it is never deferred.

This directory is distinct from:

- **`open-questions.md`** — decisions the operator can make **now**.
- **`design-decisions.md`** — decisions already made.
- **`known-issues.md`** — bugs and technical debt.
- **`todo.txt`** — the working scratchpad / judgment-call log.

Same per-lane rules as the other shared documents: append your own entries,
don't rewrite another lane's, and merge `origin/main` before trusting what you
read here.

**Entries are numbered `DQ<n>`.** They were `D-Q<n>` until 2026-09-22, when
the project went to six lanes and `D-` became lane D's open-question prefix
(`open-questions.md` ids are `<lane>-Q<n>`). The two entries that existed then
carry their old names in their headings, so a search for an old citation still
lands here, and lane D's own numbering starts at D-Q3 so that D-Q1 and D-Q2 are
never reissued. See `design-decisions.md` §1100.

---

## One file per deferred question

Each deferred question is its own file, `deferred-questions/<ID>.md` (`DQ4.md`),
whose first line is its heading. Every entry carries a `Trigger:` -- the condition
for promoting it back to `open-questions/`. When it is promoted, answered or
dropped, `git rm` its file here (promotion: create it in `open-questions/`), and say
so in the commit message; the history keeps the text.
