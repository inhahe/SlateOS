#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-symlink` SlateOS utility.

This produces `fastpy-symlink.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): an `ln -s`-style tool.
`symlink <target> <linkpath>` creates `linkpath` as a symbolic link pointing at
`target`, reads it back, and exits with a code encoding the round-trip result:

    exit 0 — link created AND readlink returned exactly <target>
    exit 3 — os.symlink failed (SYS_FS_SYMLINK rejected / errored)
    exit 4 — link created but readlink returned a mismatched/empty target

New ground vs. every prior fastpy tool: this is the first to drive the kernel's
**symbolic-link** syscalls end-to-end from AOT Python — a genuinely-new VFS
surface, not a duplicate of the content-read / dir-enumeration / mutation /
metadata paths the earlier tools cover.  The flow is:

  * `os.symlink(target, linkpath)` -> runtime `fastpy_os_symlink` -> posix libc
    `symlink()` -> kernel `SYS_FS_SYMLINK` (gated on `Rights::CREATE`) ->
    `Vfs::symlink(linkpath, target)`, returning a bare int (0 ok / -1 error);
  * `os.readlink(linkpath)` -> runtime `fastpy_os_readlink` -> posix libc
    `readlink()` -> kernel `SYS_FS_READLINK` (gated on `Rights::METADATA`),
    returning the stored target as a real FpyString.

Because the exit code is 0 *only* when the readback string equals the original
target, a stubbed/no-op symlink could not false-pass: a failed create yields 3,
and a create-without-correct-storage yields 4.  The kernel embeds the ELF via
`include_bytes!` in `kernel/src/proc/spawn.rs` and runs it as a ring-3 self-test
(`self_test_fastpy_slateos_symlink`) that grants `READ|CREATE|METADATA`, points
the link at a staged file under `/tmp`, and asserts exit code 0.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.symlink(...)` lowers to a native `fastpy_os_symlink` call returning a
    bare int (0/-1), used directly as a branch condition,
  * `os.readlink(...)` lowers to a native `fastpy_os_readlink` call returning a
    real FpyString, compared to `target` with the native string-equality path.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-symlink/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# ln -s clone: create linkpath -> target, read it back, and encode the
# round-trip result in the exit code (0 verified / 3 create failed / 4 mismatch).
# os.symlink/os.readlink lower natively to fastpy_os_symlink/fastpy_os_readlink
# -> posix libc -> SYS_FS_SYMLINK (Rights::CREATE) / SYS_FS_READLINK
# (Rights::METADATA).  The string compare proves the stored target survives the
# kernel round-trip, so a no-op symlink cannot false-pass.
SRC = (
    "import os\n"
    "import sys\n"
    "target = sys.argv[1]\n"
    "linkpath = sys.argv[2]\n"
    "rc = os.symlink(target, linkpath)\n"
    "code = 3\n"
    "if rc == 0:\n"
    "    back = os.readlink(linkpath)\n"
    "    if back == target:\n"
    "        code = 0\n"
    "    else:\n"
    "        code = 4\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-symlink.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
