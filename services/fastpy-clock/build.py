#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-clock` SlateOS utility.

This produces `fastpy-clock.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a `date`-style tool that
reads the **wall-clock** and validates it against a caller-supplied lower bound.

`clock <lower_bound_ns>` reads `time.time_ns()`, prints it, and exits with a
code encoding the result:

    exit 0 — the clock read is > 0 AND >= the supplied lower bound
    exit 3 — the clock read is 0 (dead/stub clock) or < the lower bound

New ground vs. every prior fastpy SlateOS tool: **all** of them exercised the
kernel *filesystem* syscalls (SYS_FS_*).  This is the **first fastpy tool to
exercise a different kernel subsystem — timekeeping**: `time.time_ns()` lowers
to the native runtime `fastpy_time_time_ns`, which on SlateOS calls posix libc
`gettimeofday()` -> kernel `SYS_CLOCK_REALTIME` -> `timekeeping::clock_realtime`
(boot-time CMOS RTC + TSC-elapsed).

False-pass-proof design: the kernel self-test reads its OWN
`timekeeping::clock_realtime()` immediately before spawning and passes that ns
count as `argv[1]` — a lower bound the tool's clock read must meet or exceed.
Because the tool reads the *same* clock a few milliseconds later, a correct
clock returns a value `>= lower_bound`; a zero-stub clock returns 0 (fails
`> 0`), and a clock stuck in the past returns `< lower_bound` (fails).  The
self-test additionally asserts its own clock reading is `> 0` up front, so the
comparison is meaningful (not 0 >= 0).

`time.time_ns()` returns a bare i64 nanosecond count (~1.7e18 for a 2020s date,
comfortably inside i64, so no bigint); the lower bound is parsed from argv with
pure-mode decimal integer arithmetic (`parse_dec`, digit `ord()` compares — the
same pure-mode-safe helper shape used by fastpy-settimes).

Pure-mode notes (verified bridge-free in the emitted IR — only the
`fpy_cpython_import_native` sentinels from `import time` / `import sys`):
  * `time.time_ns()` lowers to a native `fastpy_time_time_ns` call,
  * `str(now)` lowers to the native int->str formatter,
  * the decimal parse and the two int compares are pure integer arithmetic.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_clock`).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-clock/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# date/clock validator: read the wall clock via time.time_ns() (natively
# fastpy_time_time_ns -> posix gettimeofday() -> SYS_CLOCK_REALTIME), print it,
# and exit 0 iff it is a plausibly-current value: strictly positive and at
# least the lower bound the kernel passed (its own clock reading taken just
# before spawn).  This proves the timekeeping syscall path end-to-end.
SRC = (
    "import time\n"
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
    "lb = parse_dec(sys.argv[1])\n"
    "now = time.time_ns()\n"
    "print(str(now))\n"
    "code = 3\n"
    "if now > 0:\n"
    "    if now >= lb:\n"
    "        code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-clock.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
