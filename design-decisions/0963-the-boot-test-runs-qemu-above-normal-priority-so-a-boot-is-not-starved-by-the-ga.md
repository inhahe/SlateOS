## 963. The boot test runs QEMU above normal priority, so a boot is not starved by the gates and builds of the lanes sharing its machine

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A

**In short:** six lanes share one computer, and much of the time they are
all running their long pre-boot checks and builds at once. The virtual
machine a boot test starts ran at the same priority as all of that, so it
got a sliver of a processor, the operating system inside it fell behind its
own clock, and the boot test reported the guest as hung -- a failure that
was the machine's, not the code's, found only after an hour of checks. The
boot test now asks Windows to run its virtual machine at "above normal"
priority, so it runs ahead of that background work. Only one boot's virtual
machine runs at a time, and it keeps one or two of the computer's six
physical cores busy (twelve counting hyperthreads).

**What was decided.** After launching QEMU, `boot-test.sh` reads QEMU's
Windows PID from the pidfile it already writes and calls
`SetPriorityClass(ABOVE_NORMAL_PRIORITY_CLASS)` on it, in the background so
the wait for the pidfile delays nothing. Every outcome is printed: raised,
left at normal (no pidfile, no Python), or refused. `BOOT_QEMU_PRIORITY=normal`
skips it.

**Measured, not guessed.** On 2026-09-25 the host ran at 100% CPU for hours
-- 78 bash and 26 Python processes from other lanes' gates, four cargo
builds -- and plain spinners measured 0.012 to 0.117 of a core. Earlier the
same day a quick boot's heartbeats collapsed under the same kind of load.
QEMU runs are already serialized across lanes by the boot lock, so at most
one QEMU holds the boost at a time. And the starting point was lower than
"normal": a process launched from the agents' environment inherits
BELOW_NORMAL (0x4000, measured on a stand-in for QEMU), so every boot's QEMU
has been competing at the same class as all the gate work around it. The
boost takes it two classes up, to 0x8000.

**Alternatives considered.**

| option | why not |
|---|---|
| Lower everything else (gates, builds) to below normal | the same effect from the other side, but it has to be done by every lane's every heavy command, and one that is missed puts a boot back among a hundred equals |
| A cross-lane lock on the gate phase, as for QEMU | serializes an hour of checks per lane across six lanes -- a boot would wait hours for its turn -- to protect the ten minutes that are timing-sensitive |
| HIGH or REALTIME priority | no measured need, and HIGH can starve the desktop; above normal is enough to win against normal-priority throughput work |
| Leave it, and let the harness decline starved runs | the harness's suites now do decline them honestly (`scripts/hostload.py`), but a declined boot is still a boot that verified nothing |

**Corroborated on a live boot (lane D, 2026-09-25).** Lane D's boot of
`506a50749` -- a debug build, before this change reached lane D -- had its
QEMU (pid 74596) at BelowNormal on a host pinned at 100% of its twelve logical
CPUs. Between the liveness breadcrumbs at 2310 s and 2340 s the guest's
heartbeat advanced by 434; lane D then set that one process to AboveNormal by
hand at about 2350 s, and the next two 30-second windows advanced it by 2,973
and 3,016 -- about seven times as much. The boot's self-test rungs went from
359 to 390 in some 90 seconds, and to 443 by 2550 s, after minutes between
rungs before. Lane D also observed that every debug boot recorded since
2026-09-22 printed about 3,000 serial lines against about 47,000 for the last
passing one (2026-09-16): starvation, not a kernel regression, and the right
reading of lane A's 2026-09-22 TIMEOUT (2419 s wall) too. Quoted with lane
D's agreement.

**Process Lasso was undoing it (found 2026-09-25; fixed by operator decision).**
Lane B's watcher raised its QEMU to AboveNormal and minutes later found it at
BelowNormal. The cause is on the host, not in the harness: Process Lasso runs
here (its `ProcessGovernor` service), and its ProBalance restrains -- lowers
to BelowNormal -- any process above 7% of total CPU once the system is over
10% busy, after 0.9 s over quota (`C:\ProgramData\ProcessLasso\config\
prolasso.ini`, `[OutOfControlProcessRestraint]`), with no exclusions. A TCG
QEMU uses about one of the twelve logical CPUs, 8%, so on a loaded host every
boot's QEMU qualified within a second. Its log showed one QEMU (pid 46812)
restrained almost continuously: each restraint lasted about 70 s and the next
began one second after it ended (18:28:42, 18:29:57, 18:30:04). The raise
above could not survive that, and the load it exists for is exactly when
ProBalance acts.

**Decided by:** Operator (Claude proposed the exclusion; the operator chose it
over changing the setting by hand or leaving ProBalance as it was). At 18:30
`qemu-system-x86_64.exe` was added to `OocExclusions` -- the file backed up
beside itself as `prolasso.ini.bak-2026-09-25-before-qemu-exclusion`, its
UTF-16 encoding and line endings kept, that one line changed -- and the
service restarted. ProBalance has restrained the lanes' `bash.exe`,
`python.exe` and `rustc.exe` since, and no QEMU. The exclusion is host
configuration, not in this repository: a rebuilt machine needs it again, and
`raise_qemu_priority` now reads the class back after raising it and every
30 s after that for as long as QEMU runs, so that a return of the problem is a
line in the boot log rather than a mysteriously starved boot.

**What it changes for measurement.** The kernel benchmarks inside a boot now
run with less host interference, so their noise should drop from this date;
a step in `bench/history.jsonl` around 2026-09-25 may be this, not the code.
The load-canary experiments measure exactly that interference, so
`canary-load-test.sh` sets `BOOT_QEMU_PRIORITY=normal`.

**Revisit if** the guest ever runs more than one vCPU (the boost would then
take several cores), or a lane needs its QEMU to compete with host load for a
reason other than the canary.

**Where this bites:** `scripts/boot-test.sh` (`raise_qemu_priority`, called
after the QEMU traps are installed), `scripts/canary-load-test.sh`.
