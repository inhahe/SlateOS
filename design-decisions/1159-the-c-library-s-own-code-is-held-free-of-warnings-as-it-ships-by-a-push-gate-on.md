## 1159. The C library's own code is held free of warnings, as it ships, by a push gate on the library's trees -- not by making warnings errors in the crate, and not in gate 40

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** A compiler warning that appears only when the C library is
built for the real machine -- not for the development machine its tests run
on -- went unnoticed for six commits, because every build that prints it
passes anyway. A push that changes the library now has it compiled exactly
as it ships, and is refused if the library's own code draws a warning. The
refusal stops only the push that changes the library; it never fails the
shared build that all six lanes' boot tests depend on.

### Alternative 1: `#![cfg_attr(target_os = "none", deny(warnings))]` in posix

- **For:** nothing to run or wire: every build for the target -- the
  sysroot's, gate 40's, a scratch check -- fails at the source, at once.
- **Against:** the toolchain is an unpinned nightly (`cargo +nightly`), and
  a toolchain update that brings a new warning -- rustc gains lints most
  releases -- would fail `toolchain/build-sysroot.ps1`, and every lane's
  boot test with it, until lane D fixed posix, which only lane D may. A
  warning is a defect to fix, not a reason to stop five other lanes.

### Alternative 2: refuse warnings in gate 40 (`check-pinned-target-build.py`)

- **For:** it already compiles posix for bare metal on every push that
  touches it.
- **Against:** it compiles for posix's pinned `x86_64-unknown-none`, not the
  spec libc.a is built for (`posix/x86_64-slateos-libc.json`: hard-float,
  among the rest); and it judges eight crates in one verdict, lane A's
  `services/netstack` among them, over a `touches` that includes
  `netproto/` -- so a warning in posix would refuse lane A's push.

### Chosen: gate 51, `scripts/check-libc-target-warnings.py`

- It reads `build-sysroot.ps1` for the crates it builds and the settings it
  builds them with (`$sysrootFlags` as RUSTFLAGS, `$buildStd`, `--release`,
  `$spec`, its `$env:` settings), and has no verdict if that script's cargo
  line stops being the one it mirrors -- so the check cannot drift from what
  ships.
- `cargo check --message-format=json`: a warning or error in a crate whose
  manifest is in the tree and not under `vendor/` -- posix, toolchain/stubs,
  tzrules -- is refused; the vendored libm's 32 are counted and printed, not
  refused (upstream's code, kept as upstream wrote it). An error in any
  crate is refused, since the library then does not build.
- Scoped to `posix/`, `tzrules/`, `toolchain/stubs/`,
  `toolchain/build-sysroot.ps1` and itself; ~11 s when posix has changed,
  ~2 s when not. Without a nightly toolchain with rust-src it exits 3 and
  the hook reports it skipped.
- **Against:** a check, not a build, so a lint that fires only at code
  generation is not seen (`large_assignments` and its kind) -- a build of
  posix is minutes where the check is seconds. And a toolchain update that
  adds a warning still refuses lane D's pushes until it is fixed, which is
  the point: the lane that owns the code is the one held to it.

**Where:** `scripts/check-libc-target-warnings.py`; `scripts/hooks/pre-push`
(gate 51).
