#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-python-repl` SlateOS fixture.

Produces `ctest-python-repl.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-python-repl.elf`; running it as a ring-3 self-test
is the kernel's side (`kernel/src/proc/spawn.rs`), which is lane A's tree.

WHAT IT IS FOR.  `roadmap.md` has said for over a week that the CPython
initiative's remaining work is one sentence: *"Nobody has ever run it
interactively. That is the actual state, and no measurement here replaces
doing so."*  This is that measurement, automated so it can be repeated.

`self_test_cpython_on_slateos_libc` already proves the interpreter starts,
imports `encodings` out of the 20 MiB archive, and writes byte-exact output —
with stdout on a pipe.  It says nothing about a terminal.  Between it and here
lie `isatty` answering true on a pty slave, CPython taking its interactive
branch because of it, the line discipline assembling a typed line and
delivering it on ENTER, and the prompt traffic coming back.  Those are the
parts a `-c` run cannot reach and the parts a user meets first.

THE EXPRESSION IS `6*7`, and the reason is the only subtle thing here.  A pty
echoes what is typed, so the master sees the expression before the answer.
With `print(1+1)` the answer `2` also appears inside the echo, so a scan for it
matches the echo and passes without the interpreter having evaluated anything —
the same shape as a test that reads back its own writes.  `6*7` does not
contain `42`.

RUNTIME COST, stated because this is the most expensive fixture in the family.
It starts an 11 MiB interpreter and reads a 20 MiB zip off ext4.  The startup
budget is separate from and much larger than the steady-state one for exactly
that reason: the gap before the first byte is nothing like the gap between two
bytes of one line, and a single budget sized for either is wrong for the other.
There is deliberately no `alarm` — a fixture's bounds should not depend on a
subsystem other than the one under test.

Exit code 42 == the REPL evaluated the expression and returned the answer.
Anything else names the first failing check; the legend is at the top of
`main.c`.  `3` (no output at all) and `4` (output but no answer) are kept
apart on purpose: the first is a loader or staging fault and the second is
this fixture's actual subject.

`forkpty` and `sched_yield` are declared in `main.c` rather than included —
the sysroot has no `<pty.h>` or `<sched.h>`, and a wrong prototype is a link
error here rather than a surprise at runtime.  Both symbols are ours, from
`posix/src/pty.rs` and the sysroot's scheduling shims.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/ctest-python-repl/build.py

The posix sysroot (`libc.a`) must already be built and must be *current* with
`posix/src/` and with `toolchain/build-sysroot.ps1`'s RUSTFLAGS.  The rootfs
must also carry `/bin/python3` and `/usr/local/lib/python312.zip`, staged
together and never separately — an interpreter without its stdlib dies inside
`init_fs_encoding` before `main()`, which this fixture would report as exit 3.
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
        HERE / "ctest-python-repl.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
