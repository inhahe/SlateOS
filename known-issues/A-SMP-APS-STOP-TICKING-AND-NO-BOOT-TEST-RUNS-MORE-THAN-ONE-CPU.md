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

**Next step:** reproduce with `-smp 2` under MTTCG, then name the boot
thread's wait: the hard-lockup watchdog's NMI dump (`--hard-lockup-watchdog`)
or a `[sched]` dump of every task's state and wait channel at the deadline.

**What would make it stay fixed:** a boot test on more than one CPU. Today
`scripts/boot-test.sh` starts QEMU without `-smp`, so a multi-CPU regression
is invisible to every gate. Once an SMP boot is clean, it should run at least
in the release boot.
