## 1553. The boot stays at the idle level, and the anti-starvation booster lifts it until it becomes the idle loop

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** the kernel's start-up work, including every self-test, runs in
the same task that later becomes the idle loop. That task has the lowest
priority, so it only runs when every other task is waiting. That is
deliberate: a self-test that starts a helper relies on the helper running
first. But two tasks that never both wait can keep it off the processor
indefinitely. On 2026-10-08 a network benchmark's sender and the network
daemon did exactly that, and the boot sat for twenty minutes, ready to stop
them, until the watchdog failed it. The scheduler already has a safeguard for
starved tasks (it briefly lifts any task kept waiting for two seconds), but
it skipped idle-level tasks. It now lifts the boot task too, until the boot
hands over to the idle loop.

**The bug.** `check_starvation` passes over tasks at `IDLE_PRIORITY`. Task 0
is the BSP's idle task and runs `kernel_main` at that level, so nothing lifts
it. In lane-a's debug boot of a35d7960c the ring bench's busy sender and the
netstack daemon handed the CPU back and forth without a gap. The liveness
dump showed task 0 `Ready` for 119 082 ticks. Earlier boots passed the same
test only because their interleaving happened to leave idle gaps.

**Chosen:** the booster's exemption has one exception. Task 0 is boosted like
any starved task while `sched::BOOT_TASK_WORKING` is set, and `idle_loop`
clears it on entry (`sched::boot_work_done`). The boot keeps its level and
gets the CPU at least once per threshold, about every two to three seconds,
under any load. Test: `sched::self_test_boot_not_starved`. Two tasks that
never block hold the boot's CPU, and the boot must be back within six seconds
rather than when they give up at twelve.

**Alternative:** run the boot at an ordinary level, as Linux runs its
start-up in `kernel_init` (PID 1) rather than in the idle task, and drop to
the idle level at the idle loop.

| | Lift while booting (chosen) | Boot at an ordinary level |
|---|---|---|
| Self-tests that rely on what they spawn running first | unchanged | many would race: `yield_now` runs the boot again after one slice of a child, not after the child |
| The boot under a CPU-bound load | runs every 2-3 s, slowly | shares the CPU fairly |
| Idle-task bookkeeping (`IDLE_TASK_IDS`, `nr_runnable`) | unchanged | task 0 would be counted idle while doing work |

The second is the cleaner model, but it would change an assumption every
self-test in the tree was written against. The first fixes the hang without
moving that assumption. Revisit if the boot ever moves out of task 0.
