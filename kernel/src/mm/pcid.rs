//! Process Context Identifiers (PCID) — TLB tagging to avoid full flushes.
//!
//! On x86_64, switching address spaces (writing CR3) normally flushes the
//! entire TLB.  With PCID enabled, each address space gets a 12-bit tag
//! (0–4095) stored in CR3 bits 0–11.  The CPU can cache TLB entries from
//! multiple address spaces simultaneously, disambiguating them by PCID.
//!
//! ## Performance Impact
//!
//! Without PCID: every context switch flushes all TLB entries for the
//! outgoing process.  The incoming process starts with a cold TLB.
//!
//! With PCID: context switch preserves old entries (if the PCID was used
//! recently and hasn't been evicted).  The incoming process may still have
//! warm TLB entries from its last run on this CPU.
//!
//! ## INVPCID
//!
//! The INVPCID instruction (CPUID.07H:EBX bit 10) provides fine-grained
//! TLB invalidation by PCID.  We use it when available:
//! - Type 0: invalidate a single address in a single PCID.
//! - Type 1: invalidate all entries for a single PCID.
//! - Type 2: invalidate all entries in all PCIDs (global flush).
//!
//! ## PCID Allocation
//!
//! We maintain a per-CPU PCID allocator.  Each CPU independently assigns
//! PCIDs to address spaces as they are scheduled.  When all 4096 PCIDs
//! are exhausted, we flush the entire TLB and recycle all PCIDs (generation
//! bump).  This is the same approach Linux uses ("lazy PCID reclaim").
//!
//! ## References
//!
//! - Intel SDM Vol. 3A §4.10.1 "Process-Context Identifiers (PCIDs)"
//! - Linux `arch/x86/mm/tlb.c` — `choose_new_asid()`, PCID support
//! - Linux `arch/x86/include/asm/tlbflush.h` — INVPCID helpers

// PCID is an optimization layer; some helpers (single-address invalidation
// types, debug accessors) are kept for completeness with the Intel SDM
// description even though current call sites don't exercise all paths.
#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum PCID value (12-bit field, 0–4095).
/// PCID 0 is reserved for the kernel's address space.
const MAX_PCID: u16 = 4095;

/// Maximum number of CPUs we support.
const MAX_CPUS: usize = 16;

/// CR4 bit for PCIDE (Process Context Identifiers Enable).
const CR4_PCIDE: u64 = 1 << 17;

/// CR3 bit 63: when set during a MOV to CR3, the TLB is NOT flushed
/// for the new PCID.  Only valid when PCID is enabled.
const CR3_NOFLUSH: u64 = 1 << 63;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Whether the CPU supports PCID (detected at boot).
static PCID_SUPPORTED: AtomicBool = AtomicBool::new(false);

/// Whether PCID is actively enabled (CR4.PCIDE = 1).
static PCID_ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether the CPU supports the INVPCID instruction.
static INVPCID_SUPPORTED: AtomicBool = AtomicBool::new(false);

/// Statistics: total CR3 writes that benefited from PCID (no flush).
static NOFLUSH_COUNT: AtomicU64 = AtomicU64::new(0);

/// Statistics: total PCID generation flushes (all PCIDs exhausted).
static GENERATION_FLUSH_COUNT: AtomicU64 = AtomicU64::new(0);

/// Statistics: total INVPCID single-address invalidations.
static INVPCID_SINGLE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Per-CPU PCID allocator state.
///
/// Each CPU independently assigns PCIDs to address spaces.  The `next_pcid`
/// counter wraps around — when it exceeds MAX_PCID, we bump the generation
/// and flush the TLB (all old PCIDs are now stale).
struct PerCpuPcid {
    /// Next PCID to assign (1–4095; 0 is reserved for kernel).
    next_pcid: u16,
    /// Generation counter — bumped on PCID wrap-around (TLB flush).
    generation: u64,
}

impl PerCpuPcid {
    const fn new() -> Self {
        Self {
            next_pcid: 1, // 0 is reserved for kernel.
            generation: 1,
        }
    }
}

/// Per-CPU PCID state.  Indexed by CPU index.
/// Protected by disabling interrupts (only accessed by the local CPU).
static mut PER_CPU: [PerCpuPcid; MAX_CPUS] = {
    const INIT: PerCpuPcid = PerCpuPcid::new();
    [INIT; MAX_CPUS]
};

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Detect PCID and INVPCID support via CPUID.
///
/// Call once during boot after CPU feature detection.
pub fn detect() {
    // PCID: CPUID.01H:ECX bit 17.
    // LLVM reserves RBX, so we must save/restore it around CPUID.
    let ecx: u32;
    // SAFETY: CPUID is always safe to execute.  We save/restore RBX
    // because LLVM uses it as a reserved register.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "pop rbx",
            out("ecx") ecx,
            out("eax") _,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    let pcid_support = ecx & (1 << 17) != 0;
    PCID_SUPPORTED.store(pcid_support, Ordering::Release);

    // INVPCID: CPUID.07H:EBX bit 10.
    let ebx7: u32;
    // SAFETY: CPUID is always safe.  We move EBX→EAX after CPUID
    // to retrieve it (since we can't use out("ebx") directly).
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 7",
            "xor ecx, ecx",
            "cpuid",
            "mov eax, ebx",
            "pop rbx",
            out("eax") ebx7,
            out("ecx") _,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    let invpcid_support = ebx7 & (1 << 10) != 0;
    INVPCID_SUPPORTED.store(invpcid_support, Ordering::Release);

    serial_println!("[pcid] PCID supported: {}, INVPCID: {}",
        pcid_support, invpcid_support);
}

/// Enable PCID by setting CR4.PCIDE.
///
/// Must be called after detect() confirms support.  Must be called
/// on each CPU (BSP + all APs) because CR4 is per-CPU.
///
/// # Prerequisite
///
/// CR3 must have PCID=0 when enabling (the kernel always uses PCID 0).
pub fn enable_on_this_cpu() {
    if !PCID_SUPPORTED.load(Ordering::Acquire) {
        return;
    }

    // Ensure current CR3 has PCID 0 (bits 0–11 clear).
    // The bootloader/Limine sets up CR3 without PCID, so bits 0–11
    // are always part of the physical address (which is 4K-aligned → bits 0–11 = 0).
    // SAFETY: CR4 read/write is always safe in ring 0.
    unsafe {
        let cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        let new_cr4 = cr4 | CR4_PCIDE;
        core::arch::asm!("mov cr4, {}", in(reg) new_cr4, options(nomem, nostack));
    }

    PCID_ENABLED.store(true, Ordering::Release);
}

/// Check if PCID is enabled on this system.
#[inline]
#[must_use]
pub fn is_enabled() -> bool {
    PCID_ENABLED.load(Ordering::Acquire)
}

/// Check if INVPCID instruction is available.
#[inline]
#[must_use]
pub fn has_invpcid() -> bool {
    INVPCID_SUPPORTED.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// PCID allocation
// ---------------------------------------------------------------------------

/// Allocate a PCID for a new address space on the current CPU.
///
/// Returns `(pcid, generation, needs_flush)`:
/// - `pcid`: the 12-bit PCID to embed in CR3.
/// - `generation`: the generation counter (for detecting stale PCIDs).
/// - `needs_flush`: if true, all TLB entries for this PCID are stale
///   and the caller must flush (don't use CR3_NOFLUSH).
///
/// # Safety
///
/// Must be called with interrupts disabled (accessing per-CPU state).
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn alloc_pcid(cpu: usize) -> (u16, u64, bool) {
    if !is_enabled() || cpu >= MAX_CPUS {
        return (0, 0, true); // PCID disabled — always flush.
    }

    // SAFETY: We're accessing our own CPU's state with interrupts disabled.
    let state = unsafe { &mut PER_CPU[cpu] };

    let pcid = state.next_pcid;
    state.next_pcid += 1;

    if state.next_pcid > MAX_PCID {
        // All PCIDs exhausted — wrap around, bump generation, must flush.
        state.next_pcid = 1;
        state.generation += 1;
        GENERATION_FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
        // After generation bump, all existing PCIDs are stale.
        // The next few switches will need flushes until PCID entries stabilize.
        return (pcid, state.generation, true);
    }

    (pcid, state.generation, false)
}

/// Build a CR3 value with PCID and optional no-flush bit.
///
/// `pml4_phys`: physical address of the PML4 (4K-aligned, bits 0–11 = 0).
/// `pcid`: 12-bit PCID (0–4095).
/// `noflush`: if true, set CR3 bit 63 to suppress TLB flush on load.
#[inline]
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
pub fn build_cr3(pml4_phys: u64, pcid: u16, noflush: bool) -> u64 {
    debug_assert!(pml4_phys & 0xFFF == 0, "PML4 must be 4K-aligned");
    debug_assert!(pcid <= MAX_PCID, "PCID must be 0–4095");

    let mut cr3 = pml4_phys | u64::from(pcid);
    if noflush && is_enabled() {
        cr3 |= CR3_NOFLUSH;
        NOFLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    cr3
}

// ---------------------------------------------------------------------------
// INVPCID wrappers
// ---------------------------------------------------------------------------

/// Invalidate a single TLB entry in a specific PCID.
///
/// INVPCID type 0: invalidate mapping for `addr` in PCID `pcid`.
#[inline]
pub fn invpcid_addr(pcid: u16, addr: u64) {
    if !has_invpcid() {
        return;
    }
    // The descriptor is 128 bits: [63:0] = linear address, [75:64] = PCID.
    let descriptor: [u64; 2] = [addr, u64::from(pcid)];
    // SAFETY: INVPCID is available (checked above), descriptor is on stack.
    unsafe {
        core::arch::asm!(
            "invpcid {reg}, [{desc}]",
            reg = in(reg) 0u64,  // type 0 = individual address
            desc = in(reg) descriptor.as_ptr(),
            options(nostack),
        );
    }
    INVPCID_SINGLE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Invalidate all TLB entries for a specific PCID.
///
/// INVPCID type 1: invalidate all entries tagged with `pcid`.
#[inline]
pub fn invpcid_single_context(pcid: u16) {
    if !has_invpcid() {
        return;
    }
    let descriptor: [u64; 2] = [0, u64::from(pcid)];
    // SAFETY: INVPCID is available.
    unsafe {
        core::arch::asm!(
            "invpcid {reg}, [{desc}]",
            reg = in(reg) 1u64,  // type 1 = single context
            desc = in(reg) descriptor.as_ptr(),
            options(nostack),
        );
    }
}

/// Invalidate all TLB entries including global pages across all PCIDs.
///
/// INVPCID type 2: global flush (same as reloading CR3 without PCID, but
/// also flushes global entries).
#[inline]
pub fn invpcid_all() {
    if !has_invpcid() {
        // Fallback: reload CR4 to flush all TLB entries including global.
        // SAFETY: Reading and writing CR4 is safe in ring 0.
        unsafe {
            let cr4: u64;
            core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
            // Toggle PGE (bit 7) off and on to flush global entries.
            core::arch::asm!("mov cr4, {}", in(reg) cr4 & !(1u64 << 7), options(nomem, nostack));
            core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));
        }
        return;
    }
    let descriptor: [u64; 2] = [0, 0];
    // SAFETY: INVPCID is available.
    unsafe {
        core::arch::asm!(
            "invpcid {reg}, [{desc}]",
            reg = in(reg) 2u64,  // type 2 = all contexts including global
            desc = in(reg) descriptor.as_ptr(),
            options(nostack),
        );
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// PCID subsystem statistics.
#[derive(Debug, Clone, Copy)]
pub struct PcidStats {
    /// Whether PCID is enabled.
    pub enabled: bool,
    /// Whether INVPCID instruction is available.
    pub has_invpcid: bool,
    /// Number of CR3 writes that used the no-flush optimization.
    pub noflush_switches: u64,
    /// Number of times all PCIDs were exhausted (generation flush).
    pub generation_flushes: u64,
    /// Number of INVPCID single-address invalidations.
    pub invpcid_singles: u64,
}

/// Get PCID statistics.
#[must_use]
pub fn stats() -> PcidStats {
    PcidStats {
        enabled: is_enabled(),
        has_invpcid: has_invpcid(),
        noflush_switches: NOFLUSH_COUNT.load(Ordering::Relaxed),
        generation_flushes: GENERATION_FLUSH_COUNT.load(Ordering::Relaxed),
        invpcid_singles: INVPCID_SINGLE_COUNT.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the PCID subsystem.
pub fn self_test() {
    serial_println!("[pcid] Running self-test...");

    // Test 1: Detection results.
    let pcid_ok = PCID_SUPPORTED.load(Ordering::Acquire);
    let invpcid_ok = INVPCID_SUPPORTED.load(Ordering::Acquire);
    serial_println!("[pcid]   PCID supported: {}", pcid_ok);
    serial_println!("[pcid]   INVPCID supported: {}", invpcid_ok);

    // Test 2: build_cr3 correctness.
    let pml4: u64 = 0x0010_0000; // 1 MiB, 4K-aligned.
    let cr3_plain = build_cr3(pml4, 0, false);
    assert_eq!(cr3_plain, pml4); // PCID 0, no flags.

    let cr3_pcid = build_cr3(pml4, 42, false);
    assert_eq!(cr3_pcid & 0xFFF, 42); // Low 12 bits = PCID.
    assert_eq!(cr3_pcid & !0xFFF_u64 & !(1u64 << 63), pml4); // High bits = pml4.
    serial_println!("[pcid]   build_cr3: OK");

    // Test 3: If PCID is enabled, verify noflush bit.
    if is_enabled() {
        let cr3_noflush = build_cr3(pml4, 7, true);
        assert!(cr3_noflush & CR3_NOFLUSH != 0);
        assert_eq!(cr3_noflush & 0xFFF, 7);
        serial_println!("[pcid]   CR3 NOFLUSH bit: OK");

        // Test 4: Allocate a PCID.
        let cpu = crate::smp::current_cpu_index();
        let (pcid, generation, _flush) = unsafe { alloc_pcid(cpu) };
        assert!(pcid >= 1);
        assert!(pcid <= MAX_PCID);
        assert!(generation >= 1);
        serial_println!("[pcid]   alloc_pcid: OK (pcid={}, gen={})", pcid, generation);
    } else {
        serial_println!("[pcid]   (PCID not enabled on this CPU — skipping live tests)");
    }

    // Test 5: Stats.
    let st = stats();
    serial_println!("[pcid]   Stats: enabled={}, invpcid={}, noflush={}, gen_flushes={}",
        st.enabled, st.has_invpcid, st.noflush_switches, st.generation_flushes);

    serial_println!("[pcid] Self-test PASSED");
}
