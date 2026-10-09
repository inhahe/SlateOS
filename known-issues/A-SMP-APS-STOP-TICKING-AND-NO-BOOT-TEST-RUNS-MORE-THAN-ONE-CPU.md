### [A] On more than one CPU the application processors stop ticking, and no boot test runs more than one CPU -- 2026-10-08

**Status:** OPEN

**In short:** every boot test runs the kernel on a single CPU, so nothing has
been checking that it works on more than one -- and on four it does not,
quite. Booted under QEMU with four CPUs, the other three CPUs come up, then
go silent: the watchdog reports them making no progress for tens of seconds
at a time, and the boot crawls instead of finishing. A real PC has more than
one core, so this is what the system would do on actual hardware.

**How it was found:** testing `kexec` (restarting without the firmware) on
four CPUs, 2026-10-08, with the fast-boot helper given `-smp 4` and
`selftest.skip=1 kexec.selftest=1`. The first such boot found a worse bug,
fixed the same day: the system-call entry kept one per-CPU record for all
CPUs and set up only the boot CPU's MSRs, so a thread that blocked in a
system call and resumed on another CPU wrote its stack pointer to address 8
(`syscall::entry`, `PER_CPU` is now one record per CPU, set up on each by
`init_ap`; `smp::self_test` checks every online CPU). With that fixed the
same boot no longer crashes, and shows this instead.

**What the log shows** (`/tmp`-style copy: the fast boot's
`build/fast/serial.txt`, 4 vCPUs, QEMU TCG on Windows):

- `[smp] SMP bootstrap complete: 4 CPU(s) online`, each AP's idle task
  registered.
- About two seconds later: `SOFT LOCKUP on CPU 2 (heartbeat stuck at 0)`,
  `CPU 1 ... stuck at 2`, `CPU 3 ... stuck at 2` -- CPU 2 never took a single
  heartbeat tick; 1 and 3 took two and stopped.
- Lockup reports every five seconds for 45 s; then `CPU 1 recovered`. A
  network self-test's 2.5 s wait took 34 s. The boot did not reach its end in
  420 s, where one CPU takes about 60.

**The lockups were the emulator; a hang is not.** The same boot under
`-accel tcg,thread=multi` (one host thread per vCPU) has no soft lockup at
all, and the network test's 2.5 s wait takes 2.5 s again: single-threaded
TCG, round-robin across four vCPUs, was starving them. But that boot hangs
too, later: in the ring benchmark's `idle_beside_busy` load
(`net::ring_bench`), the prober task finishes (`Task 20 exiting`) and the
boot thread never prints the load's report line -- after which it would
stop the busy sender, wait at most `LOAD_DEADLINE_MS` for it, close the
sockets and go on. So the boot thread is stuck in `wait_for`'s
`sleep_ms(5)` (a timer wakeup lost across CPUs) or in `socket::close`
behind the busy sender running on another CPU (a lock or a wakeup lost the
same way). One CPU never shows it.

**It is starvation, not a deadlock** (2 vCPUs under MTTCG, the QEMU
monitor's `info registers -a` and stack words, symbolized; 2026-10-08):

- The load does end -- at its deadline: `idle_beside_busy` reports
  `"samples":2, "errors":14, "busy_bytes":10228736, "window_ms":234524,
  "timed_out":true`, and the boot goes on. On one CPU it takes about 7 s and
  fails nothing.
- Throughout, **CPU 0 is idle in `hlt`** (interrupts on), and **CPU 1 runs the
  busy sender** -- caught inside `KernelHeap::dealloc` / `check_redzone`,
  re-reading its APIC ID (`current_cpu_index`) on the heap's hot path: busy,
  not stuck. So the prober, the boot thread and the netstack daemon are
  runnable (or soon woken) on CPU 1's queue while CPU 0 has nothing: no idle
  CPU takes work from a busy one, and CPU 1 barely time-slices the busy
  sender against them (two probes in four minutes).

**What to look at:** where a woken task is queued (`sched::wake` and
`try_wake` -- the waker's CPU, the task's last one, or an idle one), whether
an idle CPU ever pulls from a loaded one (`sched_migrate`, the idle loop),
and why round-robin at priority 16 on CPU 1 does not give the prober its
turns -- the anti-starvation booster is per-CPU, and a task queued behind a
CPU-bound one should still get a slice each tick.

**What would make it stay fixed:** a boot test on more than one CPU. Today
`scripts/boot-test.sh` starts QEMU without `-smp`, so a multi-CPU regression
is invisible to every gate. Once an SMP boot is clean, it should run at least
in the release boot.
