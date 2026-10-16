## 1558. A woken task goes to an idle CPU when its last one is busy

**Date:** 2026-10-09 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** when a task that was waiting becomes runnable again, the
scheduler has to pick a CPU to queue it on. It used to pick the CPU the task
last ran on, always -- good for the cache, since the task's data is likely
still there, but bad when that CPU is busy: the task waits behind whatever is
running there while another CPU sits idle. On two CPUs that is exactly what
happened -- a probe task got two turns in four minutes behind a busy sender,
while the other CPU did nothing. Now the last CPU is kept only when it is
idle; otherwise the task goes to an idle CPU it is allowed on, if there is
one. This is what Linux does (`select_idle_sibling`).

**What exists now** (`kernel/src/sched/mod.rs`):

- `select_wake_cpu(task)`: `choose_cpu_for_task`'s answer (the last CPU, or
  the first allowed one when affinity forbids it) when that CPU is idle or
  nothing better exists; otherwise the first idle, online CPU after it that
  the task's affinity allows.
- `cpu_idle_for_wake(cpu)`: running its own idle task with nothing else
  queued. The BSP's idle task is the boot itself until the boot is done
  (`BOOT_TASK_WORKING`); during the boot the BSP counts as idle when it runs
  its boot-time idle context instead (design-decisions 1559). A queue whose
  lock is held counts as busy.
- Used where a task becomes runnable: `wake`, `try_wake`, the deferred-wake
  drain, a timed sleeper's expiry, and `resume` from a stop. Not by `spawn`,
  which still places a new task on its creator's CPU.

**Alternatives:**

| Option | For | Against |
|---|---|---|
| Always the last CPU (before) | Warm cache; no cross-CPU traffic | Starves behind a busy CPU while others idle; only the 100 ms balancer could rescue it, and an idle AP, its tick stopped, never balances |
| **Last CPU if idle, else an idle one (chosen)** | Runs at once when any CPU is free; Linux's own default | A wake-up/sleep ping-pong pair (IPC request and reply) may bounce between CPUs where one would do |
| The idlest CPU, always | Best spread | Throws away cache warmth even when the last CPU is free |
| Linux's full `wake_affine` (waker's CPU for a "sync" wake) | Keeps request/reply pairs together | Needs the waker to say it is about to sleep, which no wake site here says yet |

**Why:** a task that is runnable and waiting behind another while a CPU
idles is the worst outcome a scheduler can produce, and it was not
hypothetical -- it is what the first two-CPU boot showed. Cache warmth is
worth a lot, but not a queue behind a CPU-bound task. The ping-pong cost is
real for IPC-heavy pairs; if the benchmarks show it, the remedy is a "sync"
wake flag from the channel send path (Linux's `WF_SYNC`), not giving up the
idle CPU.

**Spawn is left alone** on purpose: the boot's self-tests spawn tasks and
let them run first, relying on the boot task's idle level; spreading new
tasks across CPUs is the next step, taken when an SMP boot test exists to
catch what it shakes loose.

**Revisit if:** the context-switch or IPC benchmarks regress on more than
one CPU (add the sync-wake flag), or machines with more than one last-level
cache appear (search the waker's cache domain first, as Linux does).
