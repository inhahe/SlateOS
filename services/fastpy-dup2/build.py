#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-dup2` SlateOS utility.

This produces `fastpy-dup2.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that exercises
`os.dup2(oldfd, newfd)` — the fd-redirection path.  Unlike `os.dup` (which
returns the lowest free fd), `dup2` installs a copy of `oldfd` at the
*caller-chosen* `newfd` (silently closing whatever was open there first),
sharing the same underlying kernel handle.  That close-then-alias-at-a-specific
-fd behavior is a distinct posix/kernel path from `dup`.

The round-trip:
  * `r, w = os.pipe()`             -> SYS_PIPE_CREATE (a kernel pipe + 2 fds),
  * `t = os.dup2(w, 9)`            -> installs w's handle at fd 9; returns 9,
  * `os.write(9, "DUP2_OK")`      -> writes through the dup2 *target* fd,
  * `back = os.read(r, 7)`        -> reads from the *original* read end.

False-pass-proof design:
  * We write **only through fd 9** — a number that becomes a valid fd solely
    because `dup2` installed the pipe write handle there.  If `dup2` were
    mis-lowered (didn't alias, or returned/targeted the wrong fd), `write(9)`
    would fail (bad fd), the pipe would stay empty, and `read(r, 7)` would block
    — the harness times out and the test fails (never a false pass).
  * The tool also asserts `dup2` *returned* the requested target fd (9).
  * The kernel self-test knows the exact constant ("DUP2_OK") and asserts the
    file the tool wrote back (`/tmp/fastpy-dup2.out`) contains exactly that.

Exit code encodes a self-consistency check:
    exit 0 — the bytes read from the original read end equal what was written
             through fd 9, dup2 returned 9, and the write reported 7 bytes
    exit 3 — any check failed (os.dup2 mis-lowered)

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_dup2`).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-dup2/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# dup2 redirection probe: create a kernel pipe, dup2 the write-end onto the
# caller-chosen fd 9, write a known message *through fd 9*, read it back from
# the *original* read end, close every fd, write the round-tripped message to a
# file the kernel reads back, and exit 0 iff dup2 aliased the pipe write handle
# at fd 9 (bytes survive) and returned the requested target fd.
SRC = (
    "import os\n"
    "import sys\n"
    "r, w = os.pipe()\n"
    "t = os.dup2(w, 9)\n"
    "msg = \"DUP2_OK\"\n"
    "n = os.write(9, msg)\n"
    "back = os.read(r, 7)\n"
    "os.close(9)\n"
    "os.close(w)\n"
    "os.close(r)\n"
    "f = open('/tmp/fastpy-dup2.out', 'w')\n"
    "f.write(back)\n"
    "f.close()\n"
    "code = 3\n"
    "if back == msg:\n"
    "    if t == 9:\n"
    "        if n == 7:\n"
    "            code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-dup2.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
