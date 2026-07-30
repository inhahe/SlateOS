#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-tls-thread` SlateOS fixture.

Produces `ctest-tls-thread.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-tls-thread.elf`, and the kernel runs it as a
ring-3 self-test (`self_test_ctls_thread`, `kernel/src/proc/spawn.rs`) that
asserts the exit code (42 == all checks passed).

Unlike the `fastpy-*` fixtures this is plain C, deliberately: only a C
compiler emits the two constructs the test is about — a `%fs`-relative
`__thread` access and a stack-protector canary load from `%fs:0x28` — in a
*child* thread.

The compile flags mirror `toolchain/x86_64-slateos.json` (static relocation,
large code model) so the object is ABI-compatible with the sysroot, plus
`-fstack-protector-all` to force a canary read into *every* function
(including the thread start routine), which is exactly the access that
faults when a child thread has no thread pointer.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/ctest-tls-thread/build.py"

The posix sysroot (`libc.a`) must already be built and must be *current*
with `posix/src/` — the child-thread TLS setup this fixture exercises lives
in the statically-linked libc, not the kernel.  See
`toolchain/build-sysroot.ps1`.
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
        "-fstack-protector-all",  # force a %fs:0x28 canary read everywhere
        "-Wall", "-Wextra", "-Werror",
        str(HERE / "main.c"),
        "-o", str(obj),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
    if result.returncode != 0 or not obj.exists():
        sys.exit(f"C cross-compile failed:\n{result.stdout}\n{result.stderr}")

    exe = toolchain._link_slateos(
        [obj],
        HERE / "ctest-tls-thread.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
