#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-tee` SlateOS utility.

This produces `fastpy-tee.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native).  It is a minimal
`tee`(1): it reads standard input (fd 0) to EOF and writes every byte,
unchanged, to BOTH standard output (fd 1) AND the file named by argv[1]
(created/truncated).

Where `fastpy-catstdin` proved a middle pipeline stage reading one pipe and
writing one pipe, `tee` proves the **fan-out** pattern: one input stream
duplicated onto TWO output fds at once — a raw file fd it opened itself AND
the inherited stdout (which, in a pipeline, is a pipe).  It is the classic
`... | tee file | ...` shell filter: the data keeps flowing downstream while
a copy is spilled to a file as a side effect.

  * `os.open(argv[1], 577, 420)` — O_WRONLY|O_CREAT|O_TRUNC, mode 0644.
  * `os.read(0, n)`  — drain stdin until EOF (a zero-length read).
  * `os.write(1, b)` / `os.write(fd, b)` — forward each chunk to both sinks.

Argv (supplied by the caller):
    argv[1] = the output file path (the "tee'd" copy)

Diagnostic exit codes:
    2 = os.open of the output file failed
    3 = missing argv[1] (no output file given)
    1 = a short write to either sink (stdout or the file)
    0 = clean EOF, both sinks fully written

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-tee/build.py"

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
    "argc = len(sys.argv)\n"
    "if argc < 2:\n"
    "    sys.exit(3)\n"
    "path = sys.argv[1]\n"
    # O_WRONLY|O_CREAT|O_TRUNC = 577, mode 0644 = 420.
    "fd = os.open(path, 577, 420)\n"
    "if fd < 0:\n"
    "    sys.exit(2)\n"
    "while True:\n"
    "    chunk = os.read(0, 4096)\n"
    "    n = len(chunk)\n"
    "    if n == 0:\n"
    "        break\n"
    "    w1 = os.write(1, chunk)\n"
    "    if w1 != n:\n"
    "        os.close(fd)\n"
    "        sys.exit(1)\n"
    "    w2 = os.write(fd, chunk)\n"
    "    if w2 != n:\n"
    "        os.close(fd)\n"
    "        sys.exit(1)\n"
    "os.close(fd)\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-tee.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
