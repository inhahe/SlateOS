#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-settimes` SlateOS utility.

This produces `fastpy-settimes.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a `touch`-style
timestamp-setting tool.  `settimes <atime_ns> <mtime_ns> <path>` parses two
decimal nanosecond-since-epoch counts and stamps them onto `<path>`, exiting
with a code encoding the result:

    exit 0 — os.utime succeeded
    exit 3 — os.utime failed (SYS_FS_SET_TIMES rejected / errored)

New ground vs. the `chmod` tool (permission bits via SYS_FS_SET_PERMS): this
is the first fastpy tool to set a file's **timestamps**, completing the
metadata-mutation trio (permissions, times, [owner]).  The flow is:

  * `os.utime(path, atime_ns, mtime_ns)` -> new native runtime
    `fastpy_os_utime` -> posix libc `utimensat()` -> kernel `SYS_FS_SET_TIMES`
    (gated on `Rights::WRITE`) -> `Vfs::set_times(path, atime_ns, mtime_ns)`.

This is the first `os.*` native call to take THREE positional arguments (a
path plus two i64 nanosecond stamps) — an AOT-simplified form of Python's
`os.utime(path, ns=(atime, mtime))` that surfaces the result as a bare int.
Setting atime and mtime to DISTINCT values lets the kernel self-test verify
each field independently — a stronger false-pass check than a single stamp.

The stamps are parsed from argv with pure-mode decimal integer arithmetic
(digit `ord()` compares — the same pure-mode-safe helper shape used by
fastpy-truncate's `parse_dec`).

The kernel self-test independently re-reads the file's metadata via the VFS
and asserts `FileMeta::accessed_ns` == atime_ns and `FileMeta::modified_ns`
== mtime_ns — a no-op utime that returned 0 without stamping could not
satisfy this.

Pure-mode notes (verified bridge-free in the emitted IR — only the two
`fpy_cpython_import_native` sentinels from `import os` / `import sys`):
  * `os.utime(...)` lowers to a native `fastpy_os_utime` call returning a bare
    int (0/-1), used directly as a branch condition,
  * the decimal parse is pure integer arithmetic over `str`-indexed chars.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_settimes`)
granting `READ|WRITE`.

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-settimes/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# touch clone: parse two decimal ns stamps from argv[1]/argv[2] and apply them
# to argv[3] via os.utime, encoding the result in the exit code (0 ok / 3
# failed).  os.utime lowers natively to fastpy_os_utime -> posix libc
# utimensat() -> SYS_FS_SET_TIMES (Rights::WRITE); the kernel self-test
# re-reads FileMeta::accessed_ns / modified_ns to confirm the stamps took.
SRC = (
    "import os\n"
    "import sys\n"
    "def parse_dec(s: str) -> int:\n"
    "    v = 0\n"
    "    i = 0\n"
    "    n = len(s)\n"
    "    while i < n:\n"
    "        c = ord(s[i])\n"
    "        d = c - 48\n"
    "        if d < 0:\n"
    "            d = 0\n"
    "        if d > 9:\n"
    "            d = 0\n"
    "        v = v * 10 + d\n"
    "        i = i + 1\n"
    "    return v\n"
    "atime = parse_dec(sys.argv[1])\n"
    "mtime = parse_dec(sys.argv[2])\n"
    "path = sys.argv[3]\n"
    "rc = os.utime(path, atime, mtime)\n"
    "code = 3\n"
    "if rc == 0:\n"
    "    code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-settimes.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
