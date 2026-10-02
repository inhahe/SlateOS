## §306 — The shared documents stay per-branch; a fetch/merge cadence is what keeps them current

**Date:** 2026-08-14
**Decided by:** Claude (operator-approved scope — the operator was offered
three options and delegated the choice: "do whichever one you think is best")

**The question.** `roadmap.md`, `known-issues.md`, `design-decisions.md`,
`open-questions.md`, `todo.txt` and `requests/` are ordinary tracked files, so
each lane branch carries **its own copy** of every one of them. Should they
instead live in one place — on `main` only — so that all three lanes read the
same bytes?

**What prompted it.** Two failures on the same day, both mine:

1. `requests/a-b-init-conflates-syscall-error-with-exit-code.md` — Lane A's
   report that `services/init/src/main.rs` treats `process_try_wait`'s negative
   kernel error as a child exit code — sat on `origin/main` **unread for a
   day**. It was never in my worktree, because `lane-b` had never once fetched
   or merged since the split (55 ahead, 72 behind).
2. I then diagnosed the repo's integration state by reading the shared docs in
   `D:\visual studio projects\os`, and concluded that no lane had ever merged
   to `main`. That was **wrong**: the `os` directory is a checkout of `main`
   sitting **67 commits behind `origin/main`**, and Lane A had in fact merged
   (`6d69d308e`). I recommended an architecture change to the operator on the
   strength of it, then had to retract the reasoning before it was acted on.
3. **A third surfaced while writing this entry, and it is the cleanest
   illustration of the three.** The first boot test after the merge failed on
   a missing `limine/BOOTX64.EFI`, and before that on six missing service ELFs
   that `kernel/src/main.rs` `include_bytes!`es. Every one of them is
   provisioned by `scripts/bootstrap-worktree.sh` — a script that exists *for
   exactly this purpose*, that landed on `main` on 2026-08-13 (`0d013beb1`,
   `60dab49d5`), and that `lane-b` had never seen. I was one `git merge` away
   from a provisioned worktree for a day, and instead diagnosed it as a
   sequence of unrelated build failures.

**The decision.** Keep the per-branch copies. Add an explicit **sync cadence**
instead, recorded in `roadmap.md` §5.5 and mirrored into `CLAUDE.md`:

- `git fetch origin && git merge origin/main` at the **start** of every task;
- push the lane and **merge the lane up into `main`** at the **end** of every
  green one;
- `origin/main` is the trunk — the `os` directory is a *view* of it that may
  be arbitrarily stale, and must be pulled before it is read as authoritative;
- **merge, never rebase**, when integrating `origin/main` into a lane (see
  below).

**The alternative, and why not.** Option B was to move the shared docs to
`main` only. It buys freshness by construction and would have prevented failure
(1) outright. It was rejected because:

- **It trades away worktree isolation — the one property the three-lane
  arrangement exists to provide.** Editing a doc that lives only on `main`
  means three agents writing the same file in the same checkout, which is
  precisely the clobbering failure mode the lanes were drawn to prevent. It
  would reintroduce it for exactly the files that are hardest to reconstruct
  from memory.
- **The per-lane conventions demonstrably work.** The merge that closed the
  72-commit gap produced **zero** conflicts in `design-decisions.md` — the
  §200/§300/§400 numbering split doing its job across two lanes appending
  simultaneously — and five elsewhere, every one of them the trivial "both
  lanes appended at the same spot" shape. Total resolution: minutes.
- **The failure was not structural.** The dropbox was not broken; nobody
  emptied it. A problem that regular fetching solves does not justify
  surrendering a safety property.

A narrower slice of B — make `requests/` alone main-only — was also considered
and dropped: a lane cannot write to the `os` worktree (the rule forbidding it
is not negotiable), so filing a request would still require getting a commit
onto `main`, which is the same merge-up step the cadence already mandates. It
would add a mechanism without removing one.

**A correction that came out of this.** `roadmap.md` rule 5 said "**Rebase on
`main`, never merge**". That is unsafe now: the lane branches are *published*
at `origin/lane-<x>`, so rebasing one requires a force-push — which the very
next bullet of the same rule forbids outright, because it destroys the other
lanes' work. The rule contradicted itself, and following its first half is what
would have stranded `lane-b`. It now reads: merge `origin/main` in; rebase
remains fine only for commits never pushed.

**Where it bites:** `roadmap.md` §5 and the new §5.5, `CLAUDE.md` ("Three
Sessions", "Branch Strategy", "When You Start a Task" step 1, "When You Finish
a Task" step 11, and the push bullet under Autonomous Work), and
`requests/b-a-fetch-and-merge-main-every-task.md` /
`requests/b-c-fetch-and-merge-main-every-task.md`, which carry the rule to the
other two lanes — since the only correct way to change their copy of a shared
document is to land it on `main` and let them merge it down.
