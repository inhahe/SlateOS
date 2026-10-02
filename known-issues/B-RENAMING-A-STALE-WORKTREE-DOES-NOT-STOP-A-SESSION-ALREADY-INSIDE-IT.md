## B-RENAMING-A-STALE-WORKTREE-DOES-NOT-STOP-A-SESSION-ALREADY-INSIDE-IT (lane B, 2026-09-07)

**In short:** the old copies of the tree on `D:` were renamed so that anyone
using a remembered path would fail loudly instead of quietly working on the
wrong copy. That protects against *looking a path up*. It does not protect
against a program that was already sitting in the directory when it was
renamed -- that program keeps working, keeps reading a tree frozen at the
moment of the move, and keeps drawing confident conclusions from it.

**Observed, not theorised.** At 2026-09-07 a session announcing itself as lane B
-- while this session was also lane B -- messaged to report that
`apps/lockscreen` did not compile and was blocking the boot test for all three
lanes, and offered to patch it. Every claim in it was true some hours earlier and
false when sent. Measured at the time, stating which tree each fact came from:

| Fact | Tree |
|---|---|
| Cited commit `87c27272d` | is the **HEAD of `D:/…/os-lane-b.MIGRATED-TO-E-2026-09-06`** |
| `git cat-file -t 87c27272d` | **"Not a valid object name"** on `E:` -- a separate object store |
| `apps/lockscreen` on `origin/main` | no `authlib::shadow`; `with_stores` already takes one path |
| The request it was about to file | already filed **and withdrawn**, same day, by this session |

Lane C had found and fixed it in `d9f1f540e` (merged `97219f95c`) before any of
it was written.

**Why this is worth an entry rather than a shrug.** The stale session's `origin`
is the same GitHub URL as everyone else's, so it retained push access to the
shared branches while being unable to see them. Nothing was damaged -- checked:
`refs/heads/lane-b` on the remote matched this worktree exactly, and no salvage
or stale branch existed -- but the exposure was real, and a force-push from a
tree 54+ commits behind would have destroyed a day's work from three lanes.

**Why the existing mitigation did not fire.** Lane A armed a "STALE TREE" halt in
the `D:` copy's own coordination directory for exactly this case. It did not
reach the session, because until `f6f820f7c` the halt's only reader was
`boot-test.sh`, and a session that greps and messages without boot-testing never
consults it. That gap is now closed for `pre-push` -- but *not* for the stale
tree, and honestly cannot be: `install-hooks.sh` lays down a trampoline that
execs the working tree's **own** tracked hook, so a tree too old to contain the
check is also too old to run it. The fix only reaches a tree that merges, and a
tree that merges is no longer stale. Self-limiting, and worth knowing rather than
worth solving.

**What actually caught it** was neither a hook nor a halt: the cited commit hash
did not resolve. A claim carrying a commit id is checkable in one command, and
`git cat-file -t` answers "is this session even in my repository?" -- which is a
question worth asking of any cross-session report that arrives with a hash and a
surprise. Reports without one are not checkable this way, which is an argument
for including the hash.

**Related:** `requests/b-c-lockscreen-calls-two-authlib-things-i-deleted-and-main-is-red.md`
(WITHDRAWN) is the same error made by *this* session against its own worktree 54
commits behind, and carries the rule both instances point at: fetch before
**reading** to draw a conclusion, not only before writing. Two independent
sessions reached the same wrong conclusion by the same route on the same day,
which suggests the hazard is structural rather than careless.
