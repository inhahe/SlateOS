#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-siginfo` SlateOS fixture.

Produces `ctest-siginfo.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld. `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-siginfo.elf`; running it at boot is the kernel's
side -- the generic rung asked for in
`requests/d-a-one-rung-for-every-c-fixture.md`, which runs each fixture
`services/ctest-generic.list` names. It goes on that list once it has passed
in a lane D boot, as the list's header requires. It needs no grant, must run
as an ordinary user (pid 1 is not its to signal), and needs a kernel with
lane A's extended signal frame (`SYS_SIGNAL_REGISTER`'s
`SIGNAL_FRAME_SIGINFO`), `SYS_SIGNAL_QUEUE` and `SYS_SIGNAL_TGKILL`.

WHAT IT IS FOR.  Until 2026-10-07 a native program was told `SI_USER` and pid
0 for every signal that came from outside it -- the kernel's frame carried
the signal's number alone -- `sigqueue` answered `ENOSYS`, and `tgkill` to
another thread arrived as a plain `kill`
(`known-issues/D-POSIX-SIGINFO-FROM-THE-KERNEL-IS-THE-NUMBER-ALONE.md`). The
library reads lane A's extended frame now (`posix/src/signal.rs`,
`Record::from_frame`) and sends through its calls; this checks what a
handler and `sigwaitinfo` are told, against Linux's answers: `raise`, `kill`,
`sigqueue` and `tgkill` of itself, `setitimer`'s `SIGALRM`, a child's `kill`,
`sigqueue` and exit, a child killed by a signal, `tgkill` into another
process, a zombie signalled, and the refusals. The legend of exit codes is
at the top of `main.c`; 42 is every check.

The same source compiles and passes on Linux, run as an ordinary user
(gcc main.c && ./a.out), which is how its expectations were checked to be
Linux's.

Build it with `python scripts/ctest-fixtures.py build --only ctest-siginfo`, from
the root of the worktree you are working in; that script's docstring says
why it, not a direct run of this file, is the entry point.

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
        HERE / "ctest-siginfo.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
