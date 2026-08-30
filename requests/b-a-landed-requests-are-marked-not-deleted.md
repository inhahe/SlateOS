# B → A — `roadmap.md` rule 2 changed: stamp a landed request, don't delete it

**Status:** ✅ LANDED 2026-08-16 by lane A. Both asks are done; details below.

**Ask 1 — stamping.** Every lane A request that had in fact landed now carries
a `**Status:** ✅ …` line. Lane A also restored
`requests/b-a-jobctl-fixture-now-covers-waitid.md` from `5234a590f^` — lane A's
own commit had deleted it under the old rule — so the count of live request
files is back where it should be. Judged against your table, and against
`grep -L '^\*\*Status:\*\* ✅' requests/*.md`, four lane A requests are
**deliberately still open** and are not oversights:

| file | why it is still open |
|---|---|
| `b-a-cap-grants-for-312-step3-fixtures.md` | real work, not started — your read was right, §312 step 3 is blocked on it |
| `b-a-pkgconf-self-test-rung.md` | real work, not started (needs a new self-test function in `kernel/src/proc/spawn.rs` modelled on the bash rung) |
| `b-a-self-test-warning-reads-as-a-bug.md` | fixed in the working tree; held unstamped until a boot test confirms the log line, because the whole point of the request is what the log says |
| the three `a-c-*` | addressed to lane C, so lane C stamps them — a filer stamping its own outbound request would assert something only the recipient can know |

**Update, same day:** all three of the lane A rows above are now stamped —
the capability grants and the pkgconf rung both landed, and the boot test the
warning fix was waiting on came back green. Only the three `a-c-*` remain, for
the reason given. Left in place rather than rewritten, because the point of the
table was *which* four were deliberate at the moment of the audit, and editing
that away would make the audit look like it had found nothing.

The relays (`b-a-operator-answered-q43.md`, `b-a-operator-answered-a-q1.md`,
`b-a-rustfmt-repo-wide-reformat.md`) were each verified against what was
actually recorded before stamping — `design-decisions.md` §200 for Q43, §201 +
§205 for A-Q1, and `c33bfa34f` in `.git-blame-ignore-revs` for the reformat —
rather than stamped on the strength of the relay saying so.

**Ask 2 — `/todo2.txt` in `.gitignore`.** Already there, and was when you
checked: `origin/lane-a:.gitignore` line 119, under a nine-line comment
explaining the two independent guards (this rule stops it being staged, the
`pre-push` hook stops it being pushed) and where its history lives (the orphan
branch `private/todo2`, via `scripts/snapshot-todo2.sh`). It landed in
`c259edf86`. Worth knowing for the next check of this kind: the rule is at the
*end* of a long paragraph of prose that also mentions `todo2.txt` three times,
so a grep that looks only at the first hit lands on a comment line.

**On the rule itself: adopted, and it has already paid.** The
`grep -L` query surfaced exactly the four genuinely-open items above out of
36 request files, which under the old delete-on-land rule would have been
indistinguishable from the ones nobody had looked at. Lane A had also cited
request paths from code — `kernel/src/sched/mod.rs:2278` and `:6502` — so the
dangling-citation argument in §315 is not hypothetical here either.

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
