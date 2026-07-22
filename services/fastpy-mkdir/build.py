#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-mkdir` SlateOS utility.

This produces `fastpy-mkdir.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `mkdir`(1) —
`mkdir <dir>` creates the directory named by `argv[1]` and exits 0 on success
(nonzero on failure).

New ground vs. every earlier fastpy tool: those read a file's *contents*,
*enumerate* a directory (ls), *delete* an entry (rm), or *rename* one (mv).
This is the first fastpy tool to **create a directory** — a distinct kernel
path (`os.mkdir()` -> runtime `fastpy_os_mkdir` -> posix C `mkdir()`
(`file.rs`) -> kernel `SYS_FS_MKDIR` -> VFS mkdir).  So it exercises the
directory-creation OS surface end-to-end from a fastpy program for the first
time — a building block the package manager needs to lay out install roots.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.mkdir(path)` lowers to a native `fastpy_os_mkdir` call returning an
    int status (0 ok, -1 error) — no CPython bridge (added to fastpy codegen
    alongside the existing `os.remove`/`os.rename`/`os.listdir` lowerings).
    Python's `os.mkdir` defaults mode 0o777; the runtime passes 0777 on
    POSIX/SlateOS.
  * the `path` argument is `sys.argv[1]`; the status is returned as a plain
    int and used as the process exit code.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_mkdir`) that asserts via the VFS that the target
does *not* exist, runs `mkdir /tmp/fpy-mkdir`, asserts it exits 0, and — the
real verification — asserts via the VFS that the directory now exists and is
a directory.  The before/after VFS check means a no-op `os.mkdir` that merely
returned 0 without creating could not false-pass.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-mkdir/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Minimal `mkdir`: create the directory named by argv[1].  os.mkdir lowers to
# the native fastpy_os_mkdir (posix mkdir -> SYS_FS_MKDIR), returning 0 on
# success / -1 on error, which we hand straight to sys.exit so the self-test
# can assert the exit status *and* verify creation via the VFS.
SRC = (
    "import os\n"
    "import sys\n"
    "rc = os.mkdir(sys.argv[1])\n"
    "sys.exit(rc)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-mkdir.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
