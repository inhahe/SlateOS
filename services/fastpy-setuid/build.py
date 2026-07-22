#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-setuid` SlateOS utility.

This produces `fastpy-setuid.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that *changes* its
own process **identity** via `os.setuid()` / `os.setgid()` and then reads it
back with `os.getuid()` / `os.getgid()` to prove the mutation took effect.

`os.setuid`/`os.setgid` are genuinely distinct from every other os.* lowering:
each is a bridge-free `fastpy_os_set{u,g}id` -> posix `set{u,g}id()` which, after
the userspace CAP_SETUID/CAP_SETGID + identity check, issues the native
`SYS_PROCESS_SET_CREDENTIALS` syscall.  That syscall *mutates* the calling
process's `ProcessCredentials` in the kernel's process-credentials table — a
write to a distinct kernel path from the read-only credential syscall
(`getuid`/`getgid`) and from every filesystem/pid/tid syscall.

Until this work, posix `setuid()`/`setgid()` were permission-checking stubs
that returned success **without changing any credential state** — an
unprivileged caller could believe it had dropped privilege while nothing
actually changed.  This utility exists to prove the whole chain (fastpy
lowering -> posix -> kernel `SYS_PROCESS_SET_CREDENTIALS` -> `pcb`) now really
updates the process identity.

The self-check in a single binary:
  * read `u0 = os.getuid()`, `g0 = os.getgid()` (the spawn identity),
  * `os.setuid(NEW_UID)`, `os.setgid(NEW_GID)` to change identity,
  * read `u1 = os.getuid()`, `g1 = os.getgid()` (the *new* identity),
  * write `"<u0>,<g0>,<u1>,<g1>"` (decimal) to `/tmp/fastpy-setuid.out`,
  * exit 0 (the kernel does the authoritative check on the written values).

False-pass-proof design:
  * The kernel self-test spawns this tool as **root** (`SpawnOptions.uid_gid =
    (0, 0)`) — every process starts with CAP_SETUID/CAP_SETGID in its effective
    set (default caps = all), so the setuid/setgid calls are permitted.  It then
    asserts the tool wrote exactly `"0,0,<NEW_UID>,<NEW_GID>"` AND that
    `pcb::get_credentials(pid)` reflects the *mutated* pair `(NEW_UID, NEW_GID)`.
  * The old stub (return success, no mutation) would leave the credentials at
    `(0, 0)`, so `os.getuid()`/`os.getgid()` after the setuid/setgid calls would
    still read 0 — the tool would write `"0,0,0,0"` and the kernel's
    `pcb::get_credentials` cross-check would still see `(0, 0)`: two independent
    failures.  NEW_UID and NEW_GID are distinct non-zero values, ruling out a
    field-swap or single-field coincidence.

Exit code:
    exit 0 always — the kernel validates the written identity and the kernel
    credential table, not the exit code.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_setuid`).  No
filesystem capabilities beyond writing the output file are needed
(`Rights::WRITE` for `/tmp`); the credential syscalls need no capability token.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-setuid/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# identity mutation probe: read the spawn identity, change it, read it back, and
# write "<u0>,<g0>,<u1>,<g1>" decimal for the kernel to cross-check against the
# credentials it can independently read via pcb::get_credentials.
SRC = (
    "import os\n"
    "u0 = os.getuid()\n"
    "g0 = os.getgid()\n"
    "os.setuid(3131)\n"
    "os.setgid(4242)\n"
    "u1 = os.getuid()\n"
    "g1 = os.getgid()\n"
    "s = str(u0) + ',' + str(g0) + ',' + str(u1) + ',' + str(g1)\n"
    "f = open('/tmp/fastpy-setuid.out', 'w')\n"
    "f.write(s)\n"
    "f.close()\n"
    "import sys\n"
    "sys.exit(0)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-setuid.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
