#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-size` SlateOS utility.

This produces `fastpy-size.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a `stat -c%s`-style
size query — `size <file>` reads the byte size of the file named by `argv[1]`
and exits with that size as the process exit code (so a caller — and the
self-test — can read the size directly).

New OS-surface ground vs. every prior fastpy tool: cat/grep/wc/head/uniq/
tail/sort/freq read a file's *contents* (`SYS_FS_READ`), ls *enumerates*
(`SYS_FS_LIST_DIR`), rm/mv/mkdir/rmdir *mutate* (`SYS_FS_DELETE`/`RENAME`/
`MKDIR`/`RMDIR`).  This is the first fastpy tool to read a file's **metadata**
— a distinct kernel path (`os.path.getsize()` -> runtime
`fastpy_os_path_getsize` -> posix C `stat()` (`file.rs`) -> kernel
`SYS_FS_STAT` -> VFS metadata).  It's the primitive a real `ls -l`, `du`, or
package manager (checking content-blob sizes) needs.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.path.getsize(path)` lowers to a native `fastpy_os_path_getsize` call
    returning the size as a bare int (-1 on error) — no CPython bridge (added
    to fastpy codegen: it was in the return-type table but not natively
    lowered, so pure mode previously hit the bridge and failed),
  * the `path` argument is `sys.argv[1]`; the size is returned as a plain int
    and used as the process exit code.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_size`) that stages `/tmp/size-input.txt` with a
known 42-byte payload, runs `size /tmp/size-input.txt`, and asserts it exits
with **exit code 42 == the byte count** (not merely exit 0 — the size flows
through the exit code, so a no-op getsize could not false-pass).  Because
`SYS_FS_STAT` gates on `Rights::METADATA`, the self-test grants
`READ|METADATA`.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-size/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Minimal size query: exit with the byte size of argv[1].  os.path.getsize
# lowers to the native fastpy_os_path_getsize (posix stat -> SYS_FS_STAT),
# returning the size as an int, which we hand straight to sys.exit so the
# self-test can assert the exact byte count via the exit code.
SRC = (
    "import os\n"
    "import sys\n"
    "n = os.path.getsize(sys.argv[1])\n"
    "sys.exit(n)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-size.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
