## `A-IDT-COUNTS-ONLY-EXCEPTIONS-AND-CALLS-THE-TOTAL-INTERRUPTS` (lane A, 2026-08-26)

**Status:** ✅ FIXED 2026-08-26 — all four steps landed. Steps 1–3 (count at
`dispatch_vector`'s choke point, widen `VECTOR_STATS_SIZE` to 256, repoint
`kcounters`/`kstat` and narrow `kshell`'s exception line to `take(32)`) landed
earlier; **step 4, the `fs::irqstat` projection, landed with this note.**

Two corrections to the plan below, both found while implementing step 4 and
worth keeping because both would have produced a *passing-looking* wrong result:

1. **The suggested `apic::tick_count()` cross-check must be `>=`, not `==`.**
   `TICK_COUNT` is deliberately bumped by the BSP only, so that `tick_count()`
   stays wall-clock rate instead of advancing N× on an N-CPU machine
   (`apic.rs:1101`). `dispatch_vector` counts the timer on *every* CPU. BSP ticks
   are therefore a strict subset of counted timer interrupts, and the equality
   this entry originally called for would fail on any SMP boot. The `>=` form
   still catches the failure that was actually worth catching — a timer entry
   path reaching `handle_timer_irq` without passing the choke point — so nothing
   is lost. Asserted in `fs::irqstat::self_test` test 5, which reads the ticks
   *first* so an interleaved tick can only widen the margin, never flake it.
2. **Per-CPU attribution was not merely deferred, it was removed from the
   output.** See the `irqstat` row in
   `A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH`'s burn-down log.

**In short:** the `counters` command reports `irq.timer_irqs: 0` on a machine
whose timer has fired millions of times, and reports `irq.total_interrupts` as a
number that counts no interrupts at all — only CPU *exceptions* (page faults,
breakpoints and the like). Neither says so. A reader takes the first as "the
timer is not firing" and the second as an interrupt rate.

**The mechanism.** `kernel/src/idt.rs` keeps `VECTOR_COUNTS: [AtomicU64; 48]`
and bumps it from `count_vector(v)`. Every call site is an *exception* handler:
vectors 0–19, and not even all of those (9 and 15 are unused, 20–31 reserved).
Nothing ever calls `count_vector` for a hardware vector. But the IDT installs
plenty of them:

| vector(s) | installed handler | counted? |
|---|---|---|
| 0–19 | CPU exception ISRs | yes |
| 32 | `isr_timer` — APIC timer | **no** |
| 33–56 | `isr_irq0`..`isr_irq23` — IOAPIC inputs, into `ioapic::handle_device_irq` | **no**, and 48–56 are past the end of the array |
| 251, 252 | TLB-shootdown IPI, reschedule IPI | **no**, out of range |
| 255 | APIC spurious | **no**, out of range |

`kernel/src/kcounters.rs:280–290` then publishes
`irq.total_interrupts = irq_counts.iter().sum()` and
`irq.timer_irqs = irq_counts[32]`. The first is a sum over a mostly-dead array;
the second reads slot 32, which nothing writes, so it is a **hard zero for the
life of the kernel** even though `apic::tick_count()` sitting one module away
holds the true figure.

**Why it is worse than a missing number.** A zero here is not blank — it is a
measurement claim. `timer_irqs: 0` is the exact signature an operator would look
for to diagnose a wedged timer, so the counter is most misleading precisely when
someone is using it for the thing it is named after. And `total_interrupts` is
wrong in a direction that flatters the machine: it will read as a quiet system
under any interrupt load whatsoever.

**Knock-on: `/proc/irqstat` has no source.** `fs::irqstat` is a
`/proc/interrupts` equivalent — per-line counts, per-CPU totals, spurious counts
— and all six of its mutators are unreachable (see
`A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH`). The projection fix that
worked for `pagecache` and `netdev` cannot be applied here yet, because the
thing it would project from does not count hardware interrupts either. This
entry is therefore a prerequisite for that one, not a duplicate of it.

**The proper fix**, in order:

1. Call `count_vector` from the hardware paths: `isr_timer`, `handle_device_irq`
   (which knows its IRQ number), the two IPI ISRs and `isr_spurious`. One
   relaxed `fetch_add` per interrupt, which is what the exception paths already
   pay and is noise beside the ISR itself.
2. Widen `VECTOR_STATS_SIZE` from 48 to 256 so vectors 48–56, 251, 252 and 255
   have somewhere to land. At 8 bytes a vector that is 2 KiB of `.bss`, against
   an array that currently cannot represent a third of the installed handlers.
3. Point `kcounters`' `timer_irqs` at the now-live slot 32 (cross-check it
   against `apic::tick_count()` in the self-test — they should agree, and a
   drift means a missed EOI path), and let `total_interrupts` become true rather
   than relabelling it.
4. Only then project `fs::irqstat`'s per-line counts from `idt::vector_counts()`.

**Per-CPU attribution is a separate question** and should not be smuggled into
step 1. `VECTOR_COUNTS` is a flat global array, so `irqstat::per_cpu()` still
has no source afterwards. Making it per-CPU would arguably be *faster* (no
cross-CPU contention on a shared cache line) but it changes an interrupt hot
path and deserves its own change and its own benchmark.

**How it was found.** Looking for a real counter to project into
`/proc/irqstat`, after the same search succeeded for `pagecache` and `netdev`.

**Not a regression.** True since the IOAPIC device vectors were installed.

### Addendum, 2026-08-26 — a choke point, and a landmine in the display side

Two findings from reading the consumers before implementing. Both change the
plan above materially, so they are recorded here rather than discovered again
halfway through the edit.

**1. Step 1 should be one call, not five.** The list above — `isr_timer`,
`handle_device_irq`, the two IPI ISRs, `isr_spurious` — is not where the count
belongs. `idt.rs`'s `dispatch_vector` is a single function that *every* hardware
vector already funnels through:

```rust
extern "C" fn dispatch_vector(frame: *mut InterruptStackFrame, vector: u64) {
    match vector {
        32 => crate::apic::handle_timer_irq(frame_ref, 0),
        251 => crate::tlb::handle_tlb_shootdown_irq(frame_ref, 0),
        252 => crate::apic::handle_reschedule_irq(frame_ref, 0),
        255 => crate::apic::handle_spurious_irq(frame_ref, 0),
        v @ 33..=56 => { ... crate::ioapic::handle_device_irq(irq); }
        _ => {}
    }
}
```

So `count_vector(vector as usize)` at the top of it covers 32, 33–56, 251, 252
and 255 in one line — and, more to the point, covers whatever vector is added
next *without anyone remembering to*. This is the same argument that decided the
block-device capability gate in `devfs`: one call at the point every caller
passes through beats a call each caller must not forget. Prefer it. The
five-site version in step 1 is strictly worse and should not be implemented.

Note the ordering constraint that comes with it: the count must be taken
*before* the `match`, not inside each arm, or the `_ => {}` arm — an interrupt
on an installed-but-unhandled vector, which is exactly the event worth
counting — stays invisible.

**2. Fixing the count breaks `kshell`'s exception health line.** At
`kernel/src/kshell.rs:123411`:

```rust
let exc_total: u64 = crate::idt::vector_counts().iter().sum();
```

printed as `"Exceptions: total="`, with a companion
`if non_pf_exceptions > 100 { "HIGH" }` indicator. That label is accurate
**only because nothing currently counts hardware vectors.** The moment step 1
lands, every timer tick — millions of them — folds into a figure labelled
"Exceptions", and the HIGH indicator pins on within the first second of uptime
and never clears.

This is the asymmetry that makes the fix one careful commit rather than a
one-liner: the same change flips `kstat.rs:159` and `kcounters.rs:279` from
wrong to right, and flips this line from right to wrong. `exc_total` must be
narrowed to `vector_counts().iter().take(32).sum()` **in the same commit**, not
in a follow-up — a commit that is correct on its own terms and leaves a
permanently-red health indicator behind it is worse than no commit.

**Also in the same blast radius**, all in `kshell.rs`:

| site | what is wrong | why it matters after the fix |
|---|---|---|
| `cmd_irqrate` (127706) | hardcodes `for i in 0..48` | 48–56, 251, 252, 255 never print |
| `cmd_irqrate` | `33..=47 => "Device IRQ"` | IOAPIC maps 33–**56**, so 48–56 print "Unknown" |
| `cmd_irqrate` | `rates.rates_x10[i]` / `EXCEPTION_NAMES[i]` with `[]` | widening `VECTOR_STATS_SIZE` makes the two arrays different lengths; index them with `.get()` |
| `cmd_exceptions` (123556) | names every `i >= 32` as just `"IRQ"` | the whole point of counting them is telling them apart |
| `kcounters.rs:288` | `irq_counts[32]` with `[]` | fine today, but `.get(32).copied().unwrap_or(0)` costs nothing and cannot panic |

`cmd_irqrate` already skips zero-rate vectors, so widening the loop will not
flood its output — it will show exactly the vectors that are actually live.

**Sizing note.** Widening `VECTOR_STATS_SIZE` 48 → 256 makes `InterruptRates`
(`rates_x10: [u64; VECTOR_STATS_SIZE]`) a ~2 KB by-value return. Acceptable:
`vector_rates()` is called only from a hand-run diagnostic command, never from
an interrupt path.
