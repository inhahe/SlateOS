#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-cwd-umask` SlateOS fixture.

Produces `ctest-cwd-umask.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-cwd-umask.elf`; the kernel rung that runs it and
asserts the exit code (42 == all checks passed) is lane A's, asked for in
`requests/d-a-run-the-ctest-cwd-umask-fixture.md`.

## What it is for

That a program's working directory and file-creation mask reach the programs
it starts -- by `fork` + `exec`, by `posix_spawn`, and by `posix_spawn` with a
`chdir` file action (design-decisions.md §960; known-issues.md
`TD-D-CWD-AND-UMASK-DO-NOT-SURVIVE-EXEC-OR-SPAWN`). Every child used to start in
`/` with umask 022.

The host tests in `posix/` cover each piece against a modelled kernel; none of
them can show that the pieces meet in a real kernel, which is the part that
was broken: libc kept both values where a new image could not find them. The
fixture is its own child (`child <dir> <mask>`), so it needs nothing else on
the image.

## Ordering

The rung must not run it before both halves are in one tree: lane A's kernel
half (`ff5f98db8`) and lane D's libc half. Against either alone, cases 1, 2
and 4 exit with code x3 -- a child in the wrong directory -- which is the
defect reproduced, not a new one. Staging it without the rung is harmless.

## It waits, boundedly

The parent blocks in `waitpid` on each child, and each child does nothing but
`getcwd`, `umask`, compare and exit, so a child that runs at all finishes. A
spawn or exec that fails returns an error rather than blocking. The kernel
rung's yield budget is what bounds the whole.

`-fno-builtin` keeps clang from folding anything into a precomputed answer
that never enters the sysroot.  Otherwise the compile flags mirror
`toolchain/x86_64-slateos.json` (static relocation, large code model).

## Build it through `ctest-fixtures.py`

    python scripts/ctest-fixtures.py build --only cwd-umask

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
        HERE / "ctest-cwd-umask.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
