//! Kernel stack allocator with hardware guard pages.
//!
//! Instead of using HHDM addresses for kernel task stacks (where overflow
//! silently corrupts adjacent memory), this module allocates stacks in a
//! dedicated virtual address region where each stack has an unmapped guard
//! page below it.  Any stack overflow triggers an immediate page fault on
//! the guard page, which the kernel's VMA system classifies as fatal.
//!
//! ## Layout (per stack slot)
//!
//! ```text
//! ┌──────────────────────────────────────────┐ ← slot_base + SLOT_SIZE
//! │         Stack (32 KiB, 2 frames)         │   Usable stack, grows down
//! │         Mapped, writable                 │
//! ├──────────────────────────────────────────┤ ← slot_base + GUARD_SIZE
//! │         Guard page (16 KiB, 1 frame)     │   Unmapped — fault on access
//! └──────────────────────────────────────────┘ ← slot_base
//! ```
//!
//! The guard page is registered as a [`VmaKind::Guard`] in the kernel
//! address space.  If the stack grows past its 32 KiB limit and touches
//! the guard page, the page fault handler recognizes the Guard VMA and
//! panics with a clear diagnostic.
//!
//! ## Virtual Address Region
//!
//! Uses `0xFFFF_C100_0000_0000` — well above the HHDM and below the
//! kernel text section.  With 48 KiB per slot, supports up to 4096
//! concurrent kernel stacks (192 MiB of virtual address space).
//!
//! ## Advantages Over Previous HHDM Stacks
//!
//! - **Immediate hardware detection**: overflow faults instantly instead
//!   of silently corrupting adjacent memory and only detecting it via
//!   canary check on next context switch.
//! - **Defense in depth**: the canary is retained as a secondary check.
//! - **No false positives**: a guard page fault is always a real overflow.

use crate::error::{KernelError, KernelResult};
use crate::mm::fault;
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::mm::vma::{Vma, VmaKind};
use crate::serial_println;
use crate::sync::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Base virtual address of the kernel stack region.
///
/// Chosen to be in the kernel half (PML4 entries 256–511), above the HHDM
/// (which starts at 0xFFFF_8000_0000_0000 and covers physical RAM), and
/// separate from test regions (0xFFFF_C800+) and kernel text.
const KSTACK_REGION_BASE: u64 = 0xFFFF_C100_0000_0000;

/// Number of frames per guard page (1 frame = 16 KiB).
const GUARD_FRAMES: usize = 1;

/// Size of the guard page in bytes.
#[allow(clippy::arithmetic_side_effects)]
const GUARD_SIZE: u64 = (GUARD_FRAMES * FRAME_SIZE) as u64;

/// Number of frames per stack (2 frames = 32 KiB).
const STACK_FRAMES: usize = 2;

/// Size of the stack in bytes.
#[allow(clippy::arithmetic_side_effects)]
pub const STACK_SIZE: u64 = (STACK_FRAMES * FRAME_SIZE) as u64;

/// Total size of one stack slot (guard + stack).
#[allow(clippy::arithmetic_side_effects)]
const SLOT_SIZE: u64 = GUARD_SIZE + STACK_SIZE;

/// Maximum number of kernel stacks (limits virtual address consumption).
/// 4096 stacks × 48 KiB = 192 MiB virtual.
const MAX_STACKS: usize = 4096;

/// Bitmap words needed to track MAX_STACKS slots (64 bits per word).
#[allow(clippy::arithmetic_side_effects)]
const BITMAP_WORDS: usize = (MAX_STACKS + 63) / 64;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Bitmap tracking which stack slots are allocated (1 = in use).
static ALLOCATOR: Mutex<KstackAllocator> = Mutex::named(KstackAllocator::new(), b"KSTACK");

/// Whether the kstack subsystem has been initialized.
static INITIALIZED: spin::Once<()> = spin::Once::new();

struct KstackAllocator {
    /// Bitmap: bit N set = slot N is allocated.
    bitmap: [u64; BITMAP_WORDS],
    /// Next slot index to try (for fast sequential allocation).
    hint: usize,
    /// Total allocated count (for diagnostics).
    count: usize,
}

impl KstackAllocator {
    const fn new() -> Self {
        Self {
            bitmap: [0u64; BITMAP_WORDS],
            hint: 0,
            count: 0,
        }
    }

    /// Find and mark the next free slot.  Returns slot index.
    fn alloc_slot(&mut self) -> Option<usize> {
        // Search from hint forward, wrapping around.
        for offset in 0..MAX_STACKS {
            #[allow(clippy::arithmetic_side_effects)]
            let slot = (self.hint + offset) % MAX_STACKS;
            #[allow(clippy::arithmetic_side_effects)]
            let word = slot / 64;
            #[allow(clippy::arithmetic_side_effects)]
            let bit = slot % 64;

            if let Some(w) = self.bitmap.get(word) {
                if *w & (1u64 << bit) == 0 {
                    // Found a free slot — mark it.
                    if let Some(w) = self.bitmap.get_mut(word) {
                        *w |= 1u64 << bit;
                    }
                    #[allow(clippy::arithmetic_side_effects)]
                    {
                        self.hint = (slot + 1) % MAX_STACKS;
                        self.count += 1;
                    }
                    return Some(slot);
                }
            }
        }
        None // All slots in use.
    }

    /// Free a previously allocated slot.
    fn free_slot(&mut self, slot: usize) {
        if slot >= MAX_STACKS {
            return;
        }
        #[allow(clippy::arithmetic_side_effects)]
        let word = slot / 64;
        #[allow(clippy::arithmetic_side_effects)]
        let bit = slot % 64;
        if let Some(w) = self.bitmap.get_mut(word) {
            *w &= !(1u64 << bit);
        }
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.count = self.count.saturating_sub(1);
        }
        // Reset hint to allow reuse of earlier slots.
        if slot < self.hint {
            self.hint = slot;
        }
    }

    #[allow(dead_code)] // Used by allocated_count() public API.
    fn count(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Information about an allocated kernel stack.
///
/// Returned by [`alloc`], consumed by [`free`].  Stores everything needed
/// to unmap and release the stack and its guard page.
#[derive(Debug, Clone, Copy)]
pub struct KstackInfo {
    /// Virtual address of the bottom of the usable stack (NOT the guard page).
    /// The stack grows downward from `stack_bottom + STACK_SIZE`.
    pub stack_bottom: u64,

    /// Virtual address of the top of the usable stack (first address above).
    /// This is where RSP starts.
    pub stack_top: u64,

    /// Physical address of the first frame of the stack.
    /// (Needed for free.)
    pub stack_phys: u64,

    /// Slot index in the allocator (for deallocation).
    pub(crate) slot: usize,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the kernel stack allocator.
///
/// Registers the guard-page VMAs with the kernel address space's page fault
/// handler.  Must be called after `mm::fault::init()`.
///
/// This only initializes the allocator state — individual guard VMAs are
/// registered on each `alloc()` call, since registering 4096 guard VMAs
/// upfront would waste kernel address space resources.
pub fn init() {
    INITIALIZED.call_once(|| {
        serial_println!("[kstack] Kernel stack allocator initialized");
        serial_println!(
            "[kstack]   Region: {:#x}..{:#x} ({} max stacks, {} KiB/slot)",
            KSTACK_REGION_BASE,
            KSTACK_REGION_BASE + (MAX_STACKS as u64) * SLOT_SIZE,
            MAX_STACKS,
            SLOT_SIZE / 1024,
        );
    });
}

/// Allocate a kernel stack with a guard page.
///
/// Returns a [`KstackInfo`] containing the stack virtual address range.
/// The guard page is mapped as a Guard VMA in the kernel address space,
/// while the stack frames are physically backed and mapped with RW
/// permissions.
///
/// # Errors
///
/// - [`KernelError::OutOfMemory`] if no free slots or physical frames.
/// - [`KernelError::NotSupported`] if the kstack subsystem is not initialized.
pub fn alloc() -> KernelResult<KstackInfo> {
    if INITIALIZED.get().is_none() {
        return Err(KernelError::NotSupported);
    }

    // Step 1: Claim a slot.
    let slot = {
        let mut allocator = ALLOCATOR.lock();
        allocator.alloc_slot().ok_or(KernelError::OutOfMemory)?
    };

    // Step 2: Compute virtual addresses for this slot.
    #[allow(clippy::arithmetic_side_effects)]
    let slot_base = KSTACK_REGION_BASE + (slot as u64) * SLOT_SIZE;
    #[allow(clippy::arithmetic_side_effects)]
    let guard_start = slot_base;
    #[allow(clippy::arithmetic_side_effects)]
    let stack_bottom = slot_base + GUARD_SIZE;
    #[allow(clippy::arithmetic_side_effects)]
    let stack_top = stack_bottom + STACK_SIZE;

    // Step 3: Register the guard page VMA (so the fault handler recognizes it).
    let guard_vma = Vma {
        start: guard_start,
        end: stack_bottom,
        kind: VmaKind::Guard,
        flags: PageFlags::empty(),
    };
    if let Err(e) = fault::add_kernel_vma(guard_vma) {
        // Failed to register VMA — free the slot and bail.
        ALLOCATOR.lock().free_slot(slot);
        return Err(e);
    }

    // Step 4: Allocate physical frames for the stack.
    let order = if STACK_FRAMES <= 1 {
        0
    } else {
        STACK_FRAMES.next_power_of_two().trailing_zeros() as usize
    };
    let phys_frame = match frame::alloc_order(order) {
        Ok(f) => f,
        Err(e) => {
            // Cleanup: remove guard VMA, free slot.
            fault::remove_kernel_vma(guard_start);
            ALLOCATOR.lock().free_slot(slot);
            return Err(e);
        }
    };
    let stack_phys = phys_frame.addr();

    // Step 5: Map the stack frames into the kernel page table.
    let pml4 = page_table::read_cr3();
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;

    // Map each frame at its corresponding virtual address.
    for i in 0..STACK_FRAMES {
        #[allow(clippy::arithmetic_side_effects)]
        let virt_offset = (i as u64) * (FRAME_SIZE as u64);
        #[allow(clippy::arithmetic_side_effects)]
        let phys_offset = (i as u64) * (FRAME_SIZE as u64);
        #[allow(clippy::arithmetic_side_effects)]
        let frame_virt = VirtAddr::new(stack_bottom + virt_offset);
        let frame_phys = PhysFrame::from_addr(stack_phys + phys_offset)
            .ok_or(KernelError::InvalidAddress)?;

        // SAFETY: We own these physical frames (just allocated them) and
        // the virtual addresses are in our dedicated region (not used by
        // anything else).  The PML4 is the active kernel page table.
        if let Err(e) = unsafe { page_table::map_frame(pml4, frame_virt, frame_phys, flags) } {
            // Partial failure: unmap what we've already mapped, free frames,
            // remove VMA, free slot.
            for j in 0..i {
                #[allow(clippy::arithmetic_side_effects)]
                let undo_virt = VirtAddr::new(stack_bottom + (j as u64) * (FRAME_SIZE as u64));
                // SAFETY: We just mapped these frames above.
                let _ = unsafe { page_table::unmap_frame(pml4, undo_virt) };
            }
            // SAFETY: We just allocated this frame.
            unsafe { let _ = frame::free_order(phys_frame, order); }
            fault::remove_kernel_vma(guard_start);
            ALLOCATOR.lock().free_slot(slot);
            return Err(e);
        }
    }

    // Step 6: Zero the stack memory via the mapped virtual address.
    // SAFETY: The stack region [stack_bottom..stack_top] is now mapped
    // and writable.  We own it exclusively.
    unsafe {
        core::ptr::write_bytes(stack_bottom as *mut u8, 0, STACK_SIZE as usize);
    }

    Ok(KstackInfo {
        stack_bottom,
        stack_top,
        stack_phys,
        slot,
    })
}

/// Free a kernel stack and its guard page.
///
/// Unmaps the stack frames, flushes TLB entries (all CPUs), frees the
/// physical memory, removes the guard VMA, and releases the slot.
///
/// # Safety
///
/// Caller must guarantee no CPU is currently using this stack (i.e., it
/// is not the current RSP on any processor).
pub unsafe fn free(info: KstackInfo) -> KernelResult<()> {
    let pml4 = page_table::read_cr3();

    // Unmap the stack frames.
    for i in 0..STACK_FRAMES {
        #[allow(clippy::arithmetic_side_effects)]
        let virt = VirtAddr::new(info.stack_bottom + (i as u64) * (FRAME_SIZE as u64));
        // SAFETY: Caller guarantees no CPU is using this stack.
        let _ = unsafe { page_table::unmap_frame(pml4, virt) };
    }

    // Flush TLB for the unmapped stack frames on all CPUs.
    // This is critical: without flushing, a subsequent re-allocation of
    // the same slot could write through stale TLB entries to freed memory,
    // causing silent corruption (or #GP if the freed frames were reused as
    // page tables).
    for i in 0..STACK_FRAMES {
        #[allow(clippy::arithmetic_side_effects)]
        let virt = VirtAddr::new(info.stack_bottom + (i as u64) * (FRAME_SIZE as u64));
        // SAFETY: invlpg is always safe in ring 0.  We use flush_frame
        // which handles SMP TLB shootdown if multiple CPUs are online.
        unsafe { page_table::flush_frame(virt); }
    }

    // Free the physical frames.
    let order = if STACK_FRAMES <= 1 {
        0
    } else {
        STACK_FRAMES.next_power_of_two().trailing_zeros() as usize
    };
    if let Some(frame) = PhysFrame::from_addr(info.stack_phys) {
        // SAFETY: Caller guarantees no CPU is using this stack.
        unsafe { frame::free_order(frame, order)?; }
    }

    // Remove the guard VMA.
    #[allow(clippy::arithmetic_side_effects)]
    let guard_start = KSTACK_REGION_BASE + (info.slot as u64) * SLOT_SIZE;
    fault::remove_kernel_vma(guard_start);

    // Release the slot.
    ALLOCATOR.lock().free_slot(info.slot);

    Ok(())
}

/// Check whether a virtual address falls within the kstack region.
///
/// Used by the page fault handler to give better diagnostics.
#[must_use]
pub fn is_kstack_region(addr: u64) -> bool {
    #[allow(clippy::arithmetic_side_effects)]
    let region_end = KSTACK_REGION_BASE + (MAX_STACKS as u64) * SLOT_SIZE;
    addr >= KSTACK_REGION_BASE && addr < region_end
}

/// Check whether a specific address is a guard page.
///
/// Returns `true` if `addr` falls in the guard portion of any allocated
/// stack slot.  Used for enhanced page fault diagnostics.
#[must_use]
pub fn is_guard_page(addr: u64) -> bool {
    if !is_kstack_region(addr) {
        return false;
    }
    #[allow(clippy::arithmetic_side_effects)]
    let offset = addr - KSTACK_REGION_BASE;
    #[allow(clippy::arithmetic_side_effects)]
    let within_slot = offset % SLOT_SIZE;
    within_slot < GUARD_SIZE
}

/// Return the number of currently allocated kernel stacks.
#[must_use]
#[allow(dead_code)] // Diagnostics API; used by kshell/vmstat when stack reporting is added.
pub fn allocated_count() -> usize {
    ALLOCATOR.lock().count()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test of the kernel stack allocator.
///
/// Tests:
/// 1. Allocate a stack, verify addresses are in the expected region.
/// 2. Write to the stack (verify it's accessible).
/// 3. Verify the guard page address detection works.
/// 4. Free the stack.
/// 5. Re-allocate (verify slot reuse).
pub fn self_test() -> KernelResult<()> {
    serial_println!("[kstack] Running self-test...");

    // Test 1: Allocate a stack.
    let info = alloc()?;
    serial_println!(
        "[kstack]   Allocated: bottom={:#x}, top={:#x}, slot={}",
        info.stack_bottom,
        info.stack_top,
        info.slot,
    );

    // Verify addresses are in the expected region.
    assert!(
        info.stack_bottom >= KSTACK_REGION_BASE,
        "stack_bottom below region base"
    );
    assert!(
        info.stack_top > info.stack_bottom,
        "stack_top <= stack_bottom"
    );
    assert!(
        is_kstack_region(info.stack_bottom),
        "stack_bottom not in kstack region"
    );
    #[allow(clippy::arithmetic_side_effects)]
    {
        assert_eq!(
            info.stack_top - info.stack_bottom,
            STACK_SIZE,
            "stack size mismatch"
        );
    }

    // Test 2: Write to the stack (should not fault).
    // Write at the bottom and top of the stack.
    unsafe {
        core::ptr::write_volatile(info.stack_bottom as *mut u64, 0xDEAD_BEEF);
        #[allow(clippy::arithmetic_side_effects)]
        let near_top = (info.stack_top - 8) as *mut u64;
        core::ptr::write_volatile(near_top, 0xCAFE_BABE);
        let v1 = core::ptr::read_volatile(info.stack_bottom as *const u64);
        let v2 = core::ptr::read_volatile(near_top as *const u64);
        assert_eq!(v1, 0xDEAD_BEEF, "stack bottom write/read mismatch");
        assert_eq!(v2, 0xCAFE_BABE, "stack top write/read mismatch");
    }
    serial_println!("[kstack]   Write/read: OK");

    // Test 3: Guard page detection.
    #[allow(clippy::arithmetic_side_effects)]
    let guard_addr = info.stack_bottom - 1; // One byte below stack = in guard page.
    assert!(
        is_guard_page(guard_addr),
        "guard page not detected at addr below stack"
    );
    assert!(
        !is_guard_page(info.stack_bottom),
        "stack_bottom falsely flagged as guard"
    );
    serial_println!("[kstack]   Guard page detection: OK");

    // Test 4: Free the stack.
    let slot = info.slot;
    // SAFETY: We're in a single-threaded self-test; no CPU is using this stack.
    unsafe { free(info)?; }
    serial_println!("[kstack]   Free: OK");

    // Test 5: Re-allocate — should reuse the same slot (hint reset).
    let info2 = alloc()?;
    assert_eq!(info2.slot, slot, "slot reuse failed");
    // SAFETY: Self-test, not in use.
    unsafe { free(info2)?; }
    serial_println!("[kstack]   Slot reuse: OK");

    // Test 6: Bulk allocation — verify unique slots.
    let mut infos = [KstackInfo {
        stack_bottom: 0,
        stack_top: 0,
        stack_phys: 0,
        slot: 0,
    }; 8];
    for info in infos.iter_mut() {
        *info = alloc()?;
    }
    // Verify all slots are distinct.
    for i in 0..infos.len() {
        for j in (i + 1)..infos.len() {
            assert_ne!(
                infos[i].slot, infos[j].slot,
                "duplicate slot: {} and {}",
                i, j
            );
            assert_ne!(
                infos[i].stack_bottom, infos[j].stack_bottom,
                "duplicate address: {} and {}",
                i, j
            );
        }
    }
    serial_println!("[kstack]   Bulk alloc (8 stacks, unique): OK");
    // Free all.
    for info in &infos {
        // SAFETY: Self-test, not in use.
        unsafe { free(*info)?; }
    }
    serial_println!("[kstack]   Bulk free: OK");

    serial_println!("[kstack] Self-test PASSED");
    Ok(())
}
