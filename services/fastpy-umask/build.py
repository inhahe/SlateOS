#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-umask` SlateOS utility.

This produces `fastpy-umask.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that sets its
process **file-mode creation mask** via `os.umask()` and then creates a file
with `os.open(..., O_CREAT, 0o777)` so the resulting on-disk permission bits
prove the mask was actually applied at creation time.

`os.umask` is genuinely distinct from every other os.* lowering: it is a
bridge-free `fastpy_os_umask` -> posix `umask()`, which stores the mask in the
userspace POSIX layer.  The *observable* effect happens on the next create: the
posix `open(O_CREAT)` / `mkdir` wrappers compute `mode & ~umask` and pass the
already-masked final permission bits to the kernel create syscall as a new 4th
arg (SYS_FS_OPEN) / 3rd arg (SYS_FS_MKDIR).  The kernel (`fs::handle::
open_with_mode` / `Vfs::mkdir_mode`) stamps exactly those bits onto the new
inode.  That is a write to a distinct kernel path — the new file's permission
metadata — from every other syscall probe in the suite.

Until this work, the create path *ignored* mode entirely: SYS_FS_OPEN/
SYS_FS_MKDIR took no mode argument and every new file was stamped a hard-coded
0o644 / new dir 0o755 regardless of the caller's mode or umask.  A process
could believe it had set a umask (posix `umask()` stored it in a userspace
static) while the created file's real permissions never reflected it.  This
utility exists to prove the whole chain — fastpy `os.umask` lowering -> posix
umask static -> posix `open()` masking -> `SYS_FS_OPEN(..., create_mode)` ->
kernel inode permission bits — now really applies the mask on disk.

The self-check in a single binary:
  * `a = os.umask(0o077)` -> returns the *previous* mask (spawn default 0o022),
  * `b = os.umask(0o022)` -> returns the mask just set (0o077), and leaves the
    active mask at 0o022 for the create below (two distinct return values prove
    `umask()` reports the prior value and the set persisted),
  * `fd = os.open('/tmp/fastpy-umask.dat', O_CREAT|O_WRONLY|O_TRUNC, 0o777)`
    -> with umask 0o022 active the new file's on-disk mode must be
    0o777 & ~0o022 == 0o755,
  * write one byte and close,
  * write "<a>,<b>" (decimal) to `/tmp/fastpy-umask.out` as the "I'm done"
    sync signal for the kernel harness.
  * then **sleep in a loop** (blocked, not busy-spinning) so the harness can
    stat the created file before killing the process.

`O_CREAT|O_WRONLY|O_TRUNC` == 0o100 | 0o1 | 0o1000 == 0o1101 == 577.  We use
the literal so the tool does not depend on `os.O_*` constant lowering.

False-pass-proof design:
  * The kernel self-test spawns this tool as **root** and, once the output
    file is fully written (the tool's sync signal that the umask+create ran and
    it is now sleeping), independently:
      (a) reads the marker file and asserts it equals exactly "18,63"
          (0o022 == 18 decimal, 0o077 == 63 decimal), proving `os.umask`
          returned the correct prior masks, AND
      (b) stats `/tmp/fastpy-umask.dat` via the kernel VFS and asserts its
          permission bits are exactly 0o755.
  * The old create path (mode ignored, always 0o644) would leave the created
    file at 0o644, not 0o755 — an independent failure from the marker check —
    so a regression to the stub cannot pass.  0o755 also differs from the
    0o644 file default and the 0o777 requested mode, ruling out a coincidence.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_umask`).  It needs
`Rights::WRITE` (create the files) for `/tmp`; umask is not capability-gated.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-umask/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# umask mutation probe: set the mask twice (capturing the prior value each
# time to prove umask() reports it and the set persists), then create a
# 0o777 file so the kernel can verify the on-disk mode is 0o755 (== 0o777 &
# ~0o022).  After writing the marker, sleep in a loop (blocked, not spinning)
# so the harness can stat the created file and then kill this process.
#   O_CREAT|O_WRONLY|O_TRUNC == 0o1101 == 577.
SRC = (
    "import os\n"
    "import time\n"
    "a = os.umask(0o077)\n"
    "b = os.umask(0o022)\n"
    "fd = os.open('/tmp/fastpy-umask.dat', 577, 0o777)\n"
    "os.write(fd, 'x')\n"
    "os.close(fd)\n"
    "s = str(a) + ',' + str(b)\n"
    "f = open('/tmp/fastpy-umask.out', 'w')\n"
    "f.write(s)\n"
    "f.close()\n"
    # Stay ALIVE (so the kernel can stat the just-created file) but BLOCKED —
    # sleep, don't busy-spin.  time.sleep() -> SYS_SLEEP blocks the task off
    # the run queue so the harness runs freely, stats the file, and kills this
    # process.  The loop makes the tool self-terminate-proof.
    "while True:\n"
    "    time.sleep(3600)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-umask.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
