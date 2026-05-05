//! Page fault resolution.
//!
//! When the CPU encounters a page fault (vector 14), the IDT handler
//! reads CR2 (faulting virtual address) and the error code, then calls
//! [`resolve`] to attempt resolution.
//!
//! ## Resolution Strategy
//!
//! 1. Look up the faulting address in the current address space's VMAs.
//! 2. If no VMA contains the address, the fault is fatal.
//! 3. Check permissions (error code vs VMA flags).
//! 4. Based on VMA kind:
//!    - **Anonymous / Stack**: allocate a frame, zero it, map it, retry.
//!    - **Guard**: always fatal (stack overflow).
//!    - **Fixed**: always fatal (PTE corruption).
//!
//! ## Performance Target
//!
//! < 10 us per page fault resolution.  Our 16 KiB pages zero 4x more
//! memory per fault than Linux's 4 KiB, so expect ~1.5-2x Linux per
//! individual fault but fewer total faults for sequential workloads.
//! See `bench/baselines.toml` for measured targets.
//!
//! ## Locking
//!
//! The kernel address space is protected by a spinlock.  The page
//! fault handler uses `try_lock()` — if the lock is already held
//! (meaning we faulted while modifying VMA state), the fault is
//! treated as fatal.  This prevents deadlocks.
//!
//! Lock ordering: `KERNEL_AS` → `PT_PAGE_POOL` → frame allocator.

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::mm::vma::{AddressSpace, Vma, VmaKind};
use crate::serial_println;
use crate::sync::Mutex;

// ---------------------------------------------------------------------------
// Page fault error code
// ---------------------------------------------------------------------------

/// Decoded `x86_64` page fault error code.
///
/// The CPU pushes this when delivering vector 14.  Each bit indicates
/// a property of the faulting access.
pub struct PageFaultError(u64);

impl PageFaultError {
    /// Create from the raw error code pushed by the CPU.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Bit 0: `true` if the fault was a protection violation on a
    /// present page.  `false` if the page was not present.
    #[must_use]
    pub const fn is_present(&self) -> bool {
        self.0 & 1 != 0
    }

    /// Bit 1: `true` if the fault was caused by a write.
    /// `false` if caused by a read.
    #[must_use]
    pub const fn is_write(&self) -> bool {
        self.0 & (1 << 1) != 0
    }

    /// Bit 2: `true` if the fault occurred in user mode (ring 3).
    /// `false` if in kernel mode (ring 0).
    #[must_use]
    #[allow(dead_code)] // Used by exception handler for fault classification.
    pub const fn is_user(&self) -> bool {
        self.0 & (1 << 2) != 0
    }

    /// Bit 3: `true` if the fault was caused by a reserved bit
    /// violation in a page table entry.  This is always a hardware
    /// or software bug — never resolvable.
    #[must_use]
    pub const fn is_reserved(&self) -> bool {
        self.0 & (1 << 3) != 0
    }

    /// Bit 4: `true` if the fault was caused by an instruction fetch.
    /// `false` if caused by a data access.
    #[must_use]
    pub const fn is_instruction_fetch(&self) -> bool {
        self.0 & (1 << 4) != 0
    }
}

// ---------------------------------------------------------------------------
// Global kernel address space
// ---------------------------------------------------------------------------

/// The kernel's virtual address space.
///
/// Initialized by [`init`]; used by [`resolve`] to look up kernel-
/// space page faults.  Protected by a spinlock.
///
/// Lock ordering: this lock is acquired BEFORE `PT_PAGE_POOL` and
/// the frame allocator lock (the demand-page path acquires those
/// while holding this lock).
static KERNEL_AS: Mutex<Option<AddressSpace>> = Mutex::named(None, b"KERNEL_AS");

/// Virtual address used for the demand-paging self-test.
///
/// Chosen to be well above the HHDM and kernel text regions, in
/// the kernel address space (upper canonical half), and separate
/// from the page table self-test address.
const DEMAND_PAGE_TEST_BASE: u64 = 0xFFFF_CA00_0000_0000;

/// Size of the demand-paging self-test VMA (one 16 KiB frame).
#[allow(clippy::arithmetic_side_effects)]
const DEMAND_PAGE_TEST_SIZE: u64 = FRAME_SIZE as u64;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the page fault subsystem.
///
/// Creates the kernel address space from the current CR3 register.
/// Must be called after the page table subsystem is initialized.
pub fn init() {
    let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());
    let kas = AddressSpace::new(pml4);
    *KERNEL_AS.lock() = Some(kas);
    serial_println!("[mm] Page fault subsystem initialized");
}

/// Attempt to resolve a page fault.
///
/// Called from the IDT page fault handler (vector 14).  Returns
/// `Ok(())` if the fault was resolved (the CPU should retry the
/// faulting instruction).
///
/// # Errors
///
/// Returns `Err(KernelError::PageFault)` if the fault is not
/// resolvable (no VMA, guard page, protection violation, reserved
/// bit, etc.).
pub fn resolve(fault_addr: u64, error_code: u64) -> KernelResult<()> {
    crate::ktrace::record(
        crate::ktrace::Category::Mm,
        crate::ktrace::event::PAGE_FAULT,
        fault_addr,
        error_code,
    );

    let error = PageFaultError::new(error_code);

    // Reserved-bit violations are hardware/software bugs, never
    // resolvable via demand paging.
    if error.is_reserved() {
        return Err(KernelError::PageFault);
    }

    let virt = VirtAddr::new(fault_addr);

    // Only handle kernel-space faults for now.  User-space fault
    // resolution will be added when process support is implemented.
    if virt.is_user() {
        return Err(KernelError::PageFault);
    }

    // Use try_lock to avoid deadlock: if we faulted while holding
    // this lock (e.g., during VMA manipulation), the fault is in
    // critical code and cannot be resolved.
    let guard = KERNEL_AS.try_lock().ok_or(KernelError::PageFault)?;
    let kas = guard.as_ref().ok_or(KernelError::PageFault)?;

    kas.resolve_fault(
        fault_addr,
        error.is_present(),
        error.is_write(),
        error.is_instruction_fetch(),
    )
}

/// Add a VMA to the kernel address space.
///
/// This is the public interface for kernel code that needs to
/// register demand-paged regions (e.g., kernel thread stacks,
/// large buffers).
///
/// # Errors
///
/// See [`AddressSpace::add_vma`].
pub fn add_kernel_vma(vma: Vma) -> KernelResult<()> {
    let mut guard = KERNEL_AS.lock();
    let kas = guard.as_mut().ok_or(KernelError::NotSupported)?;
    kas.add_vma(vma)
}

/// Remove a VMA from the kernel address space.
///
/// Returns the removed VMA, or `None` if no VMA starts at `start`.
pub fn remove_kernel_vma(start: u64) -> Option<Vma> {
    let mut guard = KERNEL_AS.lock();
    guard.as_mut().and_then(|kas| kas.remove_vma(start))
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run a boot-time self-test of the demand paging subsystem.
///
/// Tests:
/// 1. Add a demand-paged VMA to the kernel address space.
/// 2. Touch the memory (triggers a page fault).
/// 3. Verify the fault handler allocated a frame and mapped it.
/// 4. Write data and read it back.
/// 5. Clean up: unmap the frame, free it, remove the VMA.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[fault] Running demand paging self-test...");

    // -- Test 1: Register a demand-paged VMA ------------------------------------
    test_demand_page()?;

    serial_println!("[fault] Demand paging self-test PASSED");
    Ok(())
}

/// Test demand paging: register VMA, fault into it, verify, clean up.
#[allow(clippy::arithmetic_side_effects)]
fn test_demand_page() -> KernelResult<()> {
    let test_virt = VirtAddr::new(DEMAND_PAGE_TEST_BASE);
    let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());

    // Verify the test address is not already mapped.
    if page_table::translate(pml4, test_virt).is_some() {
        serial_println!(
            "[fault]   SKIP: test address {:#x} already mapped",
            DEMAND_PAGE_TEST_BASE
        );
        return Ok(());
    }

    // Add a demand-paged VMA for the test region.
    let vma = Vma {
        start: DEMAND_PAGE_TEST_BASE,
        end: DEMAND_PAGE_TEST_BASE + DEMAND_PAGE_TEST_SIZE,
        kind: VmaKind::Anonymous,
        flags: PageFlags::PRESENT
            | PageFlags::WRITABLE
            | PageFlags::GLOBAL
            | PageFlags::NO_EXECUTE,
    };
    add_kernel_vma(vma)?;
    serial_println!("[fault]   Registered demand-page VMA at {:#x}", DEMAND_PAGE_TEST_BASE);

    // Touch the memory.  This will trigger a page fault because no
    // physical frame is mapped yet.  The fault handler will:
    //   1. Find our VMA
    //   2. Allocate a frame
    //   3. Zero it
    //   4. Map it with our flags
    //   5. Flush the TLB
    //   6. Return Ok — the CPU retries the write instruction
    //
    // SAFETY: The address is in kernel space, within our VMA, and
    // the page fault handler will map it before the write completes.
    // We must use volatile to prevent the compiler from eliding
    // the write (it writes to an address with no prior mapping).
    unsafe {
        let ptr = DEMAND_PAGE_TEST_BASE as *mut u8;
        ptr.write_volatile(0xDD);
    }

    // If we get here, the page fault was resolved successfully.
    // Verify the mapping exists.
    let phys = page_table::translate(pml4, test_virt);
    if phys.is_none() {
        serial_println!("[fault]   FAIL: address not mapped after demand fault");
        remove_kernel_vma(DEMAND_PAGE_TEST_BASE);
        return Err(KernelError::InternalError);
    }
    serial_println!("[fault]   Demand page fault resolved: OK");

    // Read back the value we wrote.
    let readback = unsafe {
        let ptr = DEMAND_PAGE_TEST_BASE as *const u8;
        ptr.read_volatile()
    };
    if readback != 0xDD {
        serial_println!(
            "[fault]   FAIL: read back {:#x}, expected 0xDD",
            readback
        );
        remove_kernel_vma(DEMAND_PAGE_TEST_BASE);
        return Err(KernelError::InternalError);
    }
    serial_println!("[fault]   Write/read through demand-paged memory: OK");

    // Write across the full 16 KiB frame to verify all 4 hardware
    // pages are accessible.
    unsafe {
        let ptr = DEMAND_PAGE_TEST_BASE as *mut u8;
        for offset in (0..FRAME_SIZE).step_by(4096) {
            ptr.add(offset).write_volatile(0xEE);
        }
        for offset in (0..FRAME_SIZE).step_by(4096) {
            let val = ptr.add(offset).read_volatile();
            if val != 0xEE {
                serial_println!(
                    "[fault]   FAIL: page at offset {} reads {:#x}, expected 0xEE",
                    offset, val
                );
                remove_kernel_vma(DEMAND_PAGE_TEST_BASE);
                return Err(KernelError::InternalError);
            }
        }
    }
    serial_println!("[fault]   All 4 hardware pages accessible: OK");

    // -- Cleanup: unmap, free frame, remove VMA ---------------------------------
    //
    // SAFETY: We mapped this frame during the fault; we're the only
    // user.  After unmap + TLB flush, no references remain.
    let frame = unsafe {
        let f = page_table::unmap_frame(pml4, test_virt)?;
        page_table::flush_frame(test_virt);
        f
    };
    unsafe { frame::free_frame(frame)?; }
    remove_kernel_vma(DEMAND_PAGE_TEST_BASE);
    serial_println!("[fault]   Cleanup (unmap + free + remove VMA): OK");

    Ok(())
}
