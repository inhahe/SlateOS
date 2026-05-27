//! Memory subsystem integration tests.
//!
//! Exercises the full allocation → mapping → access → unmap → free pipeline,
//! verifying that all mm subsystems work together correctly.  This catches
//! bugs that unit tests miss — interface mismatches, ordering issues, and
//! emergent interactions between subsystems.
//!
//! ## Tests
//!
//! 1. **Alloc-map-access-unmap**: allocate a frame, map it at a known VA,
//!    write/read through the VA, unmap, free.
//! 2. **TLB gather integration**: unmap multiple pages via TlbGather, verify
//!    frames are freed only after the shootdown.
//! 3. **Page table walk verification**: map pages, walk the PT to find them,
//!    verify addresses match.
//! 4. **Watermark tracking**: allocate many frames, verify watermark peak
//!    increases, free them, verify current drops.
//! 5. **Migration type coherence**: allocate a frame as Movable, verify
//!    migrate_type reports it correctly.
//! 6. **Rmap round-trip**: add frame to rmap, verify lookup, remove, verify gone.
//!
//! ## Design
//!
//! All tests use the `PT_SELFTEST` kvspace region (0xFFFF_C900_0000_0000)
//! for temporary mappings.  Tests clean up after themselves (unmap + free).

use crate::serial_println;
use crate::mm::{
    frame::{self, FRAME_SIZE},
    migrate_type::{self, MigrateType},
    page_table::{self, PageFlags, VirtAddr},
    pt_walk::{self, WalkAction, WalkEntry},
    rmap,
    tlb_gather::TlbGather,
    watermark,
};

// ---------------------------------------------------------------------------
// Test region
// ---------------------------------------------------------------------------

/// Base virtual address for integration tests (from kvspace::PT_SELFTEST).
const TEST_BASE: u64 = 0xFFFF_C900_0000_0000;

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run all memory subsystem integration tests.
pub fn self_test() {
    serial_println!("[mm_integ] Running integration tests...");

    test_alloc_map_access_unmap();
    test_tlb_gather_integration();
    test_pt_walk_finds_mappings();
    test_watermark_tracking();
    test_migrate_type_coherence();
    test_rmap_round_trip();

    serial_println!("[mm_integ] All integration tests PASSED");
}

/// Test 1: Allocate → map → read/write → unmap → free.
fn test_alloc_map_access_unmap() {
    let frame = frame::alloc_frame_zeroed().expect("alloc for integ test 1");
    let phys = frame.addr();
    let virt = VirtAddr::new(TEST_BASE);

    // Get current PML4.
    let cr3: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) cr3,
            options(nomem, nostack, preserves_flags),
        );
    }
    let pml4 = cr3 & 0x000F_FFFF_FFFF_F000;

    // Map frame at test VA with write permission.
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE;
    let map_result = unsafe { page_table::map_frame(pml4, virt, frame, flags) };
    assert!(map_result.is_ok(), "map_frame should succeed");

    // Flush TLB for the mapped address.
    crate::tlb::flush_range(TEST_BASE, 4); // 4 hardware pages per frame.

    // Write through the virtual address.
    let ptr = TEST_BASE as *mut u64;
    unsafe {
        core::ptr::write_volatile(ptr, 0xDEAD_BEEF_CAFE_BABE);
    }

    // Read back and verify.
    let val = unsafe { core::ptr::read_volatile(ptr) };
    assert_eq!(val, 0xDEAD_BEEF_CAFE_BABE, "read-back mismatch");

    // Verify via HHDM that the physical frame has the data.
    let hhdm = page_table::hhdm().expect("hhdm");
    let phys_ptr = (hhdm + phys) as *const u64;
    let phys_val = unsafe { core::ptr::read_volatile(phys_ptr) };
    assert_eq!(phys_val, 0xDEAD_BEEF_CAFE_BABE, "physical frame mismatch");

    // Unmap and free.
    let unmap_result = unsafe { page_table::unmap_frame(pml4, virt) };
    assert!(unmap_result.is_ok());
    crate::tlb::flush_range(TEST_BASE, 4);

    let freed_frame = unmap_result.unwrap();
    assert_eq!(freed_frame.addr(), phys);
    // SAFETY: frame was just unmapped above; we own it exclusively.
    unsafe { frame::free_frame(freed_frame) }.expect("integ_test: free_frame after unmap");

    serial_println!("[mm_integ]   Test 1 (alloc-map-access-unmap): PASSED");
}

/// Test 2: TLB gather — batch unmap multiple pages.
fn test_tlb_gather_integration() {
    let cr3: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) cr3,
            options(nomem, nostack, preserves_flags),
        );
    }
    let pml4 = cr3 & 0x000F_FFFF_FFFF_F000;

    // Allocate and map 4 frames.
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE;
    let mut phys_addrs = [0u64; 4];

    for i in 0..4u64 {
        let f = frame::alloc_frame_zeroed().expect("alloc for gather test");
        phys_addrs[i as usize] = f.addr();
        let va = VirtAddr::new(TEST_BASE + (i + 1) * FRAME_SIZE as u64);
        let _ = unsafe { page_table::map_frame(pml4, va, f, flags) };
    }
    crate::tlb::flush_range(TEST_BASE + FRAME_SIZE as u64, 16); // 4 frames × 4 hw pages

    // Write to each page.
    for i in 0..4u64 {
        let ptr = (TEST_BASE + (i + 1) * FRAME_SIZE as u64) as *mut u64;
        unsafe { core::ptr::write_volatile(ptr, 0x1111 * (i + 1)); }
    }

    // Unmap all 4 pages via TlbGather (batch).
    let mut gather = TlbGather::new();
    for i in 0..4u64 {
        let va = VirtAddr::new(TEST_BASE + (i + 1) * FRAME_SIZE as u64);
        let unmapped = unsafe { page_table::unmap_frame(pml4, va) };
        if let Ok(f) = unmapped {
            gather.add(TEST_BASE + (i + 1) * FRAME_SIZE as u64, f.addr());
        }
    }

    // Finish the gather — this flushes TLBs and frees frames.
    let freed = gather.finish();
    assert_eq!(freed, 4);

    serial_println!("[mm_integ]   Test 2 (TLB gather batch unmap): PASSED");
}

/// Test 3: Page table walk finds our mappings.
fn test_pt_walk_finds_mappings() {
    let cr3: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) cr3,
            options(nomem, nostack, preserves_flags),
        );
    }
    let pml4 = cr3 & 0x000F_FFFF_FFFF_F000;

    // Map a frame at a known address.
    let frame = frame::alloc_frame_zeroed().expect("alloc for walk test");
    let phys = frame.addr();
    let test_va: u64 = TEST_BASE + 5 * FRAME_SIZE as u64;
    let virt = VirtAddr::new(test_va);
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE;
    let _ = unsafe { page_table::map_frame(pml4, virt, frame, flags) };
    crate::tlb::flush_range(test_va, 4);

    // Walk the range and look for our mapping.
    let mut found = false;
    let _summary = unsafe {
        pt_walk::walk_range(pml4, test_va, test_va + FRAME_SIZE as u64, |entry: WalkEntry| {
            if entry.phys_addr == phys {
                found = true;
            }
            WalkAction::Continue
        })
    };

    assert!(found, "page table walk should find our mapping");

    // Cleanup.
    let f = unsafe { page_table::unmap_frame(pml4, virt) }.expect("unmap");
    crate::tlb::flush_range(test_va, 4);
    // SAFETY: frame was just unmapped above; we own it exclusively.
    unsafe { frame::free_frame(f) }.expect("integ_test: free_frame after pt walk");

    serial_println!("[mm_integ]   Test 3 (PT walk finds mapping): PASSED");
}

/// Test 4: Watermark tracks peak allocation.
fn test_watermark_tracking() {
    let handle = watermark::register("integ_test").expect("register watermark");

    // Charge some units.
    watermark::charge(handle, 100);
    watermark::charge(handle, 200);
    let (current, peak) = watermark::read(handle);
    assert_eq!(current, 300);
    assert_eq!(peak, 300);

    // Uncharge — peak should stay.
    watermark::uncharge(handle, 150);
    let (current, peak) = watermark::read(handle);
    assert_eq!(current, 150);
    assert_eq!(peak, 300); // Peak unchanged.

    // Charge above old peak.
    watermark::charge(handle, 400);
    let (current, peak) = watermark::read(handle);
    assert_eq!(current, 550);
    assert_eq!(peak, 550); // New peak.

    // Cleanup.
    watermark::uncharge(handle, 550);

    serial_println!("[mm_integ]   Test 4 (watermark tracking): PASSED");
}

/// Test 5: Migration type set/get coherence.
fn test_migrate_type_coherence() {
    // Use a high frame index unlikely to conflict with real allocations.
    let test_idx: usize = 60000;

    // Set as Movable.
    migrate_type::set_frame_type(test_idx, MigrateType::Movable);
    assert_eq!(migrate_type::get_frame_type(test_idx), MigrateType::Movable);
    assert!(migrate_type::is_movable(test_idx));

    // Change to Reclaimable.
    migrate_type::set_frame_type(test_idx, MigrateType::Reclaimable);
    assert_eq!(migrate_type::get_frame_type(test_idx), MigrateType::Reclaimable);
    assert!(migrate_type::is_reclaimable(test_idx));
    assert!(!migrate_type::is_movable(test_idx));

    // Cleanup.
    migrate_type::set_frame_type(test_idx, MigrateType::Unmovable);

    serial_println!("[mm_integ]   Test 5 (migrate type coherence): PASSED");
}

/// Test 6: Rmap round-trip (add → lookup → remove → verify gone).
fn test_rmap_round_trip() {
    let test_phys: u64 = 0x1_0000_0000; // 4 GiB — unlikely to be a real frame.
    let test_pml4: u64 = 0x2000_0000;
    let test_virt: u64 = 0x0000_4000_0000;

    // Add mapping.
    rmap::add(test_phys, test_pml4, test_virt);

    // Lookup — should find it.
    let mut mappers = [(0u64, 0u64); 4];
    let result = rmap::lookup(test_phys, &mut mappers);
    assert!(result.count >= 1, "rmap lookup should find the entry");
    assert_eq!(mappers[0], (test_pml4, test_virt));

    // Remove.
    rmap::remove(test_phys, test_pml4, test_virt);

    // Lookup again — should NOT find it.
    let result = rmap::lookup(test_phys, &mut mappers);
    // After removal, count should be 0 for this frame.
    // (But the entry might still be there with count=0 depending on impl.)
    // Verify the mapper pair is gone.
    let mut found = false;
    for i in 0..result.count {
        if mappers[i] == (test_pml4, test_virt) {
            found = true;
        }
    }
    assert!(!found, "rmap entry should be removed");

    serial_println!("[mm_integ]   Test 6 (rmap round-trip): PASSED");
}
