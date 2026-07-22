#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-cat` SlateOS utility.

This produces `fastpy-cat.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native) that is the **first
shipping fastpy SlateOS component**: a minimal `cat`(1) — it opens the file
named by its first argument, reads the whole contents, writes them verbatim to
stdout, and exits with the number of bytes read.

It exercises three on-target paths at once, all now proven working:
  * argv delivery — `sys.argv[1]` (kernel `SYS_PROCESS_GET_ARGS` -> crt ->
    runtime `fpy_argv` -> `sys.argv`),
  * pure-mode file I/O — `open()`/`read()`/`close()` (runtime native file
    object -> posix `libc.a` `fopen`/`fread`/`fclose` -> `SYS_FS_*` -> VFS),
  * stdout — `print(..., end='')` (runtime `printf`/`fflush` -> posix libc
    `write(1, ...)` -> Console handle -> `SYS_CONSOLE_WRITE`).

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_cat`) that stages a known file in `/tmp`, grants the
process a File capability, spawns it with the staged path as `argv[1]`, and
asserts it exits with the expected byte count (and that its output appears on
serial).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-cat/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Minimal `cat`: read the file named by argv[1] and echo it to stdout with no
# added trailing newline (real cat preserves the file bytes exactly), then
# exit(len(data)) so the self-test can verify the byte count on-target.
SRC = (
    "import sys\n"
    "f = open(sys.argv[1], 'r')\n"
    "data = f.read()\n"
    "f.close()\n"
    "print(data, end='')\n"
    "sys.exit(len(data))\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-cat.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
