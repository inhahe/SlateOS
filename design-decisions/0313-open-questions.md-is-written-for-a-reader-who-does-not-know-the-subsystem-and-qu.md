## §313 — `open-questions.md` is written for a reader who does not know the subsystem, and questions that are not yet answerable move to `deferred-questions.md`

**Date:** 2026-08-15
**Decided by:** Operator (operator's own proposal, both halves)

**In short:** the operator said they often cannot understand the entries in
`open-questions.md` — partly terse wording, but mostly terms they do not know.
They also pointed out that one entry (Q39) says in its own text that now is not
the time to decide it, and asked whether it should live somewhere else. Both
observations are about the same failure: the file is supposed to be a list of
things the operator can act on, and it had drifted into being a list of things
Claude found interesting.

### The problem, stated plainly

`open-questions.md` is the **operator's decision queue**. Its only job is to let
the operator make a decision. An entry that is technically flawless but not
understandable has failed at that job completely — it produces no decision, and
worse, it produces *silence*, which is indistinguishable from "not yet read".
Several of today's answers arrived days after the questions were filed for
exactly this reason, and one (`Q44`) came back not as an answer but as a
question about a term used in the question itself ("i forget what 'ambient'
means").

The second failure is padding. Q39's own recommendation section read "None yet,
on purpose… Ask again then." A queue containing items that say *do not act on
this* teaches the reader to skim the queue, which costs attention on the items
that genuinely need it.

### Decision, part 1 — legibility rules for `open-questions.md`

Recorded in `CLAUDE.md` under the `open-questions.md` bullet, and mirrored in
the file's own header:

- **Every entry opens with `In short:` — 2–4 sentences, no jargon at all**:
  what is wrong now, what a user would actually see, what the choice is between.
  If a term of art seems unavoidable there, the paragraph is wrong.
- **Every term of art is glossed in-line on first use, in ≤ 10 words**, even if
  it was glossed in another entry, another file, or last week. The operator
  reads one entry at a time, months apart; nothing carries over.
- **Every option carries a one-line `What changes:`** stated as an observable
  difference ("the clock reads Eastern instead of UTC"), not an implementation,
  so options can be compared without reading the prose.
- **Every entry says what happens if it is never answered** — is today's
  behaviour safe, is anything blocked, does it get worse with time.
- **Entries are capped to what a decision needs.** Detail that only matters
  *after* the answer goes in `known-issues.md` or the `requests/` file. Prefer a
  table to a paragraph, an example to an abstraction.
- The same `In short:` opener applies to `design-decisions.md` entries, so a
  decision can be re-read a year later without reconstructing the context.

**The tension this creates, and how it resolves.** The operator also said they
do not want *a lot more reading*. Glossing terms and adding a summary both add
words, so the rules above are not purely additive — the length cap is part of
the decision, not a footnote to it. The `In short:` paragraph is meant to
*replace* the rambling first section of an entry, and the "cap it to what a
decision needs" rule is what pays for the glosses. An entry that gets longer
overall has applied the rule wrong.

**Alternative rejected: a project glossary.** A central `glossary.md` looks like
the tidy answer — define "ambient authority" once, link it everywhere. It was
rejected because it optimises for the writer, not the reader: it turns one
unfamiliar word into a click, a context switch, and a second document, at the
exact moment the reader is trying to hold a tradeoff in their head. A ten-word
parenthetical costs the writer a repetition and costs the reader nothing. The
repetition is the feature.

### Decision, part 2 — `deferred-questions.md`

A new shared document at the repo root for questions that will need the operator
**eventually** but cannot be answered usefully **today**, because the evidence or
the prerequisite does not exist.

- **Every entry carries a `Trigger:` line** — the concrete event that makes it
  answerable. Without one it is either a real open question or dead; it is never
  deferred. This is the whole mechanism: a deferred question with no trigger is
  just a question that has been hidden.
- Entries are numbered `D-Q<n>` and are append-only, same as the other shared
  docs (`roadmap.md` rule 3, now updated).
- When the trigger fires, the entry moves *back* into `open-questions.md`,
  refreshed with whatever the evidence turned out to be, and is deleted here.

**Moved on creation:** Q39 (which way the shipping default points once a fastpy
utility clears both the parity bar and the performance bar) became **D-Q1**. Its
trigger is the first fastpy utility clearing both bars; nothing does today, so
answering it now would be answering without evidence.

**The risk accepted.** A deferred question is easier to forget than an open one —
that is the point, and it is also the hazard. `deferred-questions.md` has no
process that surfaces it on a schedule; it relies on whoever fires the trigger
noticing the entry. The mitigation is that triggers are written as events in the
work (*"the first fastpy utility clears both bars"*), so the person who causes
the event is the person reading the file's subject matter at that moment. If
that proves optimistic, the escalation is a check in the task-completion
checklist, not a bigger file.
