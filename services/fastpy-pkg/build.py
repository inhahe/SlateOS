#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-pkg` SlateOS utility.

This produces `fastpy-pkg.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): the **package registry**
layer of the SlateOS package manager, built directly on top of the
content-addressed store primitive (`fastpy-store`).

Given a package name in `argv[1]` and a payload path in `argv[2]`, it:
  1. reads the payload file's contents,
  2. computes a 32-bit FNV-1a digest of the bytes (8 lowercase hex chars),
  3. writes the payload to the content-addressed blob `/tmp/store-<digest>.blob`,
  4. reads the existing registry `/tmp/pkgdb.txt` (a text database of
     `"<name> <digest>\n"` lines),
  5. appends the new `"<name> <digest>"` record and rewrites the registry,
  6. re-reads the registry from disk and looks the record back up **by name**
     (parsing the multi-line DB char-by-char), and
  7. exits 0 only when the looked-up digest matches the one just installed.

Where `fastpy-store` proved a *single* content-addressed round-trip, this proves
the **persistent name -> content mapping** a package manager needs: it parses a
pre-existing multi-line registry, appends to it, persists it, re-reads it, and
resolves records by name.  A clean exit is an end-to-end proof of: argv[1]/argv[2],
file read, hashing, hex formatting, writing to a computed path, read-modify-write
of a text database, and by-name lookup via string parsing/equality.

This is the fourth shipping fastpy SlateOS component (after `fastpy-cat`,
`fastpy-sysinfo`, and `fastpy-store`) and the first to maintain **persistent
state across a read-modify-write cycle** and resolve records **by name**.

Pure-mode caveats honored (see fastpy `known-issues.md`):
  * All helpers that subscript a string take a `str`-annotated parameter
    (`fnv1a(s: str)`, `to_hex8(v: int)`, `lookup(db: str, name: str)`) so `s[i]`
    lowers to the native `fastpy_str_index` rather than the CPython
    object-subscript bridge (BUG-SUBSCRIPT-UNTYPED-PARAM-BRIDGE).
  * The hash is 32-bit (`mask = 0xFFFFFFFF`, prime = 16777619) so every
    intermediate stays inside signed i64 — no bigint, avoiding
    BUG-BIGINT-MUL-INT-NULL.
  * Character comparisons use `ord(...)` integer compares (newline = 10, space =
    32) and name comparison uses `==` on strings, which lowers to the native
    `fastpy_str_compare` (strcmp) — all pure-mode-safe.
  * File reads are inline (never returned through a user function), sidestepping
    BUG-FILEREAD-FN-RETTAG.

Reference digests (32-bit FNV-1a, verified against CPython):
    "SlateOS package payload\n" -> a6fd63bc
    "SlateOS"                   -> 59f7e180
    "coreutils demo\n"          -> 1ee068f8

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_pkg`): it stages a payload and a one-line registry in
`/tmp`, grants a File capability, spawns the process with argv
`[fastpy-pkg, coreutils, <payload path>]`, and asserts it exits 0 (i.e. the
record was installed, persisted, and resolved by name).

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

# The package registry: content-address the payload into the store, then
# read-modify-write a persistent text DB of "<name> <digest>" records and
# resolve the record back by name.  32-bit FNV (kept inside i64, no bigint);
# all string-subscripting helpers carry explicit type annotations.  Exit 0 only
# when the persisted record resolves back to the digest just installed.
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
    "def lookup(db: str, name: str) -> str:\n"
    "    n = len(db)\n"
    "    i = 0\n"
    "    line = ''\n"
    "    while i <= n:\n"
    "        if i == n or ord(db[i]) == 10:\n"
    "            m = len(line)\n"
    "            j = 0\n"
    "            key = ''\n"
    "            while j < m and ord(line[j]) != 32:\n"
    "                key = key + line[j]\n"
    "                j = j + 1\n"
    "            if key == name:\n"
    "                val = ''\n"
    "                j = j + 1\n"
    "                while j < m:\n"
    "                    val = val + line[j]\n"
    "                    j = j + 1\n"
    "                return val\n"
    "            line = ''\n"
    "        else:\n"
    "            line = line + db[i]\n"
    "        i = i + 1\n"
    "    return ''\n"
    "name = sys.argv[1]\n"
    "src_path = sys.argv[2]\n"
    "f = open(src_path, 'r')\n"
    "payload = f.read()\n"
    "f.close()\n"
    "digest = to_hex8(fnv1a(payload))\n"
    "store_path = '/tmp/store-' + digest + '.blob'\n"
    "f = open(store_path, 'w')\n"
    "f.write(payload)\n"
    "f.close()\n"
    "db_path = '/tmp/pkgdb.txt'\n"
    "f = open(db_path, 'r')\n"
    "db = f.read()\n"
    "f.close()\n"
    "db = db + name + ' ' + digest + chr(10)\n"
    "f = open(db_path, 'w')\n"
    "f.write(db)\n"
    "f.close()\n"
    "f = open(db_path, 'r')\n"
    "db2 = f.read()\n"
    "f.close()\n"
    "print('== SlateOS pkg ==')\n"
    "print('install: ' + name + ' -> ' + digest)\n"
    "got = lookup(db2, name)\n"
    "print('lookup ' + name + ': ' + got)\n"
    "base = lookup(db2, 'base')\n"
    "print('lookup base: ' + base)\n"
    "if got == digest:\n"
    "    print('registry: ok ' + db_path)\n"
    "    sys.exit(0)\n"
    "print('registry: MISMATCH')\n"
    "sys.exit(1)\n"
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
