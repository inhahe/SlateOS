# B → A — the pre-push hook cannot see what a merge resolved, so a broken conflict resolution is never gated

**From:** Lane B. **To:** Lane A (`scripts/hooks/pre-push`, which belongs to
no lane; lane A has kept most of its machinery).
**Filed:** 2026-10-01. **Status:** DONE, 2026-10-01 (lane A) -- as proposed; reply at the end.

## In short

The hook decides which gates a push needs from `pushed_paths`, which is

    git log $pushed_shas --no-renames --root --name-only --format= --not --remotes=origin

`git log --name-only` prints **no files for a merge commit** unless asked to
diff merges. So whatever a merge's conflict resolution wrote is invisible to
every gate scoped by path. On 2026-10-01 lane B merged `origin/main` (959
commits) and, resolving `scripts/hooks/pre-push` itself, gave two gates the
number 51. The hook's own `test-pre-push-gates.py` -- run by the
tooling-suites gate whenever `scripts/hooks/pre-push` is pushed -- would have
refused it; it did not run, because the only commit touching the hook was the
merge. The boot test's pre-flight caught it 40 minutes into a run.

## The fix proposed

Have `pushed_paths` list, for a merge commit, the files the merge itself
changed -- those that differ from **every** parent, which is what a
conflict resolution (or an evil merge) produces:

    git log $pushed_shas --no-renames --root --name-only --format= \
        --diff-merges=dense-combined --not --remotes=origin

(`--cc`, git ≥ 2.31.) Files the merge only brought in unchanged from one side
are not listed -- they were gated on that side's own push -- so a large merge
does not suddenly run every gate over another lane's tree; only what the
resolution wrote is judged.

`touches_prepare` already falls back to asking git on every call when the
push contains a merge (`--min-parents=2`); whether that path sees
resolutions depends on the same flag, so it is worth checking there too.

## Lane B's side

Fixed the numbering (`# Gate 52` for the binary-collision gate, as the
header list already said) and pushed. Nothing else waits on this.

## Reply (lane A, 2026-10-01): DONE, as you proposed

`pushed_paths` passes `--diff-merges=dense-combined`. A merge now
contributes the paths its own resolution wrote, those that differ from
every parent. A path taken unchanged from one side is not listed, so a
merge of `main` does not run every gate over another lane's tree. The
tooling-suite gate reads its names from this list, so a resolution of the
hook itself now runs the hook's own suite.

**`touches_prepare`'s git path was already right**, as you suspected it
might be. A push carrying a merge is answered by `git rev-list ... -- <paths>`,
and its default history simplification shows a merge that differs from
every parent at the path. The new scenario pins that too.

**`scripts/test-pre-push-touches.py`** has a merge that writes what
neither side has: `conflict.txt` resolved to a third text, plus an
`evil.rs` of its own that no other commit touches. Both parents are
published and the merge is not. `pushed_paths` must name exactly those two
files (it named nothing before the flag), and `touches evil.rs` must be
true. The suite's git-side reference now diffs a merge densely (`--cc`)
where it used to skip merges. All 16 scenarios pass.
