### A-SERIAL-REENTRANCY-FLAG-IS-CLEARED-ONE-STATEMENT-TOO-EARLY. `with_serial` drops its "this CPU is printing" flag *before* releasing the console lock, opening a window in which a nested exception would deadlock — for a cross-CPU reason that cannot exist — 2026-08-25 — ✅ **FIXED** 2026-08-25

**In short:** The kernel's serial-print path has a safety net: if a CPU is
already halfway through printing and something interrupts it and tries to print
again, the second print must *not* wait for the first one's lock — it would wait
forever, because the first one cannot finish until the second returns. The net
is a per-CPU "I am printing" flag. The flag is currently cleared one statement
*before* the lock is released, so for those few instructions the net is down
while the trap it protects against is still armed. The comment justifying that
order appeals to another CPU reading the flag, and no other CPU ever does — the
flag is per-CPU by construction and the code says so ten lines below.

**Where.** `kernel/src/serial.rs`, `fn with_serial`:

```rust
busy.store(true, Ordering::Relaxed);
{
    let mut guard = SERIAL.lock();
    emit(&mut guard);
    // Clear before the guard drops, so the flag is never observably
    // stale while another CPU could already hold the lock.
    busy.store(false, Ordering::Relaxed);
    drop(guard);
}
```

**Why the stated reason does not hold.** `IN_PRINT` is
`[AtomicBool; MAX_CPUS]`, indexed only by `crate::smp::current_cpu_index()`, and
its own doc comment says: *"Only ever read and written by the CPU that owns the
slot (under `cli`), so `Relaxed` ordering suffices — there is no cross-CPU
handoff to order against."* A flag no other CPU reads cannot be "observably
stale" to another CPU. The ordering therefore buys nothing and costs a window.

**What the window is.** Between `busy.store(false)` and `drop(guard)` this CPU
holds `SERIAL` with the escape flag down. A nested `_print` from an exception
taken in that window would see `busy == false`, take the normal path, and spin
on a lock only the frame below it can release — the exact wedge
`B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT` documented and
the exact wedge the flag was added to prevent.

**How likely.** Low but not zero. The window spans `emit`'s return and a
`MutexGuard` destructor (one atomic store); neither dereferences user memory,
so a `#PF` is implausible. An NMI or a `#MC` is not scheduled by anything we
control, and the injected-NMI path from `--hard-lockup-watchdog` lands wherever
it lands. The point is that the window is free to close.

**The fix.** Swap the two statements — `drop(guard); busy.store(false);` — and
replace the comment with why *that* order is right: the flag must outlive the
lock, because the condition it encodes is "this CPU is inside the critical
section", and the CPU is still inside it until the guard is gone.

**Fixed.** The two statements are swapped and the comment now states the rule
(*the flag must outlive the guard, never the other way round*) together with
why the old justification was unreachable. Boot-tested green: `BOOT_OK` after
370 s, zero `!! ` lines, `kshell::self_test PASSED`. The existing
`reentrancy_self_test` covers this path and still passes.

**How it was found.** A survey (not a report) for the class of bug behind
`B-FORKEXEC-BOOT-HANG`: a lock that is acquired inside
`crate::cpu::without_interrupts` at some sites and bare at others in the same
file, which is a file contradicting its own IRQ-safety declaration. Across all
802 kernel sources exactly three (file, lock) pairs qualify, and the survey's
value was in the reading rather than the count:

| Pair | Verdict |
|---|---|
| `mm/frame.rs` : `ALLOCATOR` — 14 guarded, 2 bare | **Contract, unchecked.** The two bare sites are `pcpu_refill`/`pcpu_drain`, whose doc comments say "Called with interrupts disabled". True today; nothing enforces it. See the entry below. |
| `sched/mod.rs` : `SCHED` — 3 guarded, 75 bare | Not a contradiction. `SCHED` is deliberately bare at task level and reached from the tick only via `try_lock`; the three guarded sites guard a *sequence*, not the lock. |
| `serial.rs` : `SERIAL` — 1 guarded, 1 bare | The bare one is `init()`, once, in early boot before interrupts are on. Benign — but reading the guarded one turned up the flag-ordering defect above. |

**What the survey does not cover, and why no checker was wired.** The rule it
tests is *intra-file*: it can only see a lock whose own module already declared
it IRQ-unsafe. The class that actually bites is a lock acquired bare in module X
that an interrupt handler reaches through module Y, and finding that needs a
call-graph closure with real path resolution (`crate::sched::schedule()` is a
call this survey's regex does not even see). A first attempt at that closure
resolved 31 functions out of 12814 — an under-approximation large enough that a
clean result would have meant nothing. Per §299, a gate whose trigger cannot
support its claim is worse than no gate: it would report "IRQ-safety OK" on a
tree it barely looked at. Wiring one is deferred until the resolver exists.
