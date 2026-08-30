#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-ftruncate` SlateOS utility.

This produces `fastpy-ftruncate.elf`, a native SlateOS (`x86_64-slateos`)
binary compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that
exercises the fd-based file **truncate** path — `os.ftruncate(fd, length)` ->
posix ftruncate() -> `SYS_FS_FTRUNCATE`.  This is a genuinely distinct kernel
syscall from the path-based `os.truncate()` (`SYS_FS_TRUNCATE`) and from
lseek/write, and no prior fastpy tool touches it.

The round-trip (on a raw file fd from `os.open`):
  * `fd = os.open(path, O_RDWR|O_CREAT|O_TRUNC, 0o644)`  -> SYS_FS_OPEN,
  * `os.write(fd, "ABCDEFGH")`                          -> 8 bytes,
  * `os.ftruncate(fd, 3)`                               -> file is now 3 bytes,
  * `endpos = os.lseek(fd, 0, 2)`  (SEEK_END)            -> new size == 3,
  * `os.lseek(fd, 0, 0)` then `os.read(fd, 8)`          -> only "ABC" survives.

False-pass-proof design:
  * If `os.ftruncate` were a no-op or mis-lowered, the file would still be 8
    bytes: `SEEK_END` would report 8 and the read would return "ABCDEFGH".
    Only a real kernel truncation leaves the file at 3 bytes, so both the
    reported end-offset (3) and the read bytes ("ABC") can only be right if
    `SYS_FS_FTRUNCATE` actually shrank the file.
  * The kernel self-test knows the exact expected bytes ("ABC") and asserts the
    file the tool wrote back (`/tmp/fastpy-ftruncate.out`) contains exactly
    that.

Exit code encodes a self-consistency check:
    exit 0 — the post-truncate read returned "ABC", SEEK_END returned 3,
             the write returned 8
    exit 3 — any check failed (os.ftruncate mis-lowered)

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_ftruncate`).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-ftruncate/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# ftruncate probe: open a regular file for raw-fd I/O, write 8 known bytes,
# truncate the file to 3 bytes, confirm via SEEK_END that the new size is 3,
# rewind and read — which must yield only the surviving "ABC" — close, write
# the read bytes to a file the kernel reads back, and exit 0 iff the truncate
# really shrank the file. O_RDWR|O_CREAT|O_TRUNC = 578; mode 0o644 = 420.
SRC = (
    "import os\n"
    "import sys\n"
    "fd = os.open('/tmp/fastpy-ftruncate.dat', 578, 420)\n"
    "n = os.write(fd, \"ABCDEFGH\")\n"
    "os.ftruncate(fd, 3)\n"
    "endpos = os.lseek(fd, 0, 2)\n"
    "os.lseek(fd, 0, 0)\n"
    "back = os.read(fd, 8)\n"
    "os.close(fd)\n"
    "f = open('/tmp/fastpy-ftruncate.out', 'w')\n"
    "f.write(back)\n"
    "f.close()\n"
    "code = 3\n"
    "if back == \"ABC\":\n"
    "    if endpos == 3:\n"
    "        if n == 8:\n"
    "            code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-ftruncate.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
