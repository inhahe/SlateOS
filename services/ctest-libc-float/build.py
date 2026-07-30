#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-libc-float` SlateOS fixture.

Produces `ctest-libc-float.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-libc-float.elf`, and the kernel runs it as a
ring-3 self-test (`self_test_clibc_float`, `kernel/src/proc/spawn.rs`) that
asserts the exit code (42 == all checks passed).

Like `ctest-tls-thread` this is plain C *deliberately*: the bug it guards
(BUG-SYSROOT-SOFT-FLOAT-ABI) is a mismatch between the ABI the sysroot was
compiled with and the ABI its callers use, so the test has to be a caller
built by a different toolchain.  A Rust caller compiled from the same
workspace would not prove anything — and LLVM could see through the call
entirely.

The compile flags mirror `toolchain/x86_64-slateos.json` (static relocation,
large code model).  Note that we do *not* pass any `-msse`/`-mno-soft-float`
flag: SSE2 is baseline for x86-64, so `zig cc` already uses the hard-float
SysV ABI.  That is exactly the point — the caller was always right; it was
the sysroot that had to be fixed.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/ctest-libc-float/build.py"

The posix sysroot (`libc.a`) must already be built and must be *current*
with `posix/src/` and with `toolchain/build-sysroot.ps1`'s RUSTFLAGS — the
float ABI this fixture exercises is decided entirely by how that script
compiles the posix crate.
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
        HERE / "ctest-libc-float.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
