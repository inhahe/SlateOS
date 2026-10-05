## 32. Real `PROT_NONE` — represent "no access" as the absence of the `USER_ACCESSIBLE` page-table flag (overload the existing flag), not a new VMA field

**Date:** 2026-06-17

**Decided by:** Claude (autonomous) — clearly-correct, low-controversy mirror of
the x86-64 hardware mechanism; reversible. NOTE: this **diverges** from the lean
in the task scouting note (`todo.txt` NEXT STEPS #3), which suggested adding a
*dedicated* VMA field/flag for the access mask "rather than overloading
PageFlags." On full inspection the overload is the cleaner design (reasoning
below), so I took it.

**Context:**
Before this change, `mmap(PROT_NONE)` and `mprotect(..., PROT_NONE)` were
approximated as "read-only + no-execute": the VMA still carried
`PRESENT | USER_ACCESSIBLE | NO_EXECUTE`, so a *read* of the region demand-paged
a zero frame instead of faulting. That is wrong for the two things `PROT_NONE` is
actually used for — guard pages and reserved trap regions (notably glibc's
thread-stack guard, and `mmap(PROT_NONE)`-then-`mprotect(RW)` reservation
patterns). With full per-process VMA tracking now in place, the fault resolver
can distinguish "mapped `PROT_NONE`" from "never-mapped hole," so real
`PROT_NONE` is implementable.

`PROT_NONE` has to be enforced at **two layers**, because a region can be either
already-faulted-in (present PTEs) or still lazy (no PTE yet):
1. **Present pages** — hardware will only fault a ring-3 access if the PTE lacks
   the U/S (`USER_ACCESSIBLE`) bit. So `mprotect(PROT_NONE)` on present pages
   *must* clear `USER_ACCESSIBLE` on the PTE regardless of how the VMA records
   the protection — there is no way around touching the page-table bit.
2. **Lazy pages** — the fault resolver consults the covering VMA to decide
   whether to populate the page or fault. The VMA must record "no access."

**Options for the VMA-layer marker (layer 2):**
- **A (chosen) — Overload `USER_ACCESSIBLE`:** a `PROT_NONE` VMA carries flags
  *without* `USER_ACCESSIBLE` (e.g. `PRESENT | NO_EXECUTE`). The resolver treats
  "user fault on a VMA whose flags lack `USER_ACCESSIBLE`" as unresolvable →
  `KernelError::PageFault` → SEH-style access violation.
- **B — Dedicated `prot_none: bool` (or a full R/W/X access mask) on `Vma`:** an
  explicit second field, separate from `PageFlags`.

**Reasoning (why A):**
- **Single source of truth.** Layer 1 *forces* us to use the U bit on present
  PTEs anyway. Option A makes the VMA use the *same* bit, so the lazy-page marker
  and the present-page enforcement are one representation. Option B introduces a
  second marker that must be kept in sync with the PTE U bit — a classic
  divergence bug waiting to happen.
- **It is the literal hardware semantic, not a hack.** "Userspace cannot access
  this page" *is* the U/S bit. `PROT_NONE` ⇔ no user access ⇔ `!USER_ACCESSIBLE`.
- **Zero construction-site churn.** `Vma` has 26 struct-literal construction
  sites; option B would touch all of them. Option A touches only the handful of
  places that actually *care* about access (the mmap flag build, the resolver
  gate, `mprotect`, and `/proc/<pid>/maps` perm rendering).
- **CoW stays correct for free.** `cow::resolve_cow_fault` derives the copied
  PTE's flags from the existing PTE (`sibling.flags | WRITABLE`, minus `COW`);
  since it never *adds* `USER_ACCESSIBLE`, a CoW of a `PROT_NONE` page stays
  inaccessible — a write to a forked-then-`PROT_NONE`'d page cannot escape.
- **`mprotect` round-trip restores access.** Present `PROT_NONE` pages keep
  `PRESENT` (only U is cleared), so the physical frame and its contents survive;
  `mprotect` back to `PROT_READ|WRITE` re-adds `USER_ACCESSIBLE` and the data is
  intact — no frame leak, no re-zero.

**The one subtlety this forces:** `pcb::protect_vma_range` previously took only
`(want_write, want_exec)`, but `PROT_NONE` and `PROT_READ` are *both*
`want_write=false, want_exec=false` — indistinguishable. So `protect_vma_range`
and the `mprotect` PTE pass gained a `want_access` parameter (true unless
`prot == PROT_NONE`); when false they clear `USER_ACCESSIBLE`, when true they
set it. This is unavoidable under *either* option (the VMA marker has to be told
which of the two zero-prot cases it is).

**Scope kept to user space.** Only `pcb::try_resolve_fault` (the per-process
resolver) gained the `!USER_ACCESSIBLE ⇒ fault` gate. The kernel global-address-
space resolver (`mm::vma::AddressSpace::resolve_fault`) is deliberately **not**
gated: kernel pages legitimately lack `USER_ACCESSIBLE`, and there is no
`PROT_NONE` concept there.

**How to reverse:** if a richer access model is ever needed (e.g. separate
read-vs-execute-only distinctions the U bit can't express, or pkeys), add the
explicit access mask to `Vma` then; the resolver gate and the `mprotect`
`want_access` plumbing are the only call sites that would change.
