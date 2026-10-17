## 1194. SlateOS's own C interfaces live in `posix/include/slateos/`, held to the library's definitions, not to glibc

**Date:** 2026-10-17 · **Lane:** D · **Decided by:** Claude (autonomous)

**In short:** the C headers in `posix/include/` (§1141) declare only what
glibc 2.39 declares, and a gate refuses anything else, so that a program
written for glibc sees exactly glibc's names. The library also has calls
glibc has nothing like -- the first are the Secure Boot table's
(`secureboot_enroll`, `secureboot_remove`, `secureboot_verify`, for lane A's
fixture and `sbctl`) -- and a C program needs a header for those too. They
go under `posix/include/slateos/` (`#include <slateos/secureboot.h>`): the
same `-I` every build already passes finds them, the prefix says whose
interface it is, as Linux's `<linux/...>` does, and the overlay gate
compiles them with every other header but holds them to no glibc name or
layout. `check-libc-abi.py` still holds their types to the library's Rust
ones, and `check-libc-prototypes.py` their functions to its definitions.

**Alternatives:**

- **The header in the overlay's own namespace (`<sys/secureboot.h>`) with
  the gate told its names one by one.** No new directory; but `sys/` is
  glibc's and POSIX's namespace, a future glibc or a program's own header
  could take the name, and the gate's one rule -- glibc's names and nothing
  else -- would grow a list of exceptions nothing explains at the call site.
- **Each program declares what it calls (`extern` in its own source), as
  `ctest-fortify-abort` does glibc's `__*_chk`.** No header at all; but every
  program then carries its own copy of the declaration and the struct, which
  no gate compares with the library's definition -- exactly the drift
  `check-libc-prototypes.py` exists to catch.
- **A second include directory (`-I posix/include-slateos`).** Keeps the
  overlay directory glibc-only on disk; but every recipe, the sysroot install
  and the fixtures' freshness check would need the second path, for a
  separation the `slateos/` prefix already gives.

**What holds it:** `scripts/check-libc-overlay.py` (`is_native`): headers
under `slateos/` compile alone and with the rest in every setting (check
1), and are left out of the name, type and layout comparisons against
glibc (checks 2-4); its self-test shows a `slateos/` header's function and
struct reaching neither. Each type such a header defines still goes in
`posix/src/abi_layout.rs`, as every overlay type does.
