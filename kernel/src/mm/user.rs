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

use super::page_table::{self, PageFlags, VirtAddr, USER_SPACE_END};
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
    let end = ptr.checked_add(len_u64)
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
pub unsafe fn copy_from_user(
    user_src: u64,
    kernel_dst: *mut u8,
    len: usize,
) -> KernelResult<()> {
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
pub unsafe fn copy_to_user(
    kernel_src: *const u8,
    user_dst: u64,
    len: usize,
) -> KernelResult<()> {
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
fn user_page_phys(pml4: u64, va: u64, need_writable: bool) -> KernelResult<u64> {
    let virt = VirtAddr::new(va);
    if need_writable {
        let flags = page_table::translate_flags(pml4, virt).ok_or(KernelError::InvalidAddress)?;
        if !flags.contains(PageFlags::WRITABLE) {
            return Err(KernelError::InvalidAddress);
        }
    }
    page_table::translate(pml4, virt).ok_or(KernelError::InvalidAddress)
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
/// # Errors
///
/// [`KernelError::InvalidAddress`] if any part of the range is null, wraps,
/// escapes user space, or is not mapped in `pml4`.
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
        va = va.checked_add(n as u64).ok_or(KernelError::InvalidAddress)?;
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
/// # Errors
///
/// [`KernelError::InvalidAddress`] if any part of the range is null, wraps,
/// escapes user space, or is not mapped writable in `pml4`.
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
        let chunk = src
            .get(copied..next)
            .ok_or(KernelError::InvalidAddress)?;
        // SAFETY: `phys` is a mapped, writable physical address (checked via
        // translate_flags); the HHDM maps all physical memory, so `kva` is a
        // valid writable kernel pointer to `n` bytes within a single 4 KiB
        // page.
        let out = unsafe { core::slice::from_raw_parts_mut(kva as *mut u8, n) };
        out.copy_from_slice(chunk);
        copied = next;
        va = va.checked_add(n as u64).ok_or(KernelError::InvalidAddress)?;
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
            crate::serial_println!("[user]   FAIL: kernel addr should be invalid, got {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // Test 4: Wrapping pointer is invalid.
    match validate_user_range(u64::MAX - 10, 100, false) {
        Err(KernelError::InvalidAddress) => {} // Expected.
        other => {
            crate::serial_println!(
                "[user]   FAIL: wrapping range should be invalid, got {:?}", other
            );
            return Err(KernelError::InternalError);
        }
    }

    // Test 5: Range crossing into kernel space is invalid.
    match validate_user_range(USER_SPACE_END - 10, 20, false) {
        Err(KernelError::InvalidAddress) => {} // Expected.
        other => {
            crate::serial_println!(
                "[user]   FAIL: cross-boundary range should be invalid, got {:?}", other
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
                "[user]   FAIL: unmapped user addr should be invalid, got {:?}", other
            );
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[user] User memory validation self-test PASSED");
    Ok(())
}
