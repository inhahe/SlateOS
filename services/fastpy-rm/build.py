#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-rm` SlateOS utility.

This produces `fastpy-rm.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `rm`(1) —
`rm <file>` deletes the directory entry named by `argv[1]` and exits 0 on
success (nonzero on failure).

New ground vs. every earlier fastpy tool: those either read a file's
*contents* (cat/grep/wc/head/uniq/tail/sort/freq) or *enumerate* a directory
(ls).  This is the first fastpy tool to **delete** a filesystem entry — a
distinct kernel path (`os.remove()` -> runtime `fastpy_os_remove` -> posix
C `remove()` (`stdio.rs`) -> `unlink` (`file.rs`) -> kernel `SYS_FS_DELETE`
-> VFS delete).  So it exercises the file-deletion OS surface end-to-end from
a fastpy program for the first time — the primitive that unblocks a package
manager `gc` subcommand (reclaiming unreferenced content-addressed blobs).

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.remove(path)` lowers to a native `fastpy_os_remove` call returning an
    int status (0 ok, -1 error) — no CPython bridge (added to fastpy codegen
    alongside the existing `os.listdir` native lowering),
  * the `path` argument is `sys.argv[1]`; the status is returned as a plain
    int and used as the process exit code.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_rm`) that stages a known file (`/tmp/rmfile`),
asserts via the VFS that it exists, runs `rm /tmp/rmfile`, asserts it exits 0,
and — the real verification — asserts via the VFS that the file is now gone.
The before/after VFS check means a no-op `os.remove` that merely returned 0
without deleting could not false-pass (the lesson from `fastpy-ls`'s empty
listing).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-rm/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Minimal `rm`: delete the entry named by argv[1].  os.remove lowers to the
# native fastpy_os_remove (posix remove/unlink -> SYS_FS_DELETE), returning
# 0 on success / -1 on error, which we hand straight to sys.exit so the
# self-test can assert the exit status *and* verify deletion via the VFS.
SRC = (
    "import os\n"
    "import sys\n"
    "rc = os.remove(sys.argv[1])\n"
    "sys.exit(rc)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-rm.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
