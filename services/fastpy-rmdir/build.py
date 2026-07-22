#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-rmdir` SlateOS utility.

This produces `fastpy-rmdir.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `rmdir`(1) —
`rmdir <dir>` removes the (empty) directory named by `argv[1]` and exits 0 on
success (nonzero on failure).

New ground vs. `fastpy-mkdir` (which *creates* a directory via SYS_FS_MKDIR)
and `fastpy-rm` (which deletes a *file* via SYS_FS_DELETE): this removes a
*directory* — a distinct kernel path (`os.rmdir()` -> runtime
`fastpy_os_rmdir` -> posix C `rmdir()` (`file.rs`) -> kernel `SYS_FS_RMDIR`
-> VFS rmdir).  It completes the os.mkdir/os.rmdir/os.rename trilogy added to
fastpy's codegen/runtime.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.rmdir(path)` lowers to a native `fastpy_os_rmdir` call returning an
    int status (0 ok, -1 error) — no CPython bridge,
  * the `path` argument is `sys.argv[1]`; the status is returned as a plain
    int and used as the process exit code.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_rmdir`) that pre-creates `/tmp/fpy-rmdir` (via the
VFS), asserts it exists, runs `rmdir /tmp/fpy-rmdir`, asserts it exits 0, and —
the real verification — asserts via the VFS that the directory is now gone.
Because `SYS_FS_RMDIR` gates on `Rights::DELETE`, the self-test grants
`READ|WRITE|DELETE` (the same right `rm` needs).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-rmdir/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Minimal `rmdir`: remove the directory named by argv[1].  os.rmdir lowers to
# the native fastpy_os_rmdir (posix rmdir -> SYS_FS_RMDIR), returning 0 on
# success / -1 on error, which we hand straight to sys.exit so the self-test
# can assert the exit status *and* verify removal via the VFS.
SRC = (
    "import os\n"
    "import sys\n"
    "rc = os.rmdir(sys.argv[1])\n"
    "sys.exit(rc)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-rmdir.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
