### B-CTEST-FIXTURES-LINKED-A-STALE-LIBC. Eight of the nine native C ring-3 fixtures were testing a libc.a that is no longer in the build — 2026-08-13 — ✅ FIXED 2026-08-13 (rebuilt; the check is now fatal)

**Symptom.** A `scripts/create-ext4-rootfs.sh` run printed:

```
[rootfs] WARNING: ctest-fortify.elf is OLDER than the sysroot libc.a — it links a stale
[rootfs]          libc and proves nothing about the current one. Rebuild it: ...
   (×8: fortify, jobctl, libc-float, libm, longdouble, pgroup, scanf, tls-thread)
[rootfs] WARNING: 8 of them are stale (see above)
```

`toolchain/sysroot/lib/libc.a` was rebuilt at 16:51 on 2026-08-12; eight of the
nine `services/ctest-*/*.elf` fixtures dated from 14:48 that day. Each of them
*statically* links libc.a, so for every boot since they had been asserting
things about a two-hours-old library. That the rebuilt ELFs each grew by ~90 KiB
shows the library really had moved underneath them — this was not a
timestamp-only staleness.

**Why it went unnoticed.** The detection already existed and worked correctly;
the problem was purely that it *warned*. The warning is printed once, at
image-build time, in the middle of a few hundred lines of staging output, and
then the boot goes green. Downstream — the boot log, `boot-test.sh`'s verdict,
the exit code — carries no trace of it. Exactly the shape of
`B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` (found the same day): the check is
sound, the *reporting channel* is one nobody reads. It had already cost a boot
cycle once before, on 2026-08-12, when `ctest-jobctl` reported
`raise(SIGTSTP) failed` only because its ELF predated `SYS_SIGNAL_STOP_SELF`
existing in libc at all — the script's own comment records that incident.

**Fix.**
1. Rebuilt all eight against the current sysroot
   (`PYTHONPATH=<fastpy> python services/ctest-<name>/build.py`).
2. `create-ext4-rootfs.sh` now **exits 1** on a nonzero stale count instead of
   warning, with `ALLOW_STALE_FIXTURES=1` as the documented escape hatch for a
   host that has the sysroot but not zig. Staleness is fully detectable at image
   build time and trivially fixable there, so that is the right place to stop.

**Generalisation worth keeping.** Twice in one day the bug was not a wrong check
but a check whose result went somewhere no one looks. When adding a
correctness check, decide where its *negative* result surfaces, and make that
place one that a routine `boot-test.sh` run cannot ignore — a nonzero exit, or a
line the harness greps for. A warning in a build log is not a reporting channel.
