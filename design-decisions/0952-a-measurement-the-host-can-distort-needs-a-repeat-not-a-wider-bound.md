## 952. A measurement the host can distort needs a repeat, not a wider bound

**Date:** 2026-09-18 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; prompted by a red boot whose kernel delta was comment text

**In short:** a start-up check confirms that a 20-millisecond sleep really
takes about 20 milliseconds. Once, it took 988, and the check killed the
boot. The machine was busy compiling at the time, and the check has no way
to tell a busy machine from a broken timer. The obvious repair is to accept
a longer sleep; instead the check now measures three times and complains
only if every attempt is slow, because a busy moment passes and a broken
timer does not.

**The situation.** `sched::test_sleep_ns` asserts a 20ms sleep lands in
5ms..500ms. On boot `a17b8e0fa` it measured 988ms and panicked. The kernel
diff from the previous green boot was 4 files, 50 insertions, **all of it
`//!` comment text** -- which cannot change runtime timing. Across the 19
prior boots that logged the figure it ran 20.768ms..32.826ms, median
22.083ms, so 988ms was 30x the observed maximum; two `cargo` processes were
running while QEMU booted; and `boot-test.sh` passes neither `-icount` nor
`-rtc clock=vm`, so the guest clock follows host wall time and a host stall
inflates this number directly.

**The decision.** Retry up to 3 times, pass on the first attempt under the
ceiling, panic only if all overshoot, and print every measurement.

| option | for | against |
|---|---|---|
| raise the ceiling to ~2s | one line; never flakes again | the ceiling is the *only* reader of this signal, so widening it spends the entire detection budget of the test. dd-951 is the rule: enumerate a signal's readers before relaxing it, and here there is exactly one |
| drop the upper bound | no false boot failures | "the timer never fired" becomes undetectable, and that is the failure the test was written for |
| **retry, then panic (chosen)** | a host stall does not repeat; a dead timer overshoots every attempt, so detection survives intact | up to 3 task spawns; and a *genuinely intermittent* timer bug at <1-in-3 frequency is now silently retried rather than reported |
| pin the guest clock (`-rtc clock=vm`) | removes the host's influence at the source | changes what every other timing measurement in the boot means, including the benchmark scorecard. Far too wide a blast radius for one assertion |

**The cost, stated plainly, because it is the reason this is a decision and
not a fix.** An intermittent timer stall that happens less than one time in
three now produces a passing boot. That is a real loss of sensitivity, and
it is bought deliberately: a boot-killing assertion that fires on host load
trains its reader to disbelieve it, and a disbelieved assertion has less
detection power than a retried one. The mitigation is that the retry is not
silent -- each overshoot prints its measurement, so the frequency becomes
visible in the logs rather than being converted into a boot failure. If
those lines start appearing without a panic, that IS the intermittent-bug
report, and it arrives without costing a boot.

**A second defect, found only because two assertions disagreed.** The wait
budget was `apic::tick_count() + 50` while the panic on it read *"did not
complete within 500ms"* -- the same quantity in two units, equal only if a
tick is exactly 10ms. On this boot the tick budget outlasted the 988ms
overshoot, so the guard never fired and a downstream assertion caught the
problem instead. It was visible only because `done != 0` passed while
elapsed reported 988ms, which is impossible if both bounds are 500ms.

The general form, which is the part worth keeping: **a bound stated in units
it does not measure is a bound that lies exactly when it is needed.** The
wait is now bounded by `hrtimer::now_ns()`, which reads the HPET or the TSC
-- both free-running, so the deadline still expires when the APIC timer is
the broken thing. That property is why the real-time bound is safe here and
would not have been if the clock were timer-driven.

**What this does not settle.** Whether that 988ms was contention or a latent
stall is still unknown; the retry makes the boot survive the ambiguity and
makes the evidence legible, not the cause known. The boot lock serialises
QEMU across lanes but does not stop another lane *compiling* during a boot,
so the contention path remains open and is recorded in `known-issues.md`.
