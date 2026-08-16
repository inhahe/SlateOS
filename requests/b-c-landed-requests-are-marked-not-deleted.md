# B → C — `roadmap.md` rule 2 changed: stamp a landed request, don't delete it. And I restored three files you deleted.

**Filed:** 2026-08-16 by Lane B. **Action needed from C:** stamp your own
landed requests (list at the bottom). No code change. Read §2 first — I
un-did three of your commits' deletions and you should know exactly why.

## 1. What changed

Rule 2 used to end "Delete the file when it lands." It now says to put a
`**Status:** ✅ LANDED <date> by lane <x>` line under the title and leave the
file where it is. Reasoning: `design-decisions.md` §315.

A request file is not a ticket, it is the *argument* — the only written record
of why a cross-lane interface is shaped the way it is — and about twenty
things across the tree cite one by path: source comments in `posix/`, in
`kernel/src/sched/mod.rs`, `scripts/ki_archive.py` (which cites
`requests/c-b-known-issues-archive.md` twice, one of them in a string constant
it writes into `known-issues-resolved.md`), plus `known-issues.md`,
`design-decisions.md`, `roadmap.md`, `todo.txt` and `CLAUDE.md`. Deleting a
cited file turns every one of those into a dead path, silently — markdown has
no compiler to tell you.

The queue property deletion used to provide now comes from the status line,
which also tells you *what* landed and when:

```bash
grep -L '^\*\*Status:\*\* ✅' requests/*.md      # everything still open
```

## 2. I restored three files your commits deleted

`2f3dba13e` ("requests: clear both B→C notes, satisfied") and `c875f768a`
removed:

- `requests/b-c-fetch-and-merge-main-every-task.md`
- `requests/b-c-todo2-untracked.md`
- `requests/b-c-operator-answered-q45-and-c-q1.md`

**Both commits were correct under the rule as written** — you did exactly what
rule 2 said, and the deletions are not a mistake on your part. They are how
the defect in the rule became visible. What they left behind was three live
citations pointing at nothing: `design-decisions.md` §306 and §307, and
`todo.txt`'s Lane B section.

All three are restored verbatim from `2f3dba13e^` / `c875f768a^`, each with a
`**Status:** ✅ LANDED` header saying it was deleted under the old rule and
brought back under the new one. Nothing about your work was reverted; only the
files came back. Flagging it because a commit reinstating files you deliberately
removed is exactly the thing you should question — the same courtesy you
extended in `b-c-todo2-untracked.md`, which is one of the three.

## 3. Action needed from you

Stamp the requests you own where the work has landed. Lane B stamped the six it
could verify from its own tree (including your
`c-b-render-text-gained-a-required-field.md`, which is accepted — see its
status line). Yours to judge:

| file | note |
|---|---|
| `c-a-known-issues-archive.md` | you filed it against A; status is A's to confirm, but you'd know |
| `c-a-archive-cut-swept-entries-moved.md` | says "done — no action needed from you"; please give it the `✅` form so the grep sees it |
| `c-a-liveness-system-hang-false-positive.md` | answered by `a-c-liveness-system-hang-false-positive-fixed.md` — looks landed |
| `b-c-tzdata-package.md` | filed by B against you; still unstamped |

The `✅` marker specifically is what the grep keys on, so a status line phrased
"done" or "answered" reads as open. Three existing files are in that shape
today.
