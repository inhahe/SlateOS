#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-seals` SlateOS fixture.

Produces `ctest-seals.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld. `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-seals.elf`; running it at boot is the kernel's
side -- the generic rung asked for in
`requests/d-a-one-rung-for-every-c-fixture.md`, which runs each fixture
`services/ctest-generic.list` names. It goes on that list once it has passed
in a lane D boot, as the list's header requires. It needs `file` (its memfds
are files the kernel keeps), and a kernel with lane A's `SYS_FS_ADD_SEALS`
and `SYS_FS_GET_SEALS`.

WHAT IT IS FOR.  A program that hands another process an in-memory file
seals it first, so that what the receiver checked is what it reads -- a
Wayland compositor and its clients do exactly this. Until 2026-10-07 this
library answered `EINVAL` to `F_ADD_SEALS` and `F_GET_SEALS`, as Linux does
for a file that cannot be sealed; it asks lane A's calls now
(`posix/src/fcntl_ops.rs`, `add_seals`), and `memfd_create` starts a memfd
sealed as Linux does (`posix/src/mman.rs`, `initial_memfd_seals`). This
checks what a program sees, which is Linux's: a write refused under
`F_SEAL_WRITE`, the size held by `F_SEAL_SHRINK` and `F_SEAL_GROW`, a memfd
made without `MFD_ALLOW_SEALING` starting with `F_SEAL_SEAL`, `MFD_NOEXEC_SEAL`
starting with `F_SEAL_EXEC`, and the refusals. A truncate under
`F_SEAL_WRITE` alone is not asked: Linux allows it, this kernel does not
(lane A's design-decisions 1526). The legend of exit codes is at the top of
`main.c`; 42 is every check.

The same source compiles and passes on Linux (gcc main.c && ./a.out), which
is how its expectations were checked to be Linux's.

Build it through `scripts/ctest-fixtures.py`, from the root of the worktree
you are working in:

    python scripts/ctest-fixtures.py build --only ctest-seals

That finds the fastpy checkout and hands fastpy this worktree's sysroot.
Running this file directly works if `compiler` is importable, but fastpy
then finds the sysroot by itself, and its fallback is the `libc.a` of a
sibling checkout named `os` -- the integration tree, not this one.

The posix sysroot (`libc.a`) must already be built and current.
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
        HERE / "ctest-seals.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
