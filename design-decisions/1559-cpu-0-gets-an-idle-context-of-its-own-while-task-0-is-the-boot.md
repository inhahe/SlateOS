## 1559. CPU 0 gets an idle context of its own while task 0 is the boot

**Date:** 2026-10-09 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** when a CPU has nothing to run, the scheduler switches it to
that CPU's "idle task", a small loop that waits for work. Every CPU but the
first has one. The first CPU's idle task is also the code that boots the
system, so while the system boots -- which is when all the self-tests run --
that task is often asleep, and the first CPU had nowhere to go: it waited on
the stack of whatever task had just stopped. That task then could not run
anywhere else, even if it had just asked to move to another CPU, because
another CPU cannot run a task whose stack is still in use. Now the first CPU
has a second, hidden idle task for exactly that situation.

**What exists now** (`kernel/src/sched/mod.rs`):

- `BOOT_IDLE_ID`: a kernel task made by `sched::init`, pinned to CPU 0,
  never queued. It is created awaiting admission and never admitted
  (`Task::awaiting_admission`), so no wake or resume can queue it.
- The idle fallback in `schedule_inner` -- entered when nothing may run and
  the current task cannot go on -- first switches CPU 0 to that task
  (`fallback_idle_for`, `take_fallback_idle`) unless it is already running
  it. Its body parks at once (`block_current`), back into the fallback,
  which now idles on its own stack. The stopped task's context is saved by
  that switch, so any CPU may run it.
- The pick made just before leaving may move the current task to the CPU
  its affinity names (`classify_pick`'s `move_current`); idling on the
  stack, it still may not.
- Wake placement (`cpu_idle_for_wake`, design-decisions 1558) counts CPU 0
  as idle when it runs this context during the boot, rather than never.
- The wedge report a long idle fallback prints names the boot (task 0) when
  CPU 0 idles in this context, since that context is parked by design.

**Alternatives:**

| Option | For | Against |
|---|---|---|
| Idle on the stopped task's stack (before) | No extra task | The stopped task is tied to CPU 0 until something else runs there: `sched_setaffinity` on itself returned on CPU 0 (the affinity self-test's first two-CPU run), and a task woken while CPU 0 idled on its stack could run nowhere else |
| **A hidden boot-time idle task for CPU 0 (chosen)** | Every CPU now always has a context of its own to idle in; small and local to the scheduler; nothing else about task 0 changes | Two idle contexts on CPU 0 (the hidden one stays parked after the boot, when task 0 is the idle loop); one more switch each time the boot blocks with nothing else to run |
| Run the boot in a kernel thread of its own, task 0 the idle loop from the start (Linux's `kernel_init`) | The model every other CPU already follows; no special idle context | Task 0 *is* the boot in a dozen places: `BOOT_TASK_WORKING`, the boot's idle level that every self-test relies on to let its tasks run first (1553), `inheritable_affinity`'s idle exemption, the self-tests that run before interrupts are on, the bootloader stack the boot runs on. A rewrite of the boot's relationship to the scheduler, for a gain the chosen option already gives |
| Weaken the affinity contract: "moves at its next switch" | No code | Leaves the tie in place for woken tasks too, and a contract that holds only after the boot is one nothing can rely on |

**Why:** the failure is a property of one CPU having no context of its own
for part of its life, and the chosen option gives it one without disturbing
the boot's long-settled relationship with the scheduler. The Linux-style
split is the cleaner end state; it stays available, and this does not make
it harder.

**Revisit if:** the boot moves into a thread of its own (then this context
is task 0 itself, and `BOOT_IDLE_ID` goes), or a second CPU ever runs code
on its idle task that can block.
