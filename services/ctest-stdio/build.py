#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-stdio` SlateOS fixture.

Produces `ctest-stdio.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-stdio.elf`; the kernel rung that runs it and
asserts the exit code (42 == all checks passed) is lane A's, asked for in
`requests/d-a-run-the-ctest-stdio-fixture.md`.

## What it is for

The C library's streams over real file descriptors, which the host tests in
`posix/` cannot reach (they drive the buffering through in-memory streams,
and every descriptor call on the host is a stub): the behaviour
posix/src/stdio.rs's 2026-09-27 rewrite changed -- `fopen`'s `x` and `e`,
a hundred streams at once, `fread` from a pipe that delivers in pieces,
`fdopen`'s checks, `a` starting at the end, writing a read-only stream,
a write after a read on `r+`, two threads on one stream, a long `getline`,
`freopen(NULL, ...)`, and `popen`/`pclose` where the process has a
`/bin/sh`.  `main.c`'s header lists each check's exit code.

## It waits, boundedly

The pipe check forks a writer that delivers 8000 bytes in eight pieces and
waits for it; the thread check joins both threads.  The kernel rung's yield
budget bounds the whole.

`-fno-builtin` keeps clang from folding the stdio calls itself, so every
call reaches the sysroot.  Otherwise the compile flags mirror
`toolchain/x86_64-slateos.json` (static relocation, large code model).

## Build it through `ctest-fixtures.py`

    python scripts/ctest-fixtures.py build --only stdio

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
        HERE / "ctest-stdio.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
