#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-uts-namespace` SlateOS fixture.

Produces `ctest-uts-namespace.elf`, a native SlateOS (`x86_64-slateos`)
binary compiled from `main.c` by `zig cc` (clang + musl headers) and linked
against the posix `libc.a` sysroot with rust-lld.
`scripts/create-ext4-rootfs.sh` stages it at `/tests/ctest-uts-namespace.elf`;
lane A's generic rung runs it and asserts the exit code (42 == all checks
passed), listed in `services/ctest-generic.list`.

## What it is for

Until 2026-10-17 `unshare` and `setns` answered `ENOSYS`, and
`/proc/<pid>/ns/<kind>` opened as nothing. Lane A's kernel has UTS
namespaces and six native calls for them, and the library opens a
namespace's link into a descriptor of its own kind -- `fstat`, `readlink`
and `NS_GET_NSTYPE` answered by the kernel, no read, write or seek -- and
passes `unshare` and `setns` to the kernel. The rung grants no right over
namespaces, so the changes are checked as refusals, in Linux's order: what a
non-root process sees on Linux, where this passes too (run as a user).
`main.c`'s header numbers the checks.

## It cannot hang

Nothing in it waits, and `alarm(20)` ends the test whatever it is doing.

`-fno-builtin` keeps clang from folding anything into a precomputed answer
that never enters the sysroot.  Otherwise the compile flags mirror
`toolchain/x86_64-slateos.json` (static relocation, large code model).

## Build it through `ctest-fixtures.py`

    python scripts/ctest-fixtures.py build --only uts-namespace

which finds the fastpy checkout (whose `compiler` package this imports) and
rebuilds the sysroot first when `libc.a` is behind its inputs.
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
        HERE / "ctest-uts-namespace.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
