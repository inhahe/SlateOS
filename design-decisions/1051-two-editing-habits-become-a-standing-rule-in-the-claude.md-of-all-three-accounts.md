## 1051. Two editing habits become a standing rule, in the CLAUDE.md of all three accounts

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q19: "Add it to claude.md of all three
Claude accounts under c:\users\inhah" -- option A, both habits, which Claude
recommended). Relayed verbatim through lane F's session.

**In short:** six times in one day an edit script matched different text than
intended, or more places than one, and the wrong edit landed silently. Two
habits catch it: assert how many places matched before replacing, and never
write the explanation of a trap and the code it describes in the same pass.
The operator wants both in the user-level `CLAUDE.md` of each account.

**Applied 2026-10-09**, by lane A at the operator's direction in lane A's
session: the rule is its own section, "Edits by search-and-replace", above
the curated one in all three files below (lane A's notice of 2026-10-09;
`operator-answers/LEDGER.md` records it). Lane B had held off because it
edits a `CLAUDE.md` only on the operator's instruction in the session that
makes the edit, and this answer had reached it through another session's
relay. The text, as approved:

> **Edits by search-and-replace.** Before replacing, check that the anchor
> matched exactly as many places as you meant -- usually one -- and stop if
> it did not. And do not write a comment explaining a trap in the same pass
> as the code it describes: write it, run something (a test, a gate, a
> build), then read it back as a reader rather than as its author.

**Where:** `C:\Users\inhah\.claude\CLAUDE.md`,
`C:\Users\inhah\.claude-account-b\CLAUDE.md`,
`C:\Users\inhah\.claude-account-c\CLAUDE.md`.
