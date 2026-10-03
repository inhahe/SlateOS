### [A] TOOLING-A-JUDGING-A-BACKGROUND-TASK-BY-A-PARTIAL-READ-AND-THEN-DESTROYING-ITS-WORK — 2026-08-15 — ⚠️ HIT, recovered

**Status:** hit and recovered, no data lost. Recorded because the recovery was
luck (the victim was a throwaway scratch worktree) and the same mistake against
a lane worktree would have destroyed another agent's uncommitted work.

**What happened.** `git worktree add --detach <scratch> f79aec561` was started
with `run_in_background: true`. Fifteen seconds later its output file was still
empty and `ls` on the new directory showed only a handful of files, so it was
judged **failed**. A `cargo build` there then failed with "failed to read
`textfmt/Cargo.toml`", which looked like confirmation: a broken tree. The
cleanup that followed — `git worktree remove --force --force`, `git worktree
prune`, `rm -rf` — was run against a checkout that was **still being written**.

It had not failed. `git worktree add` was 60% through checking out 13145 files
and needed ~4 minutes on this tree; the missing `textfmt/` was simply a file it
had not reached yet. The completion notification arrived later, exit code 0 —
for the process that by then had been killed mid-write.

**Consequences, all recoverable here:** a half-deleted directory that `rm -rf`
could not finish ("Device or resource busy"), an orphaned
`git worktree add` and its `git reset --hard` child still holding
`index.lock`, and a `.git/worktrees/` entry that `prune` refused to clear.
Recovery needed killing the two PIDs by ID and re-running the removal.

**The general rule this violates.** A background task has exactly one
authoritative completion signal: the harness notification. Its output file,
its exit status and any side effects it is midway through producing are **not**
readable as progress — and a partially-populated directory is indistinguishable
from a failed one by inspection. This is the same shape as
`TOOLING-A-EDITING-A-SHELL-SCRIPT-WHILE-IT-IS-RUNNING`: acting on an artifact
that another process still owns.

**What makes it dangerous rather than merely annoying.** The forced cleanup
here (`worktree remove --force --force` + `rm -rf`) is precisely the operation
CLAUDE.md forbids against another lane's tree, for exactly this reason — an
agent that has convinced itself a worktree is "broken" will reach for it. The
mitigation is procedural and cheap: **never run a destructive cleanup on a path
a background task is still associated with.** Wait for the notification; if the
task must die first, kill its PID, confirm the PID is gone, and only then
delete.

**Reproduce:** start any long `git worktree add` in the background, read the
directory before it completes, and observe that a partial checkout looks
identical to a failed one.
