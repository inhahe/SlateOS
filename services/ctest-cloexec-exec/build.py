#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-cloexec-exec` SlateOS fixture.

Produces `ctest-cloexec-exec.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld. `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-cloexec-exec.elf` -- where it execs itself, so the
path is in `main.c` -- and running it at boot is the kernel's side: the
generic rung asked for in `requests/d-a-one-rung-for-every-c-fixture.md`,
which runs each fixture `services/ctest-generic.list` names. It goes on that
list once it has passed in a lane D boot, as the list's header requires; it
needs `file` (it execs a program off the image), and a kernel with lane A's
`SYS_PROCESS_SET_EXEC_CLOSE`.

WHAT IT IS FOR.  A native exec used to leave the handles of close-on-exec
descriptors open until the new program exited, so a close-on-exec pipe's
reader saw end-of-file only then -- what `std::process::Command`'s fork
path, an sshd session and the GUI terminal's first spawn wait on
(`requests/b-ad-close-on-exec-does-not-close-on-a-native-exec.md`). The
library now names those handles to the kernel before every exec
(`posix/src/spawn.rs`, `exec_close_list`); this checks the two together: end
of file at the exec, a failed exec keeping them open, and a handle a kept
descriptor shares staying open. The legend of exit codes is at the top of
`main.c`; 42 is every check.

The same source compiles and passes on Linux (gcc main.c && ./a.out), which
is how its expectations were checked to be Linux's.

Build it through `scripts/ctest-fixtures.py`, from the root of the worktree
you are working in:

    python scripts/ctest-fixtures.py build --only ctest-cloexec-exec

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
        HERE / "ctest-cloexec-exec.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
