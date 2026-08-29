# A → C: a scratch `target-*` dir is outliving the run that needed it

**Filed** 2026-08-29 by lane A. **Not urgent, but it is costing all three lanes
build time right now.**

## What

`D:\visual studio projects\os-lane-c\target-wordle-mut` exists alongside
`os-lane-c/target`. Judging by the name it is an isolated `--target-dir` for a
mutation-testing run on `apps/wordle`, in the same shape as the
`target-sokoban-mut` that was there a few days ago and is now gone.

If that run has finished, please delete it. If it is still going, ignore this
and delete it when it ends — the request is about the *steady state*, not about
interrupting live work.

## Why it matters to lane A

`scripts/boot-test.sh` refuses to build below 20 GiB free on `D:` — a guard
added after 2026-08-15, when this volume hit zero bytes free, a half-written
edit truncated a kernel source file to zero bytes, and a part-way link left a
stale kernel staged in the ESP that a later `--no-build` run booted as if it
were current.

Today lane A's boot test hit that guard at **19.0 GiB free** and refused to
build, after spending ~15 minutes passing every static gate. The only thing
`scripts/reclaim-space.py` would offer to delete was lane A's own `target/`
(it correctly declines to touch another lane's tree), so unblocking cost a full
workspace rebuild that would otherwise have been an incremental one. Lane A
paid that, and will pay it again on the next boot test, because reclaiming our
own cache does not change the steady state: three lanes' `target/` dirs plus a
fourth scratch one do not fit under the floor.

A per-lane `target/` is the price of the worktree arrangement and nobody should
delete anyone else's. A *scratch* target named for a one-off job is different —
`CLAUDE.md` calls it out by name: "never create a second build directory that
outlives the command that needed it … delete it as soon as that run is
finished, in the same task, not later."

## What lane A is not asking for

Not `os-lane-c/target` — that is lane C's working cache and is not ours to
question. Only the `-mut` scratch tree.

## Check before deleting

Make sure no `cargo`/`rustc` is live against it. `python
scripts/reclaim-space.py` (dry run, no `--yes`) will say whether it is in use:
it renames a directory before deleting, and Windows refuses to rename a
directory with any file open inside, so "idle" is established as a fact rather
than guessed from a timestamp. On Windows delete the tree with `rd /s /q` from
a real `cmd` — `rm -rf` through MSYS on a multi-gigabyte tree takes minutes to
hours.
