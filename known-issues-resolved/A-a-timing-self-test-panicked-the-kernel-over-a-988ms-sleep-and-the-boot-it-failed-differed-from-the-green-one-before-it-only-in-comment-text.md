### [A] A timing self-test panicked the kernel over a 988ms sleep, and the boot it failed differed from the green one before it only in comment text -- 2026-09-18
**Status:** FIXED 2026-09-18, boot-verified on ebb683642 -- `[sched] sleep_ns: PASSED (slept 37.892ms for 20ms request, attempt 1 of 3)`. No retry was needed, and the attempt number is reported either way, which is the dd-942 half: PASSED first time and PASSED after a retry are different facts. Note 37.892ms is a new maximum -- the prior observed range was 20.8-32.8ms -- so the measurement keeps drifting up, still far inside the 500ms ceiling

**In short:** the kernel checks at startup that asking to sleep for 20
milliseconds really does take about 20 milliseconds. On one boot it took 988,
so the check killed the boot. Nothing in the kernel had changed that could
affect timing -- the only difference from the previous working boot was
comment text -- and two compilers were running on the host at the time. The
check could not tell "the machine was busy" from "the timer is broken", and
treated the first as the second.

**The evidence that it is not a regression**, in the order it was gathered:

| question | answer |
|---|---|
| what changed since the last boot? | `git diff 3a29fd0d2..a17b8e0fa -- kernel/`: 4 files, 50 insertions, **all `//!` doc comment text** in `fs/{authbroker,faceunlock,filevault,sealing}.rs` |
| anything touching the timer? | no match for `hrtimer`, `apic`, `sched`, `timer`, `sync.rs` or `lockdep` in the changed-file list |
| is 988ms within normal spread? | no. Across the 19 prior boots that logged it: min 20.768ms, median 22.083ms, max 32.826ms. 988ms is **30x the observed maximum** |
| were neighbouring measurements also inflated? | no. A deliberately forced ~10ms spin measured 13ms 2,700 lines later; the `[multiwait]` waits all landed at 22-50ms |
| could the host inflate it? | yes, directly. `boot-test.sh` passes neither `-icount` nor `-rtc clock=vm`, so the guest clock follows host wall time -- and two `cargo` processes were running while QEMU booted |

Comment text cannot alter runtime timing, so the change set exonerates itself.

#### The second defect, which is the one worth keeping

The test's two bounds were **written in different units, and its message
asserted they were the same one.** The wait budget was
`apic::tick_count().saturating_add(50)` while the panic attached to it read
*"did not complete within 500ms"* -- equal only if a tick is exactly 10ms.

That is not academic: on this boot the tick budget outlasted the 988ms
overshoot, so the guard meant to cap the wait never fired, and the elapsed
assertion downstream is what caught it. Had the tick guard fired, it would
have reported "500ms" about a wait of some other length. A bound whose
message is in units it does not measure is a bound that lies exactly when it
is needed. Found only because the two assertions *disagreed* -- `done != 0`
passed while elapsed said 988ms -- which is impossible if both are 500ms.

#### The fix is a retry, not a looser ceiling

Raising the bound is the obvious move and the wrong one: the ceiling is the
only reader of this signal, and widening it trades away the sole thing the
test detects (dd-951 -- enumerate a signal's readers before relaxing it).

A one-off host stall does not repeat; a timer that is not firing overshoots
every attempt. So the test now measures up to 3 times, passes on the first
that lands under the ceiling, and panics only if **all** overshoot -- printing
all three numbers, because the single retried figure that is never shown is
dd-942 again. The too-short assertion still fails on attempt one: host load
cannot make a sleep return early, so an early return is a real bug with no
benign reading. The wait budget is now `now_ns()`-based, which reads the HPET
or TSC -- both free-running, so the deadline still expires when the APIC timer
is the broken thing.

**What is still unresolved:** whether that 988ms was host contention or a
latent timer stall. The retry does not answer it, it makes the boot survive
it -- and makes the answer legible next time, since three overshoots now
print three numbers instead of killing the boot on one. The boot lock
serialises QEMU between lanes but does **not** stop another lane compiling
while a boot runs, which is the contention path that remains open.
