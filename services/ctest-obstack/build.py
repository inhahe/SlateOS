#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-obstack` SlateOS fixture.

Produces `ctest-obstack.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-obstack.elf`; the kernel rung that runs it and
asserts the exit code (42 == every line glibc's) is lane A's, asked for in
`requests/d-a-one-rung-for-every-c-fixture.md`.

## What it is for

GNU obstacks are mostly macros: `obstack_grow`, `obstack_1grow`,
`obstack_finish`, `obstack_blank`, `obstack_free` and the rest are written out
by `posix/include/obstack.h` into the program, and only reach the library
(`posix/src/obstack.rs`) when a chunk runs out.  The host tests replay glibc's
answers against the library's functions, but they are Rust, so the header's
macros are exactly what they cannot run.  This is the same program
`posix/tools/oracle/obstack_harness.py` runs under glibc 2.39 to make those
answers, built here against our header and our library: twenty-five scenarios
of operations on an obstack, each followed by its state (object size, room,
offsets in the chunk, the chunk chain, the sizes the allocation function was
asked for); `obstack_printf`, whose assembly trampoline exists only on this
target; and the default allocation-failure handler, in a forked child, which
must print glibc's message and exit with `obstack_exit_failure`.

## `main.c` is generated -- do not edit it

The harness writes it, with glibc's lines embedded, from the same scenario
list that writes `posix/src/obstack_oracle.txt`, so the fixture and the host
tests cannot ask different questions:

    python posix/tools/oracle/obstack_harness.py

The fixture collects its lines in memory and compares them, in order, with
glibc's.  Exit 42 means every one matched; exit 1 means one did not, and the
first such line is printed on standard output beside glibc's.  Exit 2 is
its one check that is not glibc's: a failure handler that returns -- which
it must not, and after which glibc's code faults -- did not end the process
as `abort()` does (design-decisions §1162).  It waits only for its two
forked children, each of which ends as soon as its allocation fails; it
never spins.  It opens no files.

`-fno-builtin` keeps clang from expanding the `memcpy`/`memset` calls the
macros make, so every call reaches the sysroot.  Otherwise the compile flags
mirror `toolchain/x86_64-slateos.json` (static relocation, large code model).

## Build it through `ctest-fixtures.py`

    python scripts/ctest-fixtures.py build --only obstack

It finds the fastpy checkout (whose `compiler` package this imports) and
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
        "-I", str(OS_ROOT / "posix" / "include"),  # the overlay: <obstack.h> is ours
        "-Wall", "-Wextra", "-Werror",
        str(HERE / "main.c"),
        "-o", str(obj),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
    if result.returncode != 0 or not obj.exists():
        sys.exit(f"C cross-compile failed:\n{result.stdout}\n{result.stderr}")

    exe = toolchain._link_slateos(
        [obj],
        HERE / "ctest-obstack.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
