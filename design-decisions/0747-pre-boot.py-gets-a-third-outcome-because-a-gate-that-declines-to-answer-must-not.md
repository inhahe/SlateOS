## 747. `pre-boot.py` gets a third outcome, because a gate that declines to answer must not be filed as a pass or as a defect

**Date:** 2026-09-02
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** before running the long boot test, `pre-boot.py` runs about twenty
small checking scripts and prints one line each — `ok` or `FAIL`. Some of those
scripts inspect things that may not be present in the tree you are sitting in,
and when a script says "I could not look at this", those two words are the only
ones available and both are wrong: `ok` claims a clean result nobody obtained,
and `FAIL` accuses you of a defect that was never observed. The decision is to
add a third line, `SKIP`, for a script that ran and reached no verdict — printed
with the script's own explanation, counted separately, and enough to suppress
the "all clear" at the end.

This is §746 one runner over. There, the push hook learned to tell "your code is
wrong" from "my checker fell over"; here the pre-boot runner learns to tell both
of those from "there was nothing for me to look at."

### What made it visible

Lane C filed
`requests/c-b-check-libc-shape-grades-a-build-artifact-without-checking-its-age.md`.
`scripts/check-libc-shape.py` grades `toolchain/sysroot/lib/libc.a` — an
untracked **build artifact** — and it is picked up by `pre-boot.py`'s
`check-*.py` glob, so every lane runs it whether or not that lane has ever built
`posix`. Lane C's copy was dated 2026-08-21 with 57 commits to `posix/` since,
and the gate reported seven findings about it.

Measured here on 2026-09-02 against a fresh archive: **all seven were already
fixed.** Attention was available to be spent on defects that did not exist, and
lane C could not tell — which is why the file was filed rather than the findings
acted on.

That is the harmless direction. The dangerous one is the same staleness with the
opposite content: an eleven-day-old archive that happens to be clean prints
`ok`, and `ok` from *this* gate is precisely the signal that means "a GNU
package will link" (§339, §340, §348). A gate that can pass on eleven-day-old
evidence is not a gate.

### What was decided, and what was rejected

Lane C recommended: detect staleness, print a skip line, **return 0**. That
cannot work as written, and finding out why is what moved the fix into the
runner. `_report` printed `ok <label> (Ns)` for exit 0 and **discarded the
captured output**. The skip line would never have been printed at all; the run
would have shown `ok  check-libc-shape.py` — the gate asserting linkability
about an archive it had declined to open. The proposal was right about the
outcome wanted, and could only be delivered somewhere else.

| Option | Why not |
|---|---|
| Fail on staleness (lane C's A) | Correct, and it converts lane B's artifact freshness into a red gate lanes A and C cannot clear by any edit to their own code. That is the shape of thing that gets worked around rather than fixed. |
| Skip and return 0 (lane C's B) | The runner would render it `ok`. Rejected for the reason above, not for its intent — its intent is what shipped. |
| Gate on `CLAUDE_CONFIG_DIR` (lane C's C) | Narrows a correctness check by whose account is running, which has nothing to do with the thing being checked, and stops covering `main`, where the artifact question is identical and nobody owns it. |
| Grade the archive the build produces (lane C's unrecommended fourth) | Genuinely removes the question rather than answering it, and is the right long-term shape. It is a change to how the sysroot is assembled; not folded into a request about staleness. Left open. |

So: the checker keeps saying "could not check" the way it **already did** for a
missing archive — that branch's comment had already argued the case ("a caller
that treats 'could not look' as 'looked and it was fine' is how a gate ends up
permanently green") — and the runner learned to say it out loud.

### Why exit 2, and why this is a convention rather than an invention

It is already the convention; the runner was the one thing not following it.
`run-checker.sh` has had 0 clean / 1 finding / anything-else no-verdict since it
was written. `boot-test.sh`'s exit-code header states the principle directly: a
gate that "ran, produced no judgement, must not be read as having produced a
clean one." And **20 of the 21 scripts `pre-boot.py` runs already return 2 for
"could not look"** — a missing directory, an unparseable file, a baseline that
is not there.

The twenty-first is `scripts/check-generated-tables.py`, whose `return 2` is
commented "treating that as a failure". That is lane C's file (`gui/font/**`),
so it was not changed;
`requests/b-c-check-generated-tables-returns-2-which-now-means-no-verdict.md`
puts the one-line choice back to them rather than my re-deciding it inside their
tree. This is the cost of adopting a convention rather than enumerating
exceptions, and it is paid once, visibly, with the affected party told.

### One asymmetry with `run-checker.sh`, deliberately

There, an **unrecognised** status is a no-verdict. Here, only 2 is a skip and
every other non-zero remains a failure. The caller's situation differs: a push
gate faces checkers whose failure modes it cannot enumerate, and the safe
reading of an unknown status is "we do not know" — whereas `pre-boot.py` runs
scripts this repository owns, and a checker dying of an unhandled exception
(Python's exit 1) or being unlaunchable (126/127) must not be quietly absorbed
into "not applicable". Widening the skip set would re-create, at the runner, the
exact defect §746 removed from the hook.

### The staleness test itself

`libc.a`'s mtime against every file under `posix/` plus
`toolchain/build-sysroot.ps1`. The build script is in the set because the
regression this checker exists for **is a dropped flag in that script**
(`-C codegen-units=4096`, §339); an edit to it that has not been rebuilt leaves
an archive describing the old flags, which is the state where a stale `ok` is
most misleading.

mtime rather than a recorded commit or a stamp file: it is what every build
system uses for this question, and it needs nothing written down that can itself
fall out of date. A `git merge` rewrites only the files it changes, so a merge
that leaves `posix/` alone bumps nothing. The failure mode is a checkout
touching a file whose content did not change — which reports staleness that is
not real, and costs a skipped run rather than a false pass. Verified on this
worktree the same day: 0 of 2305 `posix/` files postdate a freshly built
archive, so the test is quiet where it should be quiet rather than degenerating
into an always-skip that would switch the gate off by accident.

The listing goes through `gittree.WorkTree` rather than a walk of its own, so
the rule for what counts as build output is not spelled a third time — the
second spelling of that rule had already drifted from the first
(`known-issues.md`, the pre-push-gates entry, step 7).

An explicitly named archive is **not** age-checked: that is somebody
deliberately grading a different file — a saved copy, another lane's, one under
bisection — and comparing it against this checkout's `posix/` would answer a
question nobody asked. `--ignore-age` forces a grade of the default one.
