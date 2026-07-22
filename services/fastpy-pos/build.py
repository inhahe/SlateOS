#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-pos` SlateOS utility.

This produces `fastpy-pos.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that exercises
**positioned file I/O** — `os.pwrite(fd, data, offset)` and
`os.pread(fd, n, offset)` (posix pwrite()/pread()).  Positioned I/O reads and
writes at an explicit absolute file offset *without changing the fd's current
file offset* — a genuinely distinct path from write/read + lseek, and one no
prior fastpy tool touches.

The round-trip (on a raw file fd from `os.open`):
  * `fd = os.open(path, O_RDWR|O_CREAT|O_TRUNC, 0o644)`  -> SYS_FS_OPEN,
  * `os.write(fd, "ABCDEFGH")`   -> 8 bytes; the fd offset is now 8,
  * `os.pwrite(fd, "XY", 2)`     -> overwrites bytes at offset 2..4
                                    ("ABXYEFGH"); the fd offset stays 8,
  * `cur = os.lseek(fd, 0, 1)`   (SEEK_CUR) -> must still be 8,
  * `back = os.pread(fd, 4, 1)`  -> reads offset 1..5 ("BXYE"); offset stays 8.

False-pass-proof design:
  * If `os.pwrite` moved the fd offset (behaving like write+lseek), `cur` after
    it would not be 8.  The tool asserts `cur == 8`, so a positioned write that
    illegally advanced the offset is caught.
  * If `os.pwrite`/`os.pread` ignored the `offset` argument, the bytes read back
    would not be "BXYE" (they'd reflect writes/reads at the wrong place).  Only
    correct positioned I/O yields "BXYE" while leaving the offset at 8.
  * The kernel self-test knows the exact expected bytes ("BXYE") and asserts the
    file the tool wrote back (`/tmp/fastpy-pos.out`) contains exactly that.

Exit code encodes a self-consistency check:
    exit 0 — pread returned "BXYE" AND the fd offset stayed 8 AND write was 8
    exit 3 — any check failed (os.pwrite/os.pread mis-lowered)

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_pos`).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-pos/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# positioned-I/O probe: write 8 bytes sequentially, then pwrite "XY" at offset 2
# (overwriting in place without moving the fd offset), verify SEEK_CUR is still
# 8, then pread 4 bytes at offset 1 — which must reflect the pwrite — close,
# write the read bytes to a file the kernel reads back, and exit 0 iff the
# positioned ops hit the right offsets and left the fd offset untouched.
# O_RDWR|O_CREAT|O_TRUNC = 578; mode 0o644 = 420.
SRC = (
    "import os\n"
    "import sys\n"
    "fd = os.open('/tmp/fastpy-pos.dat', 578, 420)\n"
    "n = os.write(fd, \"ABCDEFGH\")\n"
    "os.pwrite(fd, \"XY\", 2)\n"
    "cur = os.lseek(fd, 0, 1)\n"
    "back = os.pread(fd, 4, 1)\n"
    "os.close(fd)\n"
    "f = open('/tmp/fastpy-pos.out', 'w')\n"
    "f.write(back)\n"
    "f.close()\n"
    "code = 3\n"
    "if back == \"BXYE\":\n"
    "    if cur == 8:\n"
    "        if n == 8:\n"
    "            code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-pos.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
