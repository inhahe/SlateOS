## TD-KASAN-IRQ-CONTEXT-ALLOCATIONS-LOSE-SHADOW-COVERAGE

**Status:** open, deliberate. Logged 2026-08-12.

**What it is.** `mm::kasan::with_map_lock` acquires `MAP_LOCK` only with
`try_lock`, and when the caller arrived with interrupts *already disabled* — an
IRQ-context allocation — a failed attempt **gives up and returns `None`**
rather than waiting. The caller then leaves that allocation's shadow bytes
unwritten, so the allocation's redzones and freed-state poison are never
recorded and KASAN cannot detect an overflow or use-after-free on it. The
shadow fails open: an unpoisoned byte reads as "addressable", so this is a
*detection* gap, never a false positive and never a correctness problem.

**Why it is written that way.** The mapping path performs a cross-CPU TLB
shootdown while holding `MAP_LOCK`, and the shootdown blocks until every other
CPU acknowledges an IPI. A CPU spinning on `MAP_LOCK.lock()` with interrupts
disabled can never take that IPI, so the holder would wait forever for an
acknowledgement — a two-CPU deadlock. A caller that arrived with interrupts
enabled can re-enable them between attempts (and does), which lets the IPI
through and guarantees forward progress. A caller that arrived with them
disabled cannot re-enable them; that is not ours to do.

**Where it lives.** `kernel/src/mm/kasan.rs` — `with_map_lock` (the
`if !irqs_were_on { return None; }` arm) and its one caller at the lazy
shadow-map site.

**Why it matters now.** With compiler-instrumented KASAN (§107, §118) the
shadow is the primary evidence for B-KNULLJUMP. If the wild store lands in an
allocation that was made from IRQ context on a contended lock, the shadow is
silent about it and the hunt sees nothing. The window is narrow — it needs
`MAP_LOCK` to be *held* at the moment of an IRQ-context allocation that touches
an unmapped shadow frame — but it is not zero, and it is invisible when it
happens.

**Proper fix — CORRECTED 2026-08-12. Do not implement the version first written
here; its premise was wrong and it would have been a regression.**

The original note proposed: map and zero the frame under the lock, then queue
the TLB shootdown to run *after* the lock is dropped, on the grounds that "the
shadow frame is freshly mapped, so a stale *absent* TLB entry on another CPU is
the only hazard, and it resolves into a fault that re-checks the bitmap." That
is not what the code does. The unbacked state is **not absent** — it is a
*present, read-only* mapping of the shared zero page: `install_shadow_frame`
pre-fills a fresh leaf PT with `zero_page | PTE_PRESENT | PTE_NX` (no
`PTE_WRITABLE`), which is precisely what makes §118's "every check passes until
something is poisoned" work. So during a deferred-shootdown window another CPU
holds a stale **read-only present** entry, and the two cases differ sharply:

- A stale *read* is harmless. It reads `0x00` out of the zero page, which means
  "addressable", so it fails open — the same class of transient detection gap
  this entry is already about.
- A stale *write* — i.e. any poison store — hits a read-only kernel page and
  takes a `#PF` with the write bit set. `idt.rs::handle_page_fault` has **no
  spurious-fault fast path** (grepped: the only "spurious" handling in the file
  is APIC vector 255), and the shadow window is not a demand-paged VMA, so the
  fault lands on the fatal kernel-`#PF` path. That turns a silent, fail-open
  detection gap into a kernel panic — strictly worse than the bug.

Two ways to make deferring the shootdown actually sound:

- **(a) Add a spurious-kernel-fault fast path.** On a kernel `#PF` whose address
  is inside the shadow window, re-walk the page tables; if the PTE now permits
  the access, `invlpg` and return. This is exactly Linux's
  `spurious_kernel_fault`, it makes the deferred shootdown safe by construction,
  and it is self-contained. Cost: one more special case early in the fatal-fault
  path, which is the worst place in the kernel to get something subtly wrong.
- **(b) Make the unbacked state not-present rather than present-read-only.** x86
  does not create a TLB entry for a translation that faults, so a
  not-present → present transition needs no shootdown at all (the ordinary
  demand-paging result), and `MAP_LOCK` would never hold an IPI wait.
  **But this is not sufficient on its own**, and that is the subtlety that makes
  it more than a flag flip: `install_shadow_frame` also splices *private* PD/PT
  pages in place of the shared ones, and those upper-level entries are present
  both before and after. The paging-structure caches may still hold the old
  shared-table pointers, so another CPU can walk to the shared zero PT and land
  on the read-only zero page anyway. Any not-present design has to handle the
  upper-level splice separately (e.g. allocate the private tables eagerly at
  `early_init`, so only leaf entries ever change). It also inverts §118's design
  premise, though possibly harmlessly now: with outlined checks `get_shadow`
  consults the `SHADOW_MAPPED` bitmap *first* and never dereferences an unmapped
  shadow, so the zero page may now be load-bearing only for the pre-shadow
  window. That needs verifying rather than assuming — it is the same "a generic
  `core` fn is instrumented after all" class of assumption that cost two boots
  in §118.

**Do first, regardless of which:** count the give-ups. A counter next to
`SHADOW_FRAMES_MAPPED`, surfaced wherever the other KASAN stats are, turns an
invisible gap into a measured one — it says whether this is a once-a-boot event
or a never event, which decides whether (a)/(b) is worth its risk at all, and it
is the only way to tell afterwards that the fix worked.
