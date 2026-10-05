### [A] B-SMP-FAST-CPU-INDEX-PANICS-BEFORE-APIC-INIT. `smp::fast_cpu_index()` reads the APIC before it is mapped — `debug_assert` panic in debug, wild read in release — FIXED 2026-08-14

**Where:** `kernel/src/smp.rs` — the tier-3 fallback in `fast_cpu_index()`;
`kernel/src/apic.rs:~214` — `apic_read()`'s `debug_assert!(base != 0, "APIC not
initialized")`.

**What.** `fast_cpu_index()` has three tiers: RDPID, then `rdtscp`, then an APIC
MMIO read. On a CPU where neither RDPID nor `rdtscp` is advertised — which is
exactly the boot-test configuration, `qemu64,+smep,+smap,+umip` under TCG —
every call lands in tier 3 and does `crate::apic::read_id()`. Before
`apic::init` has run, `APIC_BASE_VIRT` is still 0, so:

- **debug builds:** `debug_assert!` fires → `KERNEL PANIC: APIC not initialized`.
- **release builds:** *worse* — the assert is compiled out and `apic_read`
  dereferences `(0 + offset) as *const u32`, a wild read of low memory. Silent
  garbage, or a fault, depending on what is mapped there.

**How it surfaced.** Wiring `frame_owner` ownership tagging into the frame
allocator (TD-FRAME-OWNER-1GIB) made `current_owner()` — and therefore
`fast_cpu_index()` — run on *every* frame allocation, including the allocator's
own boot-time self-test. That self-test runs long before `apic::init`, so the
kernel panicked at `[mm] Running frame allocator self-test...`:

```
!!! KERNEL PANIC !!!
panicked at kernel\src\apic.rs:214:5:
APIC not initialized
  Task: 0 (""), priority 0, cpu 0
```

**Why it was latent.** The pre-existing tier-3 callers were all gated behind
flags that only go true well after APIC init — the frame allocator's own
per-CPU cache checks `PCPU_ENABLED` first, for instance. Nothing called
`fast_cpu_index()` early, so the landmine was never stepped on. It was a real
bug regardless: the function's contract claims tier 3 "always works", and any
future early-boot caller would have hit it, in release builds silently.

**Fix.** Added `apic::is_ready()` (`APIC_BASE_VIRT != 0`) and made tier 3 check
it, returning CPU 0 when the APIC is not yet mapped. That is not a fudge: before
`apic::init` the system is strictly uniprocessor (BSP only), so 0 is the
*correct* index, not a fallback guess. Cost is one relaxed atomic load on the
already-slowest tier; tiers 1 and 2 are untouched, so real hardware pays
nothing.

**Lesson.** A "this can't happen yet" precondition that is enforced only by the
accident of who happens to call the function is not enforced at all. When the
cheap tiers of a tiered fast path are unavailable, the "always works" fallback
is the one that runs — so it is the one that has to actually always work.
