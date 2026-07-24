#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-fileio2` SlateOS utility.

This produces `fastpy-fileio2.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native). Where `fastpy-fileio`
proves the *basic* pure-mode round-trip (`open('w')`/`write`/`close` then
`open('r')`/`read`), this drives the richer **file-object surface** a real
pure-mode fastpy program uses:

  * the `with open(...) as f:` context-manager form (implicit close on exit),
  * whole-file `f.read()`,
  * line iteration (`for line in f:`),
  * `f.readline()` (single line, including its trailing newline), and
  * `f.readlines()` (list of all lines).

In pure mode these are backed by the CPython-free native file object (C stdio
over the posix `libc.a`) rather than the (absent) libpython bridge, so this test
proves that implementation works end-to-end against the SlateOS VFS.

The program first writes three known lines to a scratch file under `/tmp`, then
runs four independent checks and exits with a **bitmask**: check N (1-based)
sets bit (N-1), i.e. adds 2**(N-1) to `ok`. A full pass is 2**4 - 1 == 15; any
other value's clear bits name exactly which file-object operation misbehaved
(far more diagnostic than a bare pass count). It takes no arguments.

Checks (bit value in parentheses; EXPECTED = 15 on success):
   1. `with open(p) as f: f.read()` returns the whole file             (1)
   2. `with open(p) as f:` + `for line in f:` iterates all 3 lines     (2)
   3. `with open(p) as f: f.readline()` returns the first line + '\n'  (4)
   4. `with open(p) as f: f.readlines()` returns a 3-element list      (8)

The kernel ring-3 self-test (`self_test_fastpy_slateos_fileio2` in
`kernel/src/proc/spawn.rs`) asserts exit == 15.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-fileio2/build.py"

The posix sysroot (`libc.a`) and the pure-mode SlateOS runtime objects are
built on demand by the fastpy toolchain.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Flat `if cond: ok = ok + N` sequence (no boolean fold), staying squarely in
# pure-mode-safe codegen territory — the same discipline the other on-target
# fastpy fixtures use. No print(): a spawned native process has no console
# handle for fd 1, so the bitmask exit code carries the whole result.
SRC = (
    "import sys\n"
    "p = '/tmp/fpyio2.txt'\n"
    "f = open(p, 'w')\n"
    "f.write('alpha\\n')\n"
    "f.write('beta\\n')\n"
    "f.write('gamma\\n')\n"
    "f.close()\n"
    "ok = 0\n"
    # Check 1: context-manager read of the whole file.
    "with open(p, 'r') as f:\n"
    "    data = f.read()\n"
    "if data == 'alpha\\nbeta\\ngamma\\n':\n"
    "    ok = ok + 1\n"
    # Check 2: iterate the file object line by line.
    "count = 0\n"
    "with open(p, 'r') as f:\n"
    "    for line in f:\n"
    "        count = count + 1\n"
    "if count == 3:\n"
    "    ok = ok + 2\n"
    # Check 3: readline() returns the first line, newline included.
    "with open(p, 'r') as f:\n"
    "    first = f.readline()\n"
    "if first == 'alpha\\n':\n"
    "    ok = ok + 4\n"
    # Check 4: readlines() returns all three lines.
    "with open(p, 'r') as f:\n"
    "    lines = f.readlines()\n"
    "if len(lines) == 3:\n"
    "    ok = ok + 8\n"
    "sys.exit(ok)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-fileio2.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
