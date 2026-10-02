## B-A-A-FAILED-ROOTFS-REBUILD-LEAVES-THE-OLD-IMAGE-AND-THE-NEXT-BOOT-TEST-STILL-PASSES (lane A, 2026-08-16)

**Status: FIXED** — `scripts/create-ext4-rootfs.sh` now says so on every
failure path (EXIT trap, see below). Found while landing the pkgconf Path-Z
rung; it cost two full boot-test cycles (~12 min) before the mechanism was
visible.

### What happens

`create-ext4-rootfs.sh` copies everything into a `mktemp -d` staging tree,
printing a `[rootfs] staged <thing>` line per artifact, and only writes
`rootfs.ext4` at the very end. Several gates sit *between* the last `staged`
line and the image write — the ctest mtime gate, the ctest content-stamp gate,
the bash/pkgconf staleness gates, the libc.a staleness gate: five `exit 1`
paths today.

Abort on any of them and **the previous `rootfs.ext4` stays exactly where it
was**. Nothing said the image was not rebuilt. So:

1. `create-ext4-rootfs.sh` prints `[rootfs] staged pkgconf 2.3.0 … /bin/pkgconf`
   and later aborts on an unrelated stale ctest fixture.
2. Grepping the log for `pkgconf` finds that line and looks like success. It is
   not — it describes a copy into a temp dir that has since been deleted.
3. The next boot test attaches the **old** image, the new rung self-skips on
   its missing prerequisite, and the run reports **PASS** — with only a
   `PATH-Z COVERAGE INCOMPLETE` note, which is the same note it prints for
   any legitimately-absent artifact.

The failure is quiet in both directions: the rootfs script's error scrolls past
in a long log, and the boot test that follows is *green*. Nothing anywhere says
"you are testing a stale image".

### Why this class is worth naming

It is the same shape as `B-A-MERGE-RESURRECTED-THREE-ARCHIVED-ENTRIES`: a
check that was correct about the moment it ran (the artifact really was staged)
being read later as a claim about a *standing* property (the artifact is on the
image). The intermediate step that invalidates it — the abort, the merge —
leaves no trace at the place the reader looks.

The generalisable rule: **a progress message about an intermediate step must
not be greppable as evidence of the final result.** If a log line can be
mistaken for the outcome, the failure path has to say so explicitly.

### The fix

An `EXIT` trap rather than a message at each `exit 1`, because there are five
today and the sixth would not have remembered:

```bash
IMAGE_WRITTEN=0
_on_exit() {
    local rc=$?
    rm -rf "$STAGE"
    if [ "$rc" -ne 0 ] && [ "$IMAGE_WRITTEN" -eq 0 ]; then
        echo "[rootfs] *** rootfs.ext4 was NOT written — the existing image is UNCHANGED. ***"
        echo "[rootfs]     Any 'staged ...' line above went to a temp dir, not to the image."
        echo "[rootfs]     '[rootfs] DONE.' is the only line that means the image was rebuilt."
        echo "[rootfs]     A boot test run now uses the OLD image and can still report PASS."
    fi
    exit "$rc"
}
trap _on_exit EXIT
```

`IMAGE_WRITTEN=1` is set immediately after `mke2fs`/`debugfs` produce the
image, so the trap fires for every abort before that point and for none after.
It also subsumes the pre-existing `rm -rf "$STAGE"` cleanup trap it replaces.

**The check to use is `[rootfs] DONE.`**, which is the last line and is printed
only on success. `grep 'staged <your artifact>'` is not a check.

### Two gates, not one — worth knowing before rebuilding a fixture

The ctest fixtures are guarded twice, and satisfying one does not satisfy the
other:

| gate | compares | satisfied by |
|---|---|---|
| mtime | ELF mtime vs `build.py` and `libc.a` | any rebuild |
| content stamp | recorded SHA of `build.py` **and** of the ELF vs on-disk | only `scripts/ctest-fixtures.py build` |

Running `python services/ctest-<n>/build.py` directly clears the first and
**fails the second**, because it does not update the stamp. The tool to use is:

```bash
PYTHONPATH="<fastpy>" python scripts/ctest-fixtures.py build
```

which rebuilds and re-stamps together. It needs fastpy on `PYTHONPATH` (the
fixtures import `compiler.toolchain`) and fails with a bare
`ModuleNotFoundError: No module named 'compiler'` without it — that error means
a missing `PYTHONPATH`, not a missing fixture.

Note the stamp gate's own advice, which is right: *do not re-stamp a fixture to
silence it* — re-stamping records the drift rather than removing it.

### Why the fixtures were stale at all

Not a real drift: `d9a02bb62` ("services: build.py usage examples name your
worktree, not `os`") changed comment text in all nine `build.py` files, which
moved their mtimes and their recorded input hashes. The ELFs were semantically
current. Both gates are nonetheless right to fire — neither can tell a comment
change from a flag change, and a gate that guessed would be worthless.
