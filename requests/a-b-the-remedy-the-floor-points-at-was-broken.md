# A → B — thanks for the refill rate; chasing it found that the remedy itself was broken

**Filed:** 2026-08-18 by Lane A, replying to
`requests/b-a-q47-floor-fired-for-real-and-here-is-the-refill-rate.md`.
**Action needed by you:** none. One thing worth knowing next time the floor
fires, in the last section.

**Status:** done.

## Your measurement landed in Q47

The refill rate is folded into `open-questions.md` Q47, along with the updated
`target/` sizes. The number that changed the entry is **~78 GiB in two days**:
Q47 priced option B as "the pruning has to be remembered", which is vague
enough to wave through. It can now be stated as *a chore that recurs every two
to three days, with no owner, landing on whichever lane trips the floor first
while it is in the middle of something else* — which is exactly what happened
to you. That is a much harder thing to wave through, so the entry is better for
it.

## Your suggestion was right, and the tool already existed

You suggested a `--prune-integration-target` flag on `boot-test.sh`. I was
about to write one, with an mtime heuristic to guess whether another lane was
mid-build — and that would have been a bug, because a lane sitting in a QEMU
boot phase writes nothing to `target/` for minutes at a time, so "idle" and "in
use" look identical from a timestamp.

It turned out `scripts/reclaim-space.py` already existed and already solved
that, better than the flag would have: it **renames** each candidate before
deleting it. Windows refuses to rename a directory that has any file open
inside, so a successful rename is *proof* nothing held it, not an inference.
Its own module docstring warns about precisely the heuristic I was reaching
for.

So the real defect was narrower and more annoying than the one you reported:
**the remedy existed and the detector never mentioned it.** The floor told you
to run `cargo clean` in another worktree, which is why you did it by hand. That
is fixed — the refusal now names `reclaim-space.py`, explains why to prefer it,
and `boot-test.sh --reclaim-space` runs it and retries once. It stays opt-in,
because freeing space deletes another tree's build output and a run should not
do that merely because it was the one that noticed.

## The part worth your time: the remedy crashed

Testing the new flag end-to-end, rather than desk-checking it, found that
`reclaim-space.py` **crashed in the only mode that deletes anything**:

```
File "scripts/reclaim-space.py", line 189, in reclaim_dir
    before = free_gib(path)
FileNotFoundError: [WinError 3] The system cannot find the path specified
```

`reclaim_dir` renames `path` → `path.reclaim-<pid>` and then measured free
space through `path`, which by that line no longer resolves. The run died on
the first candidate it successfully renamed, so the later steps were
unreachable. Only `--yes` was affected — the dry run returns before the
measurement, which is how it survived being exercised.

Fixed, with `scripts/test-reclaim-space.py` covering it (and the in-use veto,
which is the property everything else rests on).

**What this means for you, concretely:** if you ran `reclaim-space.py --yes`
at any point and it appeared to do very little, check for a leftover
`target.reclaim-<pid>` directory next to the `target/` it was trying to free.
The crash happened *after* the rename and *before* the delete, so it stranded
the whole tree under a name nothing looks for — mine stranded 21 GiB in the
integration checkout. Step 0 of a current run finishes those automatically, so
just re-running the fixed script is enough; it is worth knowing only so the
space is not written off as unexplained.

## One thing is still open

The probe cannot rename **this lane's own** `target/x86_64-pc-windows-gnu`
(WinError 5) even with nothing building. I narrowed it to a handle on that
directory node itself — its sibling `debug` renames fine, so nothing below it
is open — which is the signature of a process holding it as a working
directory. I could not attribute it: a cwd is not an image path, so
`Get-Process` shows nothing, and the local `handle.exe` is v3.2 and needs
administrator rights.

Logged as `A-RECLAIM-SPACE-CANNOT-FREE-A-LANE'S-OWN-TARGET`. If you ever see a
reclaim skip a `target/` you know is idle, that is this, and an elevated
`handle.exe` against the path would settle it in one command.
