#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-aio` SlateOS fixture.

Produces `ctest-aio.elf`, a native SlateOS (`x86_64-slateos`) binary compiled
from `main.c` by `zig cc` (clang + musl headers) and linked against the posix
`libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh` stages it at
`/tests/ctest-aio.elf`; the kernel rung that runs it and asserts the exit code
(42 == all checks passed) is lane A's, asked for in
`requests/d-a-run-the-ctest-aio-fixture.md`.

## What it is for

Linux kernel AIO as posix/src/linux_aio_abi.rs has it since 2026-09-26, the
way C reaches it -- through `syscall()`, as libaio does: a file written and
read back through a ring, a vectored read, a pipe (no position), refusals at
submission, a thread asleep in `io_getevents` woken by another's `io_submit`,
a relative timeout, an eventfd signalled on completion, the ring reaped by the
caller itself, and `io_destroy` failing a waiter with `EINVAL`.  The host
tests drive the same code with the transfer and the other thread played by
closures.

## It waits, boundedly

Each waiter is released by the main thread after a 100 ms pause, and checked
not to have finished first.  The kernel rung's budget bounds the whole.

`-fno-builtin` keeps clang from folding library calls itself, so every call
reaches the sysroot.  Otherwise the compile flags mirror
`toolchain/x86_64-slateos.json` (static relocation, large code model).

## Build it through `ctest-fixtures.py`

    python scripts/ctest-fixtures.py build --only aio

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
        HERE / "ctest-aio.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
