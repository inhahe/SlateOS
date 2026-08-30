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
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Page fault statistics
// ---------------------------------------------------------------------------

/// Kernel-space page faults resolved successfully (demand page).
static KERNEL_FAULTS_RESOLVED: AtomicU64 = AtomicU64::new(0);

/// User-space page faults resolved successfully (demand page, stack growth,
/// swap-in, CoW).
static USER_FAULTS_RESOLVED: AtomicU64 = AtomicU64::new(0);

/// Page faults that were unresolvable *and* unhandled — they killed the task
/// or halted the machine.
///
/// A fault the kernel cannot resolve is not yet fatal: it is offered to the
/// faulting program first (a Linux `SIGSEGV` handler, then a native SEH
/// handler), and a program that takes it goes on running.  Only a fault
/// nothing was willing to take is counted here.  See [`FAULTS_DELIVERED`] for
/// the other outcome, and `idt::dispatch_or_kill_userspace_raw` for the site
/// that draws the line.
static FAULTS_FATAL: AtomicU64 = AtomicU64::new(0);

/// Faults the kernel could not resolve but userspace *did* — handed to a Linux
/// signal handler or a native SEH handler, after which the program continued.
///
/// Tracked apart from the buckets on either side of it, because it is neither:
/// not a resolution (the memory access never succeeded) and not a death (the
/// program survived).  Folding it into either one erases the distinction that
/// matters — a program with a working fault handler would look identical to
/// one that crashed.
static FAULTS_DELIVERED: AtomicU64 = AtomicU64::new(0);

/// Copy-on-Write faults resolved.
static COW_FAULTS: AtomicU64 = AtomicU64::new(0);

/// Swap-in faults (page restored from swap).
static SWAP_IN_FAULTS: AtomicU64 = AtomicU64::new(0);

/// Stack growth faults (guard page triggered new stack mapping).
static STACK_GROWTH_FAULTS: AtomicU64 = AtomicU64::new(0);

/// Page fault statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct PageFaultStats {
    /// Kernel-mode faults resolved via demand paging.
    pub kernel_resolved: u64,
    /// User-mode faults resolved (any mechanism).
    pub user_resolved: u64,
    /// Faults that killed the task or halted the machine — unresolvable by the
    /// kernel *and* unhandled by the program.
    pub fatal: u64,
    /// Faults the kernel could not resolve but that a userspace handler took,
    /// after which the program continued.
    pub delivered: u64,
    /// Copy-on-Write resolutions.
    pub cow: u64,
    /// Swap-in resolutions.
    pub swap_in: u64,
    /// Stack growth resolutions.
    pub stack_growth: u64,
}

/// Get current page fault statistics.
#[must_use]
pub fn fault_stats() -> PageFaultStats {
    PageFaultStats {
        kernel_resolved: KERNEL_FAULTS_RESOLVED.load(Ordering::Relaxed),
        user_resolved: USER_FAULTS_RESOLVED.load(Ordering::Relaxed),
        fatal: FAULTS_FATAL.load(Ordering::Relaxed),
        delivered: FAULTS_DELIVERED.load(Ordering::Relaxed),
        cow: COW_FAULTS.load(Ordering::Relaxed),
        swap_in: SWAP_IN_FAULTS.load(Ordering::Relaxed),
        stack_growth: STACK_GROWTH_FAULTS.load(Ordering::Relaxed),
    }
}

/// Record a successful kernel-space fault resolution.
pub(crate) fn record_kernel_resolved() {
    KERNEL_FAULTS_RESOLVED.fetch_add(1, Ordering::Relaxed);
}

/// Record a successful user-space fault resolution.
pub(crate) fn record_user_resolved() {
    USER_FAULTS_RESOLVED.fetch_add(1, Ordering::Relaxed);
}

/// Record a page fault that killed the task or halted the machine.
///
/// Call this only once the fault is *known* to be unhandled — after every
/// delivery attempt has failed — never on entry to the unresolvable path.
pub(crate) fn record_fatal() {
    FAULTS_FATAL.fetch_add(1, Ordering::Relaxed);
}

/// Record a page fault handed to a userspace fault handler.
pub(crate) fn record_delivered() {
    FAULTS_DELIVERED.fetch_add(1, Ordering::Relaxed);
}

/// Record a Copy-on-Write fault resolution.
pub(crate) fn record_cow() {
    COW_FAULTS.fetch_add(1, Ordering::Relaxed);
}

/// Record a swap-in page fault resolution.
pub(crate) fn record_swap_in() {
    SWAP_IN_FAULTS.fetch_add(1, Ordering::Relaxed);
}

/// Record a stack growth fault resolution.
pub(crate) fn record_stack_growth() {
    STACK_GROWTH_FAULTS.fetch_add(1, Ordering::Relaxed);
}

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
    let _prof_t = crate::kprofile::begin(crate::kprofile::Slot::PageFault);

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

    // This resolver handles kernel-space faults only.  User-space faults
    // are resolved on a separate path in the #PF handler (idt.rs):
    // `proc::pcb::try_resolve_fault` (per-process address space, CoW,
    // demand paging) and `try_grow_user_stack` (auto-growing stacks).
    // Returning here lets that path take over.
    if virt.is_user() {
        return Err(KernelError::PageFault);
    }

    // Use try_lock to avoid deadlock: if we faulted while holding
    // this lock (e.g., during VMA manipulation), the fault is in
    // critical code and cannot be resolved.
    let guard = KERNEL_AS.try_lock().ok_or(KernelError::PageFault)?;
    let kas = guard.as_ref().ok_or(KernelError::PageFault)?;

    let result = kas.resolve_fault(
        fault_addr,
        error.is_present(),
        error.is_write(),
        error.is_instruction_fetch(),
    );

    if result.is_ok() {
        record_kernel_resolved();
    }

    crate::kprofile::end(crate::kprofile::Slot::PageFault, _prof_t);
    result
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
// Active probe / self-test
// ---------------------------------------------------------------------------

/// Where an active demand-paging probe went wrong.
///
/// Each variant names the *stage* that failed, so a caller which cannot print
/// a running commentary — `syshealth`, which has one line per check — can
/// still say something more useful than "failed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemandPageFailure {
    /// The probe address was already mapped, so the probe could not run.
    ///
    /// Not a skip: nothing else may use this address, so it being mapped means
    /// either a previous probe leaked its frame or something is squatting on a
    /// reserved kernel address.  Both are bugs, and reporting "pass" for a test
    /// that never ran would hide them.
    AddressBusy,
    /// The kernel address space refused the probe VMA.
    VmaRejected,
    /// The fault handler returned but left the address unmapped.
    NotMapped,
    /// The byte written through the fault did not read back.
    ReadbackMismatch,
    /// One of the four 4 KiB hardware pages of the 16 KiB frame misbehaved.
    PageUnreadable,
    /// The probe worked but could not undo itself, so it leaked.
    CleanupFailed,
}

impl DemandPageFailure {
    /// A short human-readable description of the failed stage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AddressBusy => "probe address already mapped (leaked frame?)",
            Self::VmaRejected => "kernel address space refused the probe VMA",
            Self::NotMapped => "fault returned but left the address unmapped",
            Self::ReadbackMismatch => "wrote a byte through the fault, read back garbage",
            Self::PageUnreadable => "a 4 KiB page of the 16 KiB frame is unreadable",
            Self::CleanupFailed => "probe leaked — unmap/free/remove failed",
        }
    }
}

/// Actively exercise demand paging end to end, and clean up after itself.
///
/// Registers an anonymous VMA at a reserved kernel address that nothing else
/// uses, writes to it — which *must* take a page fault, because no frame is
/// mapped yet — then verifies the handler allocated, zeroed and mapped a full
/// 16 KiB frame, and finally unmaps and frees it again.
///
/// Unlike reading `fault_stats()`, this proves the resolver works *now*.  It is
/// deliberately silent and repeatable so that `syshealth` can run it on demand
/// as often as the operator likes; [`self_test`] wraps it with boot logging.
///
/// # Errors
///
/// Returns the stage that failed.  Every path that returns an error also
/// releases whatever the probe had acquired by that point, so a failure does
/// not poison the next run.
#[allow(clippy::arithmetic_side_effects)]
pub fn probe_demand_page() -> Result<(), DemandPageFailure> {
    let test_virt = VirtAddr::new(DEMAND_PAGE_TEST_BASE);
    let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());

    if page_table::translate(pml4, test_virt).is_some() {
        return Err(DemandPageFailure::AddressBusy);
    }

    // Add a demand-paged VMA for the probe region.
    let vma = Vma {
        start: DEMAND_PAGE_TEST_BASE,
        end: DEMAND_PAGE_TEST_BASE + DEMAND_PAGE_TEST_SIZE,
        kind: VmaKind::Anonymous,
        flags: PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::GLOBAL | PageFlags::NO_EXECUTE,
    };
    add_kernel_vma(vma).map_err(|_| DemandPageFailure::VmaRejected)?;

    let outcome = run_probe_body(pml4, test_virt);

    // Undo the probe on every path.  The VMA always goes; the frame only
    // exists if the fault actually mapped one, which `unmap_probe_frame`
    // establishes by translating first.
    //
    // The old boot-only version removed the VMA but *not* the frame on its
    // failure paths.  That was invisible while this ran once per boot; the
    // moment `syshealth` could run it repeatedly, one failure would have
    // leaked a frame and made every later run fail with `AddressBusy` —
    // a real bug reported as a different, fictitious one.
    let cleanup = unmap_probe_frame(pml4, test_virt);
    remove_kernel_vma(DEMAND_PAGE_TEST_BASE);

    outcome?;
    cleanup
}

/// The probe's actual assertions, run with the VMA registered.
///
/// Split out so the caller can clean up on every exit path without a
/// `goto`-shaped tangle of repeated teardown before each `return`.
#[allow(clippy::arithmetic_side_effects)]
fn run_probe_body(pml4: u64, test_virt: VirtAddr) -> Result<(), DemandPageFailure> {
    // Touch the memory.  This must trigger a page fault, because no physical
    // frame is mapped yet.  The fault handler will:
    //   1. Find our VMA
    //   2. Allocate a frame
    //   3. Zero it
    //   4. Map it with our flags
    //   5. Flush the TLB
    //   6. Return Ok — the CPU retries the write instruction
    //
    // SAFETY: The address is in kernel space, within our VMA, and the page
    // fault handler will map it before the write completes.  Volatile so the
    // compiler cannot elide a write to an address with no prior mapping.
    unsafe {
        let ptr = DEMAND_PAGE_TEST_BASE as *mut u8;
        ptr.write_volatile(0xDD);
    }

    // Reaching here means the fault was resolved.  Verify the mapping exists.
    if page_table::translate(pml4, test_virt).is_none() {
        return Err(DemandPageFailure::NotMapped);
    }

    // Read back the value written through the fault.
    // SAFETY: DEMAND_PAGE_TEST_BASE was just demand-faulted and is mapped.
    let readback = unsafe {
        let ptr = DEMAND_PAGE_TEST_BASE as *const u8;
        ptr.read_volatile()
    };
    if readback != 0xDD {
        return Err(DemandPageFailure::ReadbackMismatch);
    }

    // Write across the full 16 KiB frame to verify all 4 hardware pages are
    // accessible — our page size is 16 KiB, so a resolver that mapped only the
    // first 4 KiB would pass every check above.
    // SAFETY: DEMAND_PAGE_TEST_BASE is mapped to a full 16 KiB frame.
    unsafe {
        let ptr = DEMAND_PAGE_TEST_BASE as *mut u8;
        for offset in (0..FRAME_SIZE).step_by(4096) {
            ptr.add(offset).write_volatile(0xEE);
        }
        for offset in (0..FRAME_SIZE).step_by(4096) {
            if ptr.add(offset).read_volatile() != 0xEE {
                return Err(DemandPageFailure::PageUnreadable);
            }
        }
    }

    Ok(())
}

/// Unmap and free the probe frame, if the fault got as far as mapping one.
fn unmap_probe_frame(pml4: u64, test_virt: VirtAddr) -> Result<(), DemandPageFailure> {
    if page_table::translate(pml4, test_virt).is_none() {
        // Never mapped — nothing to release, and that is not a cleanup failure.
        return Ok(());
    }

    // SAFETY: the fault handler mapped this frame for us and we are its only
    // user.  After unmap + TLB flush no references to it remain.
    let frame = unsafe {
        let f = page_table::unmap_frame(pml4, test_virt)
            .map_err(|_| DemandPageFailure::CleanupFailed)?;
        page_table::flush_frame(test_virt);
        f
    };
    // SAFETY: `frame` was just unmapped and flushed; nothing can reach it.
    unsafe { frame::free_frame(frame) }.map_err(|_| DemandPageFailure::CleanupFailed)
}

/// Run a boot-time self-test of the demand paging subsystem.
///
/// A thin logging wrapper over [`probe_demand_page`].
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if the probe failed at any stage.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[fault] Running demand paging self-test...");

    match probe_demand_page() {
        Ok(()) => {
            serial_println!(
                "[fault]   Demand page probe (VMA -> fault -> 16 KiB R/W -> cleanup): OK"
            );
        }
        Err(e) => {
            serial_println!("[fault]   FAIL: {}", e.as_str());
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[fault] Demand paging self-test PASSED");
    Ok(())
}
