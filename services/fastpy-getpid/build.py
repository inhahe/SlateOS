#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-getpid` SlateOS utility.

This produces `fastpy-getpid.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that reports its
own process identity and proves the value is the **real kernel-assigned PID**.

The tool reads `os.getpid()`, prints the decimal PID to stdout, and exits with
a code encoding a self-consistency check:

    exit 0 — os.getpid() returned a strictly positive value
    exit 3 — os.getpid() returned <= 0 (a dead / stub identity)

New ground vs. every prior fastpy SlateOS tool: this is the first to exercise
the **process-identity** syscall.  `os.getpid()` lowers to the native runtime
`fastpy_os_getpid`, which on SlateOS calls posix libc `getpid()` -> kernel
`SYS_PROCESS_ID`.  Timekeeping (clock/sleep) and the filesystem syscalls are
about *external* resources; the process's own PID is kernel bookkeeping about
the *caller itself* — a distinct kernel path.

False-pass-proof design (two independent checks, the second kernel-side):
  1. The tool itself refuses to exit 0 unless `getpid() > 0`, so a clock/stub
     that returns 0 is caught by the exit code.
  2. The kernel self-test (`self_test_fastpy_slateos_getpid`) reads the file
     the tool writes (`/tmp/fastpy-getpid.out`) and asserts the PID it holds
     **equals the real PID the kernel assigned at spawn** (`result.pid`).  A
     stub `getpid()` that returns a constant (0, 1, ...) would not match the
     kernel's freshly-allocated PID -> fail.  This is the strong proof: only a
     genuine `SYS_PROCESS_ID` round-trip returns the caller's actual identity.

The tool writes the PID to a file (rather than printing it) because fastpy's
`print` lowers to a direct console-write syscall that bypasses the fd table —
so a spawn-time fd redirect can't capture it.  A real `open('w')`/`write` path
(fastpy native file I/O -> posix -> `SYS_FS_*`) lands the value where the
kernel can read it back by path.

Pure-mode notes (verified bridge-free in the emitted IR — only the
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.getpid()` lowers to a native `fastpy_os_getpid` call,
  * `str(pid)` lowers to the native int->str formatter,
  * `open`/`write`/`close` lower to native fastpy file I/O,
  * the compare is pure integer arithmetic.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_getpid`).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-getpid/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# process-identity probe: read os.getpid() (natively fastpy_os_getpid ->
# posix getpid() -> SYS_PROCESS_ID), write the decimal PID to a file the kernel
# reads back, and exit 0 iff the PID is strictly positive.  The kernel harness
# cross-checks the written value against the PID it assigned at spawn.
SRC = (
    "import os\n"
    "import sys\n"
    "pid = os.getpid()\n"
    "f = open('/tmp/fastpy-getpid.out', 'w')\n"
    "f.write(str(pid))\n"
    "f.close()\n"
    "code = 3\n"
    "if pid > 0:\n"
    "    code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-getpid.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
