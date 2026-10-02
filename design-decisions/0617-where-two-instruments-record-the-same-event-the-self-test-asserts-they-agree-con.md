## 617. Where two instruments record the same event, the self-test asserts they agree — conditionally, so it never depends on hardware

**Date:** 2026-08-26
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel keeps two separate records of "a device interrupt
happened": `irqbalance` flips a per-IRQ *did this line fire* flag, and `idt`
increments a per-vector counter. They are written one function call apart on
the same code path, so they can never legitimately disagree — but nothing
checked, so if one drifted from the other, nothing would notice. The
self-test now asserts they agree about whatever fired. The decision was how
to assert that without making the test depend on a device having happened to
interrupt during boot, which would be flaky.

**The problem with the obvious version.** The natural assertion is "at least
one device IRQ has fired, and its vector was counted." That is flaky by
construction: which IOAPIC lines have fired by the time a self-test runs is a
property of the emulated hardware and of where in boot the test is sequenced,
neither of which the module controls. On a headless automated boot nobody
types, so the keyboard may never interrupt at all.

Concretely, this is not hypothetical — the two interrupt self-tests in this
kernel sit on opposite sides of the line:

| Self-test | Runs at (serial line) | Device IRQs possible? |
|---|---|---|
| `fs::irqstat::self_test` | 39822 | **No** — first unmask is at 44698 |
| `irqbalance::self_test`  | 44781 | Yes — saw IRQ 1, unmasked at 44698 |

`irqstat`'s battery runs before `keyboard::init` unmasks IRQ 1, so it can only
ever observe the timer. An unconditional "a device fired" assertion there
could never pass; the same assertion in `irqbalance` would pass today and
break the first time boot order changed.

**The decision.** Assert *conditionally on what actually fired*: for every IRQ
line marked active, require that its vector was counted. Nothing fired,
nothing is claimed. The count itself stays a report.

*What this buys:* the assertion tests a property of **this kernel** (two
records of one event agree, and `vector_for_irq` names the slot the IOAPIC was
actually programmed with) rather than a property of the hardware. It is
immune to boot ordering, to CPU count, and to whether QEMU delivered anything.
It also re-checks the `irq + 32` off-by-one that §616 describes against live
traffic instead of against arithmetic — the arithmetic check (Test 8) and this
one fail for different reasons.

*What it costs — and this is a real cost:* a conditional assertion is vacuous
when the condition is empty. If device interrupts ever stopped reaching this
module entirely, Test 9 would assert nothing and still pass. That failure is
not silent — the line prints `none have fired yet`, which is greppable and
which the surrounding comment calls out — but it is a report, not a failure.
The alternative (assert unconditionally) trades a rare vacuous pass for a
regular false failure, which is worse: a test that cries wolf gets deleted.

**Considered and rejected:** asserting this from `boot-test.sh` against the
serial log instead, where the QEMU environment is controlled. That was the
previously-recorded idea. It is worse for two reasons — it can only check what
the kernel chose to print, and it puts the check in a different language and
file from the invariant it protects, where a kernel-side refactor will not
drag it along.

**Fragility noted and guarded.** Test 4 in the same function calls
`notify_irq(10)` directly and clears the flag afterward. That cleanup was
tidiness; it is now load-bearing, because a leaked flag would fail Test 9 with
a message blaming the ISR path for a fixture's mess. Both ends now say so, and
the assertion message names the possibility explicitly.

**Where it bites:** `kernel/src/irqbalance.rs` (Test 4's cleanup, Test 9).
