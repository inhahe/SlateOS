#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-sysinfo` SlateOS utility.

This produces `fastpy-sysinfo.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a small `sysinfo` tool
that reads the kernel's **procfs** virtual files (`/proc/version`,
`/proc/uptime`, `/proc/meminfo`) and prints a short report to stdout.

It is the second shipping fastpy SlateOS component (after `fastpy-cat`), and
the first to read the *generated* procfs backend rather than the writable
`/tmp` memfs — proving that fastpy pure-mode file reads stream on-the-fly
kernel content correctly (the runtime `fpy_file_read` loops `fread` until a
short read signals EOF; procfs has no fixed on-disk size).

The reads are written **inline** (not via a helper function that returns the
read result) to avoid BUG-FILEREAD-FN-RETTAG (see `known-issues.md`): a user
function returning a file-read `str` is currently mis-typed `int`.

The kernel embeds the resulting ELF via `include_bytes!` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_sysinfo`) that grants the process a File capability,
spawns it, asserts it exits 0, and (via the boot harness) confirms the report —
including the `/proc/version` string `"MintOS kernel"` — appears on serial.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-sysinfo/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Read three procfs files inline and print a labelled report. Inline reads
# (rather than a `def cat(p): ...; return f.read()` helper) sidestep the
# user-function file-read return-tag bug documented in known-issues.md.
SRC = (
    "import sys\n"
    "print('== SlateOS sysinfo ==')\n"
    "f = open('/proc/version', 'r')\n"
    "ver = f.read()\n"
    "f.close()\n"
    "print('version: ' + ver, end='')\n"
    "f = open('/proc/uptime', 'r')\n"
    "up = f.read()\n"
    "f.close()\n"
    "print('uptime:  ' + up, end='')\n"
    "f = open('/proc/meminfo', 'r')\n"
    "mem = f.read()\n"
    "f.close()\n"
    "print('meminfo: ' + str(len(mem)) + ' bytes')\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-sysinfo.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
