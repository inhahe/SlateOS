#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-countin` SlateOS utility.

This produces `fastpy-countin.elf`, a native SlateOS (`x86_64-slateos`)
binary compiled by fastpy (AOT Python -> LLVM IR -> native).  It is a
minimal **stdin consumer**: it reads its standard input (fd 0) to EOF,
counts the bytes, and exits with that count.

It exists to be the *downstream* stage of a real `cmd1 | cmd2` pipeline
(see `fastpy-pipeline`).  Where `fastpy-capture` proved that a child's
`dup2(pipe_w, 1)` stdout redirect survives `execv` (the producer side),
this program is the thing on the *other* end of the pipe: a command whose
fd 0 has been pointed at a pipe's read end via `dup2(pipe_r, 0)` before
`execv`.  If the consumer-side redirect survives exec, everything the
upstream command writes flows into this program's `os.read(0, ...)`.

  * `os.read(0, n)` — drain stdin (SYS_PIPE_READ on the inherited pipe fd)
                      until EOF (a zero-length read).

The process exits with the total number of bytes it read (mod 256, like
`cat`), so a caller can cross-check it against the upstream command's own
byte count.  It takes no arguments and touches no files — its only input
is whatever the kernel wired onto fd 0.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-countin/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

SRC = (
    "import sys\n"
    "import os\n"
    "total = 0\n"
    "while True:\n"
    "    chunk = os.read(0, 4096)\n"
    "    if len(chunk) == 0:\n"
    "        break\n"
    "    total = total + len(chunk)\n"
    "sys.exit(total)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-countin.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
