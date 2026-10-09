### [A] On more than one CPU the application processors stop ticking, and no boot test runs more than one CPU -- 2026-10-08

**Status:** OPEN -- the scheduling causes fixed on lane-a-wip 2026-10-09,
awaiting a boot on main; no boot test runs more than one CPU yet.

**In short:** every boot test runs the kernel on a single CPU, so nothing has
been checking that it works on more than one -- and on two or four it did
not, quite. Booted under QEMU with several CPUs, the other CPUs came up and
then mostly sat idle while work queued behind one busy CPU; a benchmark that
takes 7 s on one CPU took four minutes on two and then gave up. A real PC
has more than one core, so this is what the system would have done on actual
hardware. Four causes were found and fixed (below). What remains is a boot
test on more than one CPU, so that it stays fixed.

**How it was found:** testing `kexec` (restarting without the firmware) on
four CPUs, 2026-10-08, with the fast-boot helper given `-smp 4` and
`selftest.skip=1 kexec.selftest=1`. That boot first found a worse bug, fixed
the same day: the system-call entry kept one per-CPU record for all CPUs and
set up only the boot CPU's MSRs, so a thread that blocked in a system call
and resumed on another CPU wrote its stack pointer to address 8
(`syscall::entry`, `PER_CPU` is now one record per CPU, set up on each by
`init_ap`; `smp::self_test` checks every online CPU).

**What the log showed:**

- 4 vCPUs, single-threaded TCG: `SOFT LOCKUP on CPU 2 (heartbeat stuck at
  0)`, `CPU 1 ... stuck at 2`, `CPU 3 ... stuck at 2` two seconds after the
  APs came up, every five seconds for 45 s, then `CPU 1 recovered`.
- 2 vCPUs, `-accel tcg,thread=multi` (one host thread per vCPU): no lockup
  report, but the ring benchmark's `idle_beside_busy` load ended at its
  deadline -- `"samples":2, "errors":14, "window_ms":234524,
  "timed_out":true` -- where one CPU takes about 7 s. The QEMU monitor
  (`info registers -a`) showed CPU 0 in `hlt` the whole time and CPU 1
  running the load's busy sender.

**The four causes, and what was done** (lane-a-wip, 2026-10-09):

1. **A task switched in at an interrupt's exit ran with no tick.** An AP's
   idle loop stops its LAPIC timer before `hlt` and restarts it on its own
   way out. A switch made from an interrupt that arrived during that `hlt` (a
   deferred preemption, `sched::do_deferred_preempt`) never went back
   through the loop, so the task it switched to -- and everything that CPU
   ran until it next idled -- ran with no timer: never time-sliced, never
   balanced. That is the busy sender owning CPU 1 for minutes. **Fix:**
   `apic::TIMER_STOPPED` records which CPUs stopped their timer, and every
   switch to a task other than the CPU's idle task restarts it
   (`sched::leave_idle_for`).
2. **The same switch left the CPU marked idle for RCU.** The idle loops mark
   the CPU idle (quiescent) for RCU before `hlt`; a task switched in from
   the interrupt ran with the mark still set, so `rcu::synchronize` -- and
   `membarrier`'s global barrier, built on it -- passed that CPU over even
   inside a read-side critical section. On the BSP this was the common case:
   its timer tick preempts the idle loop's `hlt` on every wake. **Fix:** the
   same switch clears the mark (`rcu::leave_idle`, a sequentially consistent
   store).
3. **Both soft-lockup watchdogs reported idle APs as locked up.** An idle AP
   takes no timer ticks by design, so its heartbeat stops; `watchdog::check`
   exempted only `sched::cpu_is_idle`, which is the scheduler's idle
   *fallback*, not where an AP idles, and `sched::watchdog_check` exempted
   nothing. Hence "stuck at 0" for an AP that went idle at once, and
   "recovered" when it next ran a task. **Fix:** both exempt
   `apic::timer_stopped_on(cpu)`.
4. **A woken task went back to its busy CPU while another idled.** `wake`
   queued a task on its last CPU however busy that was, and an idle AP, its
   tick stopped, never ran the balancer that might have taken it. **Fix:**
   `sched::select_wake_cpu` keeps the last CPU only if it is idle, and
   otherwise picks an idle CPU the task may run on (design-decisions 1558).

Found on the way, and fixed with them: the voluntary context-switch path
(`yield_now`, `block_current`) ran with interrupts on between naming the
incoming task current and switching to its stack, though `switch_context`
asks for them off. An interrupt there whose exit found a preemption pending
would run a second `schedule_inner` as the wrong task and save the outgoing
task's stack as the incoming one's context. `sched::SwitchIrqs` now clears
the flag for the switch and gives each task back the one it entered with;
`sched::self_test_switch_interrupt_flag` checks both states.

**What would make it stay fixed:** a boot test on more than one CPU. Today
`scripts/boot-test.sh` starts QEMU without `-smp`, so a multi-CPU
regression is invisible to every gate. Once a full-self-test boot on two
CPUs is clean, it should run at least in the release boot.
