## §249 — Reading another process's memory resolves its faults for it, because nothing is going to fault on its behalf

**Date:** 2026-08-20
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** A debugger can ask the kernel to read or write another running
program's memory (that is how `process_vm_readv`/`writev`, and hence a debugger's
"show me that variable", work). The kernel did this by reading the other
program's memory map by hand — and gave up with a "bad address" error on two
kinds of page that the program itself would have had no trouble using: a page it
has been given but has not touched yet, and a page it is sharing with a copy of
itself after `fork` (the operating system hands out one shared copy and only
splits it when somebody writes). The second case is the killer, because
immediately after a `fork` *every* page is in that state — so poking a
just-started child process failed on every single byte. The kernel now finishes
the job the same way the processor's own fault handler would have.

**Decision.** `copy_to_user_as` / `copy_from_user_as` (kernel/src/mm/user.rs)
no longer treat a failed page-table walk as final. On failure they look up which
process owns the address space, synthesize the page-fault error code the hardware
*would* have produced for the access they are about to perform by hand, hand it
to `pcb::try_resolve_fault`, and walk once more. If the second walk fails, the
address really is unusable and the caller gets `EFAULT` exactly as before.

**Why the same-address-space path already did this and this one did not.** When
the kernel touches the *current* process's user memory, `validate_user_range`
pre-faults and pre-breaks CoW, because a real `#PF` is available as a fallback if
it misses something. The cross-address-space path has no such fallback: it reads
another process's page tables through the HHDM, so no access it performs can ever
fault against the address space it is reading. Whatever the walk does not resolve,
nothing else will. The asymmetry was not a deliberate policy — it was the
consequence of writing the cross-AS path as "the same walk, but with an explicit
pml4", and never revisiting what the walk was allowed to leave unresolved.

**Why no signature had to change.** `pcb::try_resolve_fault` already takes the
pid explicitly and operates entirely on *that* process's `pml4_phys` and VMA
list — it never reads `CR3`. `cow::resolve_cow_fault` likewise takes an explicit
pml4, and its `tlb::flush_range` broadcasts an IPI to every CPU rather than
flushing only the local one, which is conservative for the local case and exactly
right for this one. The resolver was already correct across address spaces; it
was simply never called. The only genuinely missing piece was the reverse
lookup, added as `pcb::pid_for_pml4`.

**Why `pid_for_pml4` uses `try_lock` and answers `None` rather than blocking.**
It is reached only from a path where a translation has *already* failed, so its
entire job is to upgrade an error into a success. Declining to answer costs an
`EFAULT` that would have been returned anyway, whereas blocking could deadlock a
caller already holding the process table. The scan is linear in live processes,
which is affordable precisely because the common path — the walk succeeding —
never calls it.

**Why the synthesized error code's *present* bit is the whole design.**
`try_resolve_fault` dispatches on it: `present && write` selects the CoW break,
`!present` selects demand paging, and everything else is unresolvable. So the bit
must describe the page actually found, not the access desired. The pre-existing
same-AS helper `try_fault_in_user_page` hard-codes it to zero, which is correct
*there* only because it is called solely after `translate` returned `None`.
Copying that convention across would have silently turned every CoW page into an
unresolvable demand-page request — the exact bug, reintroduced in a new place.

**Why one retry and not a loop.** Each resolvable state is cleared outright by
its resolver: a populated page is present, a broken CoW page is writable and no
longer marked `COW`. A second failure therefore means the address is genuinely
unusable — an unmapped hole, or a read-only VMA — and a loop would spin instead
of converging.

**Why this does not become a write override.** Resolution does only what a fault
would have done, so a write to a read-only mapping still fails, in both of its
forms: while the page is absent, `try_resolve_fault`'s VMA permission check
refuses; once it is present, `resolve_cow_fault` refuses a page without the `COW`
bit. Both are asserted by the self-test rather than assumed.

**How it was verified.** A new `mm::user::self_test_cross_as_resolution` builds a
throwaway process with four one-frame VMAs and exercises: a write into an
untouched committed page, a read of one, a read-only mapping in both its absent
and present forms, and — via a real `clone_address_space_cow` — an actual
copy-on-write break. The CoW case asserts its own preconditions (the page is
`PRESENT && !WRITABLE && COW`, refcount ≥ 2) before writing, then asserts the
target moved to a different writable frame carrying *both* the poke and the eight
stamped bytes it did not write (a fresh zero page would pass the first check and
fail the second), and that the forked address space still maps the original frame
with the original contents.

Three assertions are explicit **negative controls**: they call
`user_page_phys_once` — which *is* the pre-fix code path, still present as the
raw walk — and require it to fail at the same address the resolving walk then
succeeds at. Without them the test would be green for reasons it could not
distinguish from resolution doing nothing at all, which is the failure mode the
2026-08-19 "zero KASAN reports" and 2026-08-20 "clean ZFS bench" corrections were
both made of. Boot test PASSED with all of it.

**Why the existing `process_vm` self-test did not catch the bug.** It calls
`pcb::try_resolve_fault` by hand to pre-fault its target page before copying, and
only ever uses a plain writable anonymous page. It tests the transfer once the
page is already there; it works *around* the missing resolution rather than
exercising it. A test that sets up the easy case is not evidence about the hard
one.

**Where it lives.** `kernel/src/mm/user.rs` (`user_page_phys`,
`user_page_phys_once`, `try_resolve_remote`, `self_test_cross_as_resolution`);
`kernel/src/proc/pcb.rs` (`pid_for_pml4`).

**How to reverse.** Delete `try_resolve_remote` and inline
`user_page_phys_once` back into `user_page_phys`. Nothing else depends on the
retry.

**What would change this.** If cross-AS copies ever move to a
fault-catching design (an exception table, the way Linux's `copy_to_user` works)
the hand walk disappears and so does the need to resolve on its behalf. And if a
third resolvable fault class is ever added that its resolver does *not* clear
outright, the single retry has to become a bounded loop.
