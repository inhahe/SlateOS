//! Address-space page-table locks: what keeps two CPUs from installing over
//! each other in the same address space's page tables.
//!
//! A page fault is resolved in two halves: the slow half (a frame allocated,
//! the old page copied for a copy-on-write break, the data read back from
//! swap) and the install (the page-table entries changed, the refcount or the
//! swap slot settled). Two threads of one process touching the same page on
//! two CPUs each run both halves. Until 2026-10-03 nothing stood between the
//! installs, and:
//!
//! - **a copy-on-write break ran twice:** each copy installed its own frame
//!   over the other's, so a write made between the two copies was lost; and
//!   each dropped the address space's reference to the shared frame, one
//!   reference too many -- with two children sharing it, one child then
//!   wrote the page in place under the other, and freed it when it exited;
//! - **a swap-in ran twice:** the second cleared the first's mapping and
//!   mapped its own copy;
//! - **fork marked pages copy-on-write** while another thread broke the same
//!   pages' sharing.
//!
//! So the install half runs under this lock, and checks first that the
//! entries are still what the slow half found: the second CPU finds the work
//! done, drops what it prepared, and the access runs again. The slow half
//! stays outside, as Linux keeps it outside the page-table lock: it can read
//! a file or a disk, which must not happen with a spinning lock held and
//! preemption off.
//!
//! **One lock per address space, by hashing.** A field on the process would
//! need the process table to reach, and swap's reclaim list, compaction and
//! the copy-on-write code know an address space only by its PML4. A fixed
//! array of locks, the PML4 hashed to one, needs neither a map nor an
//! allocation; two address spaces that share a lock cost contention, not
//! correctness. Nothing takes two of them at once, so two address spaces
//! landing on one cannot deadlock either.
//!
//! **Order:** taken before the frame allocator, `SWAP`, `RECLAIM` and the
//! rmap; never with the process table (`PROCESS_TABLE`) held -- the fault
//! resolver lets the table go before it installs. A holder may wait for a
//! TLB shootdown, so a CPU must not wait for it with interrupts off: the
//! callers that may run so -- reclaim on an allocation failure -- use
//! [`try_lock`].

use crate::sync::{Mutex, MutexGuard};

/// How many locks the address spaces are hashed over.
const STRIPES: usize = 64;

/// The locks. Tracked mutexes, so lockdep sees their order and a stalled one
/// is named.
static LOCKS: [Mutex<()>; STRIPES] = [const { Mutex::named((), b"as_lock") }; STRIPES];

/// The lock the address space rooted at `pml4` hashes to. A PML4 is a 4 KiB
/// page, so its low twelve bits say nothing.
fn stripe(pml4: u64) -> &'static Mutex<()> {
    let index = usize::try_from((pml4 >> 12) % STRIPES as u64).unwrap_or(0);
    let [first, ..] = &LOCKS;
    // `index < STRIPES` by the modulo; the fallback is unreachable.
    LOCKS.get(index).unwrap_or(first)
}

/// Hold the address space at `pml4` against other installs, waiting for it.
/// For callers in thread context, with interrupts on.
#[must_use]
#[track_caller]
pub fn lock(pml4: u64) -> MutexGuard<'static, ()> {
    stripe(pml4).lock()
}

/// [`lock`] without waiting: `None` when another CPU holds it. For callers
/// that may run with interrupts off, which must not wait for a holder that
/// may be waiting for a TLB shootdown on them.
#[must_use]
#[track_caller]
pub fn try_lock(pml4: u64) -> Option<MutexGuard<'static, ()>> {
    stripe(pml4).try_lock()
}

/// Run the address-space lock's self-test: a held lock refuses a second
/// taker, frees on release, and every PML4 maps to a lock.
///
/// # Errors
///
/// `InternalError` when the lock does not behave so.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;

    let pml4 = 0x0012_3000_u64;
    let held = lock(pml4);
    let refused = try_lock(pml4).is_none();
    let same_stripe = try_lock(pml4 + (STRIPES as u64) * 0x1000).is_none();
    drop(held);
    let freed = try_lock(pml4).is_some();
    // Neighbouring PML4s land on different locks.
    let spread = !core::ptr::eq(stripe(pml4), stripe(pml4 + 0x1000));
    if !(refused && same_stripe && freed && spread) {
        serial_println!(
            "[as_lock]   FAIL: refused while held {} same stripe refused {} free after {} \
             neighbours spread {}",
            refused,
            same_stripe,
            freed,
            spread
        );
        return Err(crate::error::KernelError::InternalError);
    }
    serial_println!("[as_lock]   address-space page-table locks: OK");
    Ok(())
}
