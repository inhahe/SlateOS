#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-getmtime` SlateOS utility.

This produces `fastpy-getmtime.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that *reads back* a
file's modification time via `os.path.getmtime(path)`.

New OS-surface ground vs. every prior fastpy tool: `fastpy-settimes` *sets* a
file's timestamps (`os.utime` -> `SYS_FS_SET_TIMES`) and `fastpy-size` reads a
file's *size* (`os.path.getsize` pulls `st_size` out of the stat struct).  This
is the **first fastpy tool to read a timestamp field of the stat struct back
into userspace** — `os.path.getmtime` lowers natively to `fastpy_os_path_getmtime`
-> posix libc `stat()` -> `SYS_FS_STAT`, and pulls the *mtime* field (a distinct
metadata field from `getsize`'s `st_size`).  It also exercises fastpy's first
`os.*`/`os.path.*` call that returns a **float** (CPython-faithful: seconds since
the epoch as a `double`), and the `int(<float>)` truncation lowering (`fptosi`).

The round-trip (a set-then-get self-check in a single binary):
  * create the file (`open('w')` -> `SYS_FS_OPEN`/`WRITE`),
  * `os.utime(path, ns, ns)`     -> stamp a *kernel-chosen* mtime (argv-supplied
                                    whole seconds, so no constant can be baked
                                    into the ELF),
  * `back = os.path.getmtime(path)` -> read the mtime back as float seconds,
  * `got = int(back)`            -> truncate to integer seconds,
  * write `str(got)` to `/tmp/fastpy-getmtime.out`.

False-pass-proof design:
  * The expected mtime (whole seconds) arrives in `argv[2]` — chosen by the
    kernel self-test at spawn time, not compiled in.  A stub/mis-lowered
    `getmtime` that returned 0.0, a constant, the wall clock, or `st_size`
    (if it were mis-wired to `getsize`) would not equal the freshly-stamped
    value, so `got == secs` fails and the tool exits 3.
  * The tool stamps a whole number of seconds (`secs * 1e9` ns, `tv_nsec == 0`),
    so `getmtime` returns exactly `secs.0` and `int()` recovers `secs` with no
    rounding ambiguity.
  * The kernel self-test asserts BOTH that the tool exited 0 AND that the file
    it wrote (`/tmp/fastpy-getmtime.out`) contains exactly the decimal seconds
    the harness passed in — an independent readout of the value `getmtime`
    returned.

Exit code:
    exit 0 — os.utime succeeded and the mtime read back equals what was stamped
    exit 3 — utime failed or the read-back mtime did not match

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_getmtime`)
granting `READ|WRITE`.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-getmtime/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# getmtime probe: create argv[1], stamp it with the kernel-chosen whole-second
# mtime from argv[2] (converted to ns), read the mtime back via getmtime, write
# the recovered integer seconds to a file the kernel reads back, and exit 0 iff
# the read-back seconds equal what was stamped.
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
    # Bind the native os.path.getmtime float result to an intermediate variable
    # and int() it on the next line.  This exercises the fixed VKind-propagation
    # path: the assignment now stamps `back` with kind FLOAT (see the chained
    # os.path.* handler in codegen `_infer_call_type_tag`), so `int(back)` lowers
    # natively (fptosi) instead of bridging to CPython.  This form used to bridge
    # (was BUG-SCALARKIND-LOST-ON-NATIVE-MODULE-CALL-ASSIGN); keeping it here
    # makes this tool an on-target regression test for that fix.
    "back = os.path.getmtime(path)\n"
    "got = int(back)\n"
    "g = open('/tmp/fastpy-getmtime.out', 'w')\n"
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
    exe = toolchain.link_executable([obj], out / "fastpy-getmtime.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
