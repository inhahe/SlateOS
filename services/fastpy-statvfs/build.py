#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-statvfs` SlateOS utility.

This produces `fastpy-statvfs.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that reads a
*whole filesystem's* capacity/limits via `os.statvfs(path)`.

`os.statvfs` is genuinely distinct from `os.stat`.  Where `os.stat` reads one
*file's* metadata struct (mode/size/inode/...), a single `os.statvfs` call
(one `fastpy_os_statvfs` -> posix `statvfs()` -> `SYS_FS_STATVFS`) reports the
metrics of the *filesystem the path lives on* and returns them as CPython's
`os.statvfs_result` **sequence** form: a 10-int list

    (f_bsize, f_frsize, f_blocks, f_bfree, f_bavail,
     f_files, f_ffree, f_favail, f_flag, f_namemax)

(the index/tuple form).  It is returned like `os.listdir`/`os.stat` (a raw list
the program indexes with `st[i]`).

The self-check in a single binary:
  * create `TARGET` (argv[1]) with 1 byte so the path exists on its filesystem,
  * `st = os.statvfs(TARGET)` and check three internal invariants:
      - `st[0]` (f_bsize)  >= 1                    (a sane block size)
      - `st[1]` (f_frsize) == st[0]                (posix reports frsize==bsize)
      - `st[9]` (f_namemax) >= 1                   (a sane max filename length)
  * write the observed `"<f_bsize>,<f_namemax>"` (decimal) to
    `/tmp/fastpy-statvfs.out` so the **kernel** can cross-check those two
    fields *exactly* against its own independent `Vfs::statvfs` readout,
  * exit 0 iff all three invariants held (exit 6 otherwise).

False-pass-proof design:
  * The kernel self-test independently `Vfs::statvfs`es TARGET and asserts the
    tool's written `f_bsize`/`f_namemax` match the **live** filesystem's values
    exactly (applying the same posix zero-guards).  TARGET lives on `/tmp`
    (memfs), whose `block_size` is an unusual **1** and whose `max_name_len`
    is **255** — a stub returning constant/guessed values (512, 4096, 16384,
    or all-1s) cannot simultaneously match both live values.
  * Because the two values come from the *same* live filesystem read via the
    real `SYS_FS_STATVFS` syscall, a hardcoded-constant stub is defeated even
    if it happened to guess one field: it would have to reproduce the exact
    pair the kernel reads at test time.

Exit code:
    exit 0 — os.statvfs returned a struct with sane, cross-checkable fields
    exit 6 — a statvfs field was implausible (a stub or a mis-lowered struct)

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_statvfs`) granting
`READ|WRITE|METADATA` (create/write the probe file need WRITE; statvfs needs
METADATA).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-statvfs/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# statvfs probe: create a 1-byte file, os.statvfs it, check three field
# invariants, write the observed "<f_bsize>,<f_namemax>" for the kernel to
# cross-check exactly, and exit 0 iff all invariants held.
SRC = (
    "import os\n"
    "import sys\n"
    "target = sys.argv[1]\n"
    "f = open(target, 'w')\n"
    "f.write('x')\n"
    "f.close()\n"
    "st = os.statvfs(target)\n"
    "bsize = st[0]\n"
    "frsize = st[1]\n"
    "namemax = st[9]\n"
    "c1 = 0\n"
    "if bsize >= 1:\n"
    "    c1 = 1\n"
    "c2 = 0\n"
    "if frsize == bsize:\n"
    "    c2 = 1\n"
    "c3 = 0\n"
    "if namemax >= 1:\n"
    "    c3 = 1\n"
    "s = str(bsize) + ',' + str(namemax)\n"
    "g = open('/tmp/fastpy-statvfs.out', 'w')\n"
    "g.write(s)\n"
    "g.close()\n"
    "code = 6\n"
    "if c1 == 1:\n"
    "    if c2 == 1:\n"
    "        if c3 == 1:\n"
    "            code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-statvfs.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
