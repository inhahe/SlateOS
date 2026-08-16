#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-pgroup` SlateOS fixture.

Produces `ctest-pgroup.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-pgroup.elf`, and the kernel runs it as a ring-3
self-test (`self_test_cpgroup`, `kernel/src/proc/spawn.rs`) that asserts the
exit code (42 == all checks passed).

It covers process groups and sessions as reached through *our own* libc —
`getpgid`/`setpgid`/`getpgrp`/`setpgrp`/`getsid`/`setsid` in
`posix/src/process.rs` and the non-positive `kill`/`killpg` forms in
`posix/src/signal.rs` — all of which issue native syscalls 533-536 and 523.

Why a ring-3 fixture and not a unit test.  `AbiMode` is per-process, so these
wrappers are the *only* way a native-ABI program can reach the kernel's
process-group state, and the syscall arm of each wrapper is compiled only for
`target_os = "none"`: on the host triple the posix crate answers from a local
test double (`host_pg`) so `cargo test` never issues a raw SYSCALL.  A host
test therefore proves the argument handling and nothing about the wiring, and
the kernel's own dispatch self-test proves the wiring and nothing about libc.
Only a native binary in ring 3 joins the two.  See known-issues.md,
TD-POSIX-PROCESS-GROUPS-ARE-FAKE-FOR-NATIVE-ABI-PROGRAMS.

Unlike the `ctest-*` float-ABI family, `main.c` needs no hand-written
prototypes: musl's `unistd.h` and `signal.h` declare all of these, and the
symbols they name are exactly the ones the sysroot exports.  That is itself
part of the test — a mismatch in signature or symbol name is a link error
here rather than a surprise when a real port (bash's job control) is linked.

Otherwise the compile flags mirror `toolchain/x86_64-slateos.json` (static
relocation, large code model).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/ctest-pgroup/build.py

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
        HERE / "ctest-pgroup.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
