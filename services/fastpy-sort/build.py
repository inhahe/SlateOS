#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-sort` SlateOS utility.

This produces `fastpy-sort.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `sort`(1) —
`sort <file>` reads the file named by `argv[1]`, sorts its lines into ascending
lexicographic order, prints them, and exits 0.

New ground vs. every earlier fastpy tool (cat/grep/wc/head/uniq/tail): it is
the first to build and manipulate an **in-memory list of strings** and to
**order** strings (`lines[b] < lines[a]`, the `<` relational compare, not just
`==`).  cat/grep/head/tail/uniq streamed or compared lines one at a time and
never materialised the whole file as a list; this one collects every line into
a `list`, mutates that list in place (get/set/append), and sorts it — so it
proves fastpy pure-mode's native list container end-to-end on-target.

Pure-mode notes (all verified bridge-free in the emitted IR — zero
`fpy_cpython_*` calls):
  * `sort_lines(text: str) -> int` splits `text` into a `list` via
    `lines.append(line)` (native `fastpy_list_append`); the `str`-typed `text`
    param makes `text[i]` lower to native `fastpy_str_index`,
  * the in-place selection/bubble sort uses `lines[j]` reads/writes (native
    `fastpy_list_get`/`fastpy_list_set`) and the ordering test `lines[b] <
    lines[a]`, which lowers to native `fastpy_str_compare` (strcmp) — the first
    tool to use `<` on strings rather than `==`,
  * it returns the number of lines printed (a plain int — no file-read str
    crosses a call boundary, sidestepping BUG-FILEREAD-FN-RETTAG),
  * `main` reads the file inline and passes the text in (the proven
    `db = f.read(); db_search(db, q)` pattern).

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_sort`) that stages an out-of-order file, runs
`sort`, asserts it exits 0, and (via the serial log) that the lines were
emitted in ascending order.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-sort/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

SRC = (
    "import sys\n"
    # Split `text` into newline-delimited lines, sort them ascending in place,
    # and print each.  Returns the number of lines printed.  A trailing partial
    # line (no final newline) still counts.  The list container (append/get/set)
    # and the `<` ordering compare are the new pure-mode primitives here; both
    # lower to native ops (fastpy_list_*, fastpy_str_compare).
    "def sort_lines(text: str) -> int:\n"
    "    lines = []\n"
    "    n = len(text)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    while i <= n:\n"
    "        if i == n or ord(text[i]) == 10:\n"
    "            if i < n or len(line) > 0:\n"
    "                lines.append(line)\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + text[i]\n"
    "        i = i + 1\n"
    "    m = len(lines)\n"
    # Selection sort: for each position a, find the smallest remaining line and
    # swap it in.  O(n^2) is fine for a utility/self-test; the point is to prove
    # native list mutation + string ordering, not asymptotic speed.
    "    a = 0\n"
    "    while a < m:\n"
    "        b = a + 1\n"
    "        while b < m:\n"
    "            if lines[b] < lines[a]:\n"
    "                t = lines[a]\n"
    "                lines[a] = lines[b]\n"
    "                lines[b] = t\n"
    "            b = b + 1\n"
    "        a = a + 1\n"
    "    a = 0\n"
    "    while a < m:\n"
    "        print(lines[a])\n"
    "        a = a + 1\n"
    "    return m\n"
    "f = open(sys.argv[1], 'r')\n"
    "text = f.read()\n"
    "f.close()\n"
    "count = sort_lines(text)\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-sort.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
