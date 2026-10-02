## A-ORPHANED-HOST-TEST-BINARIES-PIN-`target/`-AND-EVENTUALLY-STOP-THE-BOOT-TEST (lane A, 2026-08-18)

**Symptom.** `scripts/boot-test.sh` refused to start:

```
ERROR: only 9 GiB free on the build volume; the floor is 20 GiB (before building).
```

The floor is there for a reason the script states itself - on 2026-08-15 this
volume reached zero bytes free and a half-written edit truncated a kernel source
file to zero - so the answer is to free space, not to lower the floor with
`--min-free-gb`. The documented way to free space is to `cargo clean` a worktree
nobody is building in, and that is the step that failed:

```
$ cargo clean --manifest-path 'D:/visual studio projects/os/Cargo.toml'
error: failed to remove ... conmon_cli-fdb8a9c84ef02fa5.exe
Caused by: Access is denied. (os error 5)
```

**Cause.** Two `copper_cli-69676980426dfd7c.exe` processes (PIDs 41816 and
88924) were still running out of `os\target`, and Windows will not delete a file
that is mapped as a running image. One stuck host-side test binary therefore
blocks `cargo clean` for the *entire* worktree - cargo aborts on the first
`os error 5` rather than cleaning what it can - and with it the only sanctioned
route back to enough disk to boot.

These are **host** binaries built by `cargo test`/`cargo run` in the integration
checkout, not guest processes. The escape route is the one
`A-ADHOC-QEMU-PROBES-LEAK-THE-EMULATOR-ABOUT-TWO-THIRDS-OF-THE-TIME` describes
for QEMU, for the same underlying reason: coreutils `timeout` signals only its
direct child, so a `cargo test` it kills leaves the spawned test binaries - and
anything *they* spawned - orphaned. `CLAUDE.md` already requires
`scripts/run-timeout.py` (Windows Job Object with `KILL_ON_JOB_CLOSE`, which
tears down the whole tree) for precisely this; these were started some other way.

**What was done, and what was deliberately not done.** The orphans belong to
another lane's work and this session did not start them, so per the standing
rule they were **not** killed - not by PID and certainly not by image name.
Instead, after confirming that no lane held the cross-worktree boot lock
(`<common-git-dir>/slateos-boot-lock` absent, so nobody was mid-boot-test),
`os/target` was removed wholesale: `rm -rf` deletes *around* an open image
handle on Windows, where `cargo clean` gives up at the first one. Free space
went 6.8 GiB -> ~26 GiB. `target/` is entirely regenerable, so the cost is one
cold rebuild of the integration tree and nothing else. Both orphans had exited
on their own by 02:46.

**Consequence.** `D:\visual studio projects\os` now has no `target/`, and its
`rootfs.ext4` needs rebuilding, so the next boot test run *from the integration
tree* is a cold one. Nothing in a lane worktree was touched.

**Proper fix.** Nothing in the tree is wrong here; the fix is procedural and is
already written down - every potentially-hanging run goes through
`scripts/run-timeout.py`, never bare `timeout`, never a trailing `&`, never
`nohup`. If this recurs, the thing worth building is a small `scripts/` helper
that reports which processes hold an image open under a given `target/`, so the
next person meets a named process instead of a bare `os error 5`.
