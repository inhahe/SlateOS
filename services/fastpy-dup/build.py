#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-dup` SlateOS utility.

This produces `fastpy-dup.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that exercises the
kernel **fd-duplication** path (`os.dup`) — a genuinely distinct kernel path
from the pipe create/read/write/close syscalls the fastpy-pipe tool drives.

`os.dup(fd)` lowers to native `fastpy_os_dup` -> posix `dup()`.  On SlateOS the
posix `dup()` of a pipe fd does **not** create a new kernel object: it shares
the *same* underlying kernel handle by allocating a fresh fdtable entry that
points at the original handle (see `posix/src/file.rs` `dup()` -> the `Pipe`
branch -> `fdtable::alloc_fd_with_flags(HandleKind::Pipe, entry.handle, ...)`).
So a write to the *duplicated* write-end lands in the very same kernel pipe
buffer as the original write-end, and can be read from the original read-end.

The round-trip:
  * `r, w = os.pipe()`               -> SYS_PIPE_CREATE (a kernel pipe + 2 fds),
  * `w2 = os.dup(w)`                 -> posix `dup()` aliasing the write handle,
  * `os.write(w2, msg)`             -> writes through the *dup* into the pipe,
  * `os.read(r, 6)`                 -> reads from the *original* read-end.

False-pass-proof design:
  * We deliberately write only through `w2` (the dup) and never through `w`.
    If `os.dup` were mis-lowered (e.g. returned a bogus/independent fd, or
    aliased nothing), the bytes would never reach the kernel pipe buffer the
    original read-end drains, and the round-trip would not match — there is no
    userspace echo path a stub could fake.
  * The kernel self-test knows the exact constant the tool sends ("DUP_OK")
    and asserts the file the tool wrote (`/tmp/fastpy-dup.out`) contains
    exactly that.
  * The message is NUL-free ASCII, so it round-trips cleanly through fastpy's
    NUL-terminated `str` value ABI.

Exit code encodes a self-consistency check:
    exit 0 — the bytes read from the original read-end equal the bytes written
             through the duplicated write-end (dup truly aliased the pipe)
    exit 3 — the round-trip did not match (dup mis-lowered)

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_dup`).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-dup/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# dup round-trip probe: create a kernel pipe, dup the write-end, write a known
# message *through the dup*, read it back from the *original* read-end, close
# every fd, write the round-tripped message to a file the kernel reads back,
# and exit 0 iff the bytes survived — proving os.dup aliased the same pipe.
SRC = (
    "import os\n"
    "import sys\n"
    "r, w = os.pipe()\n"
    "w2 = os.dup(w)\n"
    "msg = \"DUP_OK\"\n"
    "n = os.write(w2, msg)\n"
    "back = os.read(r, 6)\n"
    "os.close(w2)\n"
    "os.close(w)\n"
    "os.close(r)\n"
    "f = open('/tmp/fastpy-dup.out', 'w')\n"
    "f.write(back)\n"
    "f.close()\n"
    "code = 3\n"
    "if back == msg:\n"
    "    if n == 6:\n"
    "        code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-dup.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
