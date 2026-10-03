### [A] The operator's `LICENSE` has been uncommitted in the integration tree for two days, and the obvious way to resolve a merge there would have committed it under my message -- 2026-09-18
**Status:** OPEN (needs the operator; the merge hazard is closed on my side)

**In short:** the project's licence file exists on disk and is not saved
into version control, so the published project still has no licence. It is
in the one directory no lane is allowed to edit, which is also the directory
I am told to use when publishing my work -- and the normal way to finish a
publish there would have swept the operator's unsaved files in with mine,
under my description of the change.

**What is sitting in `E:\visual studio projects\os`:**

| path | state | dated |
|---|---|---|
| `LICENSE` | untracked | 2026-09-16 15:18 — MIT, `Copyright (c) 2026 Inhahe (inhahe.com)`, 21 lines, complete |
| `README.md` | modified | 2026-09-16 15:18 — adds a `## License` section pointing at it |
| `open-questions-answers.txt` | untracked | 2026-09-07 — 32 lines answering questions across all three lanes |

The answers file is **not** a missed batch: lane B found it, filed
`requests/b-a-operator-answered-eleven-lane-a-questions-2026-09-07.md`, and
nine of the eleven lane-A answers became `design-decisions.md` §914-§922,
with a correction on 09-09 adding §924. It is listed here only because it
shows how long a file can sit there -- eleven days -- and because I
rediscovered it by accident while looking at something unrelated.

**The hazard, which is the part that was mine to fix.** `os/CLAUDE.md` says
never edit files in the integration tree; `roadmap.md` says merge your lane
up to `main` *from* it. Both are right, and together they mean my publish
route passes through a directory containing three files I did not write and
must not touch. The obvious conflict resolution there is `git add -A` --
which is exactly how `c4bbff648` committed three conflict markers, except
this time it would have committed the operator's licence under a commit
message about kernel work.

Closed with `build/merge-to-main.sh`, which (a) hashes every uncommitted
path in that tree *before* merging, (b) stages conflicted paths **by name**
and refuses to automate a conflict at all, and (c) after merging proves each
recorded file is byte-identical and still unstaged before it will push. The
check is the point: it does not ask whether the paths still exist, it asks
whether the bytes moved.

`build/check-operator-drops.py` is the other half -- one `git status`
against a tree no lane writes, so anything uncommitted there is the
operator. It found `LICENSE` and the `README` change, which I had not
noticed in two days of working beside them.

**What needs the operator, and it is not a design question.** Whether to
commit `LICENSE` + the `README` section is theirs: the work is complete, not
half-finished, so the likeliest reading is that it was simply never
committed. I am not committing another party's uncommitted work under my own
authorship, and there is no option set to weigh, so this is a notice rather
than an `open-questions.md` entry -- that file is 32 deep and
`deferred-questions.md`'s header warns what padding it costs.

Worth noting the consequence plainly: **until it is committed, SlateOS has
no licence on `main`**, which is the only copy anyone else can see.
