#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-pkg` SlateOS utility.

This produces `fastpy-pkg.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): the **package manager
front-end** for SlateOS, built directly on top of the content-addressed store
primitive.  It is a real subcommand CLI over a persistent text registry
`/tmp/pkgdb.txt` (a database of `"<name> <digest>\n"` records):

    pkg install <name> <payload-path>
        read the payload, compute its 32-bit FNV-1a digest, write it to the
        content-addressed blob /tmp/store-<digest>.blob, then upsert the
        "<name> <digest>" record into the registry (replacing any prior record
        for the same name).  Prints "installed <name> <digest>", exit 0.

    pkg remove <name>
        drop the named record from the registry (read-modify-write).  Prints
        "removed <name>" + exit 0 if it was present, else "not found <name>" +
        exit 1.

    pkg query <name>
        resolve the named record and print its digest (exit 0), or
        "not found <name>" (exit 1).

    pkg list
        print every "<name> <digest>" record in the registry, exit 0.

The registry file must already exist (the installer / a package-manager bootstrap
creates an empty one); each subcommand reads it, and install/remove rewrite it.

Where `fastpy-store` proved a single content round-trip, this is the full
registry front-end a package manager needs: an argv[1] subcommand dispatch,
by-name resolution, an idempotent upsert (install replaces a prior record), and
record deletion — all over a persistent text DB parsed char-by-char.

Pure-mode caveats honored (see fastpy `known-issues.md`):
  * Every helper that subscripts a string takes a `str`-annotated parameter
    (`fnv1a(s: str)`, `to_hex8(v: int)`, `line_key(line: str)`,
    `lookup(db: str, name: str)`, `db_remove(db: str, name: str)`,
    `db_list(db: str)`) so `s[i]` lowers to the native `fastpy_str_index` rather
    than the CPython object-subscript bridge (BUG-SUBSCRIPT-UNTYPED-PARAM-BRIDGE).
  * The hash is 32-bit (`mask = 0xFFFFFFFF`, prime = 16777619) so every
    intermediate stays inside signed i64 — no bigint (BUG-BIGINT-MUL-INT-NULL).
  * Subcommand dispatch and name matching use `==` on strings, lowered to the
    native `fastpy_str_compare` (strcmp); character classification uses
    `ord(...)` integer compares (newline = 10, space = 32).  All pure-mode-safe.
  * File reads are inline (never returned through a user function), sidestepping
    BUG-FILEREAD-FN-RETTAG.

Reference digests (32-bit FNV-1a, verified against CPython):
    "SlateOS package payload\n" -> a6fd63bc
    "SlateOS"                   -> 59f7e180
    "coreutils demo\n"          -> 1ee068f8
    "grep demo\n"               -> see build output / self-test

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and drives it through a full CLI lifecycle in the
ring-3 self-test (`self_test_fastpy_slateos_pkg`): seed an empty registry, then
spawn install x2, query, remove, and query-again, asserting the exit codes and
the final registry contents (installed record present, removed record gone).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-pkg/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Package manager front-end: a subcommand CLI (install/remove/query/list) over a
# persistent text registry /tmp/pkgdb.txt built on the content-addressed store.
# 32-bit FNV (no bigint); every string-subscripting helper is type-annotated.
SRC = (
    "import sys\n"
    "def fnv1a(s: str) -> int:\n"
    "    h = 2166136261\n"
    "    mask = 4294967295\n"
    "    i = 0\n"
    "    n = len(s)\n"
    "    while i < n:\n"
    "        c = ord(s[i])\n"
    "        h = (h ^ c) & mask\n"
    "        h = (h * 16777619) & mask\n"
    "        i = i + 1\n"
    "    return h\n"
    "def to_hex8(v: int) -> str:\n"
    "    out = ''\n"
    "    i = 0\n"
    "    while i < 8:\n"
    "        shift = (7 - i) * 4\n"
    "        nib = (v >> shift) & 15\n"
    "        if nib < 10:\n"
    "            out = out + chr(48 + nib)\n"
    "        else:\n"
    "            out = out + chr(87 + nib)\n"
    "        i = i + 1\n"
    "    return out\n"
    "def line_key(line: str) -> str:\n"
    "    m = len(line)\n"
    "    j = 0\n"
    "    key = ''\n"
    "    while j < m and ord(line[j]) != 32:\n"
    "        key = key + line[j]\n"
    "        j = j + 1\n"
    "    return key\n"
    "def lookup(db: str, name: str) -> str:\n"
    "    n = len(db)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    while i <= n:\n"
    "        if i == n or ord(db[i]) == 10:\n"
    "            m = len(line)\n"
    "            if m > 0:\n"
    "                j = 0\n"
    "                key = ''\n"
    "                while j < m and ord(line[j]) != 32:\n"
    "                    key = key + line[j]\n"
    "                    j = j + 1\n"
    "                if key == name:\n"
    "                    val = ''\n"
    "                    j = j + 1\n"
    "                    while j < m:\n"
    "                        val = val + line[j]\n"
    "                        j = j + 1\n"
    "                    return val\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + db[i]\n"
    "        i = i + 1\n"
    "    return ''\n"
    "def db_remove(db: str, name: str) -> str:\n"
    "    n = len(db)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    out = ''\n"
    "    while i <= n:\n"
    "        if i == n or ord(db[i]) == 10:\n"
    "            if len(line) > 0:\n"
    "                k = line_key(line)\n"
    "                if k != name:\n"
    "                    out = out + line + chr(10)\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + db[i]\n"
    "        i = i + 1\n"
    "    return out\n"
    "def db_list(db: str) -> int:\n"
    "    n = len(db)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    count = 0\n"
    "    while i <= n:\n"
    "        if i == n or ord(db[i]) == 10:\n"
    "            if len(line) > 0:\n"
    "                print(line)\n"
    "                count = count + 1\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + db[i]\n"
    "        i = i + 1\n"
    "    return count\n"
    "cmd = sys.argv[1]\n"
    "db_path = '/tmp/pkgdb.txt'\n"
    "if cmd == 'install':\n"
    "    name = sys.argv[2]\n"
    "    src_path = sys.argv[3]\n"
    "    f = open(src_path, 'r')\n"
    "    payload = f.read()\n"
    "    f.close()\n"
    "    digest = to_hex8(fnv1a(payload))\n"
    "    store_path = '/tmp/store-' + digest + '.blob'\n"
    "    f = open(store_path, 'w')\n"
    "    f.write(payload)\n"
    "    f.close()\n"
    "    f = open(db_path, 'r')\n"
    "    db = f.read()\n"
    "    f.close()\n"
    "    db = db_remove(db, name)\n"
    "    db = db + name + ' ' + digest + chr(10)\n"
    "    f = open(db_path, 'w')\n"
    "    f.write(db)\n"
    "    f.close()\n"
    "    print('installed ' + name + ' ' + digest)\n"
    "    sys.exit(0)\n"
    "if cmd == 'remove':\n"
    "    name = sys.argv[2]\n"
    "    f = open(db_path, 'r')\n"
    "    db = f.read()\n"
    "    f.close()\n"
    "    had = lookup(db, name)\n"
    "    db = db_remove(db, name)\n"
    "    f = open(db_path, 'w')\n"
    "    f.write(db)\n"
    "    f.close()\n"
    "    if len(had) > 0:\n"
    "        print('removed ' + name)\n"
    "        sys.exit(0)\n"
    "    print('not found ' + name)\n"
    "    sys.exit(1)\n"
    "if cmd == 'query':\n"
    "    name = sys.argv[2]\n"
    "    f = open(db_path, 'r')\n"
    "    db = f.read()\n"
    "    f.close()\n"
    "    got = lookup(db, name)\n"
    "    if len(got) > 0:\n"
    "        print(got)\n"
    "        sys.exit(0)\n"
    "    print('not found ' + name)\n"
    "    sys.exit(1)\n"
    "if cmd == 'list':\n"
    "    f = open(db_path, 'r')\n"
    "    db = f.read()\n"
    "    f.close()\n"
    "    count = db_list(db)\n"
    "    print('total ' + str(count))\n"
    "    sys.exit(0)\n"
    "print('usage: pkg install|remove|query|list')\n"
    "sys.exit(2)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-pkg.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
