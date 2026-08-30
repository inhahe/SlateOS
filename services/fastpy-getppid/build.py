#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-getppid` SlateOS utility.

This produces `fastpy-getppid.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that reports its
**parent** process's PID, exercising the process-parentage syscall.

Sibling of `fastpy-getpid`.  Where `getpid` asks the kernel about the caller's
own identity (`SYS_PROCESS_ID`), this asks about its **relationship to another
process** — its parent — via a *distinct* kernel path: `os.getppid()` lowers to
native `fastpy_os_getppid` -> posix `getppid()` -> kernel `SYS_PROCESS_PARENT_ID`.

The tool reads `os.getppid()`, writes the decimal parent PID to
`/tmp/fastpy-getppid.out` (via native `open('w')`/`write` -> `SYS_FS_*`, because
fastpy's `print` lowers to a direct console-write that bypasses the fd table),
and exits with a code encoding a self-consistency check:

    exit 0 — os.getppid() returned a strictly positive value
    exit 3 — os.getppid() returned <= 0

False-pass-proof design:
  * The kernel self-test spawns this tool with `parent = 0` (kernel-spawned),
    exactly like every other ring-3 self-test.  The kernel's
    `SYS_PROCESS_PARENT_ID` handler returns 0 for a process with no recorded
    parent; posix `getppid()` then applies the POSIX reparent-to-init
    convention (0 -> 1).  So a correct round-trip yields **exactly 1**.
  * This cleanly distinguishes the parent syscall from the self syscall: if the
    lowering mistakenly called `getpid` (or returned the caller's own PID), the
    tool would report the large kernel-assigned PID, not 1 -> the self-test's
    `== 1` assertion fails.  So the test proves `os.getppid()` is wired to
    `SYS_PROCESS_PARENT_ID`, not `SYS_PROCESS_ID`.

Pure-mode notes (verified bridge-free in the emitted IR — only the
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.getppid()` lowers to a native `fastpy_os_getppid` call,
  * `str(ppid)` lowers to the native int->str formatter,
  * `open`/`write`/`close` lower to native fastpy file I/O,
  * the compare is pure integer arithmetic.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_getppid`).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-getppid/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# process-parentage probe: read os.getppid() (natively fastpy_os_getppid ->
# posix getppid() -> SYS_PROCESS_PARENT_ID), write the decimal parent PID to a
# file the kernel reads back, and exit 0 iff the value is strictly positive.
# The kernel harness spawns this with parent=0, so the correct reparent-to-init
# result is exactly 1 (distinct from the caller's own large PID).
SRC = (
    "import os\n"
    "import sys\n"
    "ppid = os.getppid()\n"
    "f = open('/tmp/fastpy-getppid.out', 'w')\n"
    "f.write(str(ppid))\n"
    "f.close()\n"
    "code = 3\n"
    "if ppid > 0:\n"
    "    code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-getppid.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
