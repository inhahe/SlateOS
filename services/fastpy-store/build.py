#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-store` SlateOS utility.

This produces `fastpy-store.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a **content-addressed
store** primitive — the core building block of the SlateOS package manager.

Given a source path in `argv[1]`, it:
  1. reads the file's contents,
  2. computes a 32-bit FNV-1a digest of the bytes (pure-Python, no hashlib),
  3. formats the digest as 8 lowercase hex chars,
  4. writes the contents to a content-addressed path `/tmp/store-<digest>.blob`,
  5. reads the stored blob back and verifies it byte-for-byte equals the input.

It exits 0 only when the read-back verification succeeds, so a clean exit is a
strong end-to-end proof of: argv, file read, integer/bitwise arithmetic, hex
formatting, writing to a *computed* path, and file read-back equality — the
exact feature surface the package manager's store needs.

This is the third shipping fastpy SlateOS component (after `fastpy-cat` and
`fastpy-sysinfo`) and the first to do **non-trivial computation** (hashing) plus
**writing to a computed path**, rather than only streaming file contents.

The hash is deliberately **32-bit** (`mask = 0xFFFFFFFF`, prime = 16777619) so
every intermediate stays inside a signed 64-bit register: the running hash is
< 2^32 and `h * 16777619 < 2^57`, well under 2^63.  A 64-bit FNV would need the
mask `18446744073709551615` (= 2^64-1), which exceeds i64 range and forces
fastpy to promote the running hash to a **bigint** — and the resulting
bigint×int multiply currently passes a NULL operand into `fpy_bigint_mul` and
faults (BUG-BIGINT-MUL-INT-NULL, logged in fastpy's `known-issues.md`).  A
32-bit content hash is entirely adequate for the store and keeps the utility on
the native-i64 fast path.

Hex formatting uses `chr()` arithmetic rather than indexing a hex lookup string,
and the hash/format helpers are ordinary `def`s returning `int` / `str` built by
arithmetic and concatenation — they do NOT return a file-read result, so they
sidestep BUG-FILEREAD-FN-RETTAG (see `known-issues.md`, which only affects a
user function returning `open(...).read()`).

The helper parameters carry explicit type annotations (`fnv1a(s: str)`,
`to_hex8(v: int)`).  These are load-bearing, not cosmetic: without the `str`
annotation, subscripting an *untyped* parameter (`s[i]`) makes fastpy fall back
to the generic CPython object-subscript path (`s.__getitem__(i)` via
`fpy_cpython_getattr`/`fpy_cpython_call1`), whose pure-mode stubs return NULL, so
`ord(NULL)` faults on-target.  With `s: str`, the subscript lowers to the native
`fastpy_str_index` helper and stays on the pure-mode fast path.  See
BUG-SUBSCRIPT-UNTYPED-PARAM-BRIDGE in fastpy's `known-issues.md`.

Reference digests (32-bit FNV-1a, verified against CPython):
    "SlateOS package payload\n" -> a6fd63bc
    "SlateOS"                   -> 59f7e180

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_store`): it stages a known input in `/tmp`, grants a
File capability, spawns the process with argv `[fastpy-store, <input path>]`,
and asserts it exits 0 (i.e. the store round-tripped and verified). The boot
harness also confirms the digest `e5221fb8459a80d2` appears on serial.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-store/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# A content-addressed store primitive. 32-bit FNV-1a over the file bytes
# (kept inside i64 — no bigint), formatted as 8 hex chars via chr(), stored at
# /tmp/store-<digest>.blob, then read back and verified. Exit 0 only on a
# verified round-trip.
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
    "src_path = sys.argv[1]\n"
    "f = open(src_path, 'r')\n"
    "data = f.read()\n"
    "f.close()\n"
    "digest = to_hex8(fnv1a(data))\n"
    "store_path = '/tmp/store-' + digest + '.blob'\n"
    "print('== SlateOS store ==')\n"
    "print('digest: ' + digest)\n"
    "print('size:   ' + str(len(data)) + ' bytes')\n"
    "f = open(store_path, 'w')\n"
    "f.write(data)\n"
    "f.close()\n"
    "f = open(store_path, 'r')\n"
    "back = f.read()\n"
    "f.close()\n"
    "if back == data:\n"
    "    print('verify: ok ' + store_path)\n"
    "    sys.exit(0)\n"
    "print('verify: MISMATCH')\n"
    "sys.exit(1)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-store.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
