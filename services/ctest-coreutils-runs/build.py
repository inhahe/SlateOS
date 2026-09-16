#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-coreutils-runs` SlateOS fixture.

Produces `ctest-coreutils-runs.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-coreutils-runs.elf`; running it as a ring-3
self-test is the kernel's side (`kernel/src/proc/spawn.rs`), which is lane A's
tree.

WHAT IT IS FOR.  The oldest open claim in this lane: **staged is not run.**
`create-ext4-rootfs.sh` places 71 Rust binaries from `userspace/coreutils` into
`/bin`, and not one of them has ever been executed under SlateOS by anything.
`requests/b-a-our-own-utilities-are-on-the-image-now-can-a-boot-test-run-one.md`
has been open since 2026-09-13 asking for exactly this.

Compiling, linking, being staged and RUNNING are four different claims. Only
the first three have evidence, and until this fixture passes, "SlateOS has 89
commands" means "89 files are present".

WHY FOUR BINARIES RATHER THAN ONE.  A single probe answers "did something
run", which is the least useful question. These four each add one capability
to the one before, so the boundary between the last pass and the first failure
is the finding:

    /bin/true      exec, run, exit 0 -- no argv, no output, no libc beyond
                   start-up and exit
    /bin/false     the same, exiting 1. With `true` this proves the exit STATUS
                   is carried rather than that a process merely ended -- a
                   `wait` that always reported 0 passes `true` alone, and every
                   shell script on the system reads that value
    /bin/echo      argv reaches the program and its stdout reaches a pipe
    /bin/basename  the first that COMPUTES: `/usr/lib/x.so` in, `x.so` out

The output comparison is EXACT rather than a substring. `echo hi` must produce
exactly `hi\n`; a substring test would pass on a program printing a usage
message containing the word, and a usage message is precisely what a broken
argv handler prints.

Exit code 42 == all four ran and answered correctly. Anything else names the
first failing check; the legend is at the top of `main.c`. Codes 1, 2 and 9 are
this fixture's own plumbing failing and are NOT findings about the utilities --
kept distinct so a broken pipe here is never read as a broken userland.

Bounds are structural (a non-blocking read gated on `poll`, a counted spin with
`sched_yield`) and there is deliberately no `alarm`: a fixture's bounds should
not depend on a subsystem other than the one under test, or a timer regression
surfaces here as a userland failure and points at the wrong place.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/ctest-coreutils-runs/build.py

The posix sysroot (`libc.a`) must already be built and must be *current* with
`posix/src/` and with `toolchain/build-sysroot.ps1`'s RUSTFLAGS. The rootfs
must also carry the four binaries this execs; all four are in
`scripts/rootfs-bin-manifest.txt`.
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
        HERE / "ctest-coreutils-runs.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
