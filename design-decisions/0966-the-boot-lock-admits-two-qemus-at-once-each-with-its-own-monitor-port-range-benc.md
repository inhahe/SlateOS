## 966. The boot lock admits two QEMUs at once, each with its own monitor-port range; benchmark and load-experiment boots still take the machine alone

**Date:** 2026-09-26 &middot; **Decided by:** Claude (operator-approved scope) -- the operator asked for the limit to be measured and adjusted "as appropriate", argued that even a large mutual slowdown is worth taking while two boots still finish sooner together than one after the other, and then left both the number and whether to run a dedicated slowdown experiment to Claude &middot; **Lane:** A

**In short:** only one lane at a time could run its test virtual machine, so a
lane that finished building while another lane's machine was running had to
wait for it -- up to 40 minutes when that machine was stuck. Now two can run
at once. Each one uses about one of the computer's twelve processor threads,
so two of them barely slow each other, and a boot that runs alongside another
still finishes far sooner than one that waits its turn. Boots that measure
performance still get the computer to themselves, because a second virtual
machine running beside them would distort what they measure.

**What was decided.**

* `BOOT_LOCK_SLOTS` (default 2) slots, each a `mkdir` lock directory: slot 1 is
  the old `slateos-boot-lock` itself, slot *k* is `slateos-boot-lock.slot`*k*.
  A lane still running a pre-slot `boot-test.sh` sees only slot 1, so the host
  never has more QEMUs than slots however the lanes' copies differ; slots are
  taken highest first so that such a lane is not left waiting behind a slot it
  cannot see.
* The ticket queue stays FIFO: a waiter may try only while fewer tickets are
  ahead of it than slots are free, and never past an exclusive ticket.
* Exclusive runs take every slot: `--bench` (which `canary-load-test.sh` uses)
  and `BOOT_LOCK_EXCLUSIVE=1`. An exclusive
  waiter collects slots as they free and holds everyone behind it meanwhile, so
  ordinary boots cannot starve it by slipping into each slot in turn.
* The QEMU monitor port is chosen after the lock, from the slot's own range of
  50 (slot *k*: 57000 + 50(*k*-1); no slot: 56950-56999). It used to be chosen
  at setup, hours before QEMU bound it -- harmless with one QEMU; with two,
  both runs would find 57000 free and the second QEMU would die binding it.
  Nothing else a boot writes is shared between worktrees (images, serial log,
  pidfile, ESP are all under the worktree; the firmware is read-only; the
  networks are user-mode with no host ports).

**Measured, not guessed -- and one measurement deliberately not taken.**
2026-09-26, the host at 100% CPU (six lanes; Process Lasso then still lowering
their tools' priority, see §963):

1. *What the one-slot lock was costing.* `bench/boot-history.jsonl`, the 12
   runs since the six-lane split (2026-09-22): the time between the build
   finishing and QEMU starting -- which contains the lock wait -- was a median
   0.8 min and at most 2.0 min. A passing run's QEMU phase was 3.6-5.3 min of
   a 141-165 min run; the gate phase (median 139 min) takes no lock. But 6 of
   the 12 runs hung and held the lock for their full 40-minute QEMU timeout
   (one for 68 min), and a lane arriving then waits that long.
2. *What a second QEMU costs a boot* was not measured by a dedicated
   experiment, on purpose. Each QEMU runs one TCG vCPU -- about one of the
   host's twelve threads -- above normal priority (§963), so two cannot come
   near doubling each other, which is the only slowdown that would make
   running them together slower than one after the other. An experiment of
   single, paired and tripled boots would have held the lock for one to two
   hours of a host already at 100%, to settle a question whose answer could
   save about a minute a boot. The boot histories record when every QEMU
   ran; boots that did overlap, from now on, are the measurement -- and a
   run of false TIMEOUTs among them is the sign to go back to one slot.

**Alternatives considered.**

| option | why not |
|---|---|
| Keep one slot | the wait it causes is rare but long (a hung boot holds it 40 min), and the reason given for it -- two TCG QEMUs "roughly double each other's boot time" -- predates QEMU running above normal priority and was never measured |
| Three or more slots | nothing measured asks for it -- two already cover the rare overlap -- and each slot is another core held above normal priority on a host at 100% |
| No lock at all | the benchmark and load experiments need the machine to themselves, and a lock is also what keeps a hung boot's QEMU count bounded |
| Choose the monitor port by retrying on bind failure | handles any clash, but QEMU fails at startup with an error that has to be recognized and the boot restarted; disjoint ranges make the clash impossible instead |

**How to reverse.** `BOOT_LOCK_SLOTS=1` in the environment restores the
one-QEMU lock for a run; changing the default in the region does it for every
lane that has merged it.

**Where this bites:** `scripts/boot-test.sh` (BOOT-LOCK-REGION,
MONITOR-PORT-REGION, `pick_monitor_port`, `--bench`),
`scripts/test-boot-lock.sh` (cases 16-30), `scripts/test-boot-test.py` (runs it).
