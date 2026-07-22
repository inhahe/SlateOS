#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-wc` SlateOS utility.

This produces `fastpy-wc.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `wc`(1) —
it reads the file named by `argv[1]` and prints its line, word, and byte
counts as `"<lines> <words> <bytes>"`, then exits 0.

Counting rules (matching `wc`):
  * bytes  = number of bytes read (`len(text)`),
  * lines  = number of newline (0x0A) bytes,
  * words  = number of maximal runs of non-whitespace (whitespace = space,
    tab, newline).

Pure-mode notes:
  * the whole count is done in `wc_report(text: str) -> str`, a helper whose
    param is `str`-annotated so `text[i]` lowers to native `fastpy_str_index`
    (no object-subscript bridge), and which *returns a freshly built string*
    (safe — never returns the file-read str across a call boundary),
  * the file read is consumed in `main` and passed into the helper as an
    argument (the proven fastpy-pkg pattern: `db = f.read(); db_search(db, q)`),
  * counts are formatted with native `str(int)`; all comparisons use `ord()`
    integer compares — bridge-free.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_wc`) that stages a known file whose counts are
computed by hand, spawns the utility, and asserts it exits 0 (and the count
line appears on serial).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-wc/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

SRC = (
    "import sys\n"
    # Count lines (newlines), words (maximal non-whitespace runs), and bytes
    # (len) of `text`; return "<lines> <words> <bytes>".  `text` is str-typed
    # so text[i] lowers to native fastpy_str_index (no object-subscript
    # bridge); the returned value is a freshly built string.
    "def wc_report(text: str) -> str:\n"
    "    n = len(text)\n"
    "    lines = 0\n"
    "    words = 0\n"
    "    inword = 0\n"
    "    i = 0\n"
    "    while i < n:\n"
    "        c = ord(text[i])\n"
    "        if c == 10:\n"
    "            lines = lines + 1\n"
    "        if c == 32 or c == 10 or c == 9:\n"
    "            inword = 0\n"
    "        else:\n"
    "            if inword == 0:\n"
    "                words = words + 1\n"
    "                inword = 1\n"
    "        i = i + 1\n"
    "    return str(lines) + ' ' + str(words) + ' ' + str(n)\n"
    "f = open(sys.argv[1], 'r')\n"
    "text = f.read()\n"
    "f.close()\n"
    "print(wc_report(text))\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-wc.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
