#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-uniq` SlateOS utility.

This produces `fastpy-uniq.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `uniq`(1) —
`uniq <file>` reads the file named by `argv[1]` and prints each line, dropping
any line that is identical to the immediately preceding printed line (adjacent
de-duplication, exactly like `uniq` with no flags), then exits 0.

New ground vs. the earlier fastpy tools (cat/grep/wc/head): it carries a
**string across loop iterations** (the previous line) and does **line-to-line
string comparison** (`line == prev`, lowered to the native `fastpy_str_compare`
strcmp).  cat/grep echoed or matched; wc/head counted; this is the first tool
whose output depends on comparing one built-up line against another.

Pure-mode notes:
  * the whole dedup runs in `uniq_lines(text: str) -> int`, whose `text` param
    is `str`-annotated so `text[i]` lowers to native `fastpy_str_index` (no
    object-subscript bridge); it returns the number of lines printed (a plain
    int — no file-read str crosses a call boundary),
  * equality is tested with `line == prev` only (never `!=`), keeping to the
    exact string op proven on-target by fastpy-pkg's name matching; the result
    is funnelled through an int flag so control flow uses int compares,
  * `main` reads the file inline and passes the text in (the proven
    `db = f.read(); db_search(db, q)` pattern).

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_uniq`) that stages a file with adjacent duplicate
lines, runs `uniq`, asserts it exits 0, and (via the serial log) that adjacent
duplicates collapsed to one while a non-adjacent repeat survived.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-uniq/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

SRC = (
    "import sys\n"
    # Print each newline-delimited line of `text`, dropping any line equal to
    # the immediately preceding *printed* line (adjacent de-dup).  Returns the
    # number of lines printed.  A trailing partial line (no final newline) still
    # counts.  Equality uses only `==` (native fastpy_str_compare); the boolean
    # is routed through the int flag `same` so branching stays int-typed.
    "def uniq_lines(text: str) -> int:\n"
    "    n = len(text)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    prev = ''\n"
    "    have_prev = 0\n"
    "    count = 0\n"
    "    while i <= n:\n"
    "        if i == n or ord(text[i]) == 10:\n"
    "            if i < n or len(line) > 0:\n"
    "                same = 0\n"
    "                if have_prev == 1:\n"
    "                    if line == prev:\n"
    "                        same = 1\n"
    "                if same == 0:\n"
    "                    print(line)\n"
    "                    count = count + 1\n"
    "                prev = line\n"
    "                have_prev = 1\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + text[i]\n"
    "        i = i + 1\n"
    "    return count\n"
    "f = open(sys.argv[1], 'r')\n"
    "text = f.read()\n"
    "f.close()\n"
    "count = uniq_lines(text)\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-uniq.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
