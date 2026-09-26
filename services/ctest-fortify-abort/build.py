#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-fortify-abort` SlateOS fixture.

Produces `ctest-fortify-abort.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-fortify-abort.elf`; the kernel rung that runs it
and asserts the exit code (42 == all checks passed) is lane A's, asked for in
`requests/d-a-run-the-ctest-fortify-abort-fixture.md`.

## What it is for

That a `_FORTIFY_SOURCE` copy which does not fit its object is refused --
glibc's message on stderr, then `abort()` -- before it writes anything, and
that `__read_chk` clamps instead (posix/src/fortify.rs; design-decisions.md
§1105; known-issues.md `TD-D-FORTIFY-MEM-AND-STR-CHK-IGNORE-THE-OBJECT-SIZE`).
The host tests in `posix/` show every copy that fits goes through; only a
process that can die shows the ones that do not, so each overflowing call runs
in a forked child whose stderr is a pipe, and the parent checks how it died.

## It waits, boundedly

The parent blocks in `waitpid` on each of its children -- nineteen since the
wide-character cases were added -- each of which makes one call and then
aborts, or exits at once if the check is missing. The one
read of its own is of a pipe whose writer is already closed. The kernel rung's
yield budget bounds the whole.

`-fno-builtin` is load-bearing here: it keeps clang from recognising the
`__*_chk` calls as builtins and folding or checking them itself, so every call
reaches the sysroot.  Otherwise the compile flags mirror
`toolchain/x86_64-slateos.json` (static relocation, large code model).

## Build it through `ctest-fixtures.py`

    python scripts/ctest-fixtures.py build --only fortify-abort

It finds the fastpy checkout (whose `compiler` package this imports) and
rebuilds the sysroot first when `libc.a` is behind its inputs -- which matters
here, since this fixture tests libc code changed in the same commits.
"""

import subprocess
import sys
from pathlib import Path

from compiler import toolchain

HERE = Path(__file__).resolve().parent
OS_ROOT = HERE.parent.parent
SYSROOT_LIB = OS_ROOT / "toolchain" / "sysroot" / "lib"


def main() -> None:
    zig = toolchain._find_zig_cc()
    if zig is None:
        sys.exit(
            "Cannot find `zig` for the SlateOS C cross-compile. Install zig "
            "(it bundles clang + musl), put it on PATH, or set FASTPY_ZIG."
        )
    if not (SYSROOT_LIB / "libc.a").exists():
        sys.exit(f"Missing sysroot libc.a in {SYSROOT_LIB}; run toolchain/build-sysroot.ps1")

    obj = HERE / "main.o"
    cmd = [
        str(zig), "cc",
        f"--target={toolchain._SLATEOS_ZIG_TARGET}",
        "-c", "-O2",
        "-fno-builtin",           # call the sysroot, don't inline/fold
        "-mcmodel=large",         # match codegen code-model=large
        "-fno-pic", "-fno-pie",   # match relocation-model=static
        "-Wall", "-Wextra", "-Werror",
        str(HERE / "main.c"),
        "-o", str(obj),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
    if result.returncode != 0 or not obj.exists():
        sys.exit(f"C cross-compile failed:\n{result.stdout}\n{result.stderr}")

    exe = toolchain._link_slateos(
        [obj],
        HERE / "ctest-fortify-abort.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
