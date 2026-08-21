//! Userspace memory validation.
//!
//! Syscall handlers must validate every pointer received from user
//! space before dereferencing it.  Without validation, a malicious
//! process could trick the kernel into reading or writing kernel
//! memory by passing kernel-space pointers as syscall arguments.
//!
//! ## Validation Rules
//!
//! 1. The entire buffer `[ptr, ptr+len)` must be in the user half of
//!    the address space (below [`USER_SPACE_END`]).
//! 2. `ptr + len` must not overflow (wrapping into kernel space).
//! 3. Every 4 KiB page in the range must be mapped in the current
//!    process's page table.  A not-present page that falls inside a
//!    committed VMA is faulted in on the spot (demand paging), mirroring
//!    Linux's `get_user_pages()`: a freshly-allocated buffer the process
//!    has not yet touched is valid, not an error.  Only an address with
//!    no backing VMA (or insufficient VMA permissions) fails.
//! 4. For write validation, every mapped page must be writable.  A
//!    copy-on-write page (present, read-only, COW bit set) counts as
//!    writable: validation breaks the CoW eagerly (private copy +
//!    writable remap) so the kernel can write through the pointer.
//!    Only a genuinely read-only mapping fails write validation.
//!
//! ## Performance
//!
//! Each validation walks the page table once per 4 KiB page in the
//! buffer.  For typical syscall buffers (< 4 KiB), this is a single
//! page table walk — about 4 memory reads.  This cost is negligible
//! compared to the syscall itself (console I/O, IPC, etc.).
//!
//! ## Future Optimizations
//!
//! Linux uses a `copy_from_user` / `copy_to_user` approach that
//! catches page faults during the copy instead of pre-validating.
//! This is faster for large buffers (no separate walk) but requires
//! exception table infrastructure that we don't have yet.  The
//! current approach is correct and sufficient for initial userspace.
//!
//! [`USER_SPACE_END`]: super::page_table::USER_SPACE_END

// KASAN debug profile: this module is exempt from compiler instrumentation.
// Dereferences raw user pointers. A user address maps to a shadow address
// with inconsistent bits 63:47 (`shadow(0x400000) = 0xDFFF_E000_0008_0000`),
// which is non-canonical — checking one would #GP rather than report.
// (`sanitize` is nightly-only, so it is gated on the `kasan_instrumented` cfg
// that `scripts/kasan-build.sh` sets; the ordinary build never sees it.)
#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]

use super::page_table::{self, PageFlags, USER_SPACE_END, VirtAddr};
use crate::error::{KernelError, KernelResult};
use crate::proc::thread;
use crate::sched;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Hardware page size (4 KiB).  Page table entries map 4 KiB pages,
/// so we validate at this granularity.
const PAGE_SIZE: u64 = 4096;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Validate that a user-space buffer is readable.
///
/// Checks that the entire range `[ptr, ptr+len)` is:
/// - Within the user half of the address space.
/// - Mapped in the current process's page table.
///
/// Returns `Ok(())` if the buffer is safe to read from kernel mode.
///
/// **Kernel context bypass**: if the current task has no owning
/// process (bare kernel task), validation is skipped — kernel code
/// uses kernel pointers that are always valid.
///
/// # Arguments
///
/// - `ptr` — start of the buffer (from a userspace register).
/// - `len` — length of the buffer in bytes.
///
/// # Errors
///
/// - [`KernelError::InvalidAddress`] if any part of the buffer is in
///   kernel space, wraps around, or is unmapped.
pub fn validate_user_read(ptr: u64, len: usize) -> KernelResult<()> {
    if is_kernel_context() {
        return Ok(());
    }
    validate_user_range(ptr, len, false)
}

/// Validate that a user-space buffer is writable.
///
/// Same as [`validate_user_read`], but additionally checks that every
/// mapped page has the `WRITABLE` flag set.
///
/// **Kernel context bypass**: same as [`validate_user_read`].
///
/// # Errors
///
/// - [`KernelError::InvalidAddress`] if any part of the buffer is in
///   kernel space, wraps around, unmapped, or read-only.
pub fn validate_user_write(ptr: u64, len: usize) -> KernelResult<()> {
    if is_kernel_context() {
        return Ok(());
    }
    validate_user_range(ptr, len, true)
}

/// Validate that a single user-space pointer refers to a valid, mapped
/// byte.  Shorthand for `validate_user_read(ptr, 1)`.
///
/// **Kernel context bypass**: same as [`validate_user_read`].
pub fn validate_user_ptr(ptr: u64) -> KernelResult<()> {
    if is_kernel_context() {
        return Ok(());
    }
    validate_user_range(ptr, 1, false)
}

// ---------------------------------------------------------------------------
// Kernel context detection
// ---------------------------------------------------------------------------

/// Returns `true` if the current task is a bare kernel task with no
/// owning user process.  Kernel tasks use kernel-space pointers that
/// don't need user-space validation.
fn is_kernel_context() -> bool {
    let task_id = sched::current_task_id();
    thread::owner_process(task_id).is_none()
}

// ---------------------------------------------------------------------------
// Internal implementation
// ---------------------------------------------------------------------------

/// Core validation: check address range and page mappings.
///
/// This function contains the actual validation logic with no
/// kernel-context bypass.  The public API (`validate_user_read`,
/// etc.) calls `is_kernel_context()` first and skips this function
/// for bare kernel tasks.
///
/// Arithmetic here is for address-range boundary checking.  Overflow
/// is the failure condition, not a bug — it means the user passed a
/// wrapping pointer range.
#[allow(clippy::arithmetic_side_effects)]
fn validate_user_range(ptr: u64, len: usize, need_writable: bool) -> KernelResult<()> {
    // Zero-length buffers are always valid (nothing to access).
    if len == 0 {
        return Ok(());
    }

    // Null pointer is never valid.
    if ptr == 0 {
        return Err(KernelError::InvalidAddress);
    }

    let len_u64 = len as u64;

    // Check for overflow: ptr + len must not wrap around.
    let end = ptr
        .checked_add(len_u64)
        .ok_or(KernelError::InvalidAddress)?;

    // The entire range must be in user space.
    if end > USER_SPACE_END {
        return Err(KernelError::InvalidAddress);
    }

    // Get the current PML4 from CR3.
    let cr3 = page_table::read_cr3();
    let pml4 = page_table::cr3_to_pml4(cr3);

    // Walk each 4 KiB page in the range and verify it's mapped.
    let mut addr = ptr & !(PAGE_SIZE - 1); // Round down to page boundary.

    while addr < end {
        let virt = VirtAddr::new(addr);

        // translate() returns None if the page is not mapped.  A
        // not-present page is not necessarily invalid: it may be a
        // committed-but-not-yet-populated demand-paged page (a fresh
        // malloc/mmap buffer the process hasn't touched yet).  A direct
        // userspace access would fault it in via the #PF handler; when
        // the kernel writes through the pointer on the process's behalf
        // it must do the same proactively — exactly what Linux's
        // get_user_pages() does before a kernel-side access.  This is
        // the common case for any syscall handed a large fresh output
        // buffer (e.g. getdents64's glibc-allocated dirent buffer): only
        // the first page is faulted in, so a strict presence check would
        // spuriously reject the rest of a perfectly valid region.
        if page_table::translate(pml4, virt).is_none() {
            // Not present — try to fault it in (demand paging), then
            // re-check.  If it still isn't mapped, the address has no
            // committed backing and is a genuine fault.
            let resolved = try_fault_in_user_page(addr, need_writable)
                && page_table::translate(pml4, virt).is_some();
            if !resolved {
                return Err(KernelError::InvalidAddress);
            }
        }

        // If write access is required, check the page flags.
        if need_writable {
            if let Some(flags) = page_flags(pml4, virt) {
                if !flags.contains(PageFlags::WRITABLE) {
                    // A copy-on-write page is present and read-only by
                    // design: a write triggers the CoW fault handler,
                    // which makes a private copy and remaps the page
                    // writable.  The kernel cannot rely on that fault
                    // firing for its own `core::ptr::write` to the user
                    // page, so break the CoW *now* — exactly what Linux
                    // does in `get_user_pages(FOLL_WRITE)` before letting
                    // the kernel write through a user pointer.  This is
                    // the common state for the whole address space right
                    // after `fork()`, so any write-validating syscall
                    // (wait4, read, pipe, gettimeofday, …) called by a
                    // freshly-forked process hits it.
                    if flags.contains(PageFlags::COW) {
                        super::cow::resolve_cow_fault(pml4, addr)?;
                        // CoW now broken; the page is privately mapped
                        // and writable.  Fall through to the next page.
                    } else {
                        // Genuinely read-only mapping (e.g. a read-only
                        // file or PROT_READ region) → real EFAULT.
                        return Err(KernelError::InvalidAddress);
                    }
                }
            }
        }

        // Move to the next 4 KiB page.  Use saturating_add to avoid
        // overflow at the top of the address space (shouldn't happen
        // since we already checked end < USER_SPACE_END).
        addr = addr.saturating_add(PAGE_SIZE);
    }

    Ok(())
}

/// Fault in a not-present user page that is backed by a committed VMA
/// but has not been populated yet (demand paging).
///
/// Synthesizes a page-fault error code matching the access the kernel is
/// about to perform and routes it through the per-process fault resolver
/// ([`crate::proc::pcb::try_resolve_fault`]) — the same path the hardware
/// #PF handler uses.  Returns `true` only if the resolver mapped the page
/// (i.e. the address really did fall inside a committed region with
/// adequate permissions); a genuinely unmapped or permission-violating
/// address returns `false`, so invalid pointers are still rejected by the
/// caller's post-check.
///
/// Returns `false` for bare kernel tasks (no owning process) — those use
/// kernel-space pointers and never reach this path because
/// [`validate_user_range`] is skipped for them via [`is_kernel_context`].
fn try_fault_in_user_page(addr: u64, need_writable: bool) -> bool {
    let task_id = sched::current_task_id();
    let Some(pid) = thread::owner_process(task_id) else {
        return false;
    };
    // x86 page-fault error code bits: bit 0 = present (0 here: the page is
    // not present), bit 1 = write, bit 2 = user.  We model a user-mode
    // access; the write bit is set only when the kernel needs to write
    // through the pointer, so a read-only validation never demands a
    // writable mapping.
    let mut error_code: u64 = 1 << 2; // user-mode access
    if need_writable {
        error_code |= 1 << 1; // write access
    }
    crate::proc::pcb::try_resolve_fault(pid, addr, error_code)
}

/// Read the page table flags for a virtual address.
///
/// Walks the page table and returns the PTE flags if the page is
/// mapped.  Returns `None` if the page is not mapped at any level.
fn page_flags(pml4_phys: u64, virt: VirtAddr) -> Option<PageFlags> {
    let hhdm = page_table::hhdm()?;

    if !virt.is_canonical() {
        return None;
    }

    // Walk PML4 → PDPT → PD → PT, same as translate() but returns
    // the leaf entry's flags instead of the physical address.
    //
    // SAFETY: pml4_phys is from CR3 (valid page table root).  The
    // HHDM is always mapped.  Index values are masked to 0..511.
    let pml4e = unsafe { page_table::read_entry(pml4_phys, virt.pml4_index(), hhdm) };
    if !pml4e.is_present() {
        return None;
    }

    let pdpte = unsafe { page_table::read_entry(pml4e.phys_addr(), virt.pdpt_index(), hhdm) };
    if !pdpte.is_present() {
        return None;
    }
    if pdpte.is_huge() {
        return Some(pdpte.flags());
    }

    let pde = unsafe { page_table::read_entry(pdpte.phys_addr(), virt.pd_index(), hhdm) };
    if !pde.is_present() {
        return None;
    }
    if pde.is_huge() {
        return Some(pde.flags());
    }

    let pte = unsafe { page_table::read_entry(pde.phys_addr(), virt.pt_index(), hhdm) };
    if !pte.is_present() {
        return None;
    }

    Some(pte.flags())
}

// ---------------------------------------------------------------------------
// SMAP-safe user memory copy primitives
// ---------------------------------------------------------------------------

/// Copy data from user-space into a kernel buffer (SMAP-safe).
///
/// Validates the source range, then copies `len` bytes from `user_src`
/// into `kernel_dst`.  When SMAP is enabled, uses STAC/CLAC to
/// temporarily permit kernel access to user pages.
///
/// # Arguments
///
/// - `user_src` — source pointer in user address space
/// - `kernel_dst` — destination pointer in kernel address space
/// - `len` — number of bytes to copy
///
/// # Errors
///
/// - [`KernelError::InvalidAddress`] if the user range is invalid
///
/// # Safety
///
/// `kernel_dst` must point to a valid, writable kernel buffer of at
/// least `len` bytes.
#[allow(dead_code)]
pub unsafe fn copy_from_user(user_src: u64, kernel_dst: *mut u8, len: usize) -> KernelResult<()> {
    if len == 0 {
        return Ok(());
    }

    // Validate the user source range.
    validate_user_read(user_src, len)?;

    // SAFETY: We validated user_src is mapped and readable.
    // STAC/CLAC provide SMAP-safe access.
    unsafe {
        crate::smep_smap::stac();
        core::ptr::copy_nonoverlapping(user_src as *const u8, kernel_dst, len);
        crate::smep_smap::clac();
    }
    Ok(())
}

/// Copy data from a kernel buffer to user-space (SMAP-safe).
///
/// Validates the destination range is writable, then copies `len` bytes
/// from `kernel_src` into `user_dst`.  When SMAP is enabled, uses
/// STAC/CLAC to temporarily permit kernel access to user pages.
///
/// # Arguments
///
/// - `kernel_src` — source pointer in kernel address space
/// - `user_dst` — destination pointer in user address space
/// - `len` — number of bytes to copy
///
/// # Errors
///
/// - [`KernelError::InvalidAddress`] if the user range is invalid or read-only
///
/// # Safety
///
/// `kernel_src` must point to a valid, readable kernel buffer of at
/// least `len` bytes.
#[allow(dead_code)]
pub unsafe fn copy_to_user(kernel_src: *const u8, user_dst: u64, len: usize) -> KernelResult<()> {
    if len == 0 {
        return Ok(());
    }

    // Validate the user destination range (must be writable).
    validate_user_write(user_dst, len)?;

    // SAFETY: We validated user_dst is mapped and writable.
    // STAC/CLAC provide SMAP-safe access.
    unsafe {
        crate::smep_smap::stac();
        core::ptr::copy_nonoverlapping(kernel_src, user_dst as *mut u8, len);
        crate::smep_smap::clac();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Cross-address-space user memory copies
// ---------------------------------------------------------------------------

/// Resolve the physical address backing user virtual address `va` in the
/// address space rooted at `pml4`, optionally requiring the page to be
/// writable.
///
/// Returns the full physical address (including the page offset).
///
/// A first failure is not final. Two perfectly ordinary states make the raw
/// page walk fail on an address the owning process would have had no trouble
/// with — an absent page that is committed but not yet populated, and a
/// present-but-read-only page whose `COW` bit means "copy me on write". Both
/// are what the hardware fault handler exists to fix, and neither can fix
/// itself here, because nothing is going to fault: this walk reads the page
/// table by hand through the HHDM. So on failure we ask the owning process's
/// resolver to do what the fault would have done, then walk once more. See
/// [`try_resolve_remote`] for why one retry is the right number.
fn user_page_phys(pml4: u64, va: u64, need_writable: bool) -> KernelResult<u64> {
    if let Ok(phys) = user_page_phys_once(pml4, va, need_writable) {
        return Ok(phys);
    }
    if !try_resolve_remote(pml4, va, need_writable) {
        return Err(KernelError::InvalidAddress);
    }
    user_page_phys_once(pml4, va, need_writable)
}

/// One raw page-table walk, with no attempt to resolve what it finds.
fn user_page_phys_once(pml4: u64, va: u64, need_writable: bool) -> KernelResult<u64> {
    let virt = VirtAddr::new(va);
    if need_writable {
        let flags = page_table::translate_flags(pml4, virt).ok_or(KernelError::InvalidAddress)?;
        if !flags.contains(PageFlags::WRITABLE) {
            return Err(KernelError::InvalidAddress);
        }
    }
    page_table::translate(pml4, virt).ok_or(KernelError::InvalidAddress)
}

/// Ask the process owning `pml4` to resolve a fault at `va`, as though the
/// hardware had raised one for the access we are about to perform by hand.
///
/// This is the cross-address-space counterpart of [`try_fault_in_user_page`],
/// and the reason it can exist at all is that
/// [`crate::proc::pcb::try_resolve_fault`] takes the pid explicitly and works
/// entirely on *that* process's `pml4_phys` and VMA list — it never consults
/// `CR3`. So the resolver was always usable from here; it simply was never
/// called, which is why `process_vm_writev` into a freshly-forked target failed
/// on every page. Right after `fork` the entire address space is CoW, so the
/// case that looks exotic is in fact the default one.
///
/// # The present bit is the whole decision
///
/// `try_resolve_fault` branches on the synthesized error code: `present &&
/// write` selects the CoW break, `!present` selects demand paging, and nothing
/// else is resolvable. So the bit must describe the page we actually found, not
/// the access we want. [`try_fault_in_user_page`] hard-codes it to zero, which
/// is correct there only because it is called solely after `translate` returned
/// `None`; copying that convention here would silently turn every CoW page into
/// an unresolvable demand-page request.
///
/// # Why one retry, not a loop
///
/// Each of the two resolvable states is cleared outright by its resolver — a
/// populated page is present, a broken CoW page is writable and no longer `COW`
/// — so a second failure means the address is genuinely unusable (an unmapped
/// hole, or a read-only VMA), and retrying would spin rather than converge.
fn try_resolve_remote(pml4: u64, va: u64, need_writable: bool) -> bool {
    let Some(pid) = crate::proc::pcb::pid_for_pml4(pml4) else {
        // No live process owns this address space: a self-test's throwaway page
        // table, or a target that exited while we were copying. Nothing to ask.
        return false;
    };
    let present = page_table::translate(pml4, VirtAddr::new(va)).is_some();
    if present && !need_writable {
        // A present page we only want to read needs no resolution, and there is
        // none to be had — `try_resolve_fault` treats a present read fault as
        // unresolvable. Reaching here means the walk failed for a reason the
        // resolver cannot address.
        return false;
    }
    // x86 page-fault error code: bit 0 present, bit 1 write, bit 2 user.
    let mut error_code: u64 = 1 << 2;
    if need_writable {
        error_code |= 1 << 1;
    }
    if present {
        error_code |= 1;
    }
    crate::proc::pcb::try_resolve_fault(pid, va, error_code)
}

/// Copy `dst.len()` bytes from the user range starting at `user_src`, in the
/// address space rooted at `pml4`, into the kernel slice `dst`.
///
/// Unlike [`copy_from_user`], which accesses the *current* address space via
/// STAC/CLAC, this walks an explicit page table and reads each user page
/// through its HHDM physical mapping.  That makes it usable for
/// cross-address-space transfers (e.g. `vmsplice`, and a future
/// `process_vm_readv`) and lets a kernel-context self-test drive it against a
/// throwaway process's page table — the current (kernel) address space has no
/// user mappings.  Going through the HHDM also sidesteps SMAP entirely, so no
/// STAC/CLAC window is opened.
///
/// In a real syscall, pass the caller's PML4
/// (`cr3_to_pml4(read_cr3())`), which is exactly the address space being read.
///
/// A page that is absent but committed is populated first, exactly as a real
/// read fault by the owning process would have populated it — see
/// [`user_page_phys`].
///
/// # Errors
///
/// [`KernelError::InvalidAddress`] if any part of the range is null, wraps,
/// escapes user space, or is not mapped in `pml4` and cannot be resolved.
//
// Arithmetic here is bounded page-walk index math (page offsets are `<
// PAGE_SIZE`, `copied < len`); overflow is the failure condition, not a bug,
// and the boundary additions use `checked_add`.
#[allow(clippy::arithmetic_side_effects)]
pub fn copy_from_user_as(pml4: u64, user_src: u64, dst: &mut [u8]) -> KernelResult<()> {
    let len = dst.len();
    if len == 0 {
        return Ok(());
    }
    if user_src == 0 {
        return Err(KernelError::InvalidAddress);
    }
    let end = user_src
        .checked_add(len as u64)
        .ok_or(KernelError::InvalidAddress)?;
    if end > USER_SPACE_END {
        return Err(KernelError::InvalidAddress);
    }
    let hhdm = page_table::hhdm().ok_or(KernelError::InvalidAddress)?;

    let mut copied: usize = 0;
    let mut va = user_src;
    while copied < len {
        let page_off = va & (PAGE_SIZE - 1);
        let in_page = (PAGE_SIZE - page_off) as usize;
        let n = in_page.min(len - copied);
        let phys = user_page_phys(pml4, va, false)?;
        let kva = hhdm.checked_add(phys).ok_or(KernelError::InvalidAddress)?;
        // SAFETY: `phys` is a mapped physical address returned by translate();
        // the HHDM maps all physical memory, so `kva` is a valid readable
        // kernel pointer to `n` bytes that stay within a single 4 KiB page.
        let src = unsafe { core::slice::from_raw_parts(kva as *const u8, n) };
        let next = copied.checked_add(n).ok_or(KernelError::InvalidAddress)?;
        dst.get_mut(copied..next)
            .ok_or(KernelError::InvalidAddress)?
            .copy_from_slice(src);
        copied = next;
        va = va
            .checked_add(n as u64)
            .ok_or(KernelError::InvalidAddress)?;
    }
    Ok(())
}

/// Copy `src.len()` bytes from the kernel slice `src` into the user range
/// starting at `user_dst`, in the address space rooted at `pml4`.
///
/// The write-side counterpart of [`copy_from_user_as`]: every destination
/// page must be present *and* writable in `pml4`, and bytes are written
/// through the HHDM physical mapping (no STAC/CLAC window).
///
/// "Must be" in the sense of `get_user_pages(FOLL_WRITE)`, not in the sense of
/// a precondition the caller has to arrange: a destination page that is absent
/// but committed is populated, and one that is present-but-CoW has its CoW
/// broken, before the write — see [`user_page_phys`]. A target that has just
/// forked has an entirely CoW address space, so without that this would fail on
/// every page of the most ordinary case there is.
///
/// # Errors
///
/// [`KernelError::InvalidAddress`] if any part of the range is null, wraps,
/// escapes user space, or is not writable in `pml4` and cannot be made so
/// (an unmapped hole, or a genuinely read-only mapping).
//
// See `copy_from_user_as` for the arithmetic justification.
#[allow(clippy::arithmetic_side_effects)]
pub fn copy_to_user_as(pml4: u64, user_dst: u64, src: &[u8]) -> KernelResult<()> {
    let len = src.len();
    if len == 0 {
        return Ok(());
    }
    if user_dst == 0 {
        return Err(KernelError::InvalidAddress);
    }
    let end = user_dst
        .checked_add(len as u64)
        .ok_or(KernelError::InvalidAddress)?;
    if end > USER_SPACE_END {
        return Err(KernelError::InvalidAddress);
    }
    let hhdm = page_table::hhdm().ok_or(KernelError::InvalidAddress)?;

    let mut copied: usize = 0;
    let mut va = user_dst;
    while copied < len {
        let page_off = va & (PAGE_SIZE - 1);
        let in_page = (PAGE_SIZE - page_off) as usize;
        let n = in_page.min(len - copied);
        let phys = user_page_phys(pml4, va, true)?;
        let kva = hhdm.checked_add(phys).ok_or(KernelError::InvalidAddress)?;
        let next = copied.checked_add(n).ok_or(KernelError::InvalidAddress)?;
        let chunk = src.get(copied..next).ok_or(KernelError::InvalidAddress)?;
        // SAFETY: `phys` is a mapped, writable physical address (checked via
        // translate_flags); the HHDM maps all physical memory, so `kva` is a
        // valid writable kernel pointer to `n` bytes within a single 4 KiB
        // page.
        let out = unsafe { core::slice::from_raw_parts_mut(kva as *mut u8, n) };
        out.copy_from_slice(chunk);
        copied = next;
        va = va
            .checked_add(n as u64)
            .ok_or(KernelError::InvalidAddress)?;
    }
    Ok(())
}

/// Write a single value to user space.
///
/// The counterpart to [`read_user_value`], for handlers that report a scalar
/// through an out-pointer (`waitpid`'s status, a futex's previous value).
///
/// Like [`read_user_value`] this goes through the byte-wise copy path rather
/// than a typed `core::ptr::write`.  That is deliberate: `user_dst` comes from
/// userspace and carries no alignment guarantee, so a typed store through it
/// would be undefined behaviour for any `T` with an alignment above 1 — and
/// nothing about a syscall ABI stops a caller from passing an odd address.
///
/// # Errors
///
/// - [`KernelError::InvalidAddress`] if the user range is not writable.
pub fn write_user_value<T: Copy>(user_dst: u64, value: T) -> KernelResult<()> {
    // SAFETY: `value` is a live, fully initialised `T` in kernel memory, so
    // `size_of::<T>()` bytes starting at its address are readable.  `T: Copy`
    // rules out a `Drop` impl, so viewing it as bytes cannot duplicate an
    // owning handle.  `copy_to_user` validates the destination itself.
    unsafe {
        let src = core::ptr::addr_of!(value).cast::<u8>();
        copy_to_user(src, user_dst, core::mem::size_of::<T>())
    }
}

// ---------------------------------------------------------------------------
// Atomic access to a user word (futexes)
// ---------------------------------------------------------------------------
//
// Everything else in this module copies user memory into or out of kernel
// memory.  A futex word is the one thing that cannot be handled that way: the
// primitive *is* the atomicity of the read-modify-write against a concurrent
// userspace CAS, and copy-in / modify / copy-out is not atomic — it
// reintroduces exactly the lost update the futex exists to prevent.
//
// So these operate in place, bracketed by STAC/CLAC.  That is legitimate here
// and nowhere else in the kernel, for one reason: the window spans a *single
// non-blocking atomic instruction*.  None of the objections that rule STAC out
// for the handler paths apply — nothing inside can block, so AC cannot leak
// into a task's saved RFLAGS across a reschedule, and the window is a couple
// of cycles rather than unbounded.  Linux draws the same line
// (`arch/x86/include/asm/futex.h`, each op wrapped in
// `__uaccess_begin()`/`__uaccess_end()`).
//
// Callers must keep any retry loop *outside* these functions, one instruction
// per call — see `user_atomic_cas_u32`.

/// Validate `addr` as a 4-byte-aligned, writable user word and hand back a
/// reference to it as an `AtomicU32`.
///
/// # Safety
///
/// The returned reference aliases *user* memory. It may only be used inside a
/// STAC window, and it must not be held across anything that can block: a peer
/// thread may `munmap` the page, and an open window across a reschedule leaves
/// SMAP disabled for the task. Every caller in this module uses it for exactly
/// one atomic operation and drops it.
unsafe fn user_atomic_u32(addr: u64) -> KernelResult<&'static core::sync::atomic::AtomicU32> {
    // Alignment is a hard requirement, not a courtesy: a `lock`-prefixed RMW
    // that straddles a cache line is not guaranteed atomic on all
    // microarchitectures, and on some it is a split-lock that stalls every
    // core.  Rejecting here is also what makes the `AtomicU32` reference
    // below well-formed.
    if addr == 0 || addr & 3 != 0 {
        return Err(KernelError::BadAlignment);
    }
    // Writable, not merely readable: even the load-only callers want to be
    // sure the word is a real futex location the owning process could have
    // written, and requiring write permission keeps one rule for all ops.
    validate_user_write(addr, 4)?;
    // SAFETY: `addr` is 4-byte aligned (checked above) and validated as a
    // mapped, writable user address, so it is a valid place for a `u32`.
    // `AtomicU32` has the same size and alignment as `u32` and no validity
    // requirement beyond that, so every bit pattern at `addr` is a legal
    // value.  The `'static` lifetime is a lie the caller is responsible for
    // — hence this function's own safety contract.
    Ok(unsafe { &*(addr as *const core::sync::atomic::AtomicU32) })
}

/// Atomically load a user futex word.
///
/// # Errors
///
/// - [`KernelError::BadAlignment`] if `addr` is null or not 4-byte aligned.
/// - [`KernelError::InvalidAddress`] if the word is not a writable user
///   address.
pub fn user_atomic_load_u32(addr: u64) -> KernelResult<u32> {
    // SAFETY: the reference is used for one non-blocking atomic load inside
    // the STAC window and dropped, per `user_atomic_u32`'s contract.
    unsafe {
        let atomic = user_atomic_u32(addr)?;
        crate::smep_smap::stac();
        let v = atomic.load(core::sync::atomic::Ordering::Acquire);
        crate::smep_smap::clac();
        Ok(v)
    }
}

/// Atomically store to a user futex word.
///
/// # Errors
///
/// As [`user_atomic_load_u32`].
pub fn user_atomic_store_u32(addr: u64, value: u32) -> KernelResult<()> {
    // SAFETY: one non-blocking atomic store inside the window; see above.
    unsafe {
        let atomic = user_atomic_u32(addr)?;
        crate::smep_smap::stac();
        atomic.store(value, core::sync::atomic::Ordering::Release);
        crate::smep_smap::clac();
    }
    Ok(())
}

/// Atomically compare-and-exchange a user futex word.
///
/// Returns `Ok(Ok(current))` when the swap happened and `Ok(Err(actual))` when
/// it did not — the inner `Result` is `AtomicU32::compare_exchange`'s, so a
/// caller retries by looping on the *outside* of this call. Deliberately not a
/// `compare_exchange_weak` loop internally: keeping one instruction per call
/// is what bounds the STAC window.
///
/// # Errors
///
/// As [`user_atomic_load_u32`].
pub fn user_atomic_cas_u32(addr: u64, current: u32, new: u32) -> KernelResult<Result<u32, u32>> {
    // SAFETY: one non-blocking atomic RMW inside the window; see above.
    unsafe {
        let atomic = user_atomic_u32(addr)?;
        crate::smep_smap::stac();
        let r = atomic.compare_exchange(
            current,
            new,
            core::sync::atomic::Ordering::AcqRel,
            core::sync::atomic::Ordering::Acquire,
        );
        crate::smep_smap::clac();
        Ok(r)
    }
}

/// Which read-modify-write [`user_atomic_rmw_u32`] should perform.
///
/// Mirrors the `FUTEX_OP_*` selectors so the `FUTEX_WAKE_OP` dispatch can hand
/// one of these straight through instead of open-coding five near-identical
/// STAC windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAtomicOp {
    /// Unconditional store, returning the previous value (`FUTEX_OP_SET`).
    Set,
    /// Wrapping add (`FUTEX_OP_ADD`).
    Add,
    /// Bitwise OR (`FUTEX_OP_OR`).
    Or,
    /// Bitwise AND with the complement of the operand (`FUTEX_OP_ANDN`).
    AndN,
    /// Bitwise XOR (`FUTEX_OP_XOR`).
    Xor,
}

/// Atomically read-modify-write a user futex word, returning the *old* value.
///
/// # Errors
///
/// As [`user_atomic_load_u32`].
pub fn user_atomic_rmw_u32(addr: u64, op: UserAtomicOp, operand: u32) -> KernelResult<u32> {
    use core::sync::atomic::Ordering;
    // SAFETY: one non-blocking atomic RMW inside the window; see above.
    unsafe {
        let atomic = user_atomic_u32(addr)?;
        crate::smep_smap::stac();
        let old = match op {
            UserAtomicOp::Set => atomic.swap(operand, Ordering::AcqRel),
            UserAtomicOp::Add => atomic.fetch_add(operand, Ordering::AcqRel),
            UserAtomicOp::Or => atomic.fetch_or(operand, Ordering::AcqRel),
            UserAtomicOp::AndN => atomic.fetch_and(!operand, Ordering::AcqRel),
            UserAtomicOp::Xor => atomic.fetch_xor(operand, Ordering::AcqRel),
        };
        crate::smep_smap::clac();
        Ok(old)
    }
}

// ---------------------------------------------------------------------------
// Bounce buffers for syscall handlers
// ---------------------------------------------------------------------------
//
// The primitives above copy between a user address and a *caller-supplied*
// kernel buffer.  The ones below own the kernel buffer, which is what a syscall
// handler actually wants: a handler must never hand a user virtual address to a
// kernel subsystem.
//
// Two reasons, either sufficient on its own:
//
//   * **SMAP.**  A supervisor access to a user page outside a STAC window
//     faults.  Wrapping the subsystem call in `stac()`/`clac()` is not a fix,
//     because many of them block: AC lives in RFLAGS, which is saved and
//     restored across a context switch, so an open window would leave SMAP
//     disabled for that task *and* for the scheduler for as long as it sleeps.
//     The window has to stay inside a single non-blocking copy — which is
//     exactly what these do.
//
//   * **TOCTOU, independent of SMAP.**  A user slice held across a blocking
//     call is a use-after-free: another thread in the same process can `munmap`
//     or `mremap` the range while the caller sleeps, and the kernel then writes
//     through a stale mapping into whatever now owns that physical page.
//
// Both are structural, so the fix is structural: copy in, work on kernel
// memory, copy out.

/// Allocate a zeroed kernel byte buffer, reporting OOM instead of aborting.
///
/// `vec![0u8; n]` and `Vec::from(slice)` are *infallible* allocations: on
/// exhaustion they call the allocation-error handler, which in a kernel means
/// the whole system goes down.  Since `n` here is usually derived from a
/// syscall argument, that turns an OOM into a userspace-triggerable panic.
/// This returns [`KernelError::OutOfMemory`] instead.
///
/// This is the buffer to pack fixed-width output records into before handing
/// them to [`write_user_items`] or [`copy_to_user`]: size it by what the
/// kernel will actually emit rather than by the caller's advertised capacity,
/// so a huge `max_entries` cannot make the kernel allocate for it.
///
/// # Errors
///
/// - [`KernelError::OutOfMemory`] if the buffer cannot be allocated.
pub fn alloc_zeroed_vec(len: usize) -> KernelResult<alloc::vec::Vec<u8>> {
    let mut buf: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    if len == 0 {
        return Ok(buf);
    }
    buf.try_reserve_exact(len)
        .map_err(|_| KernelError::OutOfMemory)?;
    buf.resize(len, 0);
    Ok(buf)
}

/// Copy a bounded user byte range into a freshly allocated kernel buffer.
///
/// This is the replacement for `validate_user_read` followed by
/// `core::slice::from_raw_parts` over the user address.  The returned `Vec`
/// is kernel memory and may be held across a blocking call.
///
/// `max` is a hard limit, not a truncation: a `len` above it is an error.
/// Silently clamping (`len.min(256)`) is worse than useless for a path — it
/// turns "/very/long/path/to/a.txt" into a *different, shorter path* that may
/// well exist, so an over-long argument would operate on the wrong file
/// instead of being rejected.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `len > max`.
/// - [`KernelError::OutOfMemory`] if the kernel buffer cannot be allocated.
/// - [`KernelError::InvalidAddress`] if the user range is not readable.
pub fn read_user_vec(user_src: u64, len: usize, max: usize) -> KernelResult<alloc::vec::Vec<u8>> {
    if len > max {
        return Err(KernelError::InvalidArgument);
    }
    let mut buf: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    if len == 0 {
        return Ok(buf);
    }
    buf.try_reserve_exact(len)
        .map_err(|_| KernelError::OutOfMemory)?;
    buf.resize(len, 0);

    // SAFETY: `buf` was just allocated with exactly `len` bytes, so it is a
    // valid writable kernel destination of that size.  `copy_from_user`
    // validates the user source range itself.
    unsafe { copy_from_user(user_src, buf.as_mut_ptr(), len)? };
    Ok(buf)
}

/// Copy a NUL-terminated user string into a kernel buffer, without the NUL.
///
/// The counterpart to [`read_user_vec`] for arguments whose length is implied
/// by a terminator rather than passed alongside the pointer.
///
/// The naive version of this — validate one byte, then scan forward through
/// user memory for the NUL — is doubly wrong.  Validating the first byte says
/// nothing about the rest: an unterminated string at the end of a mapping
/// walks the scan straight into an unmapped page, and the resulting fault is
/// taken in *kernel* mode with no exception-table entry to recover from, so a
/// user process can panic the kernel with a single unterminated buffer.  And
/// the bytes are read from a live user mapping, which SMAP forbids and which a
/// peer thread can change between the scan and the use.
///
/// This copies forward in page-bounded chunks, so the scan can never touch a
/// page that was not validated, and stops at the first NUL.  The returned
/// `Vec` is kernel memory.
///
/// `max` is a hard limit on the string length excluding the terminator; a
/// string with no NUL in `[ptr, ptr + max]` is rejected rather than truncated,
/// for the reason given on [`read_user_vec`].
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if no NUL appears within `max` bytes.
/// - [`KernelError::OutOfMemory`] if the kernel buffer cannot be allocated.
/// - [`KernelError::InvalidAddress`] if the user range is not readable up to
///   and including the terminator.
pub fn read_user_cstr(user_src: u64, max: usize) -> KernelResult<alloc::vec::Vec<u8>> {
    // Small enough to sit on a kernel stack, and chunks are additionally
    // clipped to the end of the current page so one copy never spans two.
    const CHUNK: usize = 64;

    let mut out: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut addr = user_src;
    // The terminator itself may sit one past `max` bytes of content.
    let mut remaining = max.saturating_add(1);

    while remaining > 0 {
        let page_off = addr & (PAGE_SIZE - 1);
        // `PAGE_SIZE - page_off` is in `1..=PAGE_SIZE`, so this cast is exact.
        #[allow(clippy::cast_possible_truncation)]
        let to_page_end = (PAGE_SIZE - page_off) as usize;
        let n = remaining.min(CHUNK).min(to_page_end);

        let mut scratch = [0u8; CHUNK];
        // SAFETY: `scratch` is a live stack array of `CHUNK` bytes and
        // `n <= CHUNK`, so it is a valid destination for `n` bytes.
        // `copy_from_user` validates the source range before reading it, and
        // `n` never carries the read past the end of the current page.
        unsafe { copy_from_user(addr, scratch.as_mut_ptr(), n)? };

        let Some(piece) = scratch.get(..n) else {
            // Unreachable: `n <= CHUNK` by construction.
            return Err(KernelError::InvalidArgument);
        };

        let (content, done) = match piece.iter().position(|&b| b == 0) {
            Some(i) => (piece.get(..i).unwrap_or(&[]), true),
            None => (piece, false),
        };
        out.try_reserve(content.len())
            .map_err(|_| KernelError::OutOfMemory)?;
        out.extend_from_slice(content);
        if done {
            // At most `max + 1` bytes were scanned and the last of them was
            // the terminator, so `out.len() <= max` holds by construction.
            return Ok(out);
        }

        addr = addr.saturating_add(n as u64);
        remaining = remaining.saturating_sub(n);
    }

    // Ran out of budget without seeing a terminator.
    Err(KernelError::InvalidArgument)
}

/// Serve a user output buffer through kernel scratch memory.
///
/// Allocates a zeroed kernel buffer of `cap` bytes, hands it to `fill`, and
/// copies back however many bytes `fill` reports having written.  Returns that
/// count.
///
/// This is the replacement for `validate_user_write` followed by
/// `core::slice::from_raw_parts_mut` over the user address.  It is the shape to
/// use whenever the producer might block (`pipe::read`, `channel::recv`,
/// socket receive): `fill` only ever sees kernel memory, so there is no user
/// mapping to go stale and no STAC window to hold open.
///
/// The user range is validated as writable *before* `fill` runs, so a handler
/// still rejects a bad output pointer without having done the work — and
/// `fill`'s side effects (consuming from a pipe, dequeuing a message) are not
/// wasted on a copy that was going to fail anyway.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `cap > max`, or if `fill` reports
///   having written more than `cap` bytes.
/// - [`KernelError::OutOfMemory`] if the scratch buffer cannot be allocated.
/// - [`KernelError::InvalidAddress`] if the user range is not writable.
/// - Whatever `fill` returns.
pub fn with_user_out_buf<F>(user_dst: u64, cap: usize, max: usize, fill: F) -> KernelResult<usize>
where
    F: FnOnce(&mut [u8]) -> KernelResult<usize>,
{
    if cap > max {
        return Err(KernelError::InvalidArgument);
    }
    validate_user_write(user_dst, cap)?;

    let mut buf: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    if cap > 0 {
        buf.try_reserve_exact(cap)
            .map_err(|_| KernelError::OutOfMemory)?;
        buf.resize(cap, 0);
    }

    let written = fill(&mut buf)?;
    if written > cap {
        // A subsystem reporting more than it was given is a kernel bug, and
        // copying on that count would read past the scratch buffer.
        return Err(KernelError::InvalidArgument);
    }
    if written > 0 {
        // SAFETY: `buf` is a live kernel allocation of `cap >= written` bytes,
        // so the source range is valid.  `copy_to_user` re-validates the user
        // destination.
        unsafe { copy_to_user(buf.as_ptr(), user_dst, written)? };
    }
    Ok(written)
}

/// Copy a bounded array of `count` `T`s out of user space.
///
/// The `u8` case is [`read_user_vec`]; this is for the handful of handlers
/// that take a plain-old-data array (file-descriptor maps, iovecs).
///
/// `T` must be `Copy` and must have no padding whose value the kernel would
/// then act on — the bytes come from user space and are trusted only to the
/// extent that any bit pattern is a valid `T`.
///
/// # Errors
///
/// As [`read_user_vec`], plus [`KernelError::InvalidArgument`] if the total
/// byte size overflows `usize`.
pub fn read_user_items<T: Copy>(
    user_src: u64,
    count: usize,
    max_count: usize,
) -> KernelResult<alloc::vec::Vec<T>> {
    if count > max_count {
        return Err(KernelError::InvalidArgument);
    }
    let mut out: alloc::vec::Vec<T> = alloc::vec::Vec::new();
    if count == 0 {
        return Ok(out);
    }
    let bytes = count
        .checked_mul(core::mem::size_of::<T>())
        .ok_or(KernelError::InvalidArgument)?;

    out.try_reserve_exact(count)
        .map_err(|_| KernelError::OutOfMemory)?;

    // SAFETY: `out` has capacity for `count` elements, so its buffer is
    // `bytes` valid writable bytes.  `copy_from_user` validates the source and
    // fills every one of them, after which `count` elements are initialised —
    // `T: Copy` has no drop glue, so no destructor can observe the gap.
    unsafe {
        copy_from_user(user_src, out.as_mut_ptr().cast::<u8>(), bytes)?;
        out.set_len(count);
    }
    Ok(out)
}

/// Copy one `T` out of user space.
///
/// The common case of [`read_user_items`] with `count == 1`: a handler that
/// takes a pointer to a single packed argument struct.  Reading it through a
/// `*const T` instead would dereference user memory directly, which is exactly
/// what the bounce primitives exist to prevent.
///
/// `T` carries the same requirements as in [`read_user_items`]: every bit
/// pattern must be a valid `T`, because the bytes are attacker-controlled.
///
/// # Errors
///
/// - [`KernelError::InvalidAddress`] if the user range is not readable.
pub fn read_user_value<T: Copy>(user_src: u64) -> KernelResult<T> {
    let mut value = core::mem::MaybeUninit::<T>::uninit();

    // SAFETY: `value` is a live, properly aligned allocation of exactly
    // `size_of::<T>()` bytes.  `copy_from_user` validates the source and, on
    // success, fills all of them, so the value is fully initialised before
    // `assume_init` observes it.  On failure we return without reading it.
    unsafe {
        copy_from_user(
            user_src,
            value.as_mut_ptr().cast::<u8>(),
            core::mem::size_of::<T>(),
        )?;
        Ok(value.assume_init())
    }
}

/// Copy a slice of `T`s into user space.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the total byte size overflows.
/// - [`KernelError::InvalidAddress`] if the user range is not writable.
pub fn write_user_items<T: Copy>(user_dst: u64, items: &[T]) -> KernelResult<()> {
    if items.is_empty() {
        return Ok(());
    }
    let bytes = items
        .len()
        .checked_mul(core::mem::size_of::<T>())
        .ok_or(KernelError::InvalidArgument)?;

    // SAFETY: `items` is a live slice, so `items.as_ptr()` is `bytes` valid
    // readable bytes.  `copy_to_user` validates the user destination.
    unsafe { copy_to_user(items.as_ptr().cast::<u8>(), user_dst, bytes) }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run user memory validation self-tests.
///
/// These tests exercise `validate_user_range` directly (bypassing
/// the kernel-context shortcut) to verify the actual range and
/// page-table checks work correctly.
pub fn self_test() -> KernelResult<()> {
    // Test 1: Zero-length buffer is always valid.
    validate_user_range(0x1000, 0, false)?;

    // Test 2: Null pointer is invalid.
    match validate_user_range(0, 1, false) {
        Err(KernelError::InvalidAddress) => {} // Expected.
        other => {
            crate::serial_println!("[user]   FAIL: null should be invalid, got {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // Test 3: Kernel-space pointer is invalid.
    match validate_user_range(0xFFFF_8000_0000_0000, 1, false) {
        Err(KernelError::InvalidAddress) => {} // Expected.
        other => {
            crate::serial_println!(
                "[user]   FAIL: kernel addr should be invalid, got {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // Test 4: Wrapping pointer is invalid.
    match validate_user_range(u64::MAX - 10, 100, false) {
        Err(KernelError::InvalidAddress) => {} // Expected.
        other => {
            crate::serial_println!(
                "[user]   FAIL: wrapping range should be invalid, got {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // Test 5: Range crossing into kernel space is invalid.
    match validate_user_range(USER_SPACE_END - 10, 20, false) {
        Err(KernelError::InvalidAddress) => {} // Expected.
        other => {
            crate::serial_println!(
                "[user]   FAIL: cross-boundary range should be invalid, got {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // Test 6: Unmapped user-space address is invalid.
    // Address 0x1000 is in user space but almost certainly not mapped
    // for the kernel (idle) task which has no user mappings.
    match validate_user_range(0x1000, 1, false) {
        Err(KernelError::InvalidAddress) => {} // Expected.
        other => {
            crate::serial_println!(
                "[user]   FAIL: unmapped user addr should be invalid, got {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[user] User memory validation self-test PASSED");
    Ok(())
}

/// Self-test for cross-address-space fault resolution.
///
/// [`copy_to_user_as`] / [`copy_from_user_as`] reach into *another* process's
/// address space by hand, walking its page table through the HHDM. Nothing
/// faults during such a walk, so the two states that a hardware fault would
/// have fixed have to be fixed explicitly — and for a long time they were not,
/// which made `process_vm_writev` into a freshly-forked target fail on every
/// page even though the target could have written those pages itself.
///
/// The existing `process_vm` self-test could not have caught that: it
/// pre-faults its target page with an explicit
/// [`crate::proc::pcb::try_resolve_fault`] call before copying, and only ever
/// uses a plain writable anonymous page. It tests the transfer once the page is
/// already there. This one tests getting the page there:
///
/// 1. **Demand paging.** A committed VMA the target has never touched must be
///    populated by the copy itself, in both directions.
/// 2. **Read-only mappings still fail.** Resolution must not become a way to
///    write through a mapping the target itself could not write — tested both
///    while the page is absent (the VMA-permission check) and once it is
///    present (the not-a-CoW-page check).
/// 3. **Copy-on-write.** A genuinely shared page (refcount 2, present,
///    read-only, `COW` set) must be broken by the write, giving the target a
///    private frame carrying the copied contents — and leaving the address
///    space it was sharing with untouched. This is the case that matters most,
///    because immediately after `fork` it describes the *entire* address space.
///
/// ## Errors
///
/// [`KernelError::InternalError`] if any assertion fails, or the underlying
/// error if the scaffolding (process creation, VMA insertion, address-space
/// clone) could not be set up.
pub fn self_test_cross_as_resolution() -> KernelResult<()> {
    use crate::proc::pcb;

    crate::serial_println!("[user] Running cross-address-space fault-resolution self-test...");

    let target = pcb::create("user-crossas-target", 0);
    let result = cross_as_tests(target);
    pcb::destroy(target);

    result?;
    crate::serial_println!("[user] Cross-address-space fault-resolution self-test PASSED");
    Ok(())
}

/// Bases for the four scratch regions the cross-AS self-test maps into its
/// target. 192 GiB up, clear of every heap/mmap/stack window, and spaced
/// 256 KiB apart so no two can be coalesced into one VMA.
const CROSS_AS_BASE: u64 = 0x0000_0030_0000_0000;
const CROSS_AS_STRIDE: u64 = 0x0004_0000;

/// The 16 recognizable bytes the cross-AS self-test stamps into its
/// copy-on-write scratch page before forking the address space, kept as the two
/// 8-byte halves the test checks separately: the first is overwritten by the
/// poke that breaks the CoW, the second must survive it. A private copy comes
/// back carrying the second half; a fresh zero page would not.
const CROSS_AS_STAMP_HEAD: [u8; 8] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7];
/// Second half of the cross-AS self-test's stamp. See [`CROSS_AS_STAMP_HEAD`].
const CROSS_AS_STAMP_TAIL: [u8; 8] = [0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF];

/// Body of [`self_test_cross_as_resolution`], split out so the caller can
/// destroy the throwaway process on every exit path.
#[allow(clippy::too_many_lines)]
fn cross_as_tests(target: crate::proc::pcb::ProcessId) -> KernelResult<()> {
    use crate::mm::frame::FRAME_SIZE;
    use crate::mm::vma::{Vma, VmaKind};
    use crate::proc::pcb;

    let frame_size = FRAME_SIZE as u64;
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let Some(target_pml4) = pcb::get_pml4(target).filter(|&p| p != 0) else {
        crate::serial_println!("[user]   FAIL: target process has no PML4");
        return Err(KernelError::InternalError);
    };

    let rw = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_EXECUTE;
    let ro = PageFlags::PRESENT | PageFlags::USER_ACCESSIBLE | PageFlags::NO_EXECUTE;

    // Four one-frame regions: write-into-untouched, read-from-untouched,
    // read-only, and the copy-on-write case.
    let base_of = |i: u64| CROSS_AS_BASE.wrapping_add(i.wrapping_mul(CROSS_AS_STRIDE));
    for (i, flags) in [rw, rw, ro, rw].into_iter().enumerate() {
        let start = base_of(i as u64);
        pcb::add_vma(
            target,
            Vma {
                start,
                end: start.wrapping_add(frame_size),
                kind: VmaKind::Anonymous,
                flags,
            },
        )?;
    }

    // --- Test 1: writing into a committed page the target never touched ------
    // The VMA exists; no frame does. Before the fix this was a hard EFAULT,
    // even though the target writing the same address would have been served.
    let untouched_w = base_of(0);
    const POKE: [u8; 8] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    if page_table::translate(target_pml4, VirtAddr::new(untouched_w)).is_some() {
        crate::serial_println!("[user]   FAIL: scratch page was already mapped");
        return Err(KernelError::InternalError);
    }
    // Negative control. `user_page_phys_once` *is* the pre-fix code path, so
    // asserting it fails here is what makes the success below mean something:
    // without it, a page that happened to be mapped for some other reason would
    // let this test pass while resolution did nothing.
    if user_page_phys_once(target_pml4, untouched_w, true).is_ok() {
        crate::serial_println!("[user]   FAIL: unresolved walk succeeded on an absent page");
        return Err(KernelError::InternalError);
    }
    copy_to_user_as(target_pml4, untouched_w, &POKE).map_err(|e| {
        crate::serial_println!("[user]   FAIL: write to untouched page: {:?}", e);
        KernelError::InternalError
    })?;
    let Some(phys) = page_table::translate(target_pml4, VirtAddr::new(untouched_w)) else {
        crate::serial_println!("[user]   FAIL: page still absent after resolved write");
        return Err(KernelError::InternalError);
    };
    if read_frame_bytes(phys, hhdm, 8) != POKE {
        crate::serial_println!("[user]   FAIL: write to untouched page did not land");
        return Err(KernelError::InternalError);
    }

    // --- Test 2: reading a committed page the target never touched -----------
    // Demand paging an anonymous VMA yields a zero page, which is exactly what
    // the target would read. A resolver that refused would report EFAULT for a
    // perfectly valid address.
    let untouched_r = base_of(1);
    if user_page_phys_once(target_pml4, untouched_r, false).is_ok() {
        crate::serial_println!("[user]   FAIL: unresolved walk succeeded on an absent page");
        return Err(KernelError::InternalError);
    }
    // Pre-filled with 0xFF so a copy that silently did nothing cannot pass the
    // zero check below.
    let mut got = [0xFFu8; 8];
    copy_from_user_as(target_pml4, untouched_r, &mut got).map_err(|e| {
        crate::serial_println!("[user]   FAIL: read of untouched page: {:?}", e);
        KernelError::InternalError
    })?;
    if got != [0u8; 8] {
        crate::serial_println!("[user]   FAIL: demand-paged read gave {:x?}, want zeros", got);
        return Err(KernelError::InternalError);
    }

    // --- Test 3: a read-only mapping must stay read-only ---------------------
    // Resolution exists to do what a fault would have done — and a write fault
    // on a read-only VMA is not resolvable. Checked in both of its two forms.
    let readonly = base_of(2);
    // (a) Page absent: `try_resolve_fault`'s VMA permission check must refuse.
    if copy_to_user_as(target_pml4, readonly, &POKE).is_ok() {
        crate::serial_println!("[user]   FAIL: wrote through an absent read-only mapping");
        return Err(KernelError::InternalError);
    }
    // Reading it is fine, and makes it present for (b).
    copy_from_user_as(target_pml4, readonly, &mut got).map_err(|e| {
        crate::serial_println!("[user]   FAIL: read of read-only page: {:?}", e);
        KernelError::InternalError
    })?;
    // (b) Page present, read-only, and *not* CoW: `resolve_cow_fault` must
    // refuse. Without this the CoW arm would be a universal write override.
    if copy_to_user_as(target_pml4, readonly, &POKE).is_ok() {
        crate::serial_println!("[user]   FAIL: wrote through a present read-only mapping");
        return Err(KernelError::InternalError);
    }

    // --- Test 4: copy-on-write ------------------------------------------------
    let cow_va = base_of(3);
    // Populate and stamp 16 recognizable bytes, so the post-break check can
    // tell a real copy from a fresh zero page.
    if !pcb::try_resolve_fault(target, cow_va, 1 << 2) {
        crate::serial_println!("[user]   FAIL: could not populate the CoW scratch page");
        return Err(KernelError::InternalError);
    }
    let Some(orig_phys) = page_table::translate(target_pml4, VirtAddr::new(cow_va)) else {
        crate::serial_println!("[user]   FAIL: CoW scratch page absent after populate");
        return Err(KernelError::InternalError);
    };
    for (i, b) in CROSS_AS_STAMP_HEAD
        .iter()
        .chain(CROSS_AS_STAMP_TAIL.iter())
        .enumerate()
    {
        // SAFETY: HHDM alias of a freshly populated, target-owned, writable
        // frame; 16 bytes at its start are well within the 16 KiB frame.
        unsafe {
            (orig_phys.wrapping_add(hhdm).wrapping_add(i as u64) as *mut u8).write_volatile(*b);
        }
    }

    // Fork the address space for real: this is what makes the page shared.
    // SAFETY: `target_pml4` is a live PML4 owned by a process that has never
    // run — nothing can be mutating its page tables concurrently.
    let child_pml4 = unsafe { crate::mm::cow::clone_address_space_cow(target_pml4)? };
    let verdict = cross_as_cow_test(target_pml4, child_pml4, cow_va, orig_phys, hhdm);
    // SAFETY: `child_pml4` came from `clone_address_space_cow`, is loaded in no
    // CR3, and belongs to no process — nothing else can be using it.
    unsafe {
        page_table::destroy_user_address_space(child_pml4);
    }
    verdict
}

/// The copy-on-write half of the cross-AS self-test, split out so the caller
/// tears down the cloned address space whether it passes or fails.
///
/// `orig_phys` is the frame the target mapped at `cow_va` *before* the clone;
/// after the clone both address spaces share it, read-only and `COW`-marked.
fn cross_as_cow_test(
    target_pml4: u64,
    child_pml4: u64,
    cow_va: u64,
    orig_phys: u64,
    hhdm: u64,
) -> KernelResult<()> {
    use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};

    const POKE: [u8; 8] = [0xCA, 0xFE, 0xBA, 0xBE, 0x89, 0xAB, 0xCD, 0xEF];
    let virt = VirtAddr::new(cow_va);

    // Precondition: the clone really did produce a shared CoW page. If it did
    // not, everything below would pass for the wrong reason.
    let Some(flags) = page_table::translate_flags(target_pml4, virt) else {
        crate::serial_println!("[user]   FAIL: CoW page vanished from the target");
        return Err(KernelError::InternalError);
    };
    if !flags.contains(PageFlags::COW) || flags.contains(PageFlags::WRITABLE) {
        crate::serial_println!(
            "[user]   FAIL: post-fork page is not CoW (flags {:#x})",
            flags.bits()
        );
        return Err(KernelError::InternalError);
    }
    let frame_base = orig_phys & !(FRAME_SIZE as u64 - 1);
    let Some(shared) = PhysFrame::from_addr(frame_base) else {
        crate::serial_println!("[user]   FAIL: CoW frame address is not frame-aligned");
        return Err(KernelError::InternalError);
    };
    if frame::refcount(shared) < 2 {
        crate::serial_println!(
            "[user]   FAIL: post-fork refcount is {}, want >= 2",
            frame::refcount(shared)
        );
        return Err(KernelError::InternalError);
    }

    // Negative control, as in test 1: the unresolved walk — the pre-fix code
    // path — must fail on this page, so that the write succeeding below can
    // only be the CoW break doing its job.
    if user_page_phys_once(target_pml4, cow_va, true).is_ok() {
        crate::serial_println!("[user]   FAIL: unresolved walk succeeded on a CoW page");
        return Err(KernelError::InternalError);
    }

    // The write must break the CoW rather than report EFAULT. This is the case
    // that describes an entire address space immediately after `fork`.
    copy_to_user_as(target_pml4, cow_va, &POKE).map_err(|e| {
        crate::serial_println!("[user]   FAIL: write to a CoW page: {:?}", e);
        KernelError::InternalError
    })?;

    // The target must now own a *different*, writable frame.
    let Some(new_phys) = page_table::translate(target_pml4, virt) else {
        crate::serial_println!("[user]   FAIL: CoW page absent after break");
        return Err(KernelError::InternalError);
    };
    if new_phys == orig_phys {
        crate::serial_println!("[user]   FAIL: CoW break reused the shared frame");
        return Err(KernelError::InternalError);
    }
    match page_table::translate_flags(target_pml4, virt) {
        Some(f) if f.contains(PageFlags::WRITABLE) && !f.contains(PageFlags::COW) => {}
        other => {
            crate::serial_println!("[user]   FAIL: broken page still not writable: {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // The private copy carries the poke *and* the bytes that were not written —
    // a zero page would pass the first check and fail the second.
    let head = read_frame_bytes(new_phys, hhdm, 8);
    if head != POKE {
        crate::serial_println!("[user]   FAIL: poke missing from the private copy");
        return Err(KernelError::InternalError);
    }
    let tail = read_frame_bytes(new_phys.wrapping_add(8), hhdm, 8);
    if tail != CROSS_AS_STAMP_TAIL {
        crate::serial_println!(
            "[user]   FAIL: private copy lost the untouched bytes: {:x?} != {:x?}",
            tail,
            CROSS_AS_STAMP_TAIL
        );
        return Err(KernelError::InternalError);
    }

    // And the address space we forked from must not have seen the write — the
    // whole point of breaking the sharing rather than writing through it.
    if page_table::translate(child_pml4, virt) != Some(orig_phys) {
        crate::serial_println!("[user]   FAIL: the fork's mapping moved");
        return Err(KernelError::InternalError);
    }
    let sibling = read_frame_bytes(orig_phys, hhdm, 8);
    if sibling != CROSS_AS_STAMP_HEAD {
        crate::serial_println!(
            "[user]   FAIL: the fork's page was modified: {:x?} != {:x?}",
            sibling,
            CROSS_AS_STAMP_HEAD
        );
        return Err(KernelError::InternalError);
    }

    Ok(())
}

/// Read `n` (≤ 8) bytes from physical address `phys` through the HHDM.
///
/// Used only by the cross-AS self-test to inspect a target's frames without
/// going through the very copy routines under test.
fn read_frame_bytes(phys: u64, hhdm: u64, n: usize) -> [u8; 8] {
    let mut out = [0u8; 8];
    for (i, slot) in out.iter_mut().enumerate().take(n.min(8)) {
        // SAFETY: `phys` is a live frame the caller obtained from a successful
        // page-table translation, and the HHDM maps all of physical memory; the
        // at-most-8 bytes read start within that frame.
        *slot = unsafe {
            (phys.wrapping_add(hhdm).wrapping_add(i as u64) as *const u8).read_volatile()
        };
    }
    out
}
