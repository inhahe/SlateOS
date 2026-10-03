### [D] TD-D-THE-SYSROOT-FIX-RESTS-ON-A-FLAG-RUSTC-IS-PHASING-OUT — 2026-09-22 — FIXED 2026-09-25

**Status:** FIXED 2026-09-25 (lane D), by the proper fix below, plus one thing
it had not foreseen. `libc.a` is built for `posix/x86_64-slateos-libc.json`,
which is `x86_64-unknown-none` with the hard-float ABI, the large code model
and a static relocation model. It uses `-Zbuild-std=core,compiler_builtins`,
so `core` and `compiler_builtins` are compiled for it too, and the archive no
longer mixes a soft-float `core` into a hard-float libc. The build emits no
soft-float warning. The unforeseen part: the precompiled `compiler_builtins`
carried compiler-rt's C-only builtins (its `c` feature) and a source build does
not. Diffing the two archives with `nm` found 35 missing, among them
`__muldc3`/`__divdc3`, the `-ftrapv` family, `__popcount*` and `__cmp*`.
CMake's binary uses some of them, so they are ported to Rust in
`posix/src/compiler_rt.rs`. After that, the new archive defines every
C-visible symbol the old one did, and all 19 C fixtures link against it with
nothing undefined. The sysroot stamp hashes the spec
(`scripts/ctest-fixtures.py`, one additive line). design-decisions.md §1106
records the alternatives.

*(As filed:)* found while provisioning `os-lane-f`; lane D's (`toolchain/build-sysroot.ps1`).

**What.** The fix for BUG-SYSROOT-SOFT-FLOAT-ABI, directly above, builds `posix`
and the stubs for `x86_64-unknown-none` with `-C
target-feature=+sse,+sse2,-soft-float`: it switches the ABI feature off on a
target whose ABI *is* soft-float. Current rustc accepts that with a warning,
twice per crate, on every sysroot build:

```
warning: target feature `soft-float` cannot be disabled with `-Ctarget-feature`: use a soft-float target instead
  = note: this was previously accepted by the compiler but is being phased out; it will become a hard error in a future release!
  = note: for more information, see issue #116344 <https://github.com/rust-lang/rust/issues/116344>
warning: target feature `soft-float` must be enabled to ensure that the ABI of the current target can be implemented correctly
```

**Why it matters.** On the toolchain update that makes it a hard error,
`build-sysroot.ps1` stops producing `libc.a`, and every C fixture, every
`services/*/build.py` link and the rootfs image stop with it — the whole Path-Z
pipeline breaks on a compiler update rather than on any change of ours, and it
will read as a compiler regression on the day it happens. Nothing currently
reads these warnings: they scroll past inside `bootstrap-worktree.sh`.

**Reproduce.** `bash scripts/bootstrap-worktree.sh` in a fresh worktree (it runs
`toolchain/build-sysroot.ps1`); the warnings are the first lines of the sysroot
step. Seen 2026-09-22 provisioning the new lane worktrees.

**The proper fix.** Build the sysroot for a target whose ABI is hard-float,
rather than toggling the ABI feature on a soft-float one — a target spec (the
repo already carries `toolchain/x86_64-slateos.json`, which is lane A's; check
whether its float ABI fits, or add one for the sysroot) passed with
`--target <spec> -Zbuild-std`. The kernel and the bare-metal `services/`
binaries really are soft-float and keep `x86_64-unknown-none`; only the
userspace libc needs the hard-float ABI. Then rebuild the fixtures and rootfs,
and re-check the `strtod`/`printf("%f")` cases that BUG-SYSROOT-SOFT-FLOAT-ABI
was about.

**Why it is here and not at the end of the file:** it is the continuation of
the entry above, and lane A had a 2,595-line append pending at end of file when
this was written — the collision `open-questions.md` A-Q18 is about.
