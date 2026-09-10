#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-altstack` SlateOS fixture.

Produces `ctest-altstack.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-altstack.elf`; the kernel rung that runs it and
asserts the exit code (42 == all checks passed) is lane A's, requested in
`requests/b-a-honour-sa-onstack-when-building-the-signal-frame.md`.

## What it is for

`design-decisions.md` §1009 claims that a handler registered with `SA_ONSTACK`
runs on the stack `sigaltstack` registered.  Nothing on the host can check
that: the switch is an assembly thunk gated on `target_os = "none"`, and the
host arm of `run_handler` calls the handler directly because a test may not
move the harness's own stack pointer.  So the claim was true by construction
and unrun — the shape this project keeps meeting, where a link verifies a
symbol surface and not a behaviour.

Plain C rather than Rust for the same reason as the float-ABI family beside
it: the fault being guarded is a *calling convention* one, and Rust calling
Rust agrees with itself even when both sides are wrong.

## It cannot hang

Every signal is raised with `raise()`, which dispatches synchronously
in-process — nothing waits, reads or sleeps.  The worst case is a wrong exit
code.  Deliberate: `ctest-pty` once blocked a boot test for two hours on a
read that could not return.

`-fno-builtin` keeps clang from folding anything into a precomputed answer
that never enters the sysroot.  Otherwise the compile flags mirror
`toolchain/x86_64-slateos.json` (static relocation, large code model).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in.  There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/ctest-altstack/build.py

(fastpy is on `D:` and this repo is on `E:`, so the sibling search fails and
the variable is not optional -- `known-issues.md` ->
`B-CTEST-FIXTURES-CANNOT-FIND-THE-FASTPY-CHECKOUT-AFTER-THE-E-DRIVE-MIGRATION`.)

The posix sysroot (`libc.a`) must already be built and must be *current* with
`posix/src/` -- this fixture in particular tests code added the same day, so a
stale sysroot will fail it for the wrong reason.
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
        HERE / "ctest-altstack.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
