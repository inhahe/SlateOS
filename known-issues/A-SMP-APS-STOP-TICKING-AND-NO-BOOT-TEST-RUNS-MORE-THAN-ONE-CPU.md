### [A] On more than one CPU the application processors stop ticking, and no boot test runs more than one CPU -- 2026-10-08

**Status:** OPEN -- for its second half only. The scheduling causes, and the
rest the two-CPU boots found, are fixed and on main since b083cfeca (lane
A's publish of 2026-10-09, boot-tested green at 3d83e0264); a two-CPU boot
with every self-test on is clean. No boot test runs more than one CPU by
default yet: `boot-test.sh --smp=N` exists and nothing passes it.

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

The first two-CPU boot with (4) hung at the boot's first wake after the
network tests started: task 0 is the BSP's idle task *and*, until the boot
is done, the boot itself, which blocks and is woken like any task -- and it
was not pinned, so the wake queued it on the idle AP, leaving the BSP
without its idle task. `PREV_TASK_IDS` and `running_elsewhere` already
assumed task 0 never leaves the BSP. Every idle task is now pinned to its
own CPU (`Task::new_idle`, `Task::new_ap_idle`), and the pin is not passed
on to what an idle task creates (`sched::inheritable_affinity`), so init
and the kernel's services, which the boot starts, keep every CPU.
`sched::smp_self_test` checks both.

With tasks on both CPUs, two more causes came out, each fixed:

- **An idle AP stopped its tick with timers queued on it.** A CPU's
  hrtimers are fired only by its own timer interrupt (`hrtimer::
  process_expired`), and the AP idle loop stopped the timer regardless: a
  task that slept on CPU 1 -- the netstack daemon, its 1 ms poll sleep --
  was never woken, its timer 17 s overdue when the boot gave up on it (the
  hang dump's "OVERDUE - the ISR scan is not draining"). The idle loop now
  keeps the tick while `hrtimer::has_pending_on(cpu)`, checked with
  interrupts off up to an atomic `sti; hlt`.
- **An AP ran as CPU 0 until it mapped itself.** `smp::fast_cpu_index`
  answers an unmapped APIC ID with 0, and an AP published its own mapping
  only after its GDT, SYSCALL, IDT and APIC set-up, so whatever it locked
  or allocated before that counted as CPU 0's: a heap lock's preempt count
  raised on CPU 0 and lowered on CPU 1 left the BSP's count at 1 for the
  rest of the boot, and every voluntary switch there was reported as made
  under a lock nobody held. The BSP now publishes each AP's index before
  starting it. And `sched::preempt_disable` reads the CPU index and raises
  the count with interrupts off, so a task preempted between the two and
  resumed elsewhere can no longer raise one CPU's count and lower another's.
  The switch-under-lock report now names where the count rose
  (`PREEMPT_DISABLE_SITE`) and any enable that found 0
  (`sched::preempt_underflows`) -- which is how this one was found.

**Verified (lane-a-wip, 2026-10-09):** two vCPUs under MTTCG,
`selftest.skip=1`: BOOT_OK in 85 s; no lockup, wedge, overdue timer or
switch-under-lock report; the ring benchmark's `idle_beside_busy` load
`"samples":16, "errors":0, "window_ms":1750` (was 234524 and timed out);
the network witnesses (head-of-line, late data, FIONREAD) pass on both
designs.

Found on the way, and fixed with them: the voluntary context-switch path
(`yield_now`, `block_current`) ran with interrupts on between naming the
incoming task current and switching to its stack, though `switch_context`
asks for them off. An interrupt there whose exit found a preemption pending
would run a second `schedule_inner` as the wrong task and save the outgoing
task's stack as the incoming one's context. `sched::SwitchIrqs` now clears
the flag for the switch and gives each task back the one it entered with;
`sched::self_test_switch_interrupt_flag` checks both states.

**The first full-self-test boot on two CPUs (2026-10-09)** -- QEMU with
`-smp 2 -accel tcg,thread=multi` and every self-test on (what
`boot-test.sh --smp=2` now does) -- found four more, each fixed on
lane-a-wip:

- **An AP was told it was CPU 0 until every AP had started.**
  `smp::current_cpu_index` answered 0 until the BSP set `SMP_INITIALIZED`
  at the end of bring-up, so AP 1, starting, raised CPU 0's preempt count
  taking a lock and lowered its own releasing it, leaving CPU 0's count at
  1: every voluntary switch there was reported as made under a lock nobody
  held, and once the affinity test's spinning task was pinned to CPU 0,
  CPU 0 could never preempt it and the boot starved behind it. The gate
  protected nothing (`fast_cpu_index`'s tier 0 covers the BSP's window);
  `current_cpu_index` is now `fast_cpu_index`, and an AP writes its
  `IA32_TSC_AUX` (the RDPID and rdtscp tiers) before its first lock.
- **CPU 0 had no idle context of its own while the boot ran.** With task 0
  -- the boot -- asleep, the scheduler's idle fallback idled on the stack
  of whichever task had just stopped, which tied that task to CPU 0: the
  affinity test's task pinned itself to CPU 1 and "returned on 0". CPU 0
  now has a hidden boot-time idle task the fallback switches to
  (`sched::BOOT_IDLE_ID`, design-decisions 1559).
- **A container's init ran before it was in its container**
  (`A-CONTAINER-INIT-RAN-BEFORE-IT-WAS-IN-ITS-CONTAINER`): `container::run`
  made it runnable, then bound it to its cgroup, namespaces and root. Now
  the process is spawned unstarted and started last.
- **A task spawned suspended could be started by a stray wake.** Only
  `sched::admit` starts one now (`Task::awaiting_admission`).

And five self-tests that held only on one CPU, each now waiting for what it
checks: the container `exec` and live health-check tests reaped once and
checked the cgroup while the exited task was still finishing on the other
CPU (`container::reap_until_gone`); the futex PI-timeout and service
blocking-accept tests yielded a fixed number of times for a task the wake
had placed on the other CPU (`selftest::wait_until`, bounded by the clock);
and the container run/logs/port tests now pin their init to the CPU whose
interrupts they hold off.

**Verified (lane-a-wip, 2026-10-09):** two CPUs under multi-threaded TCG,
every self-test on: BOOT_OK after 1628 s, no self-test failed or was carried
past, no lockup, wedge or switch-under-lock report (fc6aa39db). The kexec
restart works on two CPUs as well (`SYS_POWER_RELOAD`'s boot: AP 1 stopped
by NMI, restarted by the new kernel).

**What would make it stay fixed:** a boot test on more than one CPU.
`scripts/boot-test.sh --smp=N` exists since 2026-10-09 (lane-a-wip):
`-smp N` under multi-threaded TCG, recorded as a boot of the tree with its
own `cpus` population, not as an experiment. Nothing runs it by default
yet; once two-CPU boots are clean it should be the release boot's shape.
