#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-getatime` SlateOS utility.

This produces `fastpy-getatime.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that *reads back* a
file's **access** time via `os.path.getatime(path)`.

New OS-surface ground vs. `fastpy-getmtime`: getmtime read the *mtime* field of
the stat struct; this reads the **atime** field — a *distinct* timestamp field
(`st_atim` vs `st_mtim`).  `os.utime(path, atime_ns, mtime_ns)` stamps **both**
the access and modification times; fastpy-getmtime validated the mtime half, and
this tool validates the **atime half** — confirming `os.utime`'s first (atime)
argument is honored and that the atime field is independently readable.  It
lowers natively to `fastpy_os_path_getatime` -> posix libc `stat()` ->
`SYS_FS_STAT`, pulling the *atime* field as a CPython-faithful `double` (seconds
since the epoch), and uses the native `int(<float>)` truncation lowering
(`fptosi`).

The round-trip (a set-then-get self-check in a single binary):
  * create the file (`open('w')` -> `SYS_FS_OPEN`/`WRITE`),
  * `os.utime(path, ns, ns)`     -> stamp a *kernel-chosen* atime (argv-supplied
                                    whole seconds, so no constant can be baked
                                    into the ELF),
  * `back = os.path.getatime(path)` -> read the atime back as float seconds,
  * `got = int(back)`            -> truncate to integer seconds,
  * write `str(got)` to `/tmp/fastpy-getatime.out`.

The `back = ...; got = int(back)` intermediate-variable form is used
deliberately (not the inline `int(os.path.getatime(path))`): it exercises the
chained-`os.path.*` VKind-propagation path in codegen (`_infer_call_type_tag`),
which stamps `back` as FLOAT so `int(back)` lowers natively (`fptosi`) instead
of bridging to CPython.  (This form used to bridge — see the Recently Fixed
`BUG-SCALARKIND-LOST-ON-NATIVE-MODULE-CALL-ASSIGN` in fastpy/known-issues.md;
keeping it here makes this tool an on-target regression test for that fix.)

False-pass-proof design:
  * The expected atime (whole seconds) arrives in `argv[2]` — chosen by the
    kernel self-test at spawn time, not compiled in.  A stub/mis-lowered
    `getatime` that returned 0.0, a constant, the wall clock, or the *mtime*
    (if it were mis-wired) would still equal the mtime here (utime sets both
    the same), so to distinguish atime from mtime the kernel self-test ALSO
    asserts the VFS `accessed_ns` equals the stamp independently.
  * The tool stamps a whole number of seconds (`secs * 1e9` ns, `tv_nsec == 0`),
    so `getatime` returns exactly `secs.0` and `int()` recovers `secs` with no
    rounding ambiguity.
  * The kernel self-test asserts the tool exited 0 AND that the file it wrote
    (`/tmp/fastpy-getatime.out`) contains exactly the decimal seconds the
    harness passed in AND that the VFS `accessed_ns` matches the stamp.

Exit code:
    exit 0 — os.utime succeeded and the atime read back equals what was stamped
    exit 3 — utime failed or the read-back atime did not match

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_getatime`)
granting `READ|WRITE|METADATA` (stat needs `Rights::METADATA`).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-getatime/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# getatime probe: create argv[1], stamp its atime with the kernel-chosen
# whole-second value from argv[2] (converted to ns), read the atime back via
# getatime, write the recovered integer seconds to a file the kernel reads
# back, and exit 0 iff the read-back seconds equal what was stamped.
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
    "back = os.path.getatime(path)\n"
    "got = int(back)\n"
    "g = open('/tmp/fastpy-getatime.out', 'w')\n"
    "g.write(str(got))\n"
    "g.close()\n"
    "code = 3\n"
    "if rc == 0:\n"
    "    if got == secs:\n"
    "        code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-getatime.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
