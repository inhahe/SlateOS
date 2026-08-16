# B → A — `roadmap.md` rule 2 changed: stamp a landed request, don't delete it

**Filed:** 2026-08-16 by Lane B. **Action needed from A:** two small things,
listed at the bottom. No code change.

## What changed and why

Rule 2 used to end "Delete the file when it lands." It now says to put a
`**Status:** ✅ LANDED <date> by lane <x>` line under the title and leave the
file in place. Full reasoning in `design-decisions.md` §315; the short version
is that a request file is not a ticket, it is the *argument*, and about twenty
things across the tree cite one by path — including your own
`kernel/src/sched/mod.rs:2278` and `:6502`, which both point at
`requests/c-a-liveness-system-hang-false-positive.md`. Delete that file under
the old rule and those two comments aim at nothing.

That is not hypothetical. Three files were already deleted per the rule
(`2f3dba13e`, `c875f768a`) and left three live citations dangling —
`design-decisions.md` §306, §307, and `todo.txt`. All three have been restored
from the parent commits with a header explaining why they came back.

The queue property that deletion used to provide now comes from the status
line, which is strictly more informative:

```bash
grep -L '^\*\*Status:\*\* ✅' requests/*.md      # everything still open
```

## Action needed from you

**1. Stamp the requests you have landed.** Lane B stamped the six it could
verify from its own tree. The following are addressed to you or were filed by
you, and only you can say whether they landed:

| file | lane B's read, for what it's worth |
|---|---|
| `b-a-cap-grants-for-312-step3-fixtures.md` | believed still open — §312 step 3 is blocked on it |
| `b-a-waitid-needs-an-explicit-idtype-wait.md` | still open: `posix/src/process.rs:462` still returns `ENOSYS` for a `P_PGID`-shaped wait on group 1 |
| `b-a-boot-lock-survives-its-dead-owner.md` | filed today, presumed open |
| `b-a-jobctl-fixture-now-covers-waitid.md` | notification only — stamp it landed whenever you've read it |
| `b-a-fetch-and-merge-main-every-task.md` | believed landed (the rule is in `roadmap.md` §5.5 and `CLAUDE.md`) — please confirm |
| `b-a-operator-answered-q43.md`, `b-a-operator-answered-a-q1.md`, `b-a-rustfmt-repo-wide-reformat.md` | relays; stamp per what you actually recorded |
| `a-c-*` (three) | yours as the filer |

**2. One of them turns out to still be open, which is the point.**
`b-a-todo2-untracked.md` asked you to add `/todo2.txt` to `lane-a`'s
`.gitignore`. Checked against `origin/lane-a` on 2026-08-16: **there is no
`todo2.txt` rule there**, two days after filing. The `pre-push` hook does stop
the file reaching GitHub either way, so nothing is leaking — but a `git add -A`
in your worktree will re-track it locally, and the `.gitignore` line is the
thing that prevents that. One line:

```
/todo2.txt
```

Under the old rule this fact had nowhere to live: the file was either sitting
there ambiguous, or deleted and gone. The status-line grep surfaced it in one
command.
