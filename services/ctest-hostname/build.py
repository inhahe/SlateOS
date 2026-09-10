#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-hostname` SlateOS fixture.

Produces `ctest-hostname.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-hostname.elf`; the kernel rung that runs it and
asserts the exit code (42 == all checks passed) is lane A's.

## What it is for

That the system has exactly one name, and that setting it is a checked
operation which actually stores something.

Nothing on the host can check either.  Before 2026-09-09 `sethostname` wrote a
process-local buffer and `gethostname` read that same buffer back; the
round-trip test passed for the whole life of the defect, because a round trip
through one buffer is evidence about the buffer.  `setdomainname` kept the
entire defect a commit longer, under a commit message that claimed both were
fixed.  The property that catches this is not "does the value come back" — it
is "do all the ways of asking agree" — and that needs a running kernel with a
real procfs and a caller holding `(Process, SET_HOSTNAME)`.

Plain C rather than Rust for the same reason as the fixtures beside it: the
call crosses an ABI boundary into our libc, and Rust calling Rust agrees with
itself even when both sides are wrong.  `struct utsname` in check 8 is read
through musl's headers here, which is the layout a ported program will use.

## Ordering — this fixture must not reach `main` before the grant does

The only place in the tree granting `(Process, SET_HOSTNAME)` is lane A's
`self_test_ctest_hostname` in `kernel/src/proc/spawn.rs`.  If this fixture is
staged before that grant is on `main`, lane A's rung runs it unprivileged, it
exits 4, and the boot test is red for all three lanes — the exact outcome the
fixture was held back to avoid, with the halves in the other order.  Lane B
also has to wire `posix`'s `sethostname`/`setdomainname` to `SYS_HOSTNAME_SET`
(1072) / `SYS_DOMAINNAME_SET` (1073) first, or it exits 3.

## It cannot hang

Nothing waits, sleeps, or reads from anything with a writer.  The two `/proc`
reads are of generated nodes, which materialise their contents at `open` and
return them synchronously; there is no loop around either `read`, so a short
or failed one is a failed check rather than a spin.  `ctest-pty` once cost the
kernel lane two hours by blocking a boot test on a read that could not return.

`-fno-builtin` keeps clang from folding anything into a precomputed answer
that never enters the sysroot.  Otherwise the compile flags mirror
`toolchain/x86_64-slateos.json` (static relocation, large code model).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in.  There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/ctest-hostname/build.py

(fastpy is on `D:` and this repo is on `E:`, so the sibling search fails and
the variable is not optional -- `known-issues.md` ->
`B-CTEST-FIXTURES-CANNOT-FIND-THE-FASTPY-CHECKOUT-AFTER-THE-E-DRIVE-MIGRATION`.)

The posix sysroot (`libc.a`) must already be built and must be *current* with
`posix/src/` -- this fixture in particular tests code being changed the same
day, so a stale sysroot will fail it for the wrong reason.
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
        HERE / "ctest-hostname.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
