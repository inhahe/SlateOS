#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-freq` SlateOS utility.

This produces `fastpy-freq.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a line-frequency counter —
`freq <file>` reads the file named by `argv[1]`, counts how many times each
distinct line occurs, prints one `"<count> <line>"` record per distinct line,
and exits 0.  (It is the associative-array analogue of `uniq -c`, but counts
*all* occurrences of a line, not just adjacent runs.)

New ground vs. every earlier fastpy tool (cat/grep/wc/head/uniq/tail/sort): it
is the first to use an **associative container** — a `dict[str, int]`.  `sort`
proved the native `list`; this proves the native `dict`: construction (`{}`),
membership (`line in d`), subscript read (`d[line]`) and write (`d[line] = ...`),
and **key iteration** (`for k in d:`).  A fail-fast IR probe confirmed all of
these lower to native `fastpy_dict_*` runtime calls with **zero** CPython bridge
calls in pure mode, so the dict container is proven end-to-end on-target.

Pure-mode notes (all verified bridge-free in the emitted IR):
  * `count_freq(text: str) -> int` builds the `dict[str, int]` — the `str`-typed
    `text` param makes `text[i]` lower to native `fastpy_str_index`; each
    `line in d` / `d[line]` / `d[line] = v` lowers to a native
    `fastpy_dict_has_key` / `fastpy_dict_get_*` / `fastpy_dict_set_*`,
  * printing iterates `for k in d:` (native dict-key iteration) and formats each
    record as `str(d[k]) + ' ' + k` (native `str(int)` + `fastpy_str_concat`),
  * it returns the number of distinct lines (a plain int — no file-read str
    crosses a call boundary, sidestepping BUG-FILEREAD-FN-RETTAG),
  * `main` reads the file inline and passes the text in (the proven
    `db = f.read(); db_search(db, q)` pattern).

Because a dict's iteration order is an implementation detail, the kernel
self-test verifies the *set* of emitted `"<count> <line>"` records (each grepped
independently), not their order.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_freq`) that stages a file with known per-line counts,
runs `freq`, asserts it exits 0, and (via the serial log) that each distinct
line's count is correct.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-freq/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

SRC = (
    "import sys\n"
    # Count occurrences of each distinct newline-delimited line of `text` in a
    # dict[str, int], then print one "<count> <line>" record per distinct line.
    # Returns the number of distinct lines.  The dict container (construct/
    # membership/get/set) and key iteration are the new pure-mode primitives
    # here; all lower to native fastpy_dict_* ops.
    "def count_freq(text: str) -> int:\n"
    "    d = {}\n"
    "    n = len(text)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    while i <= n:\n"
    "        if i == n or ord(text[i]) == 10:\n"
    "            if i < n or len(line) > 0:\n"
    "                if line in d:\n"
    "                    d[line] = d[line] + 1\n"
    "                else:\n"
    "                    d[line] = 1\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + text[i]\n"
    "        i = i + 1\n"
    "    distinct = 0\n"
    "    for k in d:\n"
    "        print(str(d[k]) + ' ' + k)\n"
    "        distinct = distinct + 1\n"
    "    return distinct\n"
    "f = open(sys.argv[1], 'r')\n"
    "text = f.read()\n"
    "f.close()\n"
    "count = count_freq(text)\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-freq.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
