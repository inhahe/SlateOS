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

**Not yet known:** whether the APs' LAPIC timers stop (or never start: CPU 2's
heartbeat never moved), whether the APs spin with interrupts off (a lock), or
whether QEMU's TCG on this host starves vCPUs (one emulation thread round-robin
across four vCPUs would look much like this, and would not be the kernel's
fault). The first step is to tell those apart: the same boot with `-accel
whpx` or on a Linux host with MTTCG, and an NMI backtrace of a silent AP (the
hard-lockup watchdog's dump, `--hard-lockup-watchdog`).

**What would make it stay fixed:** a boot test on more than one CPU. Today
`scripts/boot-test.sh` starts QEMU without `-smp`, so a multi-CPU regression
is invisible to every gate. Once an SMP boot is clean, it should run at least
in the release boot.
