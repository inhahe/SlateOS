#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-keylayout` SlateOS fixture.

Produces `ctest-keylayout.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-keylayout.elf`; running it as a ring-3 self-test is
the kernel's side (`kernel/src/proc/spawn.rs`), which is lane A's tree.

WHAT IT IS FOR.  `SYS_KEYLAYOUT_SET` (1074).  Lane A's gated-dispatch probe
proves the number is registered and that the capability check runs before
argument validation, but it is only ever *refused* — and from a refusal, "the
gate refuses everyone" and "the gate works" are indistinguishable.  This is the
granted half: it runs holding `Rights::SET_KEYLAYOUT`, changes the layout, and
confirms through `/proc/keylayout` rather than by asking the setter.

That round trip is the point.  `localectl` used to "confirm" a keymap by
reading back the file it had itself just written, which agreed every time and
proved nothing; the fix was to make the kernel the single publisher, and this
fixture checks the publisher rather than the writer.

Exit code 42 == set worked, an unregistered name was refused, and the original
layout was restored.  Anything else names the first failing check; the legend
is at the top of `main.c`.  Exit 3 specifically means the kernel has fewer than
two registered layouts, so there was nothing to switch to — reported as
unrunnable rather than as a failure of 1074.

`setkeylayout` is declared in `main.c` rather than included from a header: the
sysroot header set has no home for a call no other system has, and a wrong
prototype becomes a link error here instead of a surprise at runtime, which is
part of what this exercises.  The symbol comes from `posix/src/unistd.rs`.

Otherwise the compile flags mirror `toolchain/x86_64-slateos.json` (static
relocation, large code model) and `services/ctest-zombiewait/build.py`.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/ctest-keylayout/build.py

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
        HERE / "ctest-keylayout.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
