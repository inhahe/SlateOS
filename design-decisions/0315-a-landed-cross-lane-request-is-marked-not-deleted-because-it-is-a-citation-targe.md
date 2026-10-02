## §315 — A landed cross-lane request is marked, not deleted, because it is a citation target

**Date:** 2026-08-16
**Decided by:** Claude (autonomous) — a correction to a mechanical rule
(`roadmap.md` rule 2) that had already misfired three times. No user-visible
behaviour, no code semantics; if the operator prefers the old rule it is one
`git revert` away.

**In short:** when one lane asks another for something, the ask is written up
as a file in `requests/`. The rule said to delete that file once the work was
done. But those write-ups turn out to be the only place the *reasoning* behind
a cross-lane decision is recorded, so comments in the source code and entries
in the shared documents point at them by filename. Deleting one therefore
leaves those pointers aimed at a file that no longer exists — which has
already happened three times. From now on a finished request gets a "done"
line at the top and stays where it is.

**What went wrong.** `roadmap.md` rule 2 ended with "Delete the file when it
lands." Two commits obeyed it:

| commit | deleted |
|---|---|
| `2f3dba13e` | `requests/b-c-fetch-and-merge-main-every-task.md`, `requests/b-c-todo2-untracked.md` |
| `c875f768a` | `requests/b-c-operator-answered-q45-and-c-q1.md` |

and left three live citations pointing at nothing: `design-decisions.md` §306
and §307, and `todo.txt`'s Lane B section. Nobody noticed, because a dead
markdown path produces no error anywhere — it is only discovered by a reader
who follows it, which is precisely the reader the citation exists to serve.

**Why the citation web exists at all, and why it is right.** A request file is
not a ticket; it is the *argument*.
`requests/a-b-init-conflates-syscall-error-with-exit-code.md` explains why
`init`'s supervisor must not read a kernel error as an exit code, with the
nine-restart incident that proved it — so the `WaitStatus` enum's doc comment
cites it rather than restating it. The same pattern holds in
`posix/src/errno.rs`, `posix/src/sys_capability.rs`, `posix/src/syscall.rs`,
`kernel/src/sched/mod.rs`, `scripts/ki_archive.py`, `known-issues.md`,
`design-decisions.md`, `roadmap.md` and `CLAUDE.md`. Roughly 20 live citations
across the tree. Cross-referencing rather than duplicating is the right call;
it is deletion that breaks it.

**Alternatives considered.**

- *Keep deleting; inline the reasoning into each citing comment instead.*
  Rejected: it multiplies the write-up across every call site, and the copies
  drift. The whole point of the request file is to be the single copy.
- *Keep deleting; move landed files to `requests/archive/`.* Rejected: the path
  changes, so every citation still breaks. An archive only helps when nothing
  points at the thing being archived — which is why it worked for
  `known-issues-resolved.md` (entries are cited by **name**, not by file) and
  would not work here.
- *Keep deleting; rewrite citations to reference the deleting commit hash.*
  Rejected: a hash is not readable, does not survive a mental model of "go read
  the request", and asks a reader to run git archaeology to answer "why is this
  code shaped like this".

**What replaces it.** A `**Status:** ✅ LANDED <date> by lane <x>` line directly
under the title, plus one sentence on what actually shipped. The queue property
— the thing deletion was providing — comes from that line instead:

```bash
grep -L '^\*\*Status:\*\* ✅' requests/*.md      # everything still open
```

which is strictly better than file-absence, because it also tells you *what*
landed and when, and it distinguishes "done" from "never filed".

**Cost.** The dropbox stops shrinking. Measured: 26 files, ~4,000 lines total,
in a repository whose `known-issues.md` alone is 21,000 lines. Against that,
the thing deletion destroys is the only written record of why a cross-lane
interface is shaped the way it is. Not close.

**Landed alongside.** The three deleted files were restored from
`2f3dba13e^` / `c875f768a^` with a status header explaining the restoration, so
the three dangling citations resolve again; six landed requests were stamped
(the five verifiable from lane B's own tree plus lane C's notification); and
the remaining 19 correctly show as open — including
`requests/b-a-todo2-untracked.md`, which the grep immediately revealed is
*still* open (`origin/lane-a`'s `.gitignore` has no `todo2.txt` rule, two days
on). Under the old rule that fact had no home: the file was either present and
ambiguous, or deleted and gone.

**Standing lesson.** A cleanup rule phrased as "delete when done" is safe only
for artifacts nothing points at. Before adopting one, ask what cites the thing
— and note that markdown, unlike code, has no compiler to tell you when the
answer changes.
