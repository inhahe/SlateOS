#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-chown` SlateOS utility.

This produces `fastpy-chown.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a `chown`-style
ownership-setting tool.  `chown <uid> <gid> <path>` parses two decimal ids and
sets them on `<path>`, exiting with a code encoding the result:

    exit 0 — os.chown succeeded
    exit 3 — os.chown failed (SYS_FS_SET_OWNER rejected / errored)

New ground vs. the `settimes` tool (timestamps via SYS_FS_SET_TIMES): this
completes the metadata-mutation trio (permissions via `chmod`, times via
`settimes`, **owner** here).  The flow is:

  * `os.chown(path, uid, gid)` -> new native runtime `fastpy_os_chown` ->
    posix libc `chown()` -> kernel `SYS_FS_SET_OWNER` (gated on
    `Rights::WRITE`) -> `Vfs::set_owner(path, uid, gid)`.

Like `os.utime`, this is a THREE-positional `os.*` native (a path plus two i64
ids) — an AOT-simplified form of Python's `os.chown(path, uid, gid)` that
surfaces the result as a bare int.  Setting uid and gid to DISTINCT values lets
the kernel self-test verify each field independently — a stronger false-pass
check than a single id.

The ids are parsed from argv with pure-mode decimal integer arithmetic (digit
`ord()` compares — the same pure-mode-safe helper shape used by
fastpy-settimes's `parse_dec`).

The kernel self-test independently re-reads the file's metadata via the VFS and
asserts `FileMeta::uid` == uid and `FileMeta::gid` == gid — a no-op chown that
returned 0 without stamping could not satisfy this.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.chown(...)` lowers to a native `fastpy_os_chown` call returning a bare
    int (0/-1), used directly as a branch condition,
  * the decimal parse is pure integer arithmetic over `str`-indexed chars.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_chown`) granting
`READ|WRITE`.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-chown/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# chown clone: parse two decimal ids from argv[1]/argv[2] and apply them to
# argv[3] via os.chown, encoding the result in the exit code (0 ok / 3 failed).
# os.chown lowers natively to fastpy_os_chown -> posix libc chown() ->
# SYS_FS_SET_OWNER (Rights::WRITE); the kernel self-test re-reads FileMeta::uid
# / gid to confirm the ids took.
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
    "uid = parse_dec(sys.argv[1])\n"
    "gid = parse_dec(sys.argv[2])\n"
    "path = sys.argv[3]\n"
    "rc = os.chown(path, uid, gid)\n"
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
    exe = toolchain.link_executable([obj], out / "fastpy-chown.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
