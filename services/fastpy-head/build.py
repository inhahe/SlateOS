#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-head` SlateOS utility.

This produces `fastpy-head.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `head`(1) —
`head <n> <file>` prints the first `<n>` lines of the file named by `argv[2]`
and exits 0.

New ground vs. the earlier fastpy tools (cat/grep/wc): it parses an **integer
argument** from `argv[1]` (`parse_int`, digit `ord` arithmetic — pure-mode
safe, same helper shape as fastpy-pkg's) and does **early-stop** line
iteration (stops emitting once `n` lines have printed rather than scanning the
whole file).

Pure-mode notes:
  * the line walk runs in `head_lines(text: str, limit: int) -> int`, whose
    `text` param is `str`-annotated so `text[i]` lowers to native
    `fastpy_str_index` (no object-subscript bridge); it builds each line by
    concatenation and returns the count printed (a plain int — no file-read str
    crosses a call boundary),
  * `main` reads the file inline and passes the text in (the proven
    `db = f.read(); db_search(db, q)` pattern),
  * early-stop is expressed with a `done` flag (no compound `and` while-guard,
    keeping to constructs already proven on-target); classification via `ord()`
    integer compares — bridge-free.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_head`) that stages a 4-line file with distinctive
line markers, runs `head 2 <file>`, asserts it exits 0, and (via the serial
log) that only the first two marker lines were printed.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-head/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

SRC = (
    "import sys\n"
    # Parse a non-negative decimal integer from `s` (ignores any non-digit
    # bytes; digit ord arithmetic stays in i64 — no bigint, no bridge).
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
    # Print the first `limit` newline-delimited lines of `text`; return the
    # number printed.  A trailing partial line (no final newline) still counts.
    # `done` short-circuits emission once the limit is hit without a compound
    # while-guard.
    "def head_lines(text: str, limit: int) -> int:\n"
    "    n = len(text)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    count = 0\n"
    "    done = 0\n"
    "    while i <= n:\n"
    "        if done == 0:\n"
    "            if i == n or ord(text[i]) == 10:\n"
    "                if i < n or len(line) > 0:\n"
    "                    print(line)\n"
    "                    count = count + 1\n"
    "                    if count >= limit:\n"
    "                        done = 1\n"
    "                line = ''\n"
    "            else:\n"
    "                line = line + text[i]\n"
    "        i = i + 1\n"
    "    return count\n"
    "n = parse_int(sys.argv[1])\n"
    "f = open(sys.argv[2], 'r')\n"
    "text = f.read()\n"
    "f.close()\n"
    "count = head_lines(text, n)\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-head.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
