#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-ls` SlateOS utility.

This produces `fastpy-ls.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a minimal `ls`(1) —
`ls <dir>` lists the names of the entries in the directory named by `argv[1]`,
prints one per line, and exits 0.

New ground vs. every earlier fastpy tool (cat/grep/wc/head/uniq/tail/sort/freq):
those all read a file's *contents* (`open`/`read` → `SYS_FS_READ`); this is the
first fastpy tool to **enumerate a directory** — a distinct kernel path
(`os.listdir()` → runtime `fastpy_os_listdir` → posix `opendir`/`readdir`
(`dirent.rs`) → kernel `SYS_FS_LIST_DIR` → VFS `readdir`).  So it exercises the
directory-listing OS surface end-to-end from a fastpy program for the first
time, reusing the now-proven native `list` container to hold the returned names.

Pure-mode notes (all verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.listdir(path)` lowers to a native `fastpy_os_listdir` call returning an
    `FpyList` of name strings (no CPython bridge),
  * the names are indexed (`names[i]`, native `fastpy_list_get_fv`) and printed
    in a `while` loop; the count is returned as a plain int,
  * the `path` argument is `str`-annotated (`list_dir(path: str) -> int`).

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_ls`) that stages a dedicated directory
(`/tmp/lsdir`) with three known files, runs `ls /tmp/lsdir`, and asserts it
exits with code 3 — the entry count returned by `os.listdir` — so an empty
listing (the original opendir arg3/buf_cap ABI bug) can no longer false-pass;
the serial log additionally shows each of the three printed names.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-ls/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

SRC = (
    "import os\n"
    "import sys\n"
    # List the entry names of directory `path`, one per line.  Returns the
    # number of entries.  `os.listdir` lowers to the native fastpy_os_listdir
    # (posix opendir/readdir → SYS_FS_LIST_DIR); the returned list is the
    # now-proven native list container.
    "def list_dir(path: str) -> int:\n"
    "    names = os.listdir(path)\n"
    "    m = len(names)\n"
    "    i = 0\n"
    "    while i < m:\n"
    "        print(names[i])\n"
    "        i = i + 1\n"
    "    return m\n"
    "count = list_dir(sys.argv[1])\n"
    # Exit with the entry count so the kernel self-test can assert the listing
    # actually returned every entry (exit_code == number of names), rather than
    # merely that the program exited 0 — the latter would false-pass on an empty
    # listing (which is exactly the opendir arg3/buf_cap ABI bug this tool first
    # exposed: the kernel needs the buffer capacity in arg3 or returns 0 names).
    "sys.exit(count)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-ls.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
