#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-groups` SlateOS fixture.

Produces `ctest-groups.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-groups.elf`; the kernel rung that runs it and
asserts the exit code (42 == all checks passed) is lane A's generic one, for
which it waits with the other fixtures
(`requests/d-a-one-rung-for-every-c-fixture.md`).

## What it is for

`getgroups` answered "no groups" for every process until 2026-10-06, while
`setgroups` and `initgroups` installed real ones in the kernel.  It now reads
the kernel's list from `/proc/self/status`, which the host tests cannot: they
have no kernel, no `/proc`, and no way to install a group.  Only here does a
process install groups and read them back -- and run under a name that
forges a `Groups:` line, which the library must not believe.

## It cannot hang

Nothing in it waits.  It needs the generic rung's `creds` grant, for
`setgroups` (exit 11 says it was refused), and its `file` grant, for
`/proc/self/status` and for the copy of itself it runs from `/tmp`.

`-fno-builtin` keeps clang from folding anything into a precomputed answer
that never enters the sysroot.  Otherwise the compile flags mirror
`toolchain/x86_64-slateos.json` (static relocation, large code model).

## Build it through `ctest-fixtures.py`

    python scripts/ctest-fixtures.py build --only groups

which finds the fastpy checkout (whose `compiler` package this imports) and
rebuilds the sysroot first when `libc.a` is behind its inputs -- which
matters here, since this fixture tests libc code changed in the same commit.
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
        "-I", str(OS_ROOT / "posix" / "include"),  # the overlay: what musl's headers lack
        "-Wall", "-Wextra", "-Werror",
        str(HERE / "main.c"),
        "-o", str(obj),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
    if result.returncode != 0 or not obj.exists():
        sys.exit(f"C cross-compile failed:\n{result.stdout}\n{result.stderr}")

    exe = toolchain._link_slateos(
        [obj],
        HERE / "ctest-groups.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
