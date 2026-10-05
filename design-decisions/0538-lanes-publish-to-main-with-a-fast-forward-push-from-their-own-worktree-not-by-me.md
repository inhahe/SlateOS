## 538. Lanes publish to `main` with a fast-forward push from their own worktree, not by merging inside the shared `os` checkout

**Date:** 2026-08-21 (answered), written up 2026-08-24
**Lane:** C
**Decided by:** Operator (Claude recommended this option) — `open-questions.md` → C-Q3, answered `b`

**In short:** The three agents each work in their own private copy of the source tree, which is what stops them overwriting one another. But the last step of every finished task used to send all three into **one shared copy** — the `os` folder — to publish. Two agents were in there at the same moment on 2026-08-21 and their publish steps tangled. From now on nobody enters that folder to publish: each lane publishes with a single server-side command that needs no folder at all, and if another lane got there first the command is simply refused, so you pull their work in, re-test, and try again.

### The problem

`roadmap.md` step 11 said: merge your lane branch into `main` from the `os` integration worktree, then push `main`. A worktree can only be in one state at a time, so two lanes merging in it simultaneously are editing the same thing — precisely the failure the per-lane worktrees exist to prevent. On 2026-08-21 that happened: git reported *"a git process may have crashed in this repository earlier"* and one lane's merge was discarded. Nothing was lost, because a discarded publish can be re-run, but the failure is silent enough to be worth removing rather than tolerating.

### The decision

**Option B: publish with `git push origin lane-c:main`.**

This works because the rules *already* require each lane to `git fetch origin && git merge origin/main` and re-run the tests **in its own worktree** before publishing. Once that is done there is nothing left to reconcile — the lane branch already contains everything `main` has — so the push is a fast-forward, which the server performs atomically with no working directory involved.

The safety property is the important part: a fast-forward push **cannot half-succeed and cannot interleave**. If another lane published in the meantime, `main` is no longer an ancestor of the lane branch and git *refuses* the push outright. The recovery is the same work the lane was already required to do: fetch, merge, re-test, push again. A refusal is a clean, loud, recoverable state, whereas a tangled merge in a shared checkout is a quiet one.

Note this is not a force-push and must never become one. `--force` here would discard another lane's published work, which is exactly the outcome the whole arrangement is built to prevent. The refusal *is* the feature.

### What it gives up

The ability to resolve a genuine merge conflict *during* publication. That was never wanted: the rules already say resolve-then-test-then-publish, and B makes that ordering mandatory rather than conventional. A conflict now has to be resolved in the lane's own worktree, where the tests that prove the resolution can actually be run — which is where it should have been resolved anyway. Resolving a conflict in `os` and pushing produces a `main` commit whose exact tree no lane ever tested.

`os` becomes a read-only window onto the combined result rather than a place work is done.

### Rejected

- **A — leave `roadmap.md` step 11 as it is.** Collisions stay rare but keep happening; each costs a re-run and looks alarming in the transcript. Rare-but-silent is the worst combination for a failure mode nobody is watching for.
- **C — add a lock around the `os` worktree,** the way QEMU runs are already serialised. Solves the same problem, but by *scheduling* access to a shared resource rather than removing the need for it. It can make an agent wait, and a lock left behind by a crashed agent blocks the other two until someone clears it — a new failure mode traded for the old one. Removing the shared resource strictly dominates queueing for it.
- **D — do nothing and document the race.** Considered implicitly and not offered: a documented race that costs a re-run every time it fires is not a resolution.

### Why this was a question rather than a change

`CLAUDE.md` and `roadmap.md` step 11 are the operator's, and `CLAUDE.md`'s own text forbids editing it except on an explicit instruction. The safe publish was already *permitted* by the existing wording — it just was not *prescribed* — so lane C had been publishing this way since the collision regardless. What needed the operator was making it the rule for all three lanes.
