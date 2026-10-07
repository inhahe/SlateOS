#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-renice` SlateOS fixture.

Produces `ctest-renice.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld. `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-renice.elf`; running it at boot is the kernel's
side -- the generic rung asked for in
`requests/d-a-one-rung-for-every-c-fixture.md`, which runs each fixture
`services/ctest-generic.list` names. It goes on that list once it has passed
in a lane D boot, as the list's header requires. It needs no grant, and must
run *without* the right to raise priority (a Thread capability carrying
IO_REALTIME), on a kernel with lane A's `SYS_PROCESS_GET_PRIORITY` and
`SYS_PROCESS_SET_PRIORITY`.

WHAT IT IS FOR.  `setpriority` -- what `renice` and every task manager call
-- changed the caller in place of the process it named until 2026-09-27, and
then refused every other process, because the native calls could name none
(`requests/e-ad-renicing-another-process-renices-the-caller.md`). It asks
lane A's calls now (`posix/src/resource.rs`, `native_priority`), which take
a process, a group or a user and decide who may; this checks the answers
every program sees, which are Linux's: a parent lowers its child, a raise
beyond `RLIMIT_NICE` is `EACCES`, a stranger `EPERM`, no such process
`ESRCH`, a group reads as its most favoured member, and `nice()` maps a
refused raise to `EPERM`. The legend of exit codes is at the top of
`main.c`; 42 is every check.

The same source compiles and passes on Linux, run as an ordinary user
(gcc main.c && ./a.out), which is how its expectations were checked to be
Linux's.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in (`scripts/lib/worktree.sh` says why
naming another checkout in a command is how a lane builds another lane's
artifact):

    PYTHONPATH="D:/visual studio projects/fastpy" \\
        python services/ctest-renice/build.py

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
        HERE / "ctest-renice.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
