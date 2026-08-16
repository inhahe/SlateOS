#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-longdouble` SlateOS fixture.

Produces `ctest-longdouble.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-longdouble.elf`, and the kernel runs it as a ring-3
self-test (`self_test_clongdouble`, `kernel/src/proc/spawn.rs`) that asserts
the exit code (42 == all checks passed).

It is the third fixture in the sysroot float-ABI family:

    ctest-libc-float   double *returns* and *varargs* doubles
    ctest-libm         *named* float arguments (%xmm0-7 by SSE class)
    ctest-longdouble   `long double`: X87/X87UP -> MEMORY, returned in %st(0)

`long double` is the one floating type that never touches an XMM register, so
neither of the other two fixtures can see a fault in it.  It guards
BUG-POSIX-LONG-DOUBLE-ABI: `printf`/`scanf` silently ignoring the `L` length
modifier (which desynchronised every *later* argument by two stack slots) and
`strtold` returning in %xmm0 where a C caller reads %st(0).

Plain C, deliberately, for the same reason as the other two: both faults are
invisible to the posix crate's own unit tests, because there Rust calls Rust
and the two sides agree on the same wrong convention.  Only a caller built by
a different toolchain — one that believes the real C ABI — can observe them.

`-fno-builtin` keeps clang from constant-folding `snprintf`/`sscanf` calls
with literal arguments into precomputed strings, which would make the fixture
pass without ever entering the sysroot.  `main.c` additionally launders every
input through a volatile global for the same reason.

Otherwise the compile flags mirror `toolchain/x86_64-slateos.json` (static
relocation, large code model).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/ctest-longdouble/build.py

The posix sysroot (`libc.a`) must already be built and must be *current* with
`posix/src/` and with `toolchain/build-sysroot.ps1`'s RUSTFLAGS.
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
        HERE / "ctest-longdouble.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
