#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-ftype` SlateOS utility.

This produces `fastpy-ftype.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a file-type classifier
in the spirit of `test -f` / `test -d` — `ftype <path>` inspects the entry
named by `argv[1]` and exits with a code encoding its type:

    exit 0 — a regular file      (os.path.isfile)
    exit 1 — a directory         (os.path.isdir)
    exit 2 — neither / missing

New ground vs. `fastpy-size`: where `size` read only `st_size`, this reads the
**file-type bits of `st_mode`**.  Both flow from the same kernel metadata path
(`os.path.is{file,dir}()` -> runtime `fastpy_os_path_is{file,dir}` -> posix C
`stat()` -> kernel `SYS_FS_STAT` -> VFS metadata), but `isfile`/`isdir` apply
`S_ISREG`/`S_ISDIR` to `st_mode`, so this is the first fastpy tool to depend on
the posix libc populating `st_mode`'s type bits from the kernel `fsstat`
`entry_type` (`posix/src/stat.rs::fill_from_fsstat`: `entry_type == 1` ->
`S_IFDIR`, else `S_IFREG`).

It is also an **on-target regression test for a fastpy codegen fix**: the
`os.path.is{file,dir}(p)` calls are used in *assignment* form (`isf = ...`),
which — before the fix that shipped alongside `fastpy-size`'s
`os.path.getsize` — bridged chained `os.path.X(...)` calls to the (pure-mode
stubbed) CPython bridge, failing to link.  `_assign_fv_fast_path`'s
chained-native guard now matches the top-level module name (`os`), so *every*
`os.path.*` function lowers natively in assignment form; building this tool
bridge-free proves the fix generalizes past `getsize`.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.path.isfile(p)` / `os.path.isdir(p)` lower to native
    `fastpy_os_path_isfile` / `fastpy_os_path_isdir` calls returning a bare
    int (1/0) — used directly as truthy branch conditions,
  * the `path` argument is `sys.argv[1]`; the classification is returned as a
    plain int process exit code.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_ftype`) that stages a regular file and a directory
under `/tmp`, runs `ftype` against each (plus a nonexistent path), and asserts
the exit codes 0 / 1 / 2 respectively — so the type distinction flows through
the exit code and a stat that ignored `st_mode` could not false-pass.  Because
`SYS_FS_STAT` gates on `Rights::METADATA`, the self-test grants
`READ|METADATA`.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-ftype/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# File-type classifier: exit 0 (regular file) / 1 (directory) / 2 (neither).
# os.path.isfile/isdir are used in *assignment* form on purpose, to regression-
# test the codegen fix that routes chained os.path.X(...) natively in the
# assignment RHS (they lower to fastpy_os_path_is{file,dir} -> posix stat ->
# SYS_FS_STAT, reading st_mode's type bits).
SRC = (
    "import os\n"
    "import sys\n"
    "p = sys.argv[1]\n"
    "isf = os.path.isfile(p)\n"
    "isd = os.path.isdir(p)\n"
    "code = 2\n"
    "if isf:\n"
    "    code = 0\n"
    "if isd:\n"
    "    code = 1\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-ftype.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
