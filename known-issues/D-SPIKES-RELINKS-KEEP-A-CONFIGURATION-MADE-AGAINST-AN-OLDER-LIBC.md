## D-SPIKES-RELINKS-KEEP-A-CONFIGURATION-MADE-AGAINST-AN-OLDER-LIBC (lane D, 2026-10-21) — OPEN

**Status:** OPEN — CMake's fixed on lane-d (`scripts/cmake-spike/configured.sh`); the other ports' relinks still have it.

**In short:** when our C library changes, the image build re-links each ported
program (bash, CPython, LLVM, GDB, binutils, GCC, Mono, Oils, eSpeak NG)
against the new `libc.a` instead of rebuilding it, which takes seconds instead
of minutes. But each port's `configure` step asked our library which functions
it has, and asked our headers what they declare, and the compiled program
encodes those answers. A relink brings in the new library's code and keeps the
old answers: a function the library gained since is still treated as missing,
and a changed header is still the old one. Nothing reports it, because the
relinked binary is newer than `libc.a` and every staleness check passes.

**Where:** each port's `scripts/<port>-spike/slatelink.sh`, run by
`spike_rebuild_if_behind` in `scripts/create-ext4-rootfs.sh`. The ports
configure through `slate_make_link_wrappers` so that configure's link probes
are answered by our `libc.a` (known-issues
`D-SPIKES-PORTS-CONFIGURE-AGAINST-ZIGS-MUSL`) — which is exactly what makes
their objects depend on that `libc.a`'s contents and not only on its headers.

**How CMake's was fixed (2026-10-21):** `run.sh` records a fingerprint of what
it configured against — `libc.a`'s C-visible defined names (Rust's mangled
names excluded: their hashes change with every build of the same code),
`posix/include`, the recipe and the compiler — and `slatelink.sh` compares it
with the tree's before linking: unchanged, it relinks; changed, it hands over
to `run.sh`. The rootfs recipe asks it before every image build, since a header
change stales the configuration without moving `libc.a`. Measured over lane D's
72 libc commits from 2026-10-07 to 2026-10-21, 59 changed neither an exported
name nor a header, so the relink still serves most rebuilds.

**The proper fix for the rest:** the same per port. `cmake_configure_fingerprint`
is CMake-specific only in the recipe it hashes; moved to `scripts/lib/` (lane
A's) with the recipe as an argument, each port's `run.sh` records and each
`slatelink.sh` compares. GCC's and LLVM's configurations are hours to redo, so
for them a mismatch should stop the image with the command to run rather than
start the build.
