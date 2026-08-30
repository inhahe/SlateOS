#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-catstdin` SlateOS utility.

This produces `fastpy-catstdin.elf`, a native SlateOS (`x86_64-slateos`)
binary compiled by fastpy (AOT Python -> LLVM IR -> native).  It is a
minimal **stdin->stdout passthrough filter**: it reads its standard input
(fd 0) to EOF and copies every byte, unchanged, to standard output (fd 1).

It exists to be a *middle* stage of a multi-stage pipeline.  The existing
fixtures cover the two ends: `fastpy-cat` is a source (writes a file to
stdout, takes an argv, does NOT read stdin) and `fastpy-countin` is a sink
(reads stdin to EOF, exits with the byte count, writes nothing).  Neither
exercises the case a 3+-stage pipeline needs: a stage whose fd 0 is a pipe
read-end AND whose fd 1 is a *different* pipe write-end, i.e. it reads from
the upstream pipe and writes to the downstream pipe *simultaneously*.  A
two-stage pipeline never wires both pipe ends onto one process, so this
passthrough filter is what makes `cat IN | catstdin | countin` prove the
middle-stage plumbing (both inherited pipe fds surviving fork+exec at once).

  * `os.read(0, n)`  — drain stdin (SYS_FS_READ on the inherited fd 0)
                       until EOF (a zero-length read).
  * `os.write(1, b)` — forward each chunk unchanged to fd 1.

It takes no arguments and touches no files — its only input is whatever the
kernel wired onto fd 0, and its only output goes to fd 1.  Exits 0 on clean
EOF; exits 1 if a write does not fully drain a chunk (short write).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-catstdin/build.py

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
    "while True:\n"
    "    chunk = os.read(0, 4096)\n"
    "    if len(chunk) == 0:\n"
    "        break\n"
    "    n = os.write(1, chunk)\n"
    "    if n != len(chunk):\n"
    "        sys.exit(1)\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-catstdin.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
