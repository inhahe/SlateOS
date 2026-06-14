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
//!    process's page table.
//! 4. For write validation, every mapped page must have the WRITABLE
//!    flag set.
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

        // translate() returns None if the page is not mapped.
        if page_table::translate(pml4, virt).is_none() {
            return Err(KernelError::InvalidAddress);
        }

        // If write access is required, check the page flags.
        if need_writable {
            if let Some(flags) = page_flags(pml4, virt) {
                if !flags.contains(PageFlags::WRITABLE) {
                    return Err(KernelError::InvalidAddress);
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

/// Read a single value from user-space (SMAP-safe).
///
/// Validates the pointer, then reads a `T`-sized value.  This is the
/// preferred way to read individual syscall arguments from user buffers.
///
/// # Safety
///
/// The user pointer must be properly aligned for type `T`.
#[allow(dead_code)]
pub unsafe fn read_user<T: Copy>(user_ptr: u64) -> KernelResult<T> {
    let len = core::mem::size_of::<T>();
    validate_user_read(user_ptr, len)?;

    // SAFETY: Validated above.  STAC/CLAC for SMAP.
    let val = unsafe {
        crate::smep_smap::stac();
        let v = core::ptr::read(user_ptr as *const T);
        crate::smep_smap::clac();
        v
    };
    Ok(val)
}

/// Write a single value to user-space (SMAP-safe).
///
/// Validates the pointer is writable, then writes a `T`-sized value.
///
/// # Safety
///
/// The user pointer must be properly aligned for type `T`.
#[allow(dead_code)]
pub unsafe fn write_user<T: Copy>(user_ptr: u64, value: T) -> KernelResult<()> {
    let len = core::mem::size_of::<T>();
    validate_user_write(user_ptr, len)?;

    // SAFETY: Validated above.  STAC/CLAC for SMAP.
    unsafe {
        crate::smep_smap::stac();
        core::ptr::write(user_ptr as *mut T, value);
        crate::smep_smap::clac();
    }
    Ok(())
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
