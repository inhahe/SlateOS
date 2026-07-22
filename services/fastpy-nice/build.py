#!/usr/bin/env python3
"""Reproducible build recipe for the `fastpy-nice` SlateOS utility.

This produces `fastpy-nice.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled by fastpy (AOT Python -> LLVM IR -> native): a tool that *changes*
its own process **scheduling priority** via `os.setpriority()` / `os.nice()`
and reads it back with `os.getpriority()` to prove the mutation took effect.

`os.nice`/`os.setpriority`/`os.getpriority` are genuinely distinct from every
other os.* lowering: each is a bridge-free `fastpy_os_{nice,getpriority,
setpriority}` -> posix `{nice,getpriority,setpriority}()` which — after the
userspace CAP_SYS_NICE check for a priority *raise* — issues the native
`SYS_PROCESS_GET_NICE`/`SYS_PROCESS_SET_NICE` syscalls.  The *set* syscall both
records the process's nice in its kernel `ProcessControlBlock` **and** maps it
to a scheduler priority level, re-prioritising every task the process owns via
`sched::set_priority`.  That is a write to a distinct kernel path — the
scheduler's per-task priority table — from every filesystem/credential syscall.

Until this work, posix `nice()`/`setpriority()` stored the nice value in a
userspace-local static that did **not** affect kernel scheduling — a process
could believe it had changed its priority while the scheduler ignored it.
This utility exists to prove the whole chain (fastpy lowering -> posix ->
kernel `SYS_PROCESS_SET_NICE` -> `pcb.linux_nice` + `sched` priority) now
really updates the process's scheduling priority.

The self-check in a single binary:
  * read `n0 = os.getpriority(PRIO_PROCESS, 0)` (the spawn nice, default 0),
  * `os.setpriority(PRIO_PROCESS, 0, -7)` to set nice = -7,
  * read `n1 = os.getpriority(PRIO_PROCESS, 0)` (now -7),
  * `r = os.nice(-5)` -> lowers the *relative* nice to -12 (returns -12),
  * read `n2 = os.getpriority(PRIO_PROCESS, 0)` (now -12),
  * write `"<n0>,<n1>,<r>,<n2>"` (decimal) to `/tmp/fastpy-nice.out`,
  * then **sleep in a loop** (blocked, *not* busy-spinning) so the kernel can
    observe the live scheduler priority before killing the process.

Why *negative* nice (a priority **raise**)?  A raise is CAP_SYS_NICE-gated,
so the tool must be spawned as **root** (which holds the cap) for the
setpriority/nice calls to succeed — this exercises the capability-gated path
end to end.  nice -12 maps to scheduler priority 6 (higher than the spawn
default of 16), distinct enough to rule out a coincidence.

Why *sleep*, not busy-spin, after writing?  The tool has just raised its own
priority to 6 — *above* the polling harness.  A busy-spin at priority 6 would
monopolise the CPU and starve the harness, livelocking the boot (the harness
could never run to read the priority and kill the tool).  `time.sleep()` ->
`SYS_SLEEP` blocks the task off the run queue, so the harness runs freely; the
task still exists (its scheduler slot and base priority are intact) so the
live priority is readable while the tool is blocked.  The `while True` wrapper
means the tool never busy-spins even if the harness's kill were somehow missed.

False-pass-proof design:
  * The kernel self-test spawns this tool as **root** and, once the output
    file is fully written (the tool's sync signal that all the nice calls
    have run and it is now sleeping), reads — while the task is still alive —
    (a) `pcb::get_nice(pid)` and (b) the task's **base scheduler priority**
    (`sched::get_base_priority`).  It asserts the tool wrote exactly
    `"0,-7,-12,-12"` AND that `pcb::get_nice(pid) == -12` AND that the
    scheduler base priority equals `nice_to_priority(-12)` (== 6).  Then it
    kills the (sleeping) process.
  * The old stub (store in a userspace static, no kernel effect) would leave
    `pcb::get_nice` at 0 and the scheduler priority at the spawn default
    (16) — two independent failures — even though the tool's *own* readback
    (the written `n0..n2`) would have looked correct from the userspace
    static.  The kernel's two independent reads are what prove the mutation
    is real.  The chosen nice -12 maps to priority 6, distinct from the
    default 16, ruling out a coincidence.

The kernel embeds the ELF via `include_bytes!` in `kernel/src/proc/spawn.rs`
and runs it as a ring-3 self-test (`self_test_fastpy_slateos_nice`).  Only
`Rights::WRITE` for `/tmp` is needed; the priority *raise* needs
CAP_SYS_NICE, which the root spawn supplies (checked in the userspace posix
wrapper — the kernel mutation syscall itself takes no capability token).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/fastpy-nice/build.py"

The posix sysroot (`libc.a`) must already be built; see
`toolchain/build-sysroot.ps1`.
"""

import ast
from pathlib import Path

from compiler.codegen import CodeGen
from compiler import toolchain

# Scheduling-priority mutation probe: read the spawn nice, set it via
# setpriority (to -7, a CAP_SYS_NICE-gated *raise*), bump it relatively via
# nice (to -12), read it back, and write
# "<n0>,<n1>,<r>,<n2>" for the kernel to cross-check against pcb::get_nice and
# the scheduler's per-task priority.  After writing, sleep in a loop (blocked,
# NOT busy-spinning — a spin at the raised priority 6 would starve the harness):
# the kernel reads the *live* scheduler priority (the task must still exist)
# and then kills this process.  PRIO_PROCESS == 0, who == 0 (the calling process).
SRC = (
    "import os\n"
    "import time\n"
    "n0 = os.getpriority(0, 0)\n"
    "os.setpriority(0, 0, -7)\n"
    "n1 = os.getpriority(0, 0)\n"
    "r = os.nice(-5)\n"
    "n2 = os.getpriority(0, 0)\n"
    "s = str(n0) + ',' + str(n1) + ',' + str(r) + ',' + str(n2)\n"
    "f = open('/tmp/fastpy-nice.out', 'w')\n"
    "f.write(s)\n"
    "f.close()\n"
    # Stay ALIVE (so the kernel can read the live scheduler priority) but
    # BLOCKED — sleep, don't busy-spin.  The tool raised its priority to 6
    # (above the default 16); a busy-spin here would starve the polling
    # harness and livelock the boot.  time.sleep() -> SYS_SLEEP blocks the
    # task off the run queue, so the harness runs freely, reads the priority,
    # and kills this process (during the first sleep).  The loop makes the
    # tool self-terminate-proof: even if the kill were somehow missed, it
    # never busy-spins.
    "while True:\n"
    "    time.sleep(3600)\n"
)


def main() -> None:
    ir = CodeGen().generate(ast.parse(SRC))
    out = Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    obj = toolchain.compile_ir_to_obj(ir, out / "prog.o", target=toolchain.SLATEOS_TARGET)
    exe = toolchain.link_executable([obj], out / "fastpy-nice.elf", target=toolchain.SLATEOS_TARGET)
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()
