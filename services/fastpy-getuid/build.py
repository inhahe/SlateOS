#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-getuid` SlateOS utility.

This produces `fastpy-getuid.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that reads its own
process **identity** via `os.getuid()` and `os.getgid()`.

`os.getuid`/`os.getgid` are genuinely distinct from the filesystem stat-family
lowerings.  Each is a bridge-free `fastpy_os_get{u,g}id` -> posix
`get{u,g}id()` -> the native `SYS_PROCESS_GET_CREDENTIALS` syscall, which reads
the calling process's `ProcessCredentials` (set by the kernel at spawn from
`SpawnOptions.uid_gid`) — a distinct kernel path (the process-credentials
table) from the pid/tid identity syscalls and from every filesystem syscall.

Until this work, the posix `getuid()`/`getgid()` were hardcoded stubs returning
0; this utility exists to prove the whole chain (fastpy lowering -> posix ->
kernel credentials) now returns the *real* identity.

The self-check in a single binary:
  * `u = os.getuid()`, `g = os.getgid()`,
  * write `"<u>,<g>"` (decimal) to `/tmp/fastpy-getuid.out`,
  * exit 0 (the kernel does the authoritative check on the written values).

False-pass-proof design:
  * The kernel self-test spawns this tool with a **non-root** identity via
    `SpawnOptions.uid_gid = (EXPECTED_UID, EXPECTED_GID)` using two distinct,
    non-obvious values.  It then asserts the tool's written `"<u>,<g>"` equals
    exactly `"<EXPECTED_UID>,<EXPECTED_GID>"` AND that
    `pcb::get_credentials(pid)` stored the same pair.
  * The old stub (return 0) would write `"0,0"` and fail immediately; a
    hardcoded-constant stub cannot know the spawn-time uid/gid the kernel chose,
    and the two ids being distinct rules out a single-field coincidence.

Exit code:
    exit 0 always — the kernel validates the written identity, not the exit
    code (there is nothing for the tool itself to decide).

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_getuid`).  No
filesystem capabilities beyond writing the output file are needed
(`Rights::WRITE` for `/tmp`); the credential syscall needs no capability.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-getuid/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# identity probe: read uid/gid and write them as "<uid>,<gid>" decimal for the
# kernel to cross-check against the spawn-time credentials it chose.
SRC = (
    "import os\n"
    "u = os.getuid()\n"
    "g = os.getgid()\n"
    "s = str(u) + ',' + str(g)\n"
    "g2 = open('/tmp/fastpy-getuid.out', 'w')\n"
    "g2.write(s)\n"
    "g2.close()\n"
    "import sys\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-getuid.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
