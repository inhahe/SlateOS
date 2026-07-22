#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-grep` SlateOS utility.

This produces `fastpy-grep.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `grep`(1) —
it reads the file named by `argv[2]`, prints every line that contains the
fixed substring `argv[1]`, and exits following grep(1) exit semantics:

    grep <pattern> <file>
        exit 0 if at least one line matched (and was printed),
        exit 1 if no line matched.

It is a fixed-string matcher (like `grep -F`): no regex, just a native
character-by-character substring scan, reusing the same proven `contains_sub`
matcher that backs `pkg search`.  It exercises the same three on-target paths
as fastpy-cat (argv delivery, pure-mode file I/O, stdout) plus per-line
iteration and the substring matcher.

Pure-mode notes (why the code is shaped this way):
  * both `contains_sub` params are `str`-annotated so `hay[i]`/`needle[j]`
    lower to native `fastpy_str_index` (avoids the object-subscript bridge),
  * the file read is consumed inline in `main` and lines are built by
    concatenation inside `grep_lines` (never returning a file-read str across
    a call boundary — avoids the file-read return-tag mis-typing bug),
  * matching / line splitting use only `ord()`, `len()`, `+`, `==` — all
    native, no CPython bridge.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_grep`) that stages a known multi-line file in
`/tmp`, grants the process a File capability, spawns it once with a pattern
that matches (asserting exit 0) and once with a pattern that does not
(asserting exit 1).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-grep/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

SRC = (
    "import sys\n"
    # Return 1 if `needle` occurs anywhere in `hay`, else 0 (native char
    # compares; both params str-annotated so hay[i]/needle[j] lower to
    # fastpy_str_index — no object-subscript bridge).
    "def contains_sub(hay: str, needle: str) -> int:\n"
    "    hn = len(hay)\n"
    "    nn = len(needle)\n"
    "    if nn == 0:\n"
    "        return 1\n"
    "    if nn > hn:\n"
    "        return 0\n"
    "    i = 0\n"
    "    while i <= hn - nn:\n"
    "        j = 0\n"
    "        ok = 1\n"
    "        while j < nn:\n"
    "            if ord(hay[i + j]) != ord(needle[j]):\n"
    "                ok = 0\n"
    "                j = nn\n"
    "            else:\n"
    "                j = j + 1\n"
    "        if ok == 1:\n"
    "            return 1\n"
    "        i = i + 1\n"
    "    return 0\n"
    # Iterate `text` line by line (newline-delimited; a trailing partial line
    # with no final newline is still considered a line).  Print each line that
    # contains `pat`; return the number of matching lines printed.
    "def grep_lines(text: str, pat: str) -> int:\n"
    "    n = len(text)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    count = 0\n"
    "    while i <= n:\n"
    "        if i == n or ord(text[i]) == 10:\n"
    "            if contains_sub(line, pat) == 1:\n"
    "                print(line)\n"
    "                count = count + 1\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + text[i]\n"
    "        i = i + 1\n"
    "    return count\n"
    "pat = sys.argv[1]\n"
    "f = open(sys.argv[2], 'r')\n"
    "text = f.read()\n"
    "f.close()\n"
    "count = grep_lines(text, pat)\n"
    "if count > 0:\n"
    "    sys.exit(0)\n"
    "else:\n"
    "    sys.exit(1)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-grep.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
