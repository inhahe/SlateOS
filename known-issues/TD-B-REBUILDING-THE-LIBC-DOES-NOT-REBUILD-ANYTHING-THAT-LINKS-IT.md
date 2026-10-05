## TD-B-REBUILDING-THE-LIBC-DOES-NOT-REBUILD-ANYTHING-THAT-LINKS-IT (lane B, 2026-09-13)

**In short:** fix a bug in our C library, rebuild the library, rebuild the
programs — and the programs still contain the bug. Cargo does not know that
`toolchain/sysroot/lib/libc.a` is an input, so nothing that links it is out of
date when it changes. The build says `Finished` and everything is stale.

### Measured, not inferred

After fixing `retrieve_initial_args` and rebuilding the sysroot (`libc.a` went
from 2026-09-12 08:11 to 2026-09-13 20:57), I rebuilt the 70 binaries the image
manifest names:

| step | result |
|---|---|
| `rm` the 70 output binaries, then `cargo build` | **1.23 s**, 0 of 70 newer than `libc.a` |
| `touch` the crate sources, then `cargo build` | 42 s, **70 of 70** newer |

Deleting the outputs does not help, and that is the part that misleads. These
files are **hardlinks** — `ls -la` shows a link count of 2 — into
`target/.../release/deps/`. Removing `release/cp` removes one name for an
inode that still exists under `deps/`, so cargo re-creates the link from its
cache without running the linker, and the restored file keeps its **original
mtime**. The obvious way to force a relink is therefore indistinguishable from
having done nothing, right down to the timestamp.

### Why it matters here more than in a normal Rust project

Nothing else in this tree links a hand-built static archive. `libc.a` is built
by `toolchain/build-sysroot.ps1`, **by hand**, outside cargo entirely — so the
one artifact every userspace binary depends on is the one artifact cargo cannot
see. A libc fix that is committed, tested and merged still ships nothing until
someone happens to dirty each dependent crate.

### What catches it

The staleness check in the `create-ext4-rootfs.sh` staging block, which
compares every staged binary against `libc.a` and warns when the binary is
older. That check was written the same day as a matter of routine, copying the
CMake one; this is what makes it load-bearing rather than decorative. Confirmed
on real data: immediately after the sysroot rebuild, all 276 built binaries
were older than `libc.a` and every one would have been reported.

### The proper fix, not done here

**FIXED 2026-09-14, and it was three crates rather than ~200.** The estimate
below counted every crate under `userspace/`; what actually ships is the
`scripts/rootfs-bin-manifest.txt` list, and **70 of its 72 binaries live in one
crate** (`coreutils`). The other two are `ar` and `logrotate`. Measuring the
scope before starting turned a change nobody wanted to make into one that took
a tick.

`userspace/sysroot-dep` holds the logic; each of the three crates has a
three-line `build.rs` calling `sysroot_dep::emit()`. It is a
**build-dependency**, not a normal one, and that is the whole design: a shared
*library* crate would not work, because cargo would re-run ITS build script,
find the output unchanged and leave the dependents alone. The
`cargo:rerun-if-changed` has to be emitted by the build script of the crate
whose rebuild it governs.

`rerun-if-changed` alone is also not enough — it only re-runs the script. The
script therefore emits `cargo:rustc-env=SYSROOT_LIBC_FINGERPRINT=<mtime>.<len>`
so the crate's own compilation input changes when the archive does. Without
that second half the fix is a no-op that looks correct.

**Verified three ways**, because one direction would not have been enough:

| | result |
|---|---|
| touch `libc.a`, rebuild | `cp` **relinks** (was: unchanged) |
| rebuild again, touching nothing | `cp` unchanged, 0.75 s — it has not made every build rebuild the world |
| touch `libc.a`, build for the **host** target | `cp.exe` unchanged — the host links its own libc and must not depend on this archive |

**And verified at scale, which the three above do not cover.** The three
checks are all about one binary. After the `libc.a` touches those measurements
required, the whole image manifest was left genuinely stale, and a single
ordinary rebuild of the three crates cleared it:

| | |
|---|---|
| before | **70 of 72** manifest binaries older than `libc.a` |
| `cargo +nightly build --release` in the three crates | 37 s, 3 s, 2 s |
| after | **0 of 72** |

Before today that same rebuild left all 70 stale and reported `Finished`. This
also confirms the precondition for making the staging gate fatal: the refusal
is satisfiable by the command the gate prints, for every binary on the image,
in about forty seconds.

### What is deliberately NOT covered, and why

The three crates fixed are the ones holding the image's binaries. Measured, the
rest of the tree is larger than that:

| | count |
|---|---|
| `userspace/` crates producing binaries | **194** |
| of those, already having a `build.rs` | **9** |

**The image is protected without touching the other 191**, and that is the
whole argument: `create-ext4-rootfs.sh` now refuses to build an image from a
binary older than `libc.a`, and the boot test runs the image. A stale binary
that is not on the image is a developer-build annoyance, not something that can
ship or be tested against.

So extending this is optional, and it is not free. The nine crates that already
have a `build.rs` would need their scripts **merged**, not replaced — and that
is not a hypothetical hazard. It is exactly how this fix broke `coreutils`:
`cat > build.rs` in a loop over three crates destroyed the one that already had
a script, which emitted the bare-metal linker script every binary in that crate
is laid out by. Two commits and 70 rebuilt binaries passed before the baseline
comment on an unrelated gate led back to it.

If the other 191 are ever done, the merge cases are the whole of the risk and
should be done by hand and read individually, not by the loop that does the 185
safe ones.

The original proper-fix note follows; the part about ~200 crates is what was
wrong with it.

A `build.rs` in the crates that link the sysroot, emitting

    cargo:rerun-if-changed=<path to sysroot>/lib/libc.a

which is how cargo is told about an input it cannot infer. That is ~200 crates
to touch, or one shared build-script crate they all depend on, and it wants
thinking about rather than a quick loop — the archive path is set by the target
JSON and the sysroot location is not currently exported to build scripts. Until
then the rootfs warning is the backstop, and it only fires at image-build time,
which is late but is at least before the bytes reach a disk.
