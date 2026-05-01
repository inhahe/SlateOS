//! TLB shootdown for SMP systems.
//!
//! When one CPU modifies a page table entry (unmap, permission change),
//! other CPUs may still have the old entry cached in their TLBs.  This
//! module provides a TLB shootdown mechanism that uses a fixed-mode IPI
//! to ask all other CPUs to invalidate their TLB entries for a given
//! address range.
//!
//! ## Protocol
//!
//! 1. Initiator stores the flush request (address + page count) in a
//!    shared static.
//! 2. Initiator sends an IPI to all other CPUs (vector 251).
//! 3. Initiator spins waiting for all other CPUs to acknowledge.
//! 4. Each receiving CPU executes `invlpg` for the range, then bumps
//!    the acknowledgement counter.
//! 5. Initiator continues once all CPUs have acknowledged.
//!
//! For a full address space flush (e.g., process exit, CR3 change),
//! we simply CR3-reload on all CPUs.
//!
//! ## Single-CPU Fast Path
//!
//! If only one CPU is online, the IPI is skipped — we just flush locally.
//!
//! ## References
//!
//! - Linux `arch/x86/mm/tlb.c` — TLB flush IPI mechanism
//! - Intel SDM Vol. 3A §4.10.4 "Invalidation of TLBs and Paging-Structure Caches"

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::serial_println;

/// IPI vector for TLB shootdown requests.
///
/// We use vector 251, which is in the high range (above device IRQs
/// at 33–56) and below the APIC spurious vector (255).
pub const TLB_SHOOTDOWN_VECTOR: u8 = 251;

/// Sentinel value meaning "flush entire TLB (CR3 reload)".
const FLUSH_ALL: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Shared shootdown request (protected by the initiator holding the lock)
// ---------------------------------------------------------------------------

/// Start address of the TLB range to flush.
///
/// `FLUSH_ALL` means reload CR3 (full flush).
static FLUSH_ADDR: AtomicU64 = AtomicU64::new(0);

/// Number of 4 KiB hardware pages to flush (1 page per invlpg).
/// Ignored if `FLUSH_ADDR == FLUSH_ALL`.
static FLUSH_PAGES: AtomicU32 = AtomicU32::new(0);

/// Number of CPUs that have acknowledged the current shootdown.
static ACK_COUNT: AtomicU32 = AtomicU32::new(0);

/// Serializes concurrent shootdown requests.
///
/// Only one CPU can initiate a shootdown at a time.  Other CPUs that
/// need to shootdown will spin on this lock.  This is acceptable because
/// shootdowns are infrequent and the critical section is very short.
static SHOOTDOWN_LOCK: spin::Mutex<()> = spin::Mutex::new(());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Flush TLB entries for a range of 4 KiB pages on all CPUs.
///
/// This performs `invlpg` for `page_count` pages starting at `vaddr`
/// on the current CPU, and sends a TLB shootdown IPI to all other CPUs.
///
/// Blocks until all online CPUs have flushed.
///
/// For single-CPU systems (or before SMP init), this just flushes locally.
pub fn flush_range(vaddr: u64, page_count: u32) {
    // Local flush first.
    local_flush_range(vaddr, page_count);

    // If only one CPU is online, no IPI needed.
    let online = crate::smp::cpu_count();
    if online <= 1 {
        return;
    }

    // Acquire the shootdown lock to serialize concurrent requests.
    let _guard = SHOOTDOWN_LOCK.lock();

    // Set up the request.
    FLUSH_ADDR.store(vaddr, Ordering::Release);
    FLUSH_PAGES.store(page_count, Ordering::Release);
    ACK_COUNT.store(0, Ordering::Release);

    // Send the IPI to all other CPUs.
    // target_acks = online - 1 (exclude self).
    #[allow(clippy::cast_possible_truncation)]
    let target_acks = (online - 1) as u32;

    // SAFETY: APIC is initialized, the vector has a valid ISR.
    unsafe {
        crate::apic::send_ipi_all_excluding_self(TLB_SHOOTDOWN_VECTOR);
    }

    // Spin-wait for all other CPUs to acknowledge.
    // This is a tight loop but shootdowns are rare and fast.
    while ACK_COUNT.load(Ordering::Acquire) < target_acks {
        core::hint::spin_loop();
    }
}

/// Flush the entire TLB on all CPUs (CR3 reload).
///
/// Use this for large-scale changes (e.g., process exit, address space
/// switch) where individual `invlpg` would be more expensive.
pub fn flush_all() {
    // Local full flush.
    local_flush_all();

    let online = crate::smp::cpu_count();
    if online <= 1 {
        return;
    }

    let _guard = SHOOTDOWN_LOCK.lock();

    FLUSH_ADDR.store(FLUSH_ALL, Ordering::Release);
    FLUSH_PAGES.store(0, Ordering::Release);
    ACK_COUNT.store(0, Ordering::Release);

    #[allow(clippy::cast_possible_truncation)]
    let target_acks = (online - 1) as u32;

    // SAFETY: APIC is initialized, the vector has a valid ISR.
    unsafe {
        crate::apic::send_ipi_all_excluding_self(TLB_SHOOTDOWN_VECTOR);
    }

    while ACK_COUNT.load(Ordering::Acquire) < target_acks {
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// IPI handler (called from IDT stub)
// ---------------------------------------------------------------------------

/// ISR handler for the TLB shootdown IPI (vector 251).
///
/// Called from the IDT assembly stub.  Reads the flush request,
/// performs the local TLB invalidation, acknowledges, and sends EOI.
///
/// Must be fast — no allocations, no lock contention, no serial output.
#[unsafe(no_mangle)]
pub extern "C" fn handle_tlb_shootdown_irq(
    _frame: &crate::idt::InterruptStackFrame,
    _error: u64,
) {
    let addr = FLUSH_ADDR.load(Ordering::Acquire);
    let pages = FLUSH_PAGES.load(Ordering::Acquire);

    if addr == FLUSH_ALL {
        local_flush_all();
    } else {
        local_flush_range(addr, pages);
    }

    // Acknowledge.
    ACK_COUNT.fetch_add(1, Ordering::Release);

    // Send EOI to the local APIC.
    // SAFETY: Always safe to write to the APIC EOI register.
    unsafe {
        crate::apic::eoi();
    }
}

// ---------------------------------------------------------------------------
// Local TLB operations
// ---------------------------------------------------------------------------

/// Flush TLB entries for a range of 4 KiB pages on the local CPU.
fn local_flush_range(vaddr: u64, page_count: u32) {
    for i in 0..page_count {
        let addr = vaddr.wrapping_add(u64::from(i) * 4096);
        // SAFETY: invlpg is always safe in ring 0.
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) addr,
                options(nostack, preserves_flags),
            );
        }
    }
}

/// Full TLB flush on the local CPU via CR3 reload.
fn local_flush_all() {
    // SAFETY: Reading and reloading CR3 is always safe in ring 0.
    // This flushes all non-global TLB entries.
    unsafe {
        let cr3: u64;
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) cr3,
            options(nomem, nostack, preserves_flags),
        );
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) cr3,
            options(nostack, preserves_flags),
        );
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify TLB shootdown infrastructure.
pub fn self_test() {
    serial_println!("[tlb] Running self-test...");

    let online = crate::smp::cpu_count();
    serial_println!("[tlb]   Online CPUs: {}", online);

    // Test 1: Local flush range (should not panic).
    local_flush_range(0x1000_0000, 4);
    serial_println!("[tlb]   Local flush_range: OK");

    // Test 2: Local flush all (should not panic).
    local_flush_all();
    serial_println!("[tlb]   Local flush_all: OK");

    // Test 3: Full shootdown (exercises IPI if SMP).
    flush_range(0x2000_0000, 1);
    serial_println!("[tlb]   Shootdown flush_range: OK ({} CPUs)", online);

    // Test 4: Full flush all (exercises IPI if SMP).
    flush_all();
    serial_println!("[tlb]   Shootdown flush_all: OK ({} CPUs)", online);

    serial_println!("[tlb] Self-test PASSED");
}
