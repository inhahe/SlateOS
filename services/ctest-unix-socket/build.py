#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-unix-socket` SlateOS fixture.

Produces `ctest-unix-socket.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld. `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-unix-socket.elf`; running it at boot is the
kernel's side -- the generic rung asked for in
`requests/d-a-one-rung-for-every-c-fixture.md`, which runs each fixture
`services/ctest-generic.list` names. It goes on that list once it has passed
in a lane D boot, as the list's header requires; it needs `file` (it binds a
path in /tmp) and a kernel with lane A's `SYS_UNIX_*` calls.

WHAT IT IS FOR.  posix/src/unix_socket.rs builds `socket(AF_UNIX, ...)` on
the kernel's Unix-domain sockets. Its host tests check the library's own
arithmetic; this checks the library and the kernel together: a stream server
and a client in two processes with the peer's credentials, datagrams by
abstract name with their senders and their whole lengths, SCM_CREDENTIALS, a
sequenced-packet pair under poll, autobind, SO_RCVTIMEO, and what FIONREAD
counts at each kind of socket and refuses at a listener. The legend of
exit codes is at the top of `main.c`; 42 is every check.

The same source compiles and passes on Linux (gcc main.c && ./a.out), which
is how its expectations were checked to be Linux's.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in (`scripts/lib/worktree.sh` says why
naming another checkout in a command is how a lane builds another lane's
artifact):

    PYTHONPATH="D:/visual studio projects/fastpy" \\
        python services/ctest-unix-socket/build.py

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
        HERE / "ctest-unix-socket.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
