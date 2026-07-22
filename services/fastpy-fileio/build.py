#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-fileio` SlateOS fixture.

This produces `fastpy-fileio.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native) that exercises **pure-mode
file I/O on-target**: it opens a file in the kernel's writable `/tmp` memfs,
writes to it, closes it, reopens it read-only, reads the contents back, and
exits with the number of bytes read.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_fileio`) that grants the process a File capability,
spawns it, and asserts it exits with the expected byte count — proving the whole
pure-mode file path end-to-end: fastpy `open()`/`write()`/`read()`/`close()` ->
runtime native file object (C stdio) -> posix `libc.a` `fopen`/`fwrite`/`fread`
-> `SYS_FS_OPEN`/`SYS_FS_READ`/`SYS_FS_WRITE` -> kernel VFS/memfs.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-fileio/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Write "slate\n" (6 bytes) then read it back; exit(len(data)) == 6.
# No print(): a spawned native process has no console handle for fd 1, and this
# test only needs the exit code to prove the file round-trip worked on-target.
SRC = (
    "import sys\n"
    "f = open('/tmp/fpyio.txt', 'w')\n"
    "f.write('slate\\n')\n"
    "f.close()\n"
    "f = open('/tmp/fpyio.txt', 'r')\n"
    "data = f.read()\n"
    "f.close()\n"
    "sys.exit(len(data))\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-fileio.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
