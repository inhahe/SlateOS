#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-pathlib` SlateOS utility.

This produces `fastpy-pathlib.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native). It is the on-target proof
for fastpy's **pure-mode `pathlib.Path`** runtime (`runtime/pathlib_pure.c` in
the fastpy repo): the CPython-free implementation of `Path(...)`, `write_text`,
`read_text`, `exists`, `is_file`, `is_dir`, `name`, `suffix`, `stem`, `parent`,
and `joinpath` that lets a pure-mode SlateOS program touch the filesystem
through the high-level Pythonic `Path` API (not just the low-level `os.*` calls
the shell-plumbing utilities already proved).

The program exercises the whole surface against a scratch file under `/tmp`,
counting how many independent checks pass, and exits with that count so the
kernel ring-3 self-test (`self_test_fastpy_slateos_pathlib` in
`kernel/src/proc/spawn.rs`) can assert the exact expected total. It takes no
arguments.

The exit status is a **bitmask**: check N (1-based) sets bit (N-1), i.e. adds
2**(N-1) to `ok`. A full pass is 2**10 - 1 == 1023; any other value's clear bits
name exactly which checks failed (far more diagnostic than a bare pass count).

Checks (bit value in parentheses; EXPECTED = 1023 on success):
   1. write_text then read_text round-trips the exact bytes        (1)
   2. exists() is true for the written file                        (2)
   3. is_file() is true for it                                     (4)
   4. is_dir() is false for it                                     (8)
   5. name == 'fpy-pathlib.txt'                                    (16)
   6. suffix == '.txt'                                             (32)
   7. stem == 'fpy-pathlib'                                        (64)
   8. str(parent) == '/tmp'                                        (128)
   9. joinpath builds '/tmp/fpy-pathlib.txt' (matches the original)(256)
  10. is_dir() is true for '/tmp'                                  (512)

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-pathlib/build.py"

The posix sysroot (`libc.a`) and the pure-mode SlateOS runtime objects are
built on demand by the fastpy toolchain.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Each satisfied check adds its unique power-of-two bit to `ok`; the process
# exits with `ok` (a bitmask). Kept as a flat sequence of `if cond: ok = ok + N`
# (no boolean fold) to stay squarely in pure-mode-safe codegen territory — the
# same discipline the shell-plumbing utilities used.
SRC = (
    "import sys\n"
    "from pathlib import Path\n"
    "p = Path('/tmp/fpy-pathlib.txt')\n"
    "p.write_text('hello pathlib\\n')\n"
    "ok = 0\n"
    "if p.read_text() == 'hello pathlib\\n':\n"
    "    ok = ok + 1\n"
    "if p.exists():\n"
    "    ok = ok + 2\n"
    "if p.is_file():\n"
    "    ok = ok + 4\n"
    "if not p.is_dir():\n"
    "    ok = ok + 8\n"
    "if p.name == 'fpy-pathlib.txt':\n"
    "    ok = ok + 16\n"
    "if p.suffix == '.txt':\n"
    "    ok = ok + 32\n"
    "if p.stem == 'fpy-pathlib':\n"
    "    ok = ok + 64\n"
    "d = p.parent\n"
    "if str(d) == '/tmp':\n"
    "    ok = ok + 128\n"
    "j = d.joinpath('fpy-pathlib.txt')\n"
    "if str(j) == '/tmp/fpy-pathlib.txt':\n"
    "    ok = ok + 256\n"
    "if d.is_dir():\n"
    "    ok = ok + 512\n"
    "sys.exit(ok)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-pathlib.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
