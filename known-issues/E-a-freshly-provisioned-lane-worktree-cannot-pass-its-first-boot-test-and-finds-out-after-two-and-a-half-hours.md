### [E] A freshly provisioned lane worktree cannot pass its first boot test, and finds out after two and a half hours -- 2026-09-25
**Status:** OPEN -- the fix belongs to `scripts/bootstrap-worktree.sh` (no lane's, A-Q11) and `scripts/boot-test.sh` (lane A): `requests/e-a-rootfs-with-no-manifest-should-fail-in-seconds-not-hours.md`

**In short:** lanes D, E and F got their worktrees on 2026-09-22 through
`scripts/bootstrap-worktree.sh`. It copies a sibling's `rootfs.ext4` -- and not
the manifest that records what was packed into it, not the 78 ring-3 fixture
programs, and not the SlateOS builds of the userland programs the image
carries. The boot test has refused such an image since the manifest check
landed, but it asks only at the *staging* step, after every gate and the whole
build. Lane E's first real boot test ran 8,712 seconds and ended there.

**What happened, measured.** Run 1 on lane E stopped at an unrelated gate
(the script index). Run 2 was stopped by hand at 6,000 s when lane F warned of
the design-decisions band test. Run 3 passed every gate, clippy and the build,
then:

```
=== Verifying rootfs.ext4 matches the built fixtures ===
[ctest] ERROR: rootfs.ext4 exists but rootfs.ext4.manifest does not.
[run-timeout] child exited: FAIL (exit 1), 8712s elapsed
```

The image cannot become valid during a run -- nothing in the boot test writes
a manifest -- so every second before that line was spent on a verdict that was
already decided.

**What provisioning actually takes** (lane E, 2026-09-25):

| step | cost |
|---|---|
| `python scripts/ctest-fixtures.py build` -- 78 fixtures, none had been built | 209 s |
| spike artifacts (bash, pkgconf, make, cmake, CPython): copied from lane A's `build/spike`, whose `libc.a` is byte-identical to this tree's (sha256 `dce695fd...`), then given current mtimes -- the recipe's staleness gate compares mtimes, and `cp -p` had kept lane A's older ones | seconds |
| SlateOS userland: `cd userspace/coreutils && CARGO_UNSTABLE_JSON_TARGET_SPEC=true cargo +nightly build --release -p coreutils -p ar -p kill -p logger -p logrotate` | 267 s (the release profile rebuilds `std` for the target) |
| `wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh` | 157 s |

Each missing piece is refused by name, but one at a time and each only when
reached: the manifest at the end of a boot test; the userland only after the
recipe had staged everything else.

**The proper fix, in two parts:**

1. **Fail in seconds.** `rootfs.ext4` without `rootfs.ext4.manifest` should be
   refused by the boot test's *prerequisite* check, before the gate phase --
   it is a fact about the tree that no step of the run can change. (Lane A's
   `scripts/boot-test.sh`; asked in the request named above.)
2. **Provision completely.** `bootstrap-worktree.sh` should either produce a
   worktree that can boot-test -- build the fixtures and the userland, then
   pack an image with its manifest -- or say in its summary that the copied
   image is unverifiable and print the four commands above. Copying the
   sibling's manifest along with its image would not help: it would record the
   sibling's fixture hashes, and a fresh tree has no fixtures to compare them
   with.
