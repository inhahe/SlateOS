#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-string` SlateOS fixture.

Produces `ctest-string.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-string.elf`; the kernel rung that runs it and
asserts the exit code (42 == all checks passed) is lane A's generic one, for
which it waits with the other fixtures
(`requests/d-a-one-rung-for-every-c-fixture.md`).

## What it is for

The C library's memory and string functions became SSE2 and `rep movsb` on
2026-10-06, and the string scanners read whole aligned 16-byte blocks --
past a string's terminator, never past its page.  The host tests prove the
logic, with guard pages of the host's 4 KiB; only here do SlateOS's 16 KiB
pages, its kernel and its large code model answer.  It checks every length
to 80 at every alignment against byte loops, strings set against a page
that faults (mmap, then the neighbours taken away), copies big enough for
`rep movsb`, overlapping moves both ways, the searches against trying every
place, and the wide functions.

## It cannot hang

Nothing in it waits, and it needs no capability.

`-fno-builtin` keeps clang from folding anything into a precomputed answer
that never enters the sysroot -- and keeps the reference loops loops.
Otherwise the compile flags mirror `toolchain/x86_64-slateos.json` (static
relocation, large code model).

## Build it through `ctest-fixtures.py`

    python scripts/ctest-fixtures.py build --only string

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
        HERE / "ctest-string.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
