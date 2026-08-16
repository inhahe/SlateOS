#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-gettid` SlateOS utility.

This produces `fastpy-gettid.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that reports its
own **kernel task ID**, exercising the scheduler's task-identity syscall.

Sibling of `fastpy-getpid` / `fastpy-getppid`.  Those two ask the kernel about
the **process** table (`SYS_PROCESS_ID` / `SYS_PROCESS_PARENT_ID`).  This asks
about the **scheduler's task table** — a genuinely distinct kernel path:
`os.gettid()` lowers to native `fastpy_os_gettid` -> posix `gettid()` -> kernel
`SYS_TASK_ID`.

The tool reads `os.gettid()`, writes the decimal task ID to
`/tmp/fastpy-gettid.out` (via native `open('w')`/`write` -> `SYS_FS_*`, because
fastpy's `print` lowers to a direct console-write that bypasses the fd table),
and exits with a code encoding a self-consistency check:

    exit 0 — os.gettid() returned a strictly positive value
    exit 3 — os.gettid() returned <= 0

False-pass-proof design:
  * The kernel self-test spawns this tool and records the main thread's task ID
    (`result.task_id`) that it assigned at spawn.  The tool writes back whatever
    `os.gettid()` returns, and the harness asserts the two are exactly equal.
  * That value is a scheduler-assigned task ID drawn from a *different* ID space
    than the process PID, so if the lowering mistakenly called `getpid` (or
    otherwise returned the caller's PID), the written value would be the PID, not
    the task ID -> the harness's `== result.task_id` assertion fails.  So the
    test proves `os.gettid()` is wired to `SYS_TASK_ID`, not `SYS_PROCESS_ID`.

Pure-mode notes (verified bridge-free in the emitted IR — only the
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.gettid()` lowers to a native `fastpy_os_gettid` call,
  * `str(tid)` lowers to the native int->str formatter,
  * `open`/`write`/`close` lower to native fastpy file I/O,
  * the compare is pure integer arithmetic.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_gettid`).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-gettid/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# task-identity probe: read os.gettid() (natively fastpy_os_gettid -> posix
# gettid() -> SYS_TASK_ID), write the decimal task ID to a file the kernel
# reads back, and exit 0 iff the value is strictly positive.  The kernel
# harness knows the exact task ID it assigned at spawn and cross-checks it.
SRC = (
    "import os\n"
    "import sys\n"
    "tid = os.gettid()\n"
    "f = open('/tmp/fastpy-gettid.out', 'w')\n"
    "f.write(str(tid))\n"
    "f.close()\n"
    "code = 3\n"
    "if tid > 0:\n"
    "    code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-gettid.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
