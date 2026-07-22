#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-chmod` SlateOS utility.

This produces `fastpy-chmod.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a `chmod`-style
permission-setting tool.  `chmod <mode> <path>` parses an octal mode from
`<mode>`, applies it to `<path>`, and exits with a code encoding the result:

    exit 0 — os.chmod succeeded
    exit 3 — os.chmod failed (SYS_FS_SET_PERMS rejected / errored)

New ground vs. the `size`/`ftype` tools (which only *read* metadata via
SYS_FS_STAT): this is the first fastpy tool to **mutate** a file's metadata.
The flow is:

  * `os.chmod(path, mode)` -> new native runtime `fastpy_os_chmod` -> posix
    libc `chmod()` -> kernel `SYS_FS_SET_PERMS` (gated on `Rights::WRITE`) ->
    `Vfs::set_permissions(path, mode)`, returning a bare int (0 ok / -1).

The mode is parsed from argv with pure-mode integer arithmetic (octal digit
`ord()` compares — the same pure-mode-safe helper shape used by fastpy-pkg's
`parse_int`), so the tool takes a human-facing octal string like "640".

The kernel self-test independently re-reads the file's metadata via the VFS
and asserts `FileMeta::permissions` now equals the requested bits — a no-op
chmod that returned 0 without persisting could not satisfy this.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.chmod(...)` lowers to a native `fastpy_os_chmod` call returning a bare
    int (0/-1), used directly as a branch condition,
  * the octal parse is pure integer arithmetic over `str`-indexed chars.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_chmod`) granting
`READ|WRITE`.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-chmod/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# chmod clone: parse an octal mode from argv[1], apply it to argv[2] via
# os.chmod, and encode the result in the exit code (0 ok / 3 failed).  os.chmod
# lowers natively to fastpy_os_chmod -> posix libc chmod() -> SYS_FS_SET_PERMS
# (Rights::WRITE); the kernel self-test re-reads FileMeta::permissions to
# confirm the bits actually persisted.
SRC = (
    "import os\n"
    "import sys\n"
    "def parse_oct(s: str) -> int:\n"
    "    v = 0\n"
    "    i = 0\n"
    "    n = len(s)\n"
    "    while i < n:\n"
    "        c = ord(s[i])\n"
    "        d = c - 48\n"
    "        if d < 0:\n"
    "            d = 0\n"
    "        if d > 7:\n"
    "            d = 0\n"
    "        v = v * 8 + d\n"
    "        i = i + 1\n"
    "    return v\n"
    "mode = parse_oct(sys.argv[1])\n"
    "path = sys.argv[2]\n"
    "rc = os.chmod(path, mode)\n"
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
    exe = toolchain.link_executable([obj], out / "fastpy-chmod.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
