#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-sleep` SlateOS utility.

This produces `fastpy-sleep.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that **blocks for
a fixed interval** and proves — against the *wall clock* — that the sleep
actually consumed real time.

`sleep-check <min_elapsed_ns>` reads `time.time_ns()`, sleeps a fixed 50 ms via
`time.sleep(0.05)`, reads the clock again, prints the observed elapsed ns, and
exits with a code encoding the result:

    exit 0 — both clock reads are > 0 AND (after - before) >= min_elapsed_ns
    exit 3 — a clock read is 0 (dead clock) or the sleep returned too early

New ground vs. every prior fastpy SlateOS tool: `fastpy-clock` was the first to
leave the filesystem syscalls and exercise **timekeeping** (`time.time_ns()` ->
`SYS_CLOCK_REALTIME`).  This is the first to exercise the **scheduler sleep /
timer-wakeup** path: `time.sleep()` lowers to the native runtime
`fastpy_time_sleep`, which on SlateOS calls posix libc `usleep()` -> kernel
`SYS_SLEEP` (`timekeeping` + scheduler block-and-timer-wake).  A read-only
timekeeping *sample* (clock) is a fundamentally different kernel operation from
a *blocking sleep that must be woken by a timer* — this tool covers the latter.

False-pass-proof design: the tool sleeps a fixed 50 ms, but the pass/fail
threshold (`min_elapsed_ns`) is supplied by the kernel self-test as `argv[1]`
(40 ms), so the accept bound is kernel-controlled, not baked into the binary.
A correct sleep blocks ~50 ms, so the wall clock advances >= 40 ms -> pass; a
*stub* sleep that returns immediately advances only a few microseconds ->
`elapsed < 40 ms` -> fail.  The self-test additionally reads its OWN
`clock_realtime()` before spawn and after the child zombifies and asserts that
kernel-observed delta is itself >= 40 ms — an independent proof that real
wall-time elapsed during the run, not merely that the tool's own arithmetic
lined up.  The tool also requires both of its clock reads to be `> 0` so the
subtraction is meaningful.

`time.time_ns()` returns a bare i64 nanosecond count (~1.7e18 for a 2020s date,
comfortably inside i64); the threshold is parsed from argv with pure-mode
decimal integer arithmetic (`parse_dec`, digit `ord()` compares — the same
pure-mode-safe helper shape used by fastpy-clock / fastpy-settimes).

Pure-mode notes (verified bridge-free in the emitted IR — only the
`fpy_cpython_import_native` sentinels from `import time` / `import sys`):
  * `time.time_ns()` lowers to a native `fastpy_time_time_ns` call,
  * `time.sleep(0.05)` lowers to a native `fastpy_time_sleep(double)` call,
  * `str(elapsed)` lowers to the native int->str formatter,
  * the decimal parse and the int compares are pure integer arithmetic.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_sleep`).

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in. There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/fastpy-sleep/build.py

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# sleep validator: sample the wall clock (time.time_ns()), block for a fixed
# 50 ms via time.sleep() (natively fastpy_time_sleep -> posix usleep() ->
# SYS_SLEEP -> scheduler block + timer wakeup), sample the clock again, print
# the observed elapsed ns, and exit 0 iff both samples are strictly positive and
# the elapsed time is at least the kernel-supplied lower bound (a stub sleep
# that returns immediately advances the clock by only microseconds and fails).
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
    "t0 = time.time_ns()\n"
    "time.sleep(0.05)\n"
    "t1 = time.time_ns()\n"
    "elapsed = t1 - t0\n"
    "print(str(elapsed))\n"
    "code = 3\n"
    "if t0 > 0:\n"
    "    if t1 > 0:\n"
    "        if elapsed >= lb:\n"
    "            code = 0\n"
    "sys.exit(code)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-sleep.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
