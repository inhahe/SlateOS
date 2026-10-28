## 1581. A Linux driver process runs Linux's own timer wheel and softirqs, with its loop as their tick and their ksoftirqd

**Date:** 2026-10-28 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers through a layer (§1577); how the layer keeps time is Claude's call, under §1578 · **Lane:** A

**In short:** Linux drivers arm timers ("call this function in 100 ms"),
schedule tasklets (short jobs run soon after an interrupt) and expect both
to run in Linux's "softirq" context. On Linux a hardware timer interrupt
drives all of it. A driver process has no interrupts of its own -- its
tasks are coroutines on one thread (§1578 rule 5) -- so something must
play that part. The choice: compile Linux's own timer and softirq code into
every driver process and let the layer's scheduling loop drive it, rather
than writing the layer's own timers. To let an idle process sleep until its
next timer instead of waking every 4 ms, the layer's Linux configuration
gains `NO_HZ_IDLE`, which is how Linux itself tells when the next timer is due.

### The decision

1. **Linux's own `kernel/time/timer.c` and `kernel/softirq.c`** (tasklets
   with it) are compiled into every driver process, from the base manifest
   (`linuxdrv/emul/manifest`). Their semantics -- `timer_delete_sync`,
   re-arming from a callback, deferrable timers, tasklet ordering, softirq
   context -- are then Linux's, not an approximation of them.
2. **The loop is the tick and ksoftirqd.** At every turn, between tasks,
   `lx_emul_turn()` (`linuxdrv/emul/lx_softirq.c`) brings `jiffies` up to
   the monotonic clock, raises `TIMER_SOFTIRQ` whenever they have moved --
   as a tick does on a busy CPU, so deferrable timers run on the first turn
   past their jiffy as on the first tick -- runs the RCU callbacks whose
   grace period has passed, and runs pending softirqs. Softirqs raised
   inside a task run at `local_bh_enable()`, as on Linux, or at the next
   turn -- where `ksoftirqd` would run them. They run on a stack of their
   own, as x86-64 Linux runs them (`hardirq_stack_ptr`), which the Rust
   half maps with a guard page below it. A task may not give up the CPU
   inside a softirq or with bottom halves off (the count is the process's
   one): it ends the process as a BUG, where Linux warns "scheduling while
   atomic".
3. **`CONFIG_NO_HZ_IDLE`**, for `get_next_timer_interrupt()`: with no task
   to run, the loop asks `lx_emul_next_event()` how long it may sleep --
   not at all while softirqs or RCU callbacks are pending, else until the
   next timer (reckoned from when the current jiffy began, as Linux's tick
   reckons it), else for ever. So a process waiting only on a timer sleeps
   until it, and one with neither a task deadline nor an event is known to
   have nothing that can ever wake it (the DEADLOCK exit). As on Linux, a
   deferrable timer does not wake an idle process; it runs at the next turn
   something else causes.
4. What the two files call and the layer does not need -- `ksoftirqd`'s
   registration, CPU hotplug states, POSIX CPU timers, high-resolution
   timers -- is defined to do nothing; what only Linux's real tick reaches
   (`update_process_times()` and its callees) is left to generated stops,
   which say so if ever reached.

### The alternatives

- **The layer's own timers**: a sorted list of `timer_list`s run from the
  loop. About 150 lines, no configuration change, nothing to stub. But it is
  a second implementation of an interface drivers lean on in subtle ways
  (`timer_delete_sync` racing a running callback, `timer_shutdown`,
  deferrable and pinned timers, `mod_timer` returning whether it was
  pending), and tasklets and softirqs would need the same again. §1578 chose
  Linux's own code wherever the bottom allows, for exactly this reason.
- **A periodic tick** (keep `HZ_PERIODIC`, wake every jiffy while the
  process lives). Simplest, but every idle driver wakes 250 times a second,
  and the loop could no longer tell a process that will never be woken: the
  DEADLOCK exit, which has caught real faults, would be lost.
- **Run softirqs on the task's stack**, shadowing `<asm/irq_stack.h>`. One
  stack fewer, but a softirq would then add its depth to whatever task
  happened to enable bottom halves, against stacks that are fixed at 64 KiB.

### What it costs

- The configuration change rebuilds every object of the shared Linux tree
  once, and, until every lane has merged it, a boot test from a lane still on
  the old configuration rebuilds it again (the tree is shared and keyed by
  version, `scripts/linux-tree.sh`). Transitional only.
- A turn raises the timer softirq once per jiffy, whether or not a timer
  is due -- the cost a periodic tick has on a busy CPU; the wheel's next
  expiry is asked only when the loop has nothing to run.
- `NO_HZ_IDLE` adds the deferrable and global timer bases to the wheel,
  which a uniprocessor process does not need; harmless.

### Where it lands

`linuxdrv/linux.config` (`CONFIG_NO_HZ_IDLE=y`), `linuxdrv/emul/manifest`,
`linuxdrv/emul/lx_softirq.c`, `linuxdrv/emul/lx_emul.c` (`lx_emul_init`
readies softirqs and timers before the loop first turns),
`linuxdrv/emul/lx_sched.c` (the sleep-while-atomic check),
`linuxdrv/kit/src/sched.rs` (the loop's turn and its next event;
`slx_stack_top`), `linuxdrv/link.ld` (`.softirqentry.text`), and the hello
driver's fourth initcall and "atomic" mode, which test it.
