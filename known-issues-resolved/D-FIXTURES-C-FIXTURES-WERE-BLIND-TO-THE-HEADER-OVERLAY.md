## D-FIXTURES-C-FIXTURES-WERE-BLIND-TO-THE-HEADER-OVERLAY — a C fixture read as current after an edit to the `posix/include` headers it is compiled against (lane D, 2026-09-30) — **Status: FIXED 2026-09-30**

**In short:** every `services/ctest-*` fixture compiles its `main.c` with
`-I posix/include`, so the overlay's macros and declarations are in its
ELF. The two gates that decide whether a fixture must be rebuilt --
`scripts/ctest-fixtures.py` (`is_stale`, which the pipeline's build step
uses) and `scripts/create-ext4-rootfs.sh` (which refuses to pack a stale
one) -- counted `build.py`, `main.c`, headers beside it and `libc.a`, but
not the overlay. An edit to a header alone moved none of those, so a
fixture went on running what the old header compiled to, and both gates
called it current: the same silent false-green as the stale fixtures of
2026-08-12/13. `ctest-obstack`, whose whole subject is `<obstack.h>`'s
macros, made it plain.

**The fix:** the newest file under `posix/include` is an input of every C
fixture in both gates, as the newest fastpy compiler source is of every
fastpy fixture; `scripts/test-ctest-fixtures.py` checks that a header
newer than the ELF makes it stale, and that a fastpy fixture does not take
the overlay.

**Where:** `scripts/ctest-fixtures.py` (`_inputs`,
`_newest_overlay_header`); `scripts/create-ext4-rootfs.sh`
(`OVERLAY_NEWEST`).
