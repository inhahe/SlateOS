## D-SIXTEEN-C-FIXTURES-FAIL-THEIR-FIRST-RUN-ON-THE-GENERIC-RUNG (lane D, 2026-10-21) — OPEN

**Status:** OPEN

**In short:** lane D's list of C test programs for the boot test's generic
rung (`services/ctest-generic.list`, 49 programs) ran for the first time on
2026-10-21, and 16 of them failed. `main`'s copy of the list is empty, so
`main` runs none of them: these are not regressions against `main`, they are
programs that had never been run where they are listed. Nothing of lane D's
goes to `main` until they pass or come off the list, because the list would
fail every lane's boot test.

**Seen:** boot test of `lane-d @ 5a572fd9d`, 2026-10-21, `SELFTEST_FAIL`,
`ctest-generic: 49 listed -- 33 passed, 16 failed` (serial log
`build/serial-failures/20261021T081649Z-5a572fd9d-rc1.txt`, in the lane D
worktree).

**Why they never ran before:** `491682b8d` (2026-10-16) put the waiting
programs on the list. The only boot since, `e3a353204` on 2026-10-17,
stopped at line 18 of the list ("a grant word this kernel does not know"),
so it ran none of them. The list's own rule -- a program goes on only after
it has passed in a lane D boot -- was not kept for that batch. This entry
was first written as "fails programs `main` passes"; `main` has no rung
that runs `ctest-eintr`, `ctest-sysvipc` or `ctest-system`.

**The sixteen** (code: the program's own exit code, explained at the top of
its `services/<name>/main.c`):

| Program | Code | Cause | State |
|---|---|---|---|
| `ctest-sysvipc` | killed, `#PF` | LLVM miscompile in `msgrcv` (D-LLVM-MACHINELICM-SPECULATES-A-GUARDED-LOAD) | fixed in `01c70b786`, not yet booted |
| `ctest-system` | 9 | no `/bin/sh` in the root: the battery runs with the image at `/mnt` (lane A's `A-THE-BOOT-TEST-BATTERY-STILL-SEES-THE-IMAGE-AT-MNT`), so this program cannot pass on this rung | take off the list until the battery runs on the image |
| `ctest-binutils-runs` | 73 | `size` gave text 54, not 6: our `as` (configured for `x86_64-linux-musl`) writes a 48-byte `.note.gnu.property`, as gas does by default for Linux targets, and Berkeley `size` counts it as text | the expectation is the program's to fix; check the note is the 48 bytes |
| `ctest-eintr` | timeout | blocked in a futex wait from 7580 ticks on, never woken | not diagnosed |
| `ctest-llvm-tools` | 51 | `ld.lld -static ... -lc` returned 262 | not diagnosed |
| `ctest-pi-mutex` | 52 | a low thread ran on ahead of the medium ones | not diagnosed |
| `ctest-mono-runs` | 12 | a probe died on a signal | not diagnosed |
| `ctest-renice` | 23 | reading pid 1's nice: `ESRCH`; the rung's programs run before init, so there may be no pid 1 | not diagnosed |
| `ctest-siginfo` | 61 | one record of three from the child, then `EINTR` | not diagnosed |
| `ctest-tmpfile` | 2 | `mkdir /tmp/ctest-tmpfile`: `EACCES` | not diagnosed |
| `ctest-seals` | 2 | `memfd_create(MFD_ALLOW_SEALING)`: `ENOENT` | not diagnosed |
| `ctest-itimers` | 11 | | not diagnosed |
| `ctest-dumpable` | 39 | | not diagnosed |
| `ctest-thread-masks` | 30 | | not diagnosed |
| `ctest-fifo` | 9 | | not diagnosed |
| `ctest-terminfo` | 10 | | not diagnosed |

**How to work it:** `python scripts/guest.py` runs one program in a guest
booted from the last boot test, with the grants its list line names, in a
minute instead of a boot. Its root is the image, not `/mnt`, so a program
that depends on where the image is mounted behaves differently there.
`target/lane-d-logs/guest-run.sh <name>...` (lane D's worktree) rebuilds the
image, runs the named programs that way and keeps each one's output in
`target/lane-d-logs/guest/`. Since `01c70b786` changed `libc.a`, the image
recipe refuses the userland until `python scripts/build-userland.py` has
relinked it; run that first.

**Next:** one cause per row: the libc, the program, the image recipe, or the
kernel (a request to lane A). Any that cannot be fixed in lane D comes off
the list with a comment saying what it waits on, so that the list holds only
programs that pass, and lane D can publish.
