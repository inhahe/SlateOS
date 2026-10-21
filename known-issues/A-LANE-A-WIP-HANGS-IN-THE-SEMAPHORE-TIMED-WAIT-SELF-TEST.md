## A-LANE-A-WIP-HANGS-IN-THE-SEMAPHORE-TIMED-WAIT-SELF-TEST (lane A, 2026-10-21) — OPEN
**Status:** OPEN -- on lane-a-wip only (a94974945); `main` (c85e42859) does not have it

**In short:** the 80-commit lane A batch on `lane-a-wip` (memory, page cache, memfd,
msync and scheduler work from 2026-10-16/17) had never been booted when lane A was
moved to the new PC. Its first boot hangs during the kernel's semaphore self-test,
and the machine sits idle until the boot test gives up. `main` boots past the same
point on the same PC. The batch must not reach `main` until this is fixed.

**What was seen** (boot test of a94974945 on DESKTOP-T84VAT6, 2026-10-21, QEMU
TCG, one CPU, debug):

- the last serial lines are `[sem]   One signal wakes 20 waiters: OK` and
  `[sem]   Grants in batches, none counted while a task waits: OK`; the serial log
  then does not grow for 2386 s and the run ends TIMEOUT;
- RIP at the timeout is `kernel::cpu::hlt+0x5`: the CPU is idle, every task is
  blocked, so this is a lost wakeup, not a spin;
- the register dump is `build/serial-test-regs.txt` in the worktree that ran it.

**Where it stops.** The next test in `kernel/src/ipc/semaphore.rs` `self_test` is
test 8: `wait_timeout(h, 1_000_000)` on a semaphore at count 0, which must return
`TimedOut`. It never returns -- the timed park is never woken by its timeout.

**Suspects.** Between `c85e42859` and `a94974945` the batch rewrote the park/wake
path: `kernel/src/sched/mod.rs` (460 lines changed), `sched/task.rs`,
`sched/priority_rr.rs`, `sched/waitqueue.rs` and `ipc/semaphore.rs` (394), among them "sched: a preemption
before a park no longer erases the wake owed to it" and the change behind
"known-issues: a woken task left Running, off CPU and out of every run queue". A
timeout's wakeup going to a task the new code considers already woken, or being
consumed before the park, would produce exactly this.

**Reproduce:** in a worktree at `lane-a-wip`, `./scripts/boot-test.sh` (add
`--bootstrap` on a fresh checkout). It hung on the first try; whether it hangs every
time is not yet known.

**Fix:** find the commit with a bisect over `c85e42859..a94974945` (boot to the
`[sem] IPC semaphore self-test PASSED` line is enough), fix the timed-park wakeup,
and boot the batch green before publishing it.
