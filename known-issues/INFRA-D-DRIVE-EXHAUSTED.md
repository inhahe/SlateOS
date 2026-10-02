### INFRA-D-DRIVE-EXHAUSTED. The build drive hit **zero** bytes free mid-session; a file write failed with `ENOSPC` — 2026-08-15 — MITIGATED (10 GB reclaimed), needs a standing policy

**In short.** `D:` (1.9 TB, holds all four OS worktrees) filled completely while
lane C was working. A plain `Write` of a 6 KB markdown file failed with
`ENOSPC: no space left on device`, leaving a **zero-byte file** behind — the
write is not atomic, so a half-applied edit is a real possibility for anything
larger. Any lane can hit this at any moment, and a build or boot test that dies
this way will not say "disk full"; it will say something misleading.

**Evidence.** `df -h /d` reported `1.9T 1.9T 0 100%` at the moment of the failed
write. Free space then oscillated 12 GB → 9.7 GB over ~10 minutes with no cargo
or rustc process running, i.e. something outside the build was still consuming.

**What was reclaimed (safe, zero rebuild cost).** Two dead target triples in the
`os` integration tree, neither referenced by any current config
(`grep -rl ouros .cargo/config.toml toolchain/` is empty — the project was
renamed OurOS → SlateOS months ago):

| Path | Size | Last touched |
|---|---|---|
| `os/target/x86_64-ouros/` | 8.56 GB (31050 files) | 2026-05-31 |
| `os/target/x86_64-pc-windows-msvc/` | 1.60 GB (896 files) | 2026-05-27 |

Free space went 9.7 GB → 18 GB. Measured project footprints for context:
`os` **122 GB**, `os-lane-b` 41 GB, `os-lane-c` 36 GB, `os-lane-a` 4.3 GB —
~203 GB of the drive is our four worktrees, nearly all of it `target/`.

**Not investigated further, deliberately.** `D:\System Volume Information` had
been written minutes before the exhaustion, which points at VSS shadow copies /
System Restore — plausible, since a churning 200 GB build tree is exactly what
makes shadow storage grow without bound. Confirming it needs
`vssadmin list shadowstorage`, which requires elevation, and resizing or
deleting shadow copies destroys the operator's restore points. **That is the
operator's call, not an agent's** — flagged to them rather than acted on. The
operator's own `LithicBackup.exe` was also running (started 03:38, ~16 h).

**Standing hazards this exposes.**

1. **No lane checks free space before a build or boot test.** A full workspace
   build plus a boot test wants several GB; starting one with <5 GB free
   produces a confusing failure rather than a clear one. A `df` guard at the top
   of `scripts/boot-test.sh` that aborts with an explicit message below a
   threshold would convert a mystery into a diagnosis.
2. **Cleaning another lane's `target/` is not safe ad hoc.** A boot test
   (`qemu-system-x86_64.exe`) was running in another lane while this was being
   investigated. Only clearly-dead artifacts — obsolete target triples, caches
   for toolchains no longer in use — can be removed without coordination.
3. **`Write` is not atomic under `ENOSPC`.** The failed write left a zero-byte
   file that had to be removed by hand before retrying. Worth remembering when a
   tool reports a write failure: check the file, do not assume it is untouched.

**Follow-up (lane B, same day).** Lane B hit the same exhaustion from the other
side and reclaimed a further **95 GB** by clearing `os/target` outright — the
integration tree is for merges, not development, so it is the cheapest place to
reclaim from, and it unblocked all three lanes. That pass also answers hazard 2
above concretely (check `Get-CimInstance Win32_Process` for a live QEMU and read
which worktree its command line points at) and turned up a worse consequence
than the failed write: see *A full disk does not fail the build — it corrupts it
silently* immediately below.
