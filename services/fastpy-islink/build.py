#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-islink` SlateOS utility.

This produces `fastpy-islink.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that tests whether
a path is a symbolic link via `os.path.islink(path)`.

`os.path.islink` is the **first fastpy lowering to use `lstat` (no-follow)
semantics**.  It lowers to `fastpy_os_path_islink` -> one posix libc `lstat()`
call -> `SYS_FS_LSTAT`, testing `S_ISLNK(st_mode)`.  Crucially, `lstat` does
*not* follow the final symlink (SlateOS `SYS_FS_LSTAT` "stat a path without
following the final symlink") — so a symlink reports True *even when its target
is missing* (the link entry itself exists), whereas the follow-symlink
`stat()`-based `exists`/`isfile`/`isdir` would see the broken link as
nonexistent.  It returns a Python **bool**.

The self-check in a single binary:
  * create `TARGET` (argv[1]) — a regular file,
  * `os.symlink(TARGET, LINK)` (argv[2]) — a symlink resolving to TARGET,
  * `os.symlink(MISSING, DANGLING)` (argv[3] -> argv[4]) — a *dangling* symlink
    whose target (MISSING, argv[4]) is never created,
  * `l1 = os.path.islink(LINK)`     -> True  (symlink to an existing target)
  * `l2 = os.path.islink(TARGET)`   -> False (a regular file is not a link)
  * `l3 = os.path.islink(DANGLING)` -> True  (a symlink, though its target is
                                              missing — proves no-follow)
  * `l4 = os.path.islink(MISSING)`  -> False (the path does not exist; lstat
                                              fails)
  * write the 4-char pattern `"1010"` (l1,l2,l3,l4) to `/tmp/fastpy-islink.out`,
  * exit 0 iff both symlinks succeeded AND `l1 and (not l2) and l3 and (not l4)`
    (pattern exactly "1010").

False-pass-proof design:
  * A stub returning a constant bool cannot produce "1010".
  * The dangling-symlink case `l3` can only be True with genuine *lstat*
    (no-follow) semantics: a follow-symlink `stat()` on a broken link returns an
    error, so a `stat`-based implementation would report False (matching
    `exists`), while `islink` must report True.  This is the decisive
    discriminator between lstat and stat.
  * The regular-file case `l2` proves the tool does not just report True for
    anything that exists.
  * The kernel self-test independently `Vfs::lstat`s each path and asserts the
    entry types: LINK and DANGLING are `Symlink`, while TARGET is not — an
    independent readout of the exact identity the tool tested.

Exit code:
    exit 0 — islink reported "1010" (link, not-file, dangling-link, missing)
    exit 3 — a symlink failed, or islink reported the wrong pattern

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_islink`)
granting `READ|WRITE|CREATE|METADATA` (symlink/create need WRITE+CREATE; lstat
needs METADATA).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-islink/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# islink probe: create a regular file, a symlink to it, and a dangling symlink;
# then test the four islink cases and write the 4-char '1'/'0' pattern the
# kernel reads back; exit 0 iff "1010".
SRC = (
    "import os\n"
    "import sys\n"
    "target = sys.argv[1]\n"
    "link = sys.argv[2]\n"
    "dangling = sys.argv[3]\n"
    "missing = sys.argv[4]\n"
    "f = open(target, 'w')\n"
    "f.write('x')\n"
    "f.close()\n"
    "rc1 = os.symlink(target, link)\n"      # link -> existing target
    "rc2 = os.symlink(missing, dangling)\n" # dangling -> missing target
    "l1 = os.path.islink(link)\n"      # True: symlink to existing target
    "l2 = os.path.islink(target)\n"    # False: a regular file
    "l3 = os.path.islink(dangling)\n"  # True: symlink even though target missing
    "l4 = os.path.islink(missing)\n"   # False: nonexistent path
    "s = ''\n"
    "if l1:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if l2:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if l3:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if l4:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "g = open('/tmp/fastpy-islink.out', 'w')\n"
    "g.write(s)\n"
    "g.close()\n"
    "code = 3\n"
    "if rc1 == 0:\n"
    "    if rc2 == 0:\n"
    "        if l1:\n"
    "            if not l2:\n"
    "                if l3:\n"
    "                    if not l4:\n"
    "                        code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-islink.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
