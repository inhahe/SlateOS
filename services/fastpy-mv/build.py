#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-mv` SlateOS utility.

This produces `fastpy-mv.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `mv`(1) —
`mv <src> <dst>` renames the entry named by `argv[1]` to `argv[2]` and exits
0 on success (nonzero on failure).

New ground vs. every earlier fastpy tool: those read a file's *contents*
(cat/grep/wc/head/uniq/tail/sort/freq), *enumerate* a directory (ls), or
*delete* an entry (rm).  This is the first fastpy tool to **rename** a
filesystem entry — a distinct kernel path (`os.rename()` -> runtime
`fastpy_os_rename` -> posix C `rename()` (`file.rs`) -> kernel `SYS_FS_RENAME`
-> VFS rename).  So it exercises the rename OS surface end-to-end from a
fastpy program for the first time.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.rename(src, dst)` lowers to a native `fastpy_os_rename` call returning
    an int status (0 ok, -1 error) — no CPython bridge (added to fastpy
    codegen alongside the existing `os.remove`/`os.listdir` native lowerings),
  * the `src`/`dst` arguments are `sys.argv[1]`/`sys.argv[2]`; the status is
    returned as a plain int and used as the process exit code.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_mv`) that stages a known file (`/tmp/mv-src` with
known contents), runs `mv /tmp/mv-src /tmp/mv-dst`, asserts it exits 0, and —
the real verification — asserts via the VFS that the source is now gone *and*
the destination exists with the original contents byte-for-byte.  The
before/after VFS check means a no-op `os.rename` that merely returned 0
without renaming could not false-pass.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-mv/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Minimal `mv`: rename argv[1] -> argv[2].  os.rename lowers to the native
# fastpy_os_rename (posix rename -> SYS_FS_RENAME), returning 0 on success /
# -1 on error, which we hand straight to sys.exit so the self-test can assert
# the exit status *and* verify the rename via the VFS.
SRC = (
    "import os\n"
    "import sys\n"
    "rc = os.rename(sys.argv[1], sys.argv[2])\n"
    "sys.exit(rc)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-mv.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
