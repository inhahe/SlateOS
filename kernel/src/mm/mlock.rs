//! Locked memory: `mlock`, `mlock2`, `munlock`, `mlockall`, `munlockall`.
//!
//! Memory a program locks stays in RAM: the reclaimer never swaps it out, so
//! a key held there never reaches the compressed pool or a swap disk, and a
//! real-time thread never waits on a page fault for it. Until 2026-10-07
//! both ABIs answered success and locked nothing, while the kernel swapped
//! user memory (requests/d-a-mlock-locks-nothing.md).
//!
//! ## How a page is locked
//!
//! By [`PageFlags::MLOCKED`], a software bit, in three places:
//!
//! - **On a VMA's flags**, which are the flags every page faulted into it is
//!   mapped with -- so a locked range locks the pages it gains later, as
//!   Linux's `VM_LOCKED` VMAs do. This is how `MLOCK_ONFAULT` works (nothing
//!   is faulted in up front; each page is locked as it arrives), and how
//!   `mlockall(MCL_FUTURE)` does: every VMA made afterwards is made locked
//!   (`pcb::add_vma`), and faulted in at once unless `MCL_ONFAULT`.
//! - **On each present page table entry**, which is what the reclaimer looks
//!   at: [`crate::mm::swap::try_reclaim`] passes by a frame any of whose four
//!   entries carries the bit, and `swap_out_page` refuses one. `mprotect`
//!   keeps the bit (it changes only the permission bits), and a copy-on-write
//!   break keeps it (the copy takes the faulting entry's flags). `fork`
//!   clears it in the child, whose memory Linux does not lock.
//! - **On a swap entry** (`swap::SWAP_KEPT_LOCKED`): a range locked while a
//!   page of it is swapped out marks that page's entry, so it comes back
//!   locked whichever path brings it back -- which for `MLOCK_ONFAULT` is a
//!   later fault.
//!
//! Memory with no VMA -- the main stack, segments the loader mapped without
//! one -- is locked by its entries alone. The stack grows locked when the
//! page it grows from is locked ([`stack_growth_locked`]), as Linux's stack
//! VMA grows keeping its flags.
//!
//! ## The rules, Linux's (`mm/mlock.c`)
//!
//! - **Who may.** Locking needs a non-zero `RLIMIT_MEMLOCK` or the
//!   `MEMORY_LOCK` right on a `ResourceLimit` capability (Linux's
//!   `CAP_IPC_LOCK`) -- `EPERM` otherwise (`can_do_mlock`).
//! - **How much.** Without that right, what is locked after the call --
//!   counting a range already locked once -- must not pass `RLIMIT_MEMLOCK`
//!   (`ENOMEM`); `mlockall(MCL_CURRENT)` compares the size of the whole
//!   address space instead. Linux's default limit, 8 MiB, is the kernel's
//!   default too.
//! - **Where.** A range that wraps past the top is `EINVAL`. A range must be
//!   mapped: past the first page with neither a VMA nor a page table entry
//!   it is `ENOMEM`, the part before that page changed and nothing faulted
//!   in (`apply_vma_lock_flags`).
//! - **Populating.** `mlock` faults in every page first, writable private
//!   ones written so no copy-on-write fault is left for later, unless
//!   `MLOCK_ONFAULT`; a `PROT_NONE` part is `ENOMEM` and a page that cannot
//!   be had `EAGAIN` (`populate_vma_page_range`, `__mlock_posix_error_return`),
//!   the lock staying either way. `mlockall(MCL_CURRENT)` and a mapping made
//!   under `MCL_FUTURE` fault in what they can and answer success.
//!
//! ## Known differences
//!
//! - `VmLck` and the limit count a locked VMA whole, and memory without a
//!   VMA by its present pages: a locked main stack counts what it has grown
//!   to, as Linux's does, but its growth is not refused at the limit
//!   (Linux's `acct_stack_growth` answers `SIGSEGV` there).
//! - `mremap` moves a locked mapping locked, but does not fault in what it
//!   grows by; those pages are locked as they are touched.

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::FRAME_SIZE;
use crate::mm::page_table::{
    self, HW_PAGE_SIZE, PageFlags, PageTableEntry, USER_SPACE_END, VirtAddr,
};
use crate::mm::swap::{SWAP_KEPT_LOCKED, SwapEntry};
use crate::proc::pcb::{self, ProcessId};

/// `RLIMIT_MEMLOCK`, the resource index of the locked-memory limit.
pub const RLIMIT_MEMLOCK: u32 = 8;

/// The ABI's page: Linux's 4 KiB, in which ranges and counts are kept.
const PAGE: u64 = HW_PAGE_SIZE as u64;

/// What `MCL_FUTURE` asked: lock every later mapping, faulting it in first
/// unless `onfault` (`MCL_ONFAULT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FutureLock {
    /// `MCL_ONFAULT`: lock pages as they are faulted in, rather than faulting
    /// the mapping in when it is made.
    pub onfault: bool,
}

/// Why a lock request was refused, for each ABI to answer in its own terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MlockError {
    /// The process may not lock memory at all (`EPERM`).
    NotPermitted,
    /// Past `RLIMIT_MEMLOCK`, the range is not mapped, or a part of it is
    /// `PROT_NONE` (`ENOMEM`).
    NoMemory,
    /// The range wraps past the top of the address space (`EINVAL`).
    Invalid,
    /// A page could not be faulted in (`EAGAIN`).
    CouldNotPopulate,
}

/// How a part of a VMA is faulted in when it is locked: Linux's
/// `populate_vma_page_range`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Populate {
    /// Fault each page in for reading: a read-only mapping, or a shared one
    /// (not dirtied for nothing).
    Read,
    /// Fault each page in for writing, breaking copy-on-write up front: a
    /// writable private mapping.
    Write,
    /// `PROT_NONE`, or a guard: never faulted in, and an error for `mlock`
    /// (Linux's `EFAULT`, which `mlock` answers as `ENOMEM`).
    Inaccessible,
    /// Mapped whole when it was made (the loader's): nothing to fault in.
    Present,
}

/// Whether `pid` may lock past `RLIMIT_MEMLOCK`: it holds the `MEMORY_LOCK`
/// right on a `ResourceLimit` capability, this kernel's `CAP_IPC_LOCK`.
fn may_lock_past_limit(pid: ProcessId) -> bool {
    pcb::has_capability_type(
        pid,
        crate::cap::ResourceType::ResourceLimit,
        crate::cap::Rights::MEMORY_LOCK,
    )
}

/// The soft `RLIMIT_MEMLOCK` of `pid`, in bytes.
fn memlock_limit(pid: ProcessId) -> u64 {
    pcb::get_rlimit(pid, RLIMIT_MEMLOCK).map_or(0, |(soft, _)| soft)
}

/// Whether `pid` may lock memory at all -- a non-zero limit, or the right to
/// pass it (Linux's `can_do_mlock`), which `mlock` asks before it looks at
/// its range.
#[must_use]
pub fn may_lock(pid: ProcessId) -> bool {
    memlock_limit(pid) != 0 || may_lock_past_limit(pid)
}

/// `pid`'s address space, or `NoMemory` when it has none.
fn address_space(pid: ProcessId) -> Result<u64, MlockError> {
    pcb::get_pml4(pid)
        .filter(|&p| p != 0)
        .ok_or(MlockError::NoMemory)
}

/// Lock (`lock`) or unlock the `len` bytes at `start` of `pid`, both 4 KiB
/// aligned: `mlock`/`mlock2` and `munlock`.
///
/// # Errors
///
/// [`MlockError`], in Linux's order: `NotPermitted` (locking only), then
/// `NoMemory` past the limit (locking only), then `Invalid` for a range that
/// wraps, then `NoMemory` for a page with neither a VMA nor an entry -- after
/// the part before it has been changed -- then, populating, `NoMemory` for a
/// `PROT_NONE` part and `CouldNotPopulate` for a page that cannot be had.
pub fn lock_range(
    pid: ProcessId,
    start: u64,
    len: u64,
    lock: bool,
    onfault: bool,
) -> Result<(), MlockError> {
    let pml4 = address_space(pid)?;
    if lock {
        if !may_lock(pid) {
            return Err(MlockError::NotPermitted);
        }
        if !may_lock_past_limit(pid) && !within_limit(pid, pml4, start, len) {
            return Err(MlockError::NoMemory);
        }
    }
    let end = start.wrapping_add(len);
    if end < start {
        return Err(MlockError::Invalid);
    }
    if end == start {
        return Ok(());
    }
    // A part of the range with no VMA must be memory the loader or the stack
    // mapped; a page with no entry either is not mapped at all, and Linux
    // changes what lies before such a hole and answers ENOMEM, unpopulated.
    let hole = first_hole(pid, pml4, start, end);
    let reach = hole.unwrap_or(end);
    // The VMAs first, so a page faulted in from here on is mapped locked;
    // then the entries already there.
    pcb::set_vma_lock_range(pid, start, reach, lock);
    set_entry_bits(pml4, start, reach, lock);
    if hole.is_some() {
        return Err(MlockError::NoMemory);
    }
    if lock && !onfault {
        populate(pid, pml4, start, end, false)?;
    }
    Ok(())
}

/// Linux's `do_mlock` limit test: what is locked now, plus `len`, less what
/// of `[start, start + len)` is locked already, within `RLIMIT_MEMLOCK`.
fn within_limit(pid: ProcessId, pml4: u64, start: u64, len: u64) -> bool {
    let limit = memlock_limit(pid);
    let after = locked_bytes_in(pid, pml4, 0, USER_SPACE_END).saturating_add(len);
    if after <= limit {
        return true;
    }
    // Linux subtracts the overlap only when over the limit, and stops the
    // range at the top rather than wrapping (`count_mm_mlocked_page_nr`).
    let overlap = locked_bytes_in(pid, pml4, start, start.saturating_add(len));
    after.saturating_sub(overlap) <= limit
}

/// The first page of `[start, end)` with neither a VMA nor an entry -- where
/// the mapped range stops -- if any. Nothing at or past [`USER_SPACE_END`] is
/// a process's (its entries there are the kernel's).
fn first_hole(pid: ProcessId, pml4: u64, start: u64, end: u64) -> Option<u64> {
    let user_end = end.min(USER_SPACE_END);
    if start < user_end {
        for (lo, hi) in pcb::vma_coverage_gaps(pid, start, user_end).unwrap_or_default() {
            let mut va = lo;
            while va < hi {
                if !entry_held(pml4, va) {
                    return Some(va);
                }
                va = va.saturating_add(PAGE);
            }
        }
    }
    (end > USER_SPACE_END).then(|| start.max(USER_SPACE_END))
}

/// Whether the 4 KiB page at `va` is mapped: present, or swapped out.
fn entry_held(pml4: u64, va: u64) -> bool {
    let frame = FRAME_SIZE as u64;
    let frame_va = va & !frame.wrapping_sub(1);
    let part = usize::try_from(va.wrapping_sub(frame_va) / PAGE).unwrap_or(0);
    page_table::read_frame_ptes(pml4, VirtAddr::new(frame_va))
        .and_then(|parts| parts.get(part).copied())
        .is_some_and(|e| e.is_present() || SwapEntry::from_pte_raw(e.raw()).is_some())
}

/// `mlockall(flags)`: lock every mapping now (`current`), every later one
/// (`future`), or both, faulting the present ones in unless `onfault`.
/// Asking `current` alone forgets an earlier `future`, as Linux's does.
///
/// # Errors
///
/// `NotPermitted` without the right to lock at all; `NoMemory` when
/// `current` and the whole address space is past the limit (Linux compares
/// its size, not what is locked). What cannot be faulted in is left to fault
/// in later, as Linux leaves it.
pub fn lock_all(
    pid: ProcessId,
    current: bool,
    future: bool,
    onfault: bool,
) -> Result<(), MlockError> {
    let pml4 = address_space(pid)?;
    if !may_lock(pid) {
        return Err(MlockError::NotPermitted);
    }
    if current && !may_lock_past_limit(pid) && mapped_bytes(pid, pml4) > memlock_limit(pid) {
        return Err(MlockError::NoMemory);
    }
    pcb::set_mlock_future(pid, future.then_some(FutureLock { onfault }));
    if current {
        pcb::set_all_vma_locks(pid, true);
        // Every entry, VMA or not: the stack and the loader's segments are
        // locked by their entries alone.
        set_entry_bits(pml4, 0, USER_SPACE_END, true);
        if !onfault {
            // Errors ignored (`mm_populate(0, TASK_SIZE)`).
            let _ = populate(pid, pml4, 0, USER_SPACE_END, true);
        }
    }
    Ok(())
}

/// `munlockall()`: unlock every mapping and stop locking later ones.
pub fn unlock_all(pid: ProcessId) {
    pcb::set_mlock_future(pid, None);
    pcb::set_all_vma_locks(pid, false);
    if let Ok(pml4) = address_space(pid) {
        set_entry_bits(pml4, 0, USER_SPACE_END, false);
    }
}

/// Whether `pid` may make a new mapping of `len` bytes. When it will be
/// locked -- `MAP_LOCKED` (`map_locked`), or after `mlockall(MCL_FUTURE)` --
/// the process must be allowed to lock at all (for `MAP_LOCKED`), and the
/// mapping must fit within `RLIMIT_MEMLOCK` with what is locked already
/// (Linux's `mlock_future_ok`, which `mmap` and `brk` ask).
///
/// # Errors
///
/// `NotPermitted` for `MAP_LOCKED` without the right to lock (`mmap`'s
/// `EPERM`); `NoMemory` past the limit (`mmap`'s `EAGAIN`).
pub fn new_mapping_allowed(pid: ProcessId, len: u64, map_locked: bool) -> Result<(), MlockError> {
    if map_locked && !may_lock(pid) {
        return Err(MlockError::NotPermitted);
    }
    let locked = map_locked || pcb::mlock_future(pid).is_some();
    if !locked || may_lock_past_limit(pid) {
        return Ok(());
    }
    let pml4 = address_space(pid)?;
    let after = locked_bytes_in(pid, pml4, 0, USER_SPACE_END).saturating_add(len);
    if after > memlock_limit(pid) {
        return Err(MlockError::NoMemory);
    }
    Ok(())
}

/// After `pid` made the mapping `[start, end)`: lock it for `MAP_LOCKED`
/// (`mlockall(MCL_FUTURE)` locked its VMA as it was made), lock the entries
/// it was made with (a committed mapping is mapped whole at once), and fault
/// it in unless `MCL_ONFAULT` asked not to -- what cannot be had is left to
/// fault in when touched, as Linux's `mm_populate` after `mmap` and `brk`
/// leaves it.
pub fn after_new_mapping(pid: ProcessId, start: u64, end: u64, map_locked: bool) {
    let future = pcb::mlock_future(pid);
    if (!map_locked && future.is_none()) || end <= start {
        return;
    }
    let Ok(pml4) = address_space(pid) else {
        return;
    };
    if map_locked {
        pcb::set_vma_lock_range(pid, start, end, true);
    }
    set_entry_bits(pml4, start, end, true);
    if !future.is_some_and(|f| f.onfault) {
        // Errors ignored, as Linux ignores them here.
        let _ = populate(pid, pml4, start, end, true);
    }
}

/// Bytes of `pid` that are locked: Linux's `mm->locked_vm`, the `VmLck`
/// of `/proc/<pid>/status`.
#[must_use]
pub fn locked_bytes(pid: ProcessId) -> u64 {
    address_space(pid).map_or(0, |pml4| locked_bytes_in(pid, pml4, 0, USER_SPACE_END))
}

/// Bytes of `[lo, hi)` of `pid` that are locked: its locked VMAs whole,
/// and the locked pages of memory without a VMA.
fn locked_bytes_in(pid: ProcessId, pml4: u64, lo: u64, hi: u64) -> u64 {
    let in_vmas = pcb::vma_locked_bytes(pid, Some((lo, hi)));
    let without = count_gap_entries(pid, pml4, lo, hi, |e| {
        if e.is_present() {
            e.flags().contains(PageFlags::MLOCKED)
        } else {
            SwapEntry::from_pte_raw(e.raw()).is_some() && e.raw() & SWAP_KEPT_LOCKED != 0
        }
    });
    in_vmas.saturating_add(without.saturating_mul(PAGE))
}

/// Bytes of `pid`'s address space: its VMAs, and the pages of memory without
/// one -- what `mlockall(MCL_CURRENT)` would lock (Linux's `total_vm`).
fn mapped_bytes(pid: ProcessId, pml4: u64) -> u64 {
    let without = count_gap_entries(pid, pml4, 0, USER_SPACE_END, |e| {
        e.is_present() || SwapEntry::from_pte_raw(e.raw()).is_some()
    });
    pcb::vma_total_bytes(pid).saturating_add(without.saturating_mul(PAGE))
}

/// How many 4 KiB entries of `[lo, hi)` outside `pid`'s VMAs `count` says
/// yes to.
fn count_gap_entries(
    pid: ProcessId,
    pml4: u64,
    lo: u64,
    hi: u64,
    count: impl Fn(PageTableEntry) -> bool,
) -> u64 {
    let hi = hi.min(USER_SPACE_END);
    if lo >= hi {
        return 0;
    }
    // The gaps first, from the process table, which the fault path holds
    // while it takes the page-table lock: never the other way round here.
    let gaps = pcb::vma_coverage_gaps(pid, lo, hi).unwrap_or_default();
    let mut n = 0u64;
    let _held = crate::mm::as_lock::lock(pml4);
    for (glo, ghi) in gaps {
        // SAFETY: `pml4` is `pid`'s live address space and its page-table
        // lock is held, so no table is freed under the walk; the visitor
        // changes nothing.
        unsafe {
            page_table::update_leaf_bits(pml4, glo, ghi, |_, e| {
                if count(e) {
                    n = n.saturating_add(1);
                }
                None
            });
        }
    }
    n
}

/// Fault in the VMAs of `[start, end)` as a lock of them asks
/// ([`Populate`]), as Linux's `__mm_populate` does: swapped-out pages back,
/// pages never touched faulted in, copy-on-write broken in a writable private
/// mapping. Memory without a VMA is present, or a hole the caller has
/// refused already.
///
/// # Errors
///
/// Unless `ignore_errors`: `NoMemory` at a `PROT_NONE` part,
/// `CouldNotPopulate` at a page that cannot be had. With it, every part is
/// tried and success answered.
fn populate(
    pid: ProcessId,
    pml4: u64,
    start: u64,
    end: u64,
    ignore_errors: bool,
) -> Result<(), MlockError> {
    for (lo, hi, how) in pcb::vma_populate_parts(pid, start, end) {
        let result = match how {
            Populate::Present => Ok(()),
            Populate::Inaccessible => Err(MlockError::NoMemory),
            Populate::Read => fault_in(pid, pml4, lo, hi, false),
            Populate::Write => fault_in(pid, pml4, lo, hi, true),
        };
        if let Err(e) = result
            && !ignore_errors
        {
            return Err(e);
        }
    }
    Ok(())
}

/// Fault in each page of `[lo, hi)`, a part of one VMA of `pid`: for writing
/// if `write`.
fn fault_in(pid: ProcessId, pml4: u64, lo: u64, hi: u64, write: bool) -> Result<(), MlockError> {
    /// Page-fault error code bits: present (a protection fault), write, user.
    const PRESENT: u64 = 1;
    const WRITE: u64 = 1 << 1;
    const USER: u64 = 1 << 2;
    let frame = FRAME_SIZE as u64;
    let mut va = lo;
    while va < hi {
        let frame_va = va & !frame.wrapping_sub(1);
        // SAFETY: `pml4` is `pid`'s live address space; swap-in takes its
        // page-table lock itself.
        if unsafe { crate::mm::swap::is_swapped(pml4, VirtAddr::new(frame_va)) } {
            // Back, each part with the flags its entry kept -- the lock bit
            // among them, which `set_entry_bits` put there -- and on the
            // reclaim list again, where the lock keeps it from being chosen.
            // SAFETY: as above; a part of the frame holds a swap entry.
            match unsafe { crate::mm::swap::swap_in_page(pml4, VirtAddr::new(frame_va)) } {
                Ok(Some(kept)) => crate::mm::swap::register_reclaimable(pml4, frame_va, kept),
                // Another CPU brought it back first, and registered it.
                Ok(None) => {}
                Err(_) => return Err(MlockError::CouldNotPopulate),
            }
        }
        let had = page_table::read_leaf_pte(pml4, VirtAddr::new(va));
        let resolved = match had {
            None => pcb::try_resolve_fault(pid, va, USER | if write { WRITE } else { 0 }),
            // A write fault now, so the copy is made here and not on the
            // first write after the lock (Linux's `FOLL_WRITE`).
            Some(e) if write && e.flags().contains(PageFlags::COW) => {
                pcb::try_resolve_fault(pid, va, PRESENT | WRITE | USER)
            }
            Some(_) => true,
        };
        if !resolved {
            return Err(MlockError::CouldNotPopulate);
        }
        va = va.saturating_add(PAGE);
    }
    Ok(())
}

/// Set (`locked`) or clear the lock bit on every entry of `[start, end)` that
/// holds a page: [`PageFlags::MLOCKED`] on a present entry, the swap entry's
/// own copy (`SWAP_KEPT_LOCKED`) on a swapped-out one. Every other bit is
/// left as it is, under the address space's page-table lock.
fn set_entry_bits(pml4: u64, start: u64, end: u64, locked: bool) {
    let _held = crate::mm::as_lock::lock(pml4);
    // SAFETY: `pml4` is a live address space whose page-table lock is held,
    // so no table is freed under the walk. The bits are ones the hardware
    // ignores where they go -- a software bit of a present entry, a high bit
    // of a non-present one -- so no TLB entry goes stale.
    unsafe {
        page_table::update_leaf_bits(pml4, start, end, |_, e| {
            if e.is_present() {
                Some((PageFlags::MLOCKED.bits(), locked))
            } else if SwapEntry::from_pte_raw(e.raw()).is_some() {
                Some((SWAP_KEPT_LOCKED, locked))
            } else {
                None
            }
        });
    }
}

/// Whether the 16 KiB frame at `frame_va` has a locked part: what the
/// reclaimer asks before it swaps a frame out.
#[must_use]
pub fn frame_is_locked(pml4: u64, frame_va: u64) -> bool {
    page_table::read_frame_ptes(pml4, VirtAddr::new(frame_va)).is_some_and(|parts| {
        parts
            .iter()
            .any(|p| p.is_present() && p.flags().contains(PageFlags::MLOCKED))
    })
}

/// Whether the main stack, grown down to a new frame at `frame_va`, grows
/// locked: as the nearest page of it above is, present or swapped out.
///
/// Linux's stack is one VMA that grows downward keeping its flags, so it
/// grows locked when its lowest part is locked -- by `mlockall(MCL_CURRENT)`
/// or an `mlock` reaching it, not by `MCL_FUTURE`, which is for VMAs made
/// later. This kernel's main stack has no VMA, so the page it grows from is
/// asked. Reads entries only, without waiting for anything: it runs in the
/// page-fault handler.
#[must_use]
pub fn stack_growth_locked(pml4: u64, frame_va: u64, stack_top: u64) -> bool {
    let frame = FRAME_SIZE as u64;
    let mut va = frame_va.saturating_add(frame);
    while va < stack_top {
        if let Some(parts) = page_table::read_frame_ptes(pml4, VirtAddr::new(va)) {
            // Lowest part first: the one nearest the new frame.
            for p in parts {
                if p.is_present() {
                    return p.flags().contains(PageFlags::MLOCKED);
                }
                if SwapEntry::from_pte_raw(p.raw()).is_some() {
                    return p.raw() & SWAP_KEPT_LOCKED != 0;
                }
            }
        }
        va = va.saturating_add(frame);
    }
    false
}

/// Self-test, on a scratch address space with one mapped frame: the bit goes
/// on the entries asked for and nowhere else, keeps every other bit, a frame
/// with one locked part of four counts as locked -- the reclaimer's test --
/// until it is unlocked; a swap entry takes the lock in its own bit, leaving
/// its slot alone; the stack grows locked under a locked page. (A process's
/// calls, with their limits, are tested in ring 3:
/// `spawn::self_test_linux_mlock`.)
///
/// # Errors
///
/// [`KernelError::InternalError`] on a failed check.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[mlock] Running self-test...");
    let pml4 = page_table::alloc_pml4()?;
    let teardown = |pml4: u64| {
        // SAFETY: our own scratch address space, never loaded in any CR3:
        // its user half (one frame and its tables) is freed, then the PML4.
        unsafe {
            page_table::clear_user_address_space(pml4);
            page_table::free_pml4(pml4);
        }
    };
    let va: u64 = 0x0000_0050_0000_0000;
    let frame_len = FRAME_SIZE as u64;
    let frame = match crate::mm::frame::alloc_frame_zeroed() {
        Ok(f) => f,
        Err(e) => {
            teardown(pml4);
            return Err(e);
        }
    };
    let flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_EXECUTE;
    // SAFETY: a fresh address space of our own and a fresh frame.
    if let Err(e) = unsafe { page_table::map_frame(pml4, VirtAddr::new(va), frame, flags) } {
        // SAFETY: never mapped, so ours alone to free.
        let _ = unsafe { crate::mm::frame::free_frame(frame) };
        teardown(pml4);
        return Err(e);
    }
    let part = |i: u64| va.wrapping_add(i.wrapping_mul(PAGE));
    let check = || -> Result<(), &'static str> {
        if frame_is_locked(pml4, va) {
            return Err("a fresh frame reads as locked");
        }
        // Lock the second 4 KiB part of four.
        set_entry_bits(pml4, part(1), part(2), true);
        if !frame_is_locked(pml4, va) {
            return Err("a frame with a locked part is not locked");
        }
        let parts = page_table::read_frame_ptes(pml4, VirtAddr::new(va)).ok_or("no entries")?;
        let (Some(first), Some(second)) = (parts.first(), parts.get(1)) else {
            return Err("no entries");
        };
        if first.flags().contains(PageFlags::MLOCKED)
            || !second.flags().contains(PageFlags::MLOCKED)
            || !second.flags().contains(PageFlags::WRITABLE)
            || second.phys_addr() != first.phys_addr().wrapping_add(PAGE)
        {
            return Err("the bit went on the wrong part, or took other bits with it");
        }
        // The stack grows locked under a locked page, and not under an
        // unlocked one: grown at the frame below, asking the frame above.
        let below = va.wrapping_sub(frame_len);
        let top = va.wrapping_add(frame_len);
        if stack_growth_locked(pml4, below, top) {
            return Err("the stack grows locked under an unlocked lowest page");
        }
        set_entry_bits(pml4, part(0), part(1), true);
        if !stack_growth_locked(pml4, below, top) {
            return Err("the stack grows unlocked under a locked page");
        }
        set_entry_bits(pml4, va, va.wrapping_add(frame_len), false);
        if frame_is_locked(pml4, va) {
            return Err("unlocking left the frame locked");
        }
        // A swap entry takes the lock in its own bit, its slot untouched,
        // and gives it back as `MLOCKED` (what swap-in maps the page with).
        let slot = SwapEntry::new(0x2A5).ok_or("no swap entry")?;
        let raw = slot.to_pte_raw_keeping(flags);
        // SAFETY: our own scratch address space; part 3 is replaced by a swap
        // entry and given back below, so the frame is still freed whole.
        let was = unsafe {
            page_table::exchange_frame_pte(
                pml4,
                VirtAddr::new(va),
                3,
                PageTableEntry::from_raw(raw),
            )
        }
        .map_err(|_| "no entry to replace")?;
        set_entry_bits(pml4, part(3), part(4), true);
        let locked_raw = page_table::read_frame_ptes(pml4, VirtAddr::new(va))
            .and_then(|p| p.get(3).copied())
            .map_or(0, |e| e.raw());
        set_entry_bits(pml4, part(3), part(4), false);
        let unlocked_raw = page_table::read_frame_ptes(pml4, VirtAddr::new(va))
            .and_then(|p| p.get(3).copied())
            .map_or(0, |e| e.raw());
        // SAFETY: as above: the part's own entry back.
        let _ = unsafe { page_table::exchange_frame_pte(pml4, VirtAddr::new(va), 3, was) };
        if locked_raw != raw | SWAP_KEPT_LOCKED
            || SwapEntry::from_pte_raw(locked_raw) != Some(slot)
            || !SwapEntry::kept_flags(locked_raw).contains(PageFlags::MLOCKED)
            || unlocked_raw != raw
        {
            return Err("a swap entry's lock bit went wrong");
        }
        Ok(())
    };
    let result = check();
    teardown(pml4);
    if let Err(what) = result {
        crate::serial_println!("[mlock]   FAIL: {}", what);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[mlock]   lock bits on entries and swap entries, a frame locked by one part, stack growth: OK"
    );
    Ok(())
}
