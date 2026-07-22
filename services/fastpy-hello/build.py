#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-hello` SlateOS fixture.

This produces `fastpy-hello.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native) and linked against the
posix `libc.a` sysroot. The kernel embeds the resulting ELF via
`include_bytes!("../../../services/fastpy-hello/fastpy-hello.elf")` in
`kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_tls`) to confirm on-target execution of a real
fastpy-built component — the "first real component" milestone (initiative F).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-hello/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

SRC = (
    "print('hello from fastpy on SlateOS')\n"
    "total = 0\n"
    "for x in [1, 2, 3, 4, 5]:\n"
    "    total += x\n"
    "print(total)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-hello.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
