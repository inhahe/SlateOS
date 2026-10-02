### D-FUTEX-ATOMICS-OPERATE-DIRECTLY-ON-USER-WORDS — 2026-08-13 — TECH DEBT (blocked enabling SMAP) — ✅ FIXED 2026-08-13 (`kernel/src/ipc/futex.rs`, `kernel/src/mm/user.rs`)

**What.** Fourteen sites in `futex.rs` do

```rust
let atomic = unsafe { &*(addr as *const AtomicU32) };
```

over a *user* virtual address and then load / CAS / `fetch_or` / `swap` through
it. Under CR4.SMAP every one of those is a supervisor access to a user page
outside a STAC window, so they all fault.

**Why this was missed.** It is not in
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE`'s file list, and the
grep that drove that entry —
`from_raw_parts|write_unaligned|core::ptr::write|copy_nonoverlapping|…` — does
not match any of them. `futex.rs` reaches user memory through a *reference
cast*, a shape none of the other files use. It surfaced only from a second
sweep looking for callers of `validate_user_write` outside the syscall files.
**The lesson: an "is it all converted?" grep proves nothing about code that
reaches user memory by a shape the grep does not know about. Enumerate the
files that *take user addresses* and read them, rather than enumerating the
syntax you expect to find.**

**Why the bounce is the wrong fix here — uniquely.** Everywhere else in that
entry the answer is copy-in / operate / copy-out. A futex word cannot be
handled that way: the whole primitive *is* the atomicity of the RMW against
concurrent userspace CAS. Copy-in, modify, copy-out is not atomic and would
reintroduce exactly the lost-update race the futex exists to prevent.

This is therefore the one legitimate use of `stac()`/`clac()` in the kernel:
the window brackets a **single non-blocking atomic instruction**, so none of
the objections that rule it out for the handler sites apply — nothing blocks
inside it, so AC cannot leak into a saved RFLAGS across a reschedule, and the
window is a couple of cycles rather than unbounded. It is what Linux does
(`futex_atomic_cmpxchg_inatomic` and friends, `arch/x86/include/asm/futex.h`,
each wrapped in `__uaccess_begin()`/`__uaccess_end()`).

**Proper fix.** Add per-operation accessors to `mm::user` — `user_atomic_load_u32`,
`user_atomic_cas_u32`, `user_atomic_rmw_u32` — each of which validates the
address, opens the window, performs *one* atomic operation, and closes it.
Convert all fourteen sites to those. Critically, several sites currently hold
the `&AtomicU32` reference across code that blocks (`futex_lock_pi` at ~1492,
the requeue-PI paths at ~2272/~2453); those must become repeated per-operation
calls, with any CAS retry loop running *outside* the window, one instruction
per iteration. That is a correctness improvement independent of SMAP for the
same reason as the rest of the entry above: the reference is a raw user pointer
held across a sleep, so a peer thread's `munmap` turns it into a
use-after-free.

**Fixed as designed.** `mm::user` gained `user_atomic_load_u32`,
`user_atomic_store_u32`, `user_atomic_cas_u32` and
`user_atomic_rmw_u32(op, operand)` (`UserAtomicOp::{Set,Add,Or,AndN,Xor}`), each
validating the address and bracketing exactly one atomic instruction. All
fourteen sites converted; `futex.rs` now contains no raw-pointer cast at all
(`grep 'as \*const\|as \*mut\|from_raw_parts'` → no matches).

Three sites needed more than a mechanical swap, because making a previously
infallible store fallible introduces error paths that must not corrupt the
kernel-side bookkeeping:

- **`futex_unlock_pi`** — the ownership-transfer store now returns a
  `KernelResult`, but the handoff (`register_pi_owner` + `sched::wake`)
  completes *regardless*. The selected waiter has already been removed from the
  wait queue under the table lock, so bailing out on the store would park it
  forever on a lock nobody owns. `lock_pi_inner` consults the kernel ownership
  record, not the user word, so the handoff stays coherent; the error is
  reported to the unlocker, whose mapping is the one that vanished.
- **`futex_cmp_requeue_pi`** — the PI word is now read *before* each waiter is
  dequeued, so a faulting read cannot strand a waiter that has already left the
  condvar queue, and the first fault is reported only after every waiter has
  landed somewhere.
- **`try_acquire_ownerless`** — split into `ownerless_claim_value` (pure
  decision), `acquire_ownerless_with` (the generic CAS retry loop) and the
  user-address wrapper. Necessary because `test_owner_died_relock` drives it
  with a *kernel* `AtomicU32`, which the new accessors correctly reject; the
  self-test now drives the real retry loop through closures instead of testing
  a parallel reimplementation.

Boot test green, all futex self-tests pass (including requeue-PI, PI
owner-death handoff and OWNER_DIED relock).
