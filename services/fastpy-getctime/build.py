#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-getctime` SlateOS utility.

This produces `fastpy-getctime.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that *reads back* a
file's **metadata-change** time (ctime) via `os.path.getctime(path)`.

This **completes the `os.path.get{a,m,c}time` trio**.  getmtime read the mtime
field and getatime read the atime field; this reads the **ctime** field
(`st_ctim`) — the inode metadata-change time.  Crucially, ctime is *different in
kind* from atime/mtime: it is **not settable via `os.utime`** (utime stamps only
atime and mtime).  On SlateOS, `changed_ns` is set to the wall-clock realtime at
file creation / metadata change; `os.utime` deliberately does not touch it.  So
this tool exercises a *distinct validation pattern*: rather than reading back a
kernel-chosen stamp, it reads whatever the kernel set ctime to and proves the
value came from the ctime field (not mtime).

It lowers natively to `fastpy_os_path_getctime` -> posix libc `stat()` ->
`SYS_FS_STAT`, pulling the *ctime* field as a CPython-faithful `double` (seconds
since the epoch), and uses the native `int(<float>)` truncation (`fptosi`).

The self-check in a single binary:
  * create the file (`open('w')` -> `SYS_FS_OPEN`/`WRITE`) — this sets ctime to
    the current realtime,
  * `os.utime(path, ns, ns)`     -> stamp atime and mtime to a kernel-chosen
                                    whole-second value (argv[2]), leaving ctime
                                    untouched,
  * `back = os.path.getctime(path)` -> read the ctime back as float seconds,
  * `got = int(back)`            -> truncate to integer seconds,
  * write `str(got)` to `/tmp/fastpy-getctime.out`,
  * exit 0 iff utime succeeded AND `got != secs` (ctime is NOT the stamped
    atime/mtime — proving getctime read the ctime field, not mtime) AND
    `got > 0` (a real value, not the -1.0 stat error or a 0 stub).

The `back = ...; got = int(back)` intermediate-variable form is used
deliberately (regression test for the chained-`os.path.*` VKind-propagation fix,
BUG-SCALARKIND-LOST-ON-NATIVE-MODULE-CALL-ASSIGN): `back` is tagged FLOAT so
`int(back)` lowers natively (`fptosi`) instead of bridging to CPython.

False-pass-proof design:
  * The mtime stamp (argv[2]) is a year-2023 value (1700000000) while ctime is
    the *real* creation realtime (year 2026-ish on this host).  A getctime
    mis-wired to read mtime would return exactly `secs` -> the tool's `got != secs`
    check fails and it exits 3.  A stub returning 0.0 or the -1.0 error would
    fail the `got > 0` check.
  * The kernel self-test additionally reads the VFS `changed_ns` *independently*
    and asserts it is non-zero AND that the decimal seconds the tool wrote equal
    `changed_ns / 1e9` — an independent readout of the exact ctime field the tool
    returned (a value the tool could not fabricate).

Exit code:
    exit 0 — os.utime succeeded and getctime returned a real ctime distinct from
             the stamped mtime
    exit 3 — utime failed, or getctime returned the mtime / an error / zero

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_getctime`)
granting `READ|WRITE|METADATA` (stat needs `Rights::METADATA`).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-getctime/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# getctime probe: create argv[1] (sets ctime to now), stamp atime/mtime to the
# kernel-chosen whole-second value from argv[2] (ctime is left untouched by
# utime), read the ctime back via getctime, write the recovered integer seconds
# to a file the kernel reads back, and exit 0 iff ctime is a real value distinct
# from the stamped mtime.
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
    "path = sys.argv[1]\n"
    "secs = parse_dec(sys.argv[2])\n"
    "ns = secs * 1000000000\n"
    "f = open(path, 'w')\n"
    "f.write('x')\n"
    "f.close()\n"
    "rc = os.utime(path, ns, ns)\n"
    # Intermediate-var form on purpose (regression test for the chained
    # os.path.* scalar-kind fix): `back` is tagged FLOAT so int(back) lowers
    # natively (fptosi) rather than bridging to CPython.
    "back = os.path.getctime(path)\n"
    "got = int(back)\n"
    "g = open('/tmp/fastpy-getctime.out', 'w')\n"
    "g.write(str(got))\n"
    "g.close()\n"
    "code = 3\n"
    "if rc == 0:\n"
    "    if got != secs:\n"
    "        if got > 0:\n"
    "            code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-getctime.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
