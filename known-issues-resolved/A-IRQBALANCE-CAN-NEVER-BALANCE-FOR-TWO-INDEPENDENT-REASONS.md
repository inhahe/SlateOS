## `A-IRQBALANCE-CAN-NEVER-BALANCE-FOR-TWO-INDEPENDENT-REASONS` (lane A, 2026-08-26) — ✅ FIXED 2026-08-26, and it was three

**Fixed** in `1ae48ea8c` (the wiring and the off-by-one) on top of `16d2a7857`
(the counting). Boot-verified — see the closing note at the end of this entry.

**There was a third reason, found only while fixing the first two**, and it is
the interesting one: `balance()` computed the vector for IRQ *N* as `N + 32`,
but `ioapic.rs` programs IOAPIC input *N* onto `IRQ_VECTOR_BASE + N` = **33** +
*N*, deliberately "keeping vector 32 reserved for the LAPIC timer". So IRQ 0's
rate was read from the *timer's* slot, IRQ 1's from IRQ 0's, and so on down the
line.

**That third defect was harmless only because of the second one.** With no
hardware vector counted, every slot read zero — and a shifted index into an
all-zero array also reads zero. The moment the counts became real, slot 32
started carrying ~1000 ticks a second, far above `MIN_RATE_THRESHOLD`. Fixing
reasons 1 and 2 alone would therefore have produced a balancer that concluded
IRQ 0 was the busiest line on the machine and began migrating it on the strength
of the timer's traffic. **Dead would have become actively wrong**, and the
"fix" would have been the thing that armed it.

The general lesson, which is worth more than the bug: *a dormant subsystem can
hide defects in its own logic, because dormancy feeds it uniform inputs that
every version of the logic maps to the same output.* Reviving one is not one
change but two — turn it on, and separately re-derive whether its arithmetic was
ever right. Do not assume the code between the entry and the exit was correct
merely because nobody had complained; nobody could have.

`vector_for_irq()` now derives the mapping from `ioapic::IRQ_VECTOR_BASE` rather
than restating it, since restating it is exactly how the two drifted apart.

**In short:** the interrupt balancer — `kernel/src/irqbalance.rs`, ~500 lines of
greedy bin-packing that is supposed to spread device interrupts across CPUs —
has never moved a single interrupt, and cannot. Its `balance()` pass takes an
early `return` on every invocation, for **two separate reasons, either of which
alone is sufficient.** Its self-test prints `[irqbalance] Self-test PASSED` on
every boot.

**Reason 1 — no IRQ is ever marked active.** `balance()` skips any IRQ whose
`state.active` flag is false:

```rust
for (i, state) in IRQ_STATES.iter().enumerate() {
    if !state.active.load(Ordering::Relaxed) { continue; }      // always taken
```

The only thing that sets that flag is `notify_irq()`, whose doc says *"call from
ISR path"*. It has **no callers on any ISR path** — the only two calls in the
repository are at `irqbalance.rs:465` and `:490`, both inside the module's own
`self_test`. So the loop body never executes in production and `active_count`
stays 0.

**Reason 2 — the counts it would read are all zero.** Even with the flag set,
the rate comes from:

```rust
let vector = i + 32;
let current = vector_counts.get(vector).copied().unwrap_or(0);
let delta = current.saturating_sub(last);
if delta >= MIN_RATE_THRESHOLD  // = 10
```

`idt::vector_counts()` is only ever incremented by `count_vector()`, which is
called exclusively from the CPU **exception** handlers. `dispatch_vector()` —
the one function every hardware interrupt passes through — never calls it (see
`A-IDT-COUNTS-ONLY-EXCEPTIONS-AND-CALLS-THE-TOTAL-INTERRUPTS` above). So every
vector at 32 and up reads 0 forever, `delta` is 0, and 0 is never `>= 10`.

Then:

```rust
if active_count == 0 {
    BALANCE_OPS.fetch_add(1, Ordering::Relaxed);
    return; // Nothing to balance.
}
```

`BALANCE_OPS` still increments, so `irqbalance` stats report a healthy and
rising number of balance passes. Every one of them did nothing.

**Why no test caught it — and it is the lesson already written down in this
tree.** `self_test`'s Test 4 is:

```rust
assert!(!IRQ_STATES[10].active.load(Ordering::Relaxed));
notify_irq(10);
assert!(IRQ_STATES[10].active.load(Ordering::Relaxed));
```

It calls `notify_irq` itself and checks the flag moved. That verifies the
setter, and is structurally incapable of detecting that nothing else in the
kernel ever calls it. `check-tick-wiring.py`'s own failure text says this in as
many words: *"write the regression test through handle_event, never against the
advancing function: a test that calls tick() directly cannot tell a wired app
from an unwired one, which is how all five of these shipped green."* This is the
same shape one subsystem over, and it is worth noting that the existing gate
cannot see it — that gate asks about `Event::Tick` in GUI apps specifically, not
about kernel ISR wiring.

**The proper fix**, and both halves are required — fixing one leaves the
balancer just as dead:

1. Call `count_vector(vector as usize)` at the top of `idt::dispatch_vector`,
   and widen `VECTOR_STATS_SIZE` from 48 to 256 so vectors 251/252/255 are not
   silently dropped by the `.get()`. This is the other known-issue entry and
   fixes reason 2 for every consumer at once (`kstat`, `kcounters`, `irqstat`,
   `irqrate`) because `dispatch_vector` is the single choke point.
2. Call `irqbalance::notify_irq(irq)` from `ioapic::handle_device_irq`, which is
   where the ISR path actually knows the IRQ number. That fixes reason 1.
3. Then write the regression test **through the dispatch path**, not through
   `notify_irq` — otherwise the replacement test has the same blind spot as the
   one it replaces.

**Do not "fix" this by lowering `MIN_RATE_THRESHOLD`.** The threshold is not the
bug; it is the only thing that would stop a balancer fed real data from
migrating an IRQ that fired twice. It becomes load-bearing the moment reason 2
is fixed.

**Open question once it works.** Nobody has ever observed this algorithm running
against real counts, so its behaviour is entirely untested in the sense that
matters — greedy bin-packing that migrates an interrupt every pass would be
worse than not balancing at all. Expect to need a damping/hysteresis rule, and
measure before trusting it.

**How it was found.** Tracing the consumers of `idt::vector_counts()` while
scoping the `dispatch_vector` counting fix; `irqbalance` turned out to be the
consumer with real consequences rather than display ones.

**Not a regression.** True since the module was written.
