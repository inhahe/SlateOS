### A-TWO-AUDITS-THAT-FOUND-NOTHING-2026-09-12 (lane A) — NEGATIVE RESULTS, with their limits

**In short:** two searches for known bug-shapes in the kernel, both prompted by lane B
finding the shape elsewhere. Neither found anything. Recorded so the next person does not
repeat them, and so the *limits* are on record rather than the reassurance.

#### 1. Safety checks that fail open — none found

Lane B found `userspace/mkfs` and `userspace/fsck` answering "is this device mounted?"
with `Err(_) => return false`, so a single non-UTF-8 line anywhere in `/proc/mounts` made
every device read as unmounted and `mkfs` would format a live filesystem. Their
generalisation: **for any check guarding a destructive or privileged action, "I do not
know" and "it is safe" must not be the same value.**

Searched `kernel/src` for `Err(_) => false`, `unwrap_or(false)`, `unwrap_or(true)` and
`None => true`. Every candidate that guards an action resolves in the *safe* direction.

**The one that had to be read twice**, because it matches the bug's shape exactly —
`fs/reclaim.rs:441`:

```rust
None => true, // Can't check → assume OK.
```

A self-described "can't check, assume OK" guarding *reclamation*, which deletes data.
But **polarity decides it, not shape**: the function is `check_below_target`, and every
caller reads `if check_below_target(low) { finalize; return }`. Returning `true` *stops*
reclaiming. So "cannot measure" halts the destructive action, which is correct. The
comment is ambiguous prose over right behaviour — the opposite of tonight's usual finding.

Also cleared: `btrfs::probe` (`false` = do not claim the device, and its doc says the
permissiveness is deliberate), `storageclean`'s `retain` (keeps unpathed items),
and the `None => true` "never run, so due now" arms in `sysmaint`, `tasksched` and
`backupsched`, which are scheduling semantics rather than gates.

**Limit:** this searched for four syntactic shapes. A fail-open written as an early
`return Ok(())`, or as a permission derived from an absent record, would not match. The
search is a filter, not a proof.

#### 2. `hpet::elapsed_ns()` on a hot path — none found in the one place checked

Removing that read from `record_version` took a 256-byte write from 30,867 ns to 12,326
under WHPX, because the MMIO access is a VM exit costing ~13.5 µs where a monotonic clock
read costs ~450 ns under TCG. **432 call sites remain across 193 files.** Most are
obviously cold — event logs, schedulers, hotplug.

Checked the one that is genuinely hot in a microkernel: `ipc/futex.rs`, five sites, all in
`futex_wait_bitset`. **Clean.** The value-mismatch fast path returns at line 426; the
timestamp is taken at line 453, after the waiter is enqueued and immediately before
blocking. So it is paid only when the thread actually parks, where it is amortised against
a context switch rather than charged to an uncontended call.

One narrow exception, not worth fixing: if a signal is found pending at lines 470-478 the
call returns without blocking, having taken the timestamp. That needs a signal to arrive
in the window between enqueue and registration.

**Limit, and it is the important half:** 1 of 193 files was examined. This entry does not
say the kernel has no HPET read on a hot path; it says futex does not. Anyone finding a
surprising cost under hardware virtualisation should suspect this before suspecting their
own code — the tell is a cost that is *larger* under WHPX than under TCG, which is a VM
exit and not work.
