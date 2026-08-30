#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-lseek` SlateOS utility.

This produces `fastpy-lseek.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that exercises two
genuinely-new kernel paths for fastpy — raw-fd file **open** (`os.open` ->
posix open() -> SYS_FS_OPEN) and file-offset **seek** (`os.lseek` -> posix
lseek() -> SYS_FS_SEEK).

Every other fastpy tool opens files through the high-level builtin `open()`
(FILE*-based buffered I/O).  This one instead uses `os.open` to obtain a *raw
integer file fd*, drives it with native raw-fd `os.write`/`os.read`/`os.close`,
and repositions its kernel file offset with `os.lseek` — a path no prior fastpy
tool touches.

The round-trip:
  * `fd = os.open(path, O_RDWR|O_CREAT|O_TRUNC, 0o644)`  -> SYS_FS_OPEN,
  * `os.write(fd, "ABCDEFGH")`                          -> 8 bytes at offset 0,
  * `pos = os.lseek(fd, 4, 0)`  (SEEK_SET)               -> repositions to 4,
  * `back = os.read(fd, 4)`                              -> reads from offset 4.

False-pass-proof design:
  * If `os.lseek` were a no-op or mis-lowered, the read after it would return
    the bytes at offset 0 (`"ABCD"`), not offset 4 (`"EFGH"`).  Only a real
    kernel-offset reposition yields `"EFGH"`, so the read bytes cannot be faked.
  * The tool also asserts `os.lseek` *returned* the new absolute offset 4 and
    that the write reported 8 bytes.
  * The kernel self-test knows the exact expected bytes (`"EFGH"`) and asserts
    the file the tool wrote back (`/tmp/fastpy-lseek.out`) contains exactly
    that.
  * `os.open` returning a valid (non-negative) fd is itself proven: a bad fd
    would make the subsequent write/lseek/read all fail and the round-trip
    would not match.

Exit code encodes a self-consistency check:
    exit 0 — read-after-seek returned "EFGH", lseek returned 4, write returned 8
    exit 3 — any check failed (os.open / os.lseek mis-lowered)

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_lseek`).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-lseek/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# lseek round-trip probe: open a regular file for raw-fd I/O, write 8 known
# bytes, seek the kernel file offset to 4 (SEEK_SET), read 4 bytes back — which
# must be the bytes at offset 4, not offset 0 — close, write the read bytes to
# a file the kernel reads back, and exit 0 iff the seek truly repositioned the
# offset. O_RDWR|O_CREAT|O_TRUNC = 2|0o100|0o1000 = 578; mode 0o644 = 420.
SRC = (
    "import os\n"
    "import sys\n"
    "fd = os.open('/tmp/fastpy-lseek.dat', 578, 420)\n"
    "n = os.write(fd, \"ABCDEFGH\")\n"
    "pos = os.lseek(fd, 4, 0)\n"
    "back = os.read(fd, 4)\n"
    "os.close(fd)\n"
    "f = open('/tmp/fastpy-lseek.out', 'w')\n"
    "f.write(back)\n"
    "f.close()\n"
    "code = 3\n"
    "if back == \"EFGH\":\n"
    "    if pos == 4:\n"
    "        if n == 8:\n"
    "            code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-lseek.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
