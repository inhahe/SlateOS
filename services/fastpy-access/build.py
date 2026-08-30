#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-access` SlateOS utility.

This produces `fastpy-access.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that probes file
accessibility via `os.access(path, mode)`.

Unlike the `os.path.get{a,m,c}time` trio (which read *stat fields* back), this
exercises a **distinct kernel/libc entry point**: `os.access` lowers to
`fastpy_os_access` -> posix libc `access()` -> `SYS_FS_STAT`, and — crucially —
the **mode argument is genuinely honored**.  POSIX `access()` validates the mode
bits: any bit outside `R_OK|W_OK|X_OK` (i.e. `mode & ~7 != 0`) is rejected with
`EINVAL`.  So `os.access(path, 8)` returns **False even for an existing file**,
proving the mode reached the syscall rather than being ignored.  `os.access`
returns a Python **bool** (the first top-level `os.*` function to do so; the
existing bool returns are all `os.path.*`).

The self-check in a single binary probes four cases and writes a 4-char
`'1'/'0'` pattern the kernel reads back:
  * `a = os.access(TARGET, 0)`   -> F_OK on an existing file       -> True  ('1')
  * `b = os.access(TARGET, 7)`   -> R|W|X (all valid) on existing  -> True  ('1')
  * `c = os.access(TARGET, 8)`   -> invalid mode bit (EINVAL)      -> False ('0')
  * `d = os.access(MISSING, 0)`  -> F_OK on a nonexistent path     -> False ('0')

  * write `"1100"` (a,b,c,d) to `/tmp/fastpy-access.out`,
  * exit 0 iff `a and b and (not c) and (not d)` — i.e. the pattern is exactly
    "1100".

False-pass-proof design:
  * A stub that always returns True fails case `c`/`d` (writes '1' where '0' is
    required) -> exits 3.  A stub that always returns False fails `a`/`b`.
  * Case `c` (mode 8 -> False on an *existing* file) can only be produced if the
    mode argument is actually forwarded to `access()` and validated — a getsize-
    style existence-only wiring that ignored the mode would return True here and
    fail.
  * The kernel self-test independently confirms via the VFS that TARGET exists
    and MISSING does not (anchoring cases `a` and `d`), and asserts the tool
    wrote exactly "1100".

Exit code:
    exit 0 — access reported "1100" (existing accessible, invalid-mode rejected,
             missing inaccessible)
    exit 3 — any case disagreed (stub, mode ignored, or wrong existence result)

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_access`)
granting `READ|WRITE|METADATA` (access -> stat needs `Rights::METADATA`;
WRITE creates the probe file).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-access/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# access probe: create argv[1] (an existing file), then probe four cases and
# write the 4-char '1'/'0' pattern the kernel reads back; exit 0 iff "1100".
SRC = (
    "import os\n"
    "import sys\n"
    "path = sys.argv[1]\n"
    "missing = sys.argv[2]\n"
    "f = open(path, 'w')\n"
    "f.write('x')\n"
    "f.close()\n"
    "a = os.access(path, 0)\n"      # F_OK, exists -> True
    "b = os.access(path, 7)\n"      # R|W|X valid, exists -> True
    "c = os.access(path, 8)\n"      # invalid mode bit -> False (EINVAL)
    "d = os.access(missing, 0)\n"   # F_OK, missing -> False
    "s = ''\n"
    "if a:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if b:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if c:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "if d:\n"
    "    s = s + '1'\n"
    "else:\n"
    "    s = s + '0'\n"
    "g = open('/tmp/fastpy-access.out', 'w')\n"
    "g.write(s)\n"
    "g.close()\n"
    "code = 3\n"
    "if a:\n"
    "    if b:\n"
    "        if not c:\n"
    "            if not d:\n"
    "                code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-access.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
