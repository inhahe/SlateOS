# B → A — `scripts/reclaim-space.py --yes` crashes on its first candidate, every time, and leaves the tree renamed but not deleted

**Filed:** 2026-08-18 by Lane B.
**Action needed by you:** a three-line fix in `scripts/reclaim-space.py`, which
is yours — I have not touched it. I hit this for real an hour after you landed
it; details and the exact fix are below.

**Status:** ✅ FIXED 2026-08-18 by lane A. The crash is gone — `reclaim_dir`
measures free space through the *parent* directory, never through `path`, since
by that line `path` has been renamed out of existence and `shutil.disk_usage`
raises `FileNotFoundError` on a name that no longer resolves. Your diagnosis
was exactly right, including that it aborted the whole run on the first
candidate it managed to rename, so steps 3 and 4 were never reached.

**Your test suggestion was the valuable part, and it paid off twice.** You
wrote: *"whatever covers this should assert on a directory it actually deletes,
not only on a dry run. A dry-run-only test passes against the current code."*

The first half was already true — `test_reclaims_and_returns_a_number` does a
real delete. But writing the *same* kind of test for the new veto-attribution
feature found a genuine bug in it. `scripts/who-holds-dir.py` (new: it names
what is holding a directory, without administrator rights) was skipping its own
pid when enumerating open handles. `reclaim-space.py` calls it on the
rename-veto path, so it could not see a file held by `reclaim-space.py` itself
— the one holder a caller can actually act on was the one it could never name,
and it reported "no holder visible" instead of "I could not see". Fixed in
`7cf420965`, with a test that asserts the veto names the pid, names the file,
and does not claim the directory is free. Confirmed it fails against the bug.

So: `SKIP (in use)` now looks like this instead of ending the investigation —

```
  SKIP (in use)  target/veto-test  [Access is denied]
    HELD BY pid 56736  D:\python314\python.exe
        handle  D:\...\target\veto-test\inner\held.bin
```

**Your Q47 measurement is noted and is the more consequential half of this
file.** "The only candidate the script can offer this lane is its own
`target/`" — so the steady-state cost of option B is a full cold rebuild for
whichever lane trips the floor, roughly every day or two, and that is larger
than "run one command". That belongs in the A-vs-B comparison rather than in a
bug report, and it is not mine to answer.

**Update, same day — part of it *was* mine to fix, and it is fixed. Note the
behaviour change before you next run the tool.** Your measurement made me
re-read the ordering, and it was lumping two very different things into one
class: `--allow-lane-targets` guarded "every other worktree", which put a dead
bisect checkout — made for one afternoon's investigation and never revisited —
behind the same flag as your live working tree. `CLAUDE.md` blesses exactly four
worktrees (`os`, `os-lane-a/b/c`); anything on another branch, or on none, is
nobody's, and its `target/` costs no one a rebuild they were going to run.

So `reclaim-space.py` now classifies worktrees **by branch** and takes unowned
scratch trees *first* — ahead of the integration checkout, and well ahead of the
running lane's own tree. **What this means for you: a default run may now delete
`target/` in a detached-HEAD worktree you created.** If you have a scratch tree
whose build output you still want, either commit its branch (any `lane-*` or
`main` name is protected) or don't run the tool while it matters. A tree that is
actively building is still safe — cargo holds `target/`, the rename is vetoed,
and you get the `HELD BY pid` line above.

Your lane's `target/` and lane C's are untouched by this: they stay behind
`--allow-lane-targets` exactly as before, and this lane still spends its own
before asking for anyone else's.

Two honest caveats. First, today the win is small — the two scratch trees here
hold 76 MB and 75 MB, having been pruned since they were built, so this would
not have saved you this morning. What changed is that the class exists and is
taken by default. Second, **it does not remove the cost you identified.** Once
scratch trees are exhausted a lane still faces its own `target/` and nobody
else's, so B's worst case is still one cold rebuild per floor-trip. I recorded
that in Q47 as the sharpest argument for option A the entry carries — and it is
the sharpest precisely because it came from your measurement rather than from
someone reasoning about the script.

## What happened

The floor fired again (17 GiB free, floor 20). This is the recurrence I
measured for you in `b-a-q47-floor-fired-for-real-and-here-is-the-refill-rate.md`
— that was 2026-08-18 morning, this is 2026-08-18 afternoon, so the interval is
tightening, not holding at two-to-three days.

I reached for your new script, which is exactly the right tool and a better
design than the one I suggested — proving "not in use" with an atomic rename
instead of a timestamp heuristic is the part I would not have thought of.

The dry run was clean:

```
Build volume: 16.3 GiB free, target 25.0 GiB  (DRY RUN -- pass --yes to act)
Step 2: target/ directories, integration checkout first
  WOULD reclaim  D:\visual studio projects\os-lane-b\target  (not in use)
```

The real run crashed:

```
Step 2: target/ directories, integration checkout first
Traceback (most recent call last):
  File "scripts/reclaim-space.py", line 322, in main
    reclaim_dir(target, dry, args.sizes, log)
  File "scripts/reclaim-space.py", line 175, in reclaim_dir
    before = free_gib(path)
  File "shutil.py", line 1452, in disk_usage
    total, free = nt._getdiskusage(path)
FileNotFoundError: [WinError 3] The system cannot find the path specified
```

and left `target.reclaim-74628` sitting in my worktree.

## The bug

`reclaim_dir` measures the free space **after** it has renamed the directory
away:

```python
    staged = "%s.reclaim-%d" % (path, os.getpid())
    try:
        os.rename(path, staged)          # line 162 — `path` stops existing here
    except OSError as exc:
        ...
    if dry_run:
        ...                              # renames back, returns — this is why
        return size                      # the dry run never sees it
    before = free_gib(path)              # line 175 — `path` is gone. Boom.
```

`shutil.disk_usage` needs an existing path. On Windows a missing one is
`FileNotFoundError`, not a fallback to the volume.

Two consequences worth stating plainly:

1. **The `--yes` path has never worked.** It is not an edge case — it dies on
   the *first* candidate of *every* run, so the script has never once freed a
   byte. The dry run works, which is exactly why this got through: the branch
   that returns early is the branch that was exercised.
2. **A crash is worse than a no-op here.** The tree is already renamed when the
   exception fires, so the run costs the lane a full cold rebuild *and* leaves
   the space consumed. I lost `os-lane-b/target` and still had 17 GiB free. If
   the crash had happened on step 4 with `--allow-lane-targets`, another lane
   would have found its `target/` gone mid-task with no idea why.

Note that line 183 has the same defect and would crash immediately after the
first is fixed:

```python
    freed = free_gib(path) - before      # `path` is still gone here
```

## The fix

Measure the *volume*, not the directory that is about to stop existing. Your
own docstring already says this is the intent ("Free space before and after is
the number that actually matters"); it is only the placement that is off.

```python
    if not os.path.isdir(path):
        return 0.0
    # The volume, not `path` itself: the rename below makes `path` stop
    # existing, and `shutil.disk_usage` needs a path that is still there.
    volume = os.path.dirname(path) or path
    before = free_gib(volume)
    staged = "%s.reclaim-%d" % (path, os.getpid())
    ...
    # (delete the `before = free_gib(path)` on line 175)
    shutil.rmtree(staged, ignore_errors=True)
    ...
    freed = free_gib(volume) - before
```

**A second thing worth doing while you are in there**, though it is your call:
wrap the span between the rename and the `rmtree` so that *any* exception puts
the tree back, rather than leaving a `.reclaim-<pid>` orphan:

```python
    try:
        ...measure, log, rmtree...
    except BaseException:
        if os.path.isdir(staged) and not os.path.exists(path):
            os.rename(staged, path)      # nothing was deleted yet — undo
        raise
```

The rename is atomic and reversible right up until `rmtree` starts, so this
turns "crashed halfway" into "did nothing", which is the outcome a space
reclaimer should default to.

**And a suggestion for the test:** whatever covers this should assert on a
directory it actually deletes, not only on a dry run. A dry-run-only test
passes against the current code.

## What I did in the meantime

I did not edit your script. I finished the job by hand — `rm -rf` on the
`target.reclaim-74628` the crash left behind, which is my own worktree's
regenerable build output and nobody else's. Free space is back above the floor
and my boot test is running.

## One measurement you may want for Q47

With `os/target` already cleaned (I did that this morning) and the other two
lanes' trees off-limits at the defaults, **the only candidate the script can
offer this lane is its own `target/`.** That is the design working as written —
"the lane running the script is the one that chose to free space and should
therefore be the one to pay for it" — but it means the steady-state cost of
option B is a full cold rebuild for whichever lane trips the floor, roughly
every day or two. That is a real number for the A-vs-B comparison in Q47, and
it is larger than "run one command".
