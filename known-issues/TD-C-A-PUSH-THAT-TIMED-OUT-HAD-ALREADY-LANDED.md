## `TD-C-A-PUSH-THAT-TIMED-OUT-HAD-ALREADY-LANDED` (lane C, 2026-09-21)

**Status:** OPEN (procedure, not code)

**In short:** pushing `main` runs a long check battery before the bytes leave
the machine. It takes longer than the ten minutes a foreground command is
allowed, so the command is killed — and the push finishes anyway, on the
server, after the client is gone. Pushing again then fails with a message
that reads like a conflict and means the opposite: it is already done.

**What it looks like.** Twice today `git push origin main` was killed at the
ten-minute ceiling with no output. The log held pages of the pre-push gate's
own selftest narration — `ok refuses a stateful module`, `ok an undeclared
difference is refused` — so the command was working, not hung. Re-running it
in the background produced:

```
 ! [remote rejected]  main -> main (cannot lock ref 'refs/heads/main':
   is at ee8b797a0... but expected e616c737c...)
```

"Expected `e616c737c`" is the value *this* push was written against; "is at
`ee8b797a0`" is the commit it was trying to create. The remote had the push
already. The wording invites the opposite reading — that somebody else moved
the ref — and the check that settles it in one command is
`git fetch origin && git log --oneline -1 origin/main`.

**Why it matters beyond the wasted minutes.** The dangerous response is the
obvious one: see a rejection, assume a race, and reach for a fetch-merge-push
cycle that rebuilds work already on the server. Nothing was lost here, but
the same sequence with a `--force` in it is how a lane loses somebody else's
commits.

**What to do instead, and what this lane now does.**

| | |
|---|---|
| push `main` | background it (`run_in_background`), never foreground |
| a push that "times out" | check `origin/main` before re-running; it may have landed |
| a `cannot lock ref` rejection | read both hashes — `is at` is your own commit if it succeeded |

**Not fixed, because the fix is not mine.** The battery is the pre-push hook
in the shared tree and it earns its runtime: it has caught a control byte in
`known-issues.md` and a line-ending fault in this lane alone. Shortening it
is a decision about what the gate stops checking, which is lane A's hook and
a conversation rather than an edit. What is recorded here is the
*misreading*, which cost a false alarm and is free to avoid.
