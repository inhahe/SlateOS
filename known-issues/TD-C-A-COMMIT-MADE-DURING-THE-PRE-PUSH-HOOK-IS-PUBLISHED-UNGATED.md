## `TD-C-A-COMMIT-MADE-DURING-THE-PRE-PUSH-HOOK-IS-PUBLISHED-UNGATED` (lane C, 2026-09-17)

**In short:** the pre-push gates take several minutes to run. If you commit
anything while they are running, that newer commit is what reaches the server
-- but the gates were handed the older one and never looked at it. "The gates
passed" then describes a commit that is not the commit now on the remote. With
an agent that commits every few minutes and a gate run of four to ten, the
window is wide open rather than theoretical.

**Observed, on `lane-c`, with timestamps.**

| Time | Event |
|---|---|
| 16:49:00 | commit `2f5f98d0e` |
| ~16:49 | `git push origin lane-c` starts; the hook is handed `2f5f98d0e` on stdin |
| 16:50:13 | commit `8ec3d9dc0` made while the hook is still running |
| 16:52:26 | the push reports `9a4f42229..2f5f98d0e`, and the reflog records `update by push` to `2f5f98d0e` |
| 16:52:31 | a `fetch` five seconds later fast-forwards `origin/lane-c` to **`8ec3d9dc0`** |

`git ls-remote` confirms the server holds `8ec3d9dc0`. Nothing else pushed it:
the only hook installed is `pre-push` (no post-commit), no scheduled job
pushes, and no other `git push` was running.

**What is established, and what is not.** Established: a commit created during
the hook window reached the remote, while the hook was given the earlier SHA
-- the gate's own record and the server's contents disagree. Not established:
git's internal reason. The consistent reading is that the hook is fed the ref
list computed *before* it runs, while the update sent afterwards resolves the
branch again at transfer time; that fits every observation above, but it is
inferred from behaviour, not from reading git's source. **The remedy below
does not depend on which it is,** which is the point of preferring it to a
diagnosis.

**Why this matters more than it looks.** The gates' value is containment --
they exist to stop one lane's breakage reaching the other two, which is what
justifies every lane paying their cost on every push. A commit that slips past
them is published into `lane-c`, and from there merges to `main` and into the
other two lanes' next merge. The failure is silent at exactly the moment the
mechanism is supposed to be earning its keep.

In this instance nothing was harmed: `8ec3d9dc0` edits `known-issues.md` and
nothing else, so no ungated code was published. That is luck, not design.

**The remedy: push a pinned SHA.**

    git push origin <sha>:lane-c

Naming the commit explicitly means the thing sent cannot drift from the thing
the gates were given, whatever git does with a slow hook in between. Verify
afterwards against the server rather than the push's own output, which named
the stale SHA here:

    git ls-remote origin lane-c

**The weaker rule, for when a pinned push is inconvenient:** do not commit
while a push is in flight. Sound, but it depends on remembering, and the whole
reason this was found is that the natural working rhythm -- read something,
notice a mistake, fix it, commit -- fills exactly that window. Prefer the
pinned SHA.

**Do not conclude "the gates are broken".** They ran, and they ran correctly,
on the commit they were given. What is broken is the inference from "the push
succeeded" to "what is on the server has been checked".
