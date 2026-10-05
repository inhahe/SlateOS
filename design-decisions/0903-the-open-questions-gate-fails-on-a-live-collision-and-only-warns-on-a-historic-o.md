## §903 — the open-questions gate fails on a live collision and only warns on a historic one, and never fails on another lane's heading text

**Date:** 2026-09-03. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** `open-questions.md` is the list of decisions waiting on the
operator. It had eight questions filed *below* the `# Resolved` heading — the
archive — even though the sentence directly under that heading says the
questions live above it; two different questions both called `Q57`; and one
question filed twice on the same day by the same lane with opposite
recommendations. All three were committed by lanes that had read the header
stating the rules, so a fourth sentence of prose was not going to hold. The
file now has a checker. This entry records the two places where the checker
deliberately does *not* fail: a duplicate number where both copies are already
answered, and an entry with no number at all.

### Why a checker rather than a clearer header

The header already says what the rules are. Every violation above was
introduced by an agent that had the header in its context at the time. That is
the argument for mechanising it and the only argument needed — a rule stated
once more, in slightly firmer language, has an observed 0-for-3 record here.

The checker (`scripts/check-open-questions.py`) hard-fails on three things: an
open `## ` entry below `# Resolved`; an identifier used twice where at least
one use is a live question; and a body entry whose own `Status:` is not
`OPEN`. It carries discovery floors — a document that yields fewer than five
body entries or fewer than ten index entries exits **2**, not 0 — because a
gate that parses nothing reports no failures, which on a terminal is
indistinguishable from a pass. Every hard rule has a fixture in `--self-test`
that makes it fire, and a clean fixture that proves the checker is not one that
simply fails on everything.

### Decision 1: a duplicate number fails only if one of the copies is still open

`Q38` and `Q45` were each issued to two different questions in the
single-agent, append-only era. Both pairs are long answered, and the surviving
`Q45` entry says so in its own text. The rule could have been "a number is
unique, full stop", which would have meant renumbering four archived entries.

| | Fail on any duplicate | Fail only when a live entry is involved |
|---|---|---|
| Archive | four entries renumbered to satisfy a script | left as filed, with a note saying why |
| What breaks | an operator reply citing `Q45` no longer matches the file that was answered under that name | nothing — nothing archived can be mis-answered |
| Cost | a permanently clean report | a permanent two-line warning in every run |

The deciding argument is that the harm being prevented is *ambiguity in an
answer*: the operator's replies take the form "do B on Q47", and a number that
names two open questions makes that sentence unactionable. An archived pair
cannot be answered at all, so there is no ambiguity left to prevent — only a
record to preserve. As the checker's own comment puts it: *editing an archive
to satisfy a checker would be falsifying the record of what the numbers meant
when they were answered, and nothing can be mis-answered now, so it is
reported and not enforced.*

The counter-case is real: a warning that is expected to appear forever is a
warning readers learn to skip, and it costs the run its "0 warnings" line. The
mitigation is that both historic collisions now carry a parenthetical in the
archive text itself explaining that the number is deliberately left as it was,
so a reader who chases the warning finds an answer rather than a puzzle.

### Decision 2: a missing identifier is a counted warning, never a failure

Two entries in the file are lane C's and have no number. The checker could
require one.

| | Fail on a missing identifier | Count it as a warning |
|---|---|---|
| *What changes:* | lane A's gate can turn lane C's boot test red over a heading | the number stays at zero only if lane C acts; nothing is blocked |
| Fix path | lane A edits lane C's heading, or lane C is blocked | lane A files a request; lane C edits its own heading |

`roadmap.md` rule 3 exists to stop exactly this: a hard failure on another
lane's heading text is cross-lane breakage, and the boot test is shared, so the
failure would land on whichever lane ran it next rather than on whoever wrote
the heading. Renaming the headings myself is the other tempting option and is
worse — it is a merge conflict in lane C's tree and a citation in my own reply
that no longer matches what lane C wrote. So the gate reports and
`requests/a-c-two-of-your-open-questions-have-no-number-so-the-operator-cannot-cite-them.md`
asks. If lane C numbers them `C-Q10`/`C-Q11`, the count goes to zero on its
own.

### What reversing this looks like

Both splits are one condition each. Decision 1 is the `if any("(body)" in w
...)` branch in the collision loop — delete the branch and every duplicate
fails; the cost is renumbering four archived entries and losing the match
between an old operator reply and the entry it answered. Decision 2 is the
`f.warnings.append` for a heading with no identifier — move it to
`f.failures` and the rule becomes enforced. The signal that Decision 2 is
worth revisiting is the warning count staying above zero after lane C has had
the request for a while, which would mean the polite route does not work and
the choice is between an enforced rule and no rule.
