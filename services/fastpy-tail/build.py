#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-tail` SlateOS utility.

This produces `fastpy-tail.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `tail`(1) —
`tail <n> <file>` prints the last `<n>` lines of the file named by `argv[2]`
and exits 0.

New ground vs. the earlier fastpy tools: it is the first **two-pass** tool.
Because the last `n` lines can't be known until the total is known, it scans
the text **twice** — `count_lines` counts the total, then `tail_lines` re-scans
and prints only lines at index >= (total - n).  (head could early-stop in a
single pass; tail fundamentally cannot without buffering, so two passes over
the same immutable text is the clean pure-mode approach — no list-of-strings
needed.)

Pure-mode notes:
  * both passes run in `str`-typed helpers (`count_lines(text: str) -> int`,
    `tail_lines(text: str, skip: int) -> int`) so `text[i]` lowers to native
    `fastpy_str_index`; each returns a plain int (no file-read str crosses a
    call boundary),
  * the integer arg is parsed with `parse_int` (digit `ord` arithmetic, the
    fastpy-pkg helper shape), and `skip = total - n` is clamped at 0 so
    `tail <n>` with n >= total prints the whole file,
  * `main` reads the file inline and passes the text to both passes (the proven
    `db = f.read(); db_search(db, q)` pattern).

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_tail`) that stages a 5-line marker file, runs
`tail 2 <file>`, asserts it exits 0, and (via the serial log) that only the
last two marker lines were printed.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-tail/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

SRC = (
    "import sys\n"
    # Parse a non-negative decimal integer from `s` (digit ord arithmetic, i64).
    "def parse_int(s: str) -> int:\n"
    "    n = len(s)\n"
    "    v = 0\n"
    "    i = 0\n"
    "    while i < n:\n"
    "        c = ord(s[i])\n"
    "        if c >= 48 and c <= 57:\n"
    "            v = v * 10 + (c - 48)\n"
    "        i = i + 1\n"
    "    return v\n"
    # Pass 1: count newline-delimited lines (a trailing partial line counts).
    "def count_lines(text: str) -> int:\n"
    "    n = len(text)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    count = 0\n"
    "    while i <= n:\n"
    "        if i == n or ord(text[i]) == 10:\n"
    "            if i < n or len(line) > 0:\n"
    "                count = count + 1\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + text[i]\n"
    "        i = i + 1\n"
    "    return count\n"
    # Pass 2: print lines whose 0-based index is >= `skip`; return count printed.
    "def tail_lines(text: str, skip: int) -> int:\n"
    "    n = len(text)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    idx = 0\n"
    "    count = 0\n"
    "    while i <= n:\n"
    "        if i == n or ord(text[i]) == 10:\n"
    "            if i < n or len(line) > 0:\n"
    "                if idx >= skip:\n"
    "                    print(line)\n"
    "                    count = count + 1\n"
    "                idx = idx + 1\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + text[i]\n"
    "        i = i + 1\n"
    "    return count\n"
    "n = parse_int(sys.argv[1])\n"
    "f = open(sys.argv[2], 'r')\n"
    "text = f.read()\n"
    "f.close()\n"
    "total = count_lines(text)\n"
    "skip = total - n\n"
    "if skip < 0:\n"
    "    skip = 0\n"
    "count = tail_lines(text, skip)\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-tail.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
