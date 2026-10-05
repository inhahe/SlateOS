## 1221. The file manager asks what to do with a taken name, unless told once what to do with all of them

**Date:** 2026-09-27
**Lane:** E
**Decided by:** Claude (autonomous) -- within C-Q26's answer (option A,
§1418), which put the choice in `explorer.yaml`

**In short:** when you paste or drop a file into a folder that already has
one with that name, the file manager now stops and asks: keep both, replace,
skip, or stop -- with "do the same for every other taken name". Before, it
always kept both, numbering the new one. The folder menu's "When the name is
taken" still lets you choose one answer for good (keep both, skip, replace if
newer, replace), and that choice is kept.

**Why asking is the default.** It is the one choice that decides nothing on
the user's behalf: keep both leaves numbered copies to sort out later, which
is rarely what somebody pasting a newer version wanted; replace and skip each
lose one of the two files. Every mainstream file manager asks, so it is also
the behaviour a user arrives expecting. Asking loses nothing -- the operation
waits, and "Stop" leaves everything done so far done.

**Alternatives:**
- *Keep both by default* (what it did) -- safe, and quiet about the fact
  that the user now has two files where they meant to have one.
- *Replace by default* -- what a user pasting a newer version usually
  wants, and the one default that destroys data when they did not.

**How it is built.** The copy engine's `ConflictPolicy::Ask` stops at the
file and waits (`OperationExecutor::waiting_on`/`answer`) instead of skipping
it, as it did; the window draws its own prompt, since `guitk`'s alert offers
only OK/Cancel/Yes/No.
