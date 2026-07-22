#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-samefile` SlateOS utility.

This produces `fastpy-samefile.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that tests file
identity via `os.path.samefile(a, b)`.

`os.path.samefile` is the **first fastpy lowering to exercise the `st_ino`
field**.  It lowers to `fastpy_os_path_samefile` -> two posix libc `stat()`
calls -> `SYS_FS_STAT`, comparing the `(st_dev, st_ino)` identity of the two
results.  Both stats **follow symlinks** (POSIX stat semantics; SlateOS
`SYS_FS_STAT` "Follows symlinks"), so a file and a symlink pointing at it are
the *same file* (same inode), while two independently-created files have
distinct synthetic inodes.  It returns a Python **bool**.

The self-check in a single binary:
  * create `TARGET` (argv[1]) and `OTHER` (argv[3]) — two distinct files, each
    with its own synthetic inode,
  * `os.symlink(TARGET, LINK)` (argv[2]) — a symlink resolving to TARGET,
  * `s1 = os.path.samefile(TARGET, LINK)`   -> True  (symlink resolves to
                                               TARGET's inode)
  * `s2 = os.path.samefile(TARGET, OTHER)`  -> False (distinct inodes)
  * `s3 = os.path.samefile(TARGET, TARGET)` -> True  (identical path)
  * write the 3-char pattern `"101"` (s1, s2, s3) to `/tmp/fastpy-samefile.out`,
  * exit 0 iff `os.symlink` succeeded AND `s1 and (not s2) and s3` (pattern
    exactly "101").

False-pass-proof design:
  * A stub returning a constant bool cannot produce "101" for these inputs.
  * The symlink-identity case `s1` can only be True if `stat()` actually follows
    the symlink and the inode is compared — a lstat-style (no-follow) or
    path-string comparison would give False.
  * The discrimination case `s2` proves distinct files yield distinct inodes; a
    samefile that ignored the inode (e.g. always-equal) would wrongly return
    True and fail.
  * The kernel self-test independently reads the VFS inode numbers
    (`FileMeta::ino`, which also follows symlinks) and asserts
    `ino(LINK) == ino(TARGET)` (validating `s1`) AND
    `ino(OTHER) != ino(TARGET)` (validating `s2`) — an independent readout of
    the exact identity the tool compared.

Exit code:
    exit 0 — samefile reported "101" (file==symlink, file!=other, file==file)
    exit 3 — symlink failed, or samefile reported the wrong identity pattern

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_samefile`)
granting `READ|WRITE|METADATA` (symlink/create need WRITE; stat needs METADATA).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-samefile/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# samefile probe: create two distinct files + a symlink to the first, then test
# three identity cases and write the 3-char '1'/'0' pattern the kernel reads
# back; exit 0 iff "101".
SRC = (
    "import os\n"
    "import sys\n"
    "target = sys.argv[1]\n"
    "link = sys.argv[2]\n"
    "other = sys.argv[3]\n"
    "f = open(target, 'w')\n"
    "f.write('x')\n"
    "f.close()\n"
    "h = open(other, 'w')\n"
    "h.write('y')\n"
    "h.close()\n"
    "rc = os.symlink(target, link)\n"
    "s1 = os.path.samefile(target, link)\n"    # True: symlink -> target inode
    "s2 = os.path.samefile(target, other)\n"   # False: distinct inodes
    "s3 = os.path.samefile(target, target)\n"  # True: identical path
    "s = ''\n"
    "if s1:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if s2:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if s3:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "g = open('/tmp/fastpy-samefile.out', 'w')\n"
    "g.write(s)\n"
    "g.close()\n"
    "code = 3\n"
    "if rc == 0:\n"
    "    if s1:\n"
    "        if not s2:\n"
    "            if s3:\n"
    "                code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-samefile.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
