#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-argp` SlateOS fixture.

Produces `ctest-argp.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-argp.elf`; the kernel rung that runs it and
asserts the exit code (42 == every line glibc's) is lane A's, asked for in
`requests/d-a-one-rung-for-every-c-fixture.md` -- it goes on
`services/ctest-generic.list` with a File grant (it writes its scratch files
in `/tmp`) once that rung is in.

## What it is for

argp (`posix/src/argp.rs`, design-decisions §1163) as a C program meets it:
the parsers are C functions, called through C structures; `argp_error` and
`argp_failure` are variadic, and their assembly trampolines exist only on
this target -- the host tests reach the `va_list` forms underneath and never
the trampolines; the help and usage go to real streams.  This is the program
`posix/tools/oracle/argp_harness.py` runs under glibc 2.39 to make the host
tests' answers, built here against `<argp.h>` and this library: 169 of its
175 scenarios -- each an `argp_parse` or `argp_help` in a child of its own,
its parsers' calls, standard output and standard error caught in files, and
how it ended -- compared, line by line, with glibc's.  The six it leaves out
are the ones this library answers otherwise, by design; the host tests hold
those.

## `main.c` is generated -- do not edit it

The harness writes it, with glibc's lines embedded, from the same scenario
list that writes `posix/src/argp_oracle.txt`:

    python posix/tools/oracle/argp_harness.py

Exit 42 means every line matched; exit 1 means one did not, and it is
printed beside glibc's; exit 3, that `/tmp` had no room for its scratch
files.  It forks one child a scenario and waits for each, and none can
spin: every scenario left ends, under glibc and here.

`-fno-builtin` keeps clang from folding the library's calls, so each one
reaches the sysroot.  Otherwise the compile flags mirror
`toolchain/x86_64-slateos.json` (static relocation, large code model).

## Build it through `ctest-fixtures.py`

    python scripts/ctest-fixtures.py build --only argp

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
        "-I", str(OS_ROOT / "posix" / "include"),  # the overlay: <argp.h> is ours
        "-Wall", "-Wextra", "-Werror",
        str(HERE / "main.c"),
        "-o", str(obj),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
    if result.returncode != 0 or not obj.exists():
        sys.exit(f"C cross-compile failed:\n{result.stdout}\n{result.stderr}")

    exe = toolchain._link_slateos(
        [obj],
        HERE / "ctest-argp.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
