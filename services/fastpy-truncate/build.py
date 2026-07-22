#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-truncate` SlateOS utility.

This produces `fastpy-truncate.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a `truncate`-style
file-resize tool.  `truncate <size> <path>` parses a decimal byte count from
`<size>`, resizes `<path>` to exactly that many bytes, and exits with a code
encoding the result:

    exit 0 — os.truncate succeeded
    exit 3 — os.truncate failed (SYS_FS_TRUNCATE rejected / errored)

New ground vs. the `chmod` tool (which mutates *metadata* via
SYS_FS_SET_PERMS): this is the first fastpy tool to **resize a file's
content** — shrinking discards the tail, growing zero-fills.  The flow is:

  * `os.truncate(path, size)` -> new native runtime `fastpy_os_truncate` ->
    posix libc `truncate()` -> kernel `SYS_FS_TRUNCATE` (gated on
    `Rights::WRITE`) -> `Vfs::truncate(path, size)`, returning a bare int.

The size is parsed from argv with pure-mode decimal integer arithmetic (digit
`ord()` compares — the same pure-mode-safe helper shape used by fastpy-pkg's
`parse_int`), so the tool takes a human-facing decimal string like "8".

The kernel self-test independently re-reads the file's metadata via the VFS
and asserts `FileMeta::size` now equals the requested byte count, and that
reading the file back returns exactly the surviving prefix — a no-op truncate
that returned 0 without resizing could not satisfy this.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.truncate(...)` lowers to a native `fastpy_os_truncate` call returning a
    bare int (0/-1), used directly as a branch condition,
  * the decimal parse is pure integer arithmetic over `str`-indexed chars.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_truncate`)
granting `READ|WRITE`.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-truncate/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# truncate clone: parse a decimal byte count from argv[1], resize argv[2] to
# that size via os.truncate, and encode the result in the exit code (0 ok /
# 3 failed).  os.truncate lowers natively to fastpy_os_truncate -> posix libc
# truncate() -> SYS_FS_TRUNCATE (Rights::WRITE); the kernel self-test re-reads
# FileMeta::size to confirm the resize actually took effect.
SRC = (
    "import os\n"
    "import sys\n"
    "def parse_dec(s: str) -> int:\n"
    "    v = 0\n"
    "    i = 0\n"
    "    n = len(s)\n"
    "    while i < n:\n"
    "        c = ord(s[i])\n"
    "        d = c - 48\n"
    "        if d < 0:\n"
    "            d = 0\n"
    "        if d > 9:\n"
    "            d = 0\n"
    "        v = v * 10 + d\n"
    "        i = i + 1\n"
    "    return v\n"
    "size = parse_dec(sys.argv[1])\n"
    "path = sys.argv[2]\n"
    "rc = os.truncate(path, size)\n"
    "code = 3\n"
    "if rc == 0:\n"
    "    code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-truncate.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
