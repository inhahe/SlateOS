#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-zombiewait` SlateOS fixture.

Produces `ctest-zombiewait.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-zombiewait.elf`; running it as a ring-3 self-test
is the kernel's side (`kernel/src/proc/spawn.rs`), which is lane A's tree.

WHAT IT IS FOR.  B-FORKEXEC-BOOT-HANG.  A boot on 2026-09-15 reproduced the
documented signature on `spawn-test-dash-statpath` rather than on `forkexec`,
and statpath does one stat and exits — so whatever idles is not specific to
fork+exec.  statpath still loads ld.so and dash, though, so it does not
separate the loader from the reap path.  This fixture removes both: static,
no execve, no shell.  It forks, the child exits, the parent waits.

That makes it a two-probe rather than another occurrence.  If it hangs, the
exec path is exonerated and the search collapses to the wait/reap wakeup.  If
it never hangs, the loader is implicated.  Both answers are worth having,
which is the reason to build it rather than wait.

Exit code 42 == both orderings (already-zombie, and waiter-blocked-first)
reached a zombie and reaped it with the right pid and status.  Anything else
names the first failing check; the legend is at the top of `main.c`.  A HANG
is the finding and the fixture deliberately carries no alarm of its own — the
kernel's bounded yield budget is what should notice, and a self-imposed
timeout would hide the thing this was built to show.

Unlike the `ctest-*` float-ABI family, `main.c` needs no hand-written
prototypes: musl's `unistd.h` and `sys/wait.h` declare `pipe`, `fork`, `read`,
`write`, `close`, `_exit` and `wait`, and the symbols they name are exactly
the ones the sysroot exports.  That is part of the test — a mismatch in
signature or symbol name is a link error here rather than a surprise later.

Otherwise the compile flags mirror `toolchain/x86_64-slateos.json` (static
relocation, large code model) and `services/ctest-pgroup/build.py`.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/ctest-zombiewait/build.py

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
        HERE / "ctest-zombiewait.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
