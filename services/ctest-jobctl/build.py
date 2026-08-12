#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-jobctl` SlateOS fixture.

Produces `ctest-jobctl.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-jobctl.elf`, and the kernel runs it as a ring-3
self-test (`self_test_jobctl`, `kernel/src/proc/spawn.rs`) that asserts the
exit code (42 == all checks passed).

It covers job-control stop and continue as reached through *our own* libc:
`raise(SIGTSTP)` → `posix/src/signal.rs::stop_self` → `SYS_SIGNAL_STOP_SELF`
(1062) in the child, and `waitpid(..., WUNTRACED | WCONTINUED)` →
`posix/src/process.rs::waitpid` → `SYS_PROCESS_WAIT_STATUS` (1063) in the
parent, plus a real cross-process `kill(child, SIGCONT)`.

Why a ring-3 fixture and not a unit test.  The syscall arm of `stop_self` is
compiled only for `target_os = "none"`, so on the host triple every stop
reports `ENOSYS` and `cargo test` proves only the argument handling.  The
kernel's own dispatch self-test is equally blind: it calls the handlers
directly with synthetic job-control records, and it can only ever pass
`WNOHANG`, because a *real* stop issued from the boot thread would park the
one task left to resume it.  Two real processes in ring 3 are the only way to
observe a child stopping itself and a parent both seeing and undoing it.

Unlike the `ctest-*` float-ABI family, `main.c` needs no hand-written
prototypes: musl's `signal.h`, `unistd.h` and `sys/wait.h` declare everything
it uses, and the `W*` status macros it decodes with are musl's own — so the
encoding the kernel produces is checked against a third party's idea of what
a wait status means, not against our own constants.

Otherwise the compile flags mirror `toolchain/x86_64-slateos.json` (static
relocation, large code model).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/ctest-jobctl/build.py"

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
        HERE / "ctest-jobctl.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
