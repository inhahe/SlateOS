#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-gcc-runs` SlateOS fixture.

Produces `ctest-gcc-runs.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld. `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-gcc-runs.elf`; running it at boot is the kernel's
side -- the generic rung asked for in
`requests/d-a-one-rung-for-every-c-fixture.md`, which runs each fixture
`services/ctest-generic.list` names. It goes on that list once it has passed
in a lane D boot, as the list's header requires.

WHAT IT IS FOR.  scripts/gcc-spike/ builds GCC 16.2 to run on SlateOS, with
libgcc and libstdc++, and the rootfs recipe stages it under /usr with the C
library's headers, libc.a and binutils' assembler and linker
(design-decisions.md 1185). This runs it there: the drivers start, the
preprocessor expands a macro, and gcc and g++ each compile and link a
program that then runs -- C through the C library's headers, C++ through
libstdc++, an exception and a thread. The legend of exit codes is at the top
of `main.c`; 42 is every check.

The same source runs on Linux against the host's GCC, to check the fixture's
own expectations (main.c's macros say how).

Build it with `python scripts/ctest-fixtures.py build --only ctest-gcc-runs`, from
the root of the worktree you are working in; that script's docstring says
why it, not a direct run of this file, is the entry point.

The posix sysroot (`libc.a`) must already be built and current. The rootfs
must carry GCC, which scripts/gcc-spike/native.sh builds.
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
        HERE / "ctest-gcc-runs.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
