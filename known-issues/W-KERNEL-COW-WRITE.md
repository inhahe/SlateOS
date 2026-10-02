### W-KERNEL-COW-WRITE. Kernel-mode write fault on a user COW page is not routed to the resolver — WATCH (not currently reproducible)

**Where:** `kernel/src/idt.rs` page-fault handler (~line 1787). After
`mm::fault::resolve()` (kernel-VMA demand paging) declines a user
address, the user-fault resolver chain (swap-in →
`proc::pcb::try_resolve_fault`/CoW → stack growth) is entered **only**
when `error & 4` (CPL3, ring-3 access). A *kernel-mode* (ring-0) write
to a **present, read-only** user page (`error == 0x3`) therefore skips
CoW resolution and falls straight through to "FATAL: Unrecoverable
kernel page fault. Halting."

**[A] 2026-09-12 — the syscall layer has no raw write through a user mapping, which
completes the static half of this entry's own prescription.** The entry asks to *"identify
the specific kernel write path that reaches a user COW page without pre-validating"*. With
the four `mm/user.rs` primitives already audited clean (above), the only remaining way in
is a **raw** pointer write that bypasses them. Enumerated:

- `write_volatile` / `copy_nonoverlapping` / raw `*mut` stores across `kernel/src`: 174
  sites in 53 files — overwhelmingly device MMIO (`ahci`, `apic`, `e1000`, `fb`, `drm`),
  which write physical or HHDM addresses and cannot fault on a user page.
- **Inside `kernel/src/syscall/`, where a user buffer is actually in scope: 10 sites.**
  Every one writes either a *kernel local* (`ex2`, `frame_buf`, `buf` — each with a SAFETY
  comment saying the regions are distinct locals) or an **HHDM address** obtained by
  translating the target `pml4` (`linux.rs:51024`, which stamps a pattern through
  `phys + hhdm` rather than through the user mapping, exactly as `copy_to_user_as` does).

So no syscall path writes through a user virtual address in ring 0. Every user write in
that layer goes through the four primitives, and those break CoW.

**Stopping here deliberately, and the reason is proportionality rather than completeness.**
What remains unaudited is raw writes *outside* `syscall/` that might target a user address —
in `fs/`, `ipc/`, `proc/`. That is the long tail of the 174, it is mostly MMIO, and
distinguishing "this address is user" from "this address is a device" needs reading each
site rather than grepping. Against a defect that has produced **one** observation in June,
has never appeared in 722 recorded boots, and whose signature is distinctive enough to
recognise instantly if it recurs, that audit is disproportionate today.

What would change that: a recurrence, or a cheap way to key the search on *user-address
provenance* rather than on the write itself. The second is the interesting one and I do not
have it — noting the gap rather than pretending the enumeration is finished.

**[A] 2026-09-12 — checked the recorded boots for this signature, and the answer is WEAK
EVIDENCE rather than reassurance. The distinction is the point of this note.**

`bench/boot-history.jsonl` has captured every kernel exception across 722 boots. All 22 of
them, in full: 15 deliberate breakpoints, 3 page faults (`error=0x2`, `0x0`, `0x2`), and
one invalid opcode. **None with `error=0x3`** — the present/write/kernel combination this
entry describes.

**Why that is worth much less here than the identical measurement was for
`B-PTHREAD-TEARDOWN-PF`.** That entry got a strong negative from the same data because its
trigger is *known to run*: `spawn-test-glibc-pthread` executes every boot, verified in the
serial log and absent from the skip ledger, so 700 boots at a 1-in-5 rate should have
produced ~100 recurrences and produced none.

This entry has no such trigger. The fault needs a kernel path that writes through a user
pointer into a present, read-only COW page **without** first calling
`mm::user::validate_user_write`. Whether any code path does that during a normal boot is
exactly what is unknown — so zero occurrences is equally consistent with *the bug is gone*
and with *nothing has ever exercised it*. The measurement cannot distinguish them.

**So this is recorded as a null result, not a negative one.** The same query, the same
population, the same zero — and it closes one entry while saying almost nothing about
another, because the strength of an absence depends entirely on whether the thing that
would produce it was running. An absence with no known trigger is not evidence; it is the
shape of evidence.

What *would* be evidence: a self-test that deliberately performs a ring-0 write through a
user pointer into a `MAP_PRIVATE` file page that has not been touched since mapping, and
asserts the write succeeds rather than halting. That is a rung, not a stress run, and it
would convert this entry from WATCH to either fixed or reproducible in one boot.

**Why it matters now:** the read-only page cache (§36) maps writable
`MAP_PRIVATE` file pages **RO + COW** on first fault (so writes copy out
of the shared frame), whereas the old private path mapped them
**writable** directly. Any kernel path that writes through a user
pointer into such a page *without first calling*
`mm::user::validate_user_write` (which breaks CoW eagerly at a safe
point, mirroring Linux `get_user_pages(FOLL_WRITE)`) would now trip a
ring-0 write fault on a COW page and halt. With pre-validation, the
correct kernel paths never hit this, which is why two full boots
(BOOT_OK, shrinker exercised under critical pressure) are clean.

**Status:** a prior-session boot showed a one-off
`EXCEPTION: Page Fault (#PF) ... error=0x3` at a USER_MMAP address
(`0x6000213450`) consistent with this scenario, but it has **not**
reproduced against the current source (two deterministic green boots).
Most likely it was a transient intermediate-edit state, not the
committed code. Left as a WATCH rather than a fix because the obvious
"route ring-0 user-address faults to `try_resolve_fault`" hardening
risks lock re-entrancy/deadlock (the faulting kernel code may already
hold the VMA/process locks the resolver takes) — exactly what
pre-validation exists to avoid.

**Proper fix if it recurs:** identify the specific kernel write path
that reaches a user COW page without pre-validating and make it call
`validate_user_write` (the architecturally-correct point to break CoW),
rather than weakening the fault handler. Only if a path genuinely cannot
pre-validate should the handler route ring-0 user-address faults to the
resolver, and then only with a fault-fixup/exception-table mechanism so
an unresolvable access returns `-EFAULT` instead of halting.

**[A] 2026-09-12 — the four write-through-user-pointer primitives are all clean, so the
search narrows.** The entry's own prescription is to *"identify the specific kernel write
path that reaches a user COW page without pre-validating"*, which is a static question and
does not need the repro this has been blocked on since June. Audited `mm/user.rs`'s four
public write primitives:

| primitive | breaks CoW via |
|---|---|
| `copy_to_user` | `validate_user_write` directly |
| `write_user_value<T>` | delegates to `copy_to_user` |
| `write_user_items<T>` | delegates to `copy_to_user` |
| `copy_to_user_as` | `user_page_phys(.., need_writable=true)` → `try_resolve_remote` |

The last one is worth spelling out because it looks like a gap and is not. It never writes
through the user mapping at all: it walks the target `pml4`, converts the frame to an HHDM
kernel address and writes there, so no CoW *fault* can occur. The protection is that
`user_page_phys` with `need_writable` fails the walk on a read-only page and then calls
`try_resolve_remote` to break CoW in the remote address space before retrying. Different
mechanism, same guarantee — and had it merely *checked* writability and returned an error,
the bug would be worse than a halt: a silent write into a frame another process shares.

**The narrowed search is now also done, and it found no candidate.** Of the whole kernel,
60 files do raw mutable pointer writes; only **three** of those also handle user addresses
(`idt.rs` and `proc/pcb.rs`, which are the fault *resolvers* rather than faulters, and
`syscall/linux.rs`). `linux.rs` contains exactly three raw writes, and every one writes
through an HHDM address to a physical frame rather than through a user mapping:

| site | enclosing fn | why CoW cannot apply |
|---|---|---|
| 50727 | `self_test_madvise_dontneed` | self-test, page deliberately faulted in first |
| 51014 | `self_test_process_vm_cross_as` | self-test, same |
| 12504 | `linux_file_mmap_fill` | **production**, but the frame comes from `frame::alloc_frame_zeroed()` immediately above — freshly allocated, exclusively owned, not yet mapped |

So no production path writes into a user frame without either pre-validating CoW or owning
the frame outright. That **supports this entry's own hypothesis** that the June fault was a
transient intermediate-edit state rather than committed code.

**Treat that as support, not proof, and here is precisely why.** The search was bounded by
what greps can see: it looked for `from_raw_parts_mut`, `write_volatile` and `as *mut u8`,
and for files that also mention a user-address symbol. A write through a pointer passed
into a helper, built by arithmetic this pattern does not match, or reached via a trait
object would not appear. Today alone, five single-line or bare-identifier patterns of mine
missed multi-line or indirect forms, so the base rate for this kind of sweep missing
something is not low. The WATCH should stay armed; what has changed is that the diagnostic
in `idt.rs` is now the *only* thing expected to find this, rather than one of two.

**So if the unvalidated path exists, it is not one of these four.** It would be a raw
write through a user pointer that bypasses `mm::user` entirely. That is where to look next,
and it is a narrower search than "any kernel write path".

One false lead, recorded so nobody re-walks it: `mm/user.rs` defines
`const PAGE_SIZE: u64 = 4096`, which reads as a violation of the project's
"16 KiB pages, not 4 KiB" rule and of `CLAUDE.md`'s "the entire memory subsystem must be
built around this". It is neither. `page_table.rs`'s module doc says it outright — *"16 KiB
Frames on 4 KiB Hardware Pages… to map a 16 KiB frame, we set 4 consecutive page table
entries"* — so 16 KiB is the logical allocation unit and 4 KiB is the hardware granularity
a page-table walk must use. The constant's own doc comment says so too.

**Discovered:** 2026-06-30 (page-cache §36 sub-task 4 review).

**[A] 2026-08-14 — made self-identifying (diagnostic only, not a fix).**
The fix stays blocked on a repro, but *recognising* a recurrence did not
have to be. `kernel/src/idt.rs` (fatal-#PF path, just after the
`CS/RFLAGS/RSP/SS` line) now tests the exact signature — `error & 4 == 0`
(ring-0) && `error & 2 != 0` (write) && `error & 1 != 0` (present) &&
`cr2 < USER_SPACE_END` — and on a match prints
`*** MATCHES known-issues.md W-KERNEL-COW-WRITE ***` followed by the
faulting PTE's flags via `page_table::translate_flags()`.

The PTE dump is what makes this decisive rather than suggestive, because
`PageFlags::COW` is an explicit software bit (bit 9) with the documented
invariant "only meaningful when PRESENT is set and WRITABLE is cleared".
So the three outcomes are distinguishable without further inference:
- `USER_ACCESSIBLE && !WRITABLE && COW` → **CONFIRMED** unbroken CoW page;
  the printed RIP names the kernel write path that must call
  `mm::user::validate_user_write()` (the fix above).
- `USER_ACCESSIBLE && !WRITABLE && !COW` → not CoW at all; a kernel write
  to a genuinely read-only mapping — a different bug, and the diagnostic
  says so rather than mislabelling it.
- writable and/or not user-accessible → the error code matched but the PTE
  disagrees, i.e. a stale TLB entry or a concurrent unmap.

Deliberately *not* done: routing ring-0 user-address faults into
`try_resolve_fault`. That remains rejected for the lock-re-entrancy reason
in the paragraph above; this change cannot deadlock because it only reads
page tables on a path already committed to `halt_loop()` with interrupts
disabled. Same tactic as the bytes-at-RIP dump added for
`B-PTHREAD-TEARDOWN-PF`: when a bug is rare and unreproducible, the
actionable work is to guarantee the *next* occurrence is self-explaining.

**[A] 2026-08-20 — the audit this entry asks for was done, and found a real
instance in a place this entry does not cover.** The "proper fix if it recurs"
above says to hunt for kernel paths that write through a user pointer without
pre-validating. That hunt does not need a repro, so it was run. It found one —
but not a ring-0 fault path, because the path in question can never fault at
all: `mm::user::copy_to_user_as` / `copy_from_user_as` walk *another* process's
page tables through the HHDM, so no access they perform faults against the
address space they are reading. They were returning a hard `EFAULT` for both an
untouched-but-committed page and a present CoW page — and immediately after
`fork` an address space is entirely CoW, so `process_vm_writev` into a
just-forked target failed on every page. Fixed by resolving explicitly (see
design-decisions §249), with a self-test covering demand paging, CoW breaks, and
read-only mappings in both their absent and present forms.

Two consequences for *this* entry. First, its scope is narrower than its title
suggests: the entry is about the **same-address-space, ring-0 fault** case, and
the cross-address-space case was never in view because there is no fault there
to route. Second, it does **not** make this entry reproducible — the fixed path
is one that could not have raised the `error=0x3` signature described above, so
the WATCH stands unchanged and the diagnostic added on 2026-08-14 is still the
mechanism that will identify a genuine recurrence.

**Audit of the continuation — done 2026-08-22, one finding.** The open item
here was "every *other* kernel path that writes through a user pointer in the
current address space without calling `validate_user_write` first". That is now
swept, and the sweep is enforced by `scripts/check-user-access-sites.py`, gated
in `boot-test.sh` before the build.

*Why the enumeration is complete rather than a sampling.* SMAP is enabled and
its enforcement re-verified at every boot (`[smep_smap] SMAP enforcement:
VERIFIED`). A ring-0 access to a user *virtual* address therefore faults unless
it sits inside a `stac()`/`clac()` window, so the set of same-address-space
user-access paths is exactly the set of `stac()` call sites — a grep, not a
guess. All six live in `mm/user.rs`, and all six validate for **write**: the two
`copy_*` primitives, plus the four futex atomics, which require write permission
even on the load-only path so that one rule covers every operation. So the
question as originally posed has the answer "none", and the check now keeps it
that way.

*The finding was in the class the original question did not cover.* The other
way to reach a user page is through the **HHDM alias of its physical frame**,
and that is invisible to the whole mechanism above: the alias is a *kernel*
address in a legitimately writable mapping, so neither SMAP nor the user
mapping's write-protect bit can stop a write through it. A present-but-read-only
page — i.e. every page of a freshly forked address space — gets modified **in
place**, and the process sharing the frame silently diverges. That is strictly
worse than the fault this entry tracks, which at least announces itself.

`proc/linux_stack.rs::write_user_image` was such a path. It was a second,
hand-rolled implementation of `mm::user::copy_to_user_as`, resolving each page
with plain `page_table::translate` — which reports the frame behind a mapping
without consulting its flags — and it had silently lost three of the
primitive's checks: the `WRITABLE` test, the CoW break and demand-population
via the owning process's fault resolver, and the null/wrap/`USER_SPACE_END`
bound. It was **not reachable today**: its only caller runs after
`setup_user_stack`, which maps all stack frames eagerly, present + writable,
from freshly allocated memory into an address space the calling thread solely
owns. The defect was that every precondition held by construction *in the
caller*, while the function's own contract merely asserted them. Making the
stack demand-paged, or reaching that path with a forked address space, would
have converted a caller-side change into silent cross-process corruption here.
It now calls `copy_to_user_as` and inherits that function's existing CoW
coverage (`mm::user::self_test_cross_as_resolution`).

The generalisation is the one §284 recorded for the duplicated `uname`
literals: a hand-copied second implementation does not stay equivalent to the
first. `copy_to_user_as` gained its CoW break in §249; this copy did not,
because nothing connected them. Hence a checker rather than a resolution to be
careful — and it was validated by running it against the pre-fix tree and
confirming it fires, not merely by observing it pass on the fixed one.

Three other `translate`-then-HHDM sites were adjudicated as safe on their own
terms rather than whitelisted: `proc/pcb.rs`'s fault handler uses `translate`
as a *negative* presence guard (it skips subpages that are already present, so
it writes only to a proven-absent page) and derives the written address from
the frame it allocated itself, not from the translate result; `mm/swap.rs`
*reads* through the alias, and a CoW page reads correctly from either sharer's
view; and the sites in `syscall/linux.rs` are inside `self_test_*` functions
that build and tear down the address spaces they stamp.
