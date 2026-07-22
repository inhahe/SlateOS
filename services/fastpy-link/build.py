#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-link` SlateOS utility.

This produces `fastpy-link.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): an `ln`-style HARD-link
tool.  `link <target> <linkpath>` creates `linkpath` as a hard link to the same
inode as `target`, reads the file back *through the new name*, and exits with a
code encoding the result:

    exit 0 — link created AND a read through <linkpath> returned data
    exit 3 — os.link failed (SYS_FS_LINK rejected / errored)
    exit 4 — link created but the read-back through it was empty (broken)

New ground vs. `fastpy-symlink` (which drove SYS_FS_SYMLINK/READLINK): this is
the first fastpy tool to create a **hard link** — a second directory entry
pointing at the *same inode* as an existing file, not a separate object storing
a target string.  The flow is:

  * `os.link(target, linkpath)` -> new native runtime `fastpy_os_link` -> posix
    libc `link()` -> kernel `SYS_FS_LINK` (gated on `Rights::CREATE`) ->
    `Vfs::link_no_follow(target, linkpath)`, returning a bare int (0 ok / -1).

Reading the file's contents *through the new name* proves the link resolves to
the target's data.  The kernel self-test additionally re-reads both names via
the VFS and asserts they report the **same inode** (`FileMeta::ino`) — the
defining property of a hard link, which a mere copy could not satisfy.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.link(...)` lowers to a native `fastpy_os_link` call returning a bare
    int (0/-1), used directly as a branch condition,
  * the read-back uses the native pure-mode file object (`open`/`read`/`close`
    -> C stdio -> `SYS_FS_OPEN`/`SYS_FS_READ`).

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_link`) granting
`READ|CREATE`.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-link/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# ln (hard link) clone: create linkpath as a hard link to target, read the
# contents back through the new name, and encode the result in the exit code
# (0 verified / 3 create failed / 4 empty-readback).  os.link lowers natively to
# fastpy_os_link -> posix libc link() -> SYS_FS_LINK (Rights::CREATE); the
# read-back proves the new name resolves to the target's data.
SRC = (
    "import os\n"
    "import sys\n"
    "target = sys.argv[1]\n"
    "linkpath = sys.argv[2]\n"
    "rc = os.link(target, linkpath)\n"
    "code = 3\n"
    "if rc == 0:\n"
    "    f = open(linkpath, 'r')\n"
    "    data = f.read()\n"
    "    f.close()\n"
    "    n = len(data)\n"
    "    if n > 0:\n"
    "        code = 0\n"
    "    else:\n"
    "        code = 4\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-link.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
