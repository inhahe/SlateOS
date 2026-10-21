## D-LANE-D-FAILS-SIXTEEN-C-FIXTURES-THAT-MAIN-PASSES (lane D, 2026-10-21) — OPEN

**Status:** OPEN

**In short:** lane D's branch, booted with everything rebuilt from it, fails 16
of the C test programs the boot test runs, which `main` passes. The kernel is
the same as `main`'s, so the cause is lane D's own work that is not on `main`
yet: the C library (`posix/`) and the services, about 27,900 added lines.
Nothing of lane D's goes to `main` until they pass.

**Seen:** boot test of `lane-d @ 5a572fd9d` on 2026-10-21, `SELFTEST_FAIL`
(serial log `build/serial-failures/20261021T081649Z-5a572fd9d-rc1.txt`, in
the lane D worktree). Failing, with their first words: `ctest-eintr` (still
running at its 120 s deadline), `ctest-sysvipc` (exit -8), `ctest-llvm-tools`
(51: the tool did not run), `ctest-pi-mutex` (52), `ctest-system` (9),
`ctest-mono-runs` (12), `ctest-binutils-runs` (73: output lacked its line),
`ctest-renice` (23: reading pid 1's nice, errno 3), `ctest-siginfo` (61:
errno 4), `ctest-tmpfile` (2: `mkdir /tmp/ctest-tmpfile`, errno 13),
`ctest-seals` (2: `memfd_create`, errno 2), `ctest-itimers` (11),
`ctest-dumpable` (39), `ctest-thread-masks` (30), `ctest-fifo` (9),
`ctest-terminfo` (10). The failures are spread through the run between passing
programs, not a cascade from one point.

**Why it is not the move to the new PC:**

* `git diff c85e42859 5a572fd9d -- kernel/` is empty: lane-d's kernel is
  `main`'s, and `main` (`c85e42859`) passed its boot test on this PC.
* Lane B's boot of 2026-10-21 06:34 passed with an image built on this PC
  (01:46), so the PC's toolchain makes images that boot clean.
* `ctest-eintr`, `ctest-sysvipc` and `ctest-system` exist on `main` and pass
  there; the others are lane D's newer programs for newer libc features.
* lane-d's last clean boot was `5d460cd49` (2026-10-08). Its boot of
  `e3a353204` on 2026-10-17, on the old PC, failed 2 (`ctest-generic`,
  `ctest-pgroup`), with an image built before the libc commits of 10-17
  onward -- so no boot had run this C library before.

**Candidates, from the libc commits since 10-08** (`git log 5d460cd49..lane-d
-- posix/src`): the signal work (each thread its own mask; signals 32 and 33
the library's and SIGRTMIN 34; a default-action death ending the process as
killed by the signal), the setuid family over the kernel's four ids, named
pipes, SIGEV_THREAD timers, `mmap` of a file through `SYS_MMAP_FILE` (and
`msync` through `SYS_MSYNC`, which the kernel does not define), and init's
ServiceManager. `ctest-tmpfile`'s EACCES and `ctest-renice`'s ESRCH for pid 1
point at credentials or namespaces rather than signals.

**Next:** bisect by boot over `5d460cd49..lane-d`'s `posix/` and
`services/init` commits, reading each failing program's own codes in
`services/<name>/`.
