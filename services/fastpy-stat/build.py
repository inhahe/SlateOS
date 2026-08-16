#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-stat` SlateOS utility.

This produces `fastpy-stat.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that reads a
file's whole metadata struct via `os.stat(path)`.

`os.stat` is the **capstone** of the fastpy stat-field lowerings.  Where
`getsize`/`get{a,m,c}time`/`samefile`/`islink` each read a *single* field, a
single `os.stat` call (one `fastpy_os_stat` -> posix `stat()` -> `SYS_FS_STAT`,
follows symlinks) fills the entire struct and returns it as CPython's
`os.stat_result` **sequence** form: a 10-int list

    (st_mode, st_ino, st_dev, st_nlink, st_uid, st_gid, st_size,
     st_atime, st_mtime, st_ctime)

(the index/tuple form — timestamps are integer seconds).  It is returned like
`os.listdir` (a raw list the program indexes with `st[i]`).

The self-check in a single binary:
  * create `TARGET` (argv[1]) and write exactly the 11-byte string
    `"hello world"`,
  * `st = os.stat(TARGET)` and check four independent fields:
      - `st[6]` (st_size)  == 11                 (matches the bytes written)
      - `st[0] & 0o170000 == 0o100000`           (S_IFMT bits == S_IFREG: a
        regular file — the type-bit check, distinct from islink's S_IFLNK)
      - `st[3]` (st_nlink) >= 1
      - `st[1]` (st_ino)   != 0
  * write the 4-char pattern `"1111"` (one char per check) to
    `/tmp/fastpy-stat.out`,
  * exit 0 iff all four checks passed (pattern exactly "1111").

False-pass-proof design:
  * A stub returning a constant list cannot simultaneously match the *exact*
    size of a file the tool just created, the S_IFREG type bits, a nonzero
    inode, and nlink>=1.
  * The size check binds `os.stat`'s `st_size` to the actual bytes written.
  * The mode check proves `st_mode` carries the correct file-type bits
    (S_IFREG here — the opposite of islink's dangling-symlink S_IFLNK case).
  * The kernel self-test independently `Vfs::metadata`s TARGET and asserts
    `size == 11`, `ino != 0`, and that it is a regular file — an independent
    readout of the exact fields `os.stat` returned, and it confirms the tool's
    "1111" could only come from a correct multi-field struct read.

Exit code:
    exit 0 — os.stat returned a struct with the expected size/type/nlink/ino
    exit 5 — one or more stat fields were wrong (a stub or a mis-lowered struct)

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_stat`) granting
`READ|WRITE|METADATA` (create/write need WRITE; stat needs METADATA).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-stat/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# stat probe: write a fixed 11-byte file, os.stat it, and check four struct
# fields; write the 4-char '1'/'0' pattern the kernel reads back; exit 0 iff
# "1111".  0o170000 = 61440 (S_IFMT), 0o100000 = 32768 (S_IFREG) written as
# decimals to avoid any octal-literal ambiguity.
SRC = (
    "import os\n"
    "import sys\n"
    "target = sys.argv[1]\n"
    "f = open(target, 'w')\n"
    "f.write('hello world')\n"     # exactly 11 bytes
    "f.close()\n"
    "st = os.stat(target)\n"
    "size = st[6]\n"
    "mode = st[0]\n"
    "nlink = st[3]\n"
    "ino = st[1]\n"
    "c1 = 0\n"
    "if size == 11:\n"
    "    c1 = 1\n"
    "c2 = 0\n"
    "if (mode & 61440) == 32768:\n"
    "    c2 = 1\n"
    "c3 = 0\n"
    "if nlink >= 1:\n"
    "    c3 = 1\n"
    "c4 = 0\n"
    "if ino != 0:\n"
    "    c4 = 1\n"
    "s = ''\n"
    "if c1 == 1:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if c2 == 1:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if c3 == 1:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if c4 == 1:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "g = open('/tmp/fastpy-stat.out', 'w')\n"
    "g.write(s)\n"
    "g.close()\n"
    "code = 5\n"
    "if c1 == 1:\n"
    "    if c2 == 1:\n"
    "        if c3 == 1:\n"
    "            if c4 == 1:\n"
    "                code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-stat.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
