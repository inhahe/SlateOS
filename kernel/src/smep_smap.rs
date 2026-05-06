//! SMEP/SMAP — Supervisor Mode Execution/Access Prevention.
//!
//! Hardware protection against the kernel accidentally (or maliciously via
//! exploit) accessing user-space memory:
//!
//! ## SMEP (Supervisor Mode Execution Prevention)
//!
//! When CR4.SMEP is set, any attempt by the kernel (CPL < 3) to fetch
//! and execute instructions from a user-mode page (U/S bit set in PTE)
//! triggers a #PF with RSVD bit set.  This defeats "ret2usr" exploits
//! where an attacker gets the kernel to jump to shellcode mapped in the
//! process's user address space.
//!
//! ## SMAP (Supervisor Mode Access Prevention)
//!
//! When CR4.SMAP is set, any attempt by the kernel to read or write a
//! user-mode page triggers a #PF — UNLESS the EFLAGS.AC (Alignment Check)
//! flag is set.  The kernel uses `STAC` (Set AC flag) before intentionally
//! accessing user memory and `CLAC` (Clear AC flag) immediately after.
//! This prevents:
//! - Confused-deputy bugs where kernel code accidentally dereferences a
//!   user-supplied pointer without validation
//! - Exploits that redirect kernel reads/writes to user-mapped pages
//!
//! ## Usage Pattern
//!
//! ```ignore
//! // In syscall handler that needs to read user memory:
//! let data = unsafe {
//!     smep_smap::with_user_access(|| {
//!         // SMAP temporarily disabled — user pages accessible
//!         core::ptr::read(user_ptr)
//!     })
//! };
//! ```
//!
//! ## Performance
//!
//! - SMEP: Zero overhead once enabled (purely hardware-enforced)
//! - SMAP: ~2 cycles for STAC, ~2 cycles for CLAC. Negligible.
//! - Both features have been in Intel CPUs since Haswell (2013) and
//!   AMD since Zen (2017).  Virtually all x86_64 systems support them.
//!
//! ## References
//!
//! - Intel SDM Vol. 3A §4.6 "Access Rights"
//! - Linux: arch/x86/include/asm/smap.h (stac/clac)
//! - Linux: arch/x86/mm/fault.c (SMAP violation detection)

use crate::serial_println;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// CR4 bits
// ---------------------------------------------------------------------------

/// CR4.SMEP — Supervisor Mode Execution Prevention (bit 20).
const CR4_SMEP: u64 = 1 << 20;
/// CR4.SMAP — Supervisor Mode Access Prevention (bit 21).
const CR4_SMAP: u64 = 1 << 21;
/// CR4.UMIP — User-Mode Instruction Prevention (bit 11).
const CR4_UMIP: u64 = 1 << 11;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether SMEP is currently enabled.
static SMEP_ENABLED: AtomicBool = AtomicBool::new(false);
/// Whether SMAP is currently enabled.
static SMAP_ENABLED: AtomicBool = AtomicBool::new(false);
/// Whether UMIP is currently enabled.
static UMIP_ENABLED: AtomicBool = AtomicBool::new(false);
/// Count of intentional user-access windows opened (STAC/CLAC pairs).
static USER_ACCESS_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Detect and enable SMEP/SMAP on the current CPU.
///
/// Called during early boot after `cpu::detect_features()`.  Each AP also
/// calls this during SMP init (CR4 is per-CPU, so each CPU must set its own).
///
/// It is safe to call multiple times (idempotent).
pub fn init() {
    let Some(features) = crate::cpu::features() else {
        serial_println!("[smep_smap] CPU features not detected — skipping");
        return;
    };

    let cr4 = read_cr4();
    let mut new_cr4 = cr4;

    // Enable SMEP if supported.
    if features.smep {
        if cr4 & CR4_SMEP == 0 {
            new_cr4 |= CR4_SMEP;
            serial_println!("[smep_smap] Enabling SMEP (kernel exec of user pages blocked)");
        }
        SMEP_ENABLED.store(true, Ordering::Release);
    } else {
        serial_println!("[smep_smap] SMEP not supported by CPU");
    }

    // SMAP: detected but NOT enabled yet.
    // SMAP requires all kernel→user memory access paths to use STAC/CLAC.
    // Until copy_from_user / copy_to_user are instrumented, enabling SMAP
    // would fault on legitimate kernel reads of user buffers during syscalls.
    // The infrastructure (stac/clac/with_user_access) is ready — enablement
    // is deferred until user access paths are properly annotated.
    if features.smap {
        serial_println!("[smep_smap] SMAP supported (enablement deferred — needs STAC/CLAC instrumentation)");
        // Do NOT enable — leave SMAP_ENABLED = false.
    } else {
        serial_println!("[smep_smap] SMAP not supported by CPU");
    }

    // Enable UMIP if supported.
    // UMIP blocks user-mode execution of SGDT, SIDT, SLDT, SMSW, STR.
    // These instructions leak kernel addresses (GDT/IDT base, LDT selector)
    // which could be used to bypass KASLR.  Safe to enable unconditionally
    // since user-mode code should never need these instructions.
    if features.umip {
        if cr4 & CR4_UMIP == 0 {
            new_cr4 |= CR4_UMIP;
            serial_println!("[smep_smap] Enabling UMIP (user SGDT/SIDT/SLDT/SMSW/STR blocked)");
        }
        UMIP_ENABLED.store(true, Ordering::Release);
    } else {
        serial_println!("[smep_smap] UMIP not supported by CPU");
    }

    // Apply CR4 changes if any bits were added.
    if new_cr4 != cr4 {
        // SAFETY: We've verified the CPU supports these features via CPUID.
        // Adding SMEP/SMAP bits to CR4 is safe as long as the kernel doesn't
        // intentionally execute user pages (it shouldn't!) and uses STAC/CLAC
        // when accessing user memory.
        unsafe { write_cr4(new_cr4); }
        serial_println!("[smep_smap] CR4 updated: {:#x} → {:#x}", cr4, new_cr4);
    }
}

/// Enable SMEP/SMAP on an Application Processor during SMP bootstrap.
///
/// Each CPU has its own CR4, so each AP must independently enable these bits.
#[allow(dead_code)]
pub fn init_ap() {
    let Some(features) = crate::cpu::features() else { return };

    let cr4 = read_cr4();
    let mut new_cr4 = cr4;

    if features.smep && (cr4 & CR4_SMEP == 0) {
        new_cr4 |= CR4_SMEP;
    }
    // SMAP intentionally skipped (deferred until access paths instrumented).
    if features.umip && (cr4 & CR4_UMIP == 0) {
        new_cr4 |= CR4_UMIP;
    }

    if new_cr4 != cr4 {
        // SAFETY: Same as init() — CPU supports these features.
        unsafe { write_cr4(new_cr4); }
    }
}

// ---------------------------------------------------------------------------
// User memory access primitives (STAC/CLAC)
// ---------------------------------------------------------------------------

/// Temporarily allow kernel access to user-mode pages (clear SMAP enforcement).
///
/// Sets the AC flag in EFLAGS, which tells the CPU to permit supervisor-mode
/// accesses to user pages.  MUST be paired with [`clac()`] as soon as the
/// access is complete.
///
/// If SMAP is not enabled, this is a no-op (STAC is still safe to execute;
/// it just sets a flag that nothing checks).
///
/// # Safety
///
/// Caller must ensure that the user memory being accessed has been validated
/// (address range is in user space, pages are mapped with appropriate permissions).
/// The AC flag window must be as short as possible to minimize the attack surface.
#[inline(always)]
pub unsafe fn stac() {
    // SAFETY: STAC is always safe to execute; it sets EFLAGS.AC which only
    // has an effect when CR4.SMAP is set.
    unsafe {
        core::arch::asm!("stac", options(nomem, nostack, preserves_flags));
    }
    USER_ACCESS_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Re-enable SMAP enforcement after user memory access.
///
/// Clears the AC flag in EFLAGS.  After this, any kernel access to user
/// pages will fault.
///
/// # Safety
///
/// Must be called after a corresponding [`stac()`] call.  Forgetting CLAC
/// leaves the kernel vulnerable until the next context switch or IRET.
#[inline(always)]
pub unsafe fn clac() {
    // SAFETY: CLAC is always safe to execute; it clears EFLAGS.AC.
    unsafe {
        core::arch::asm!("clac", options(nomem, nostack, preserves_flags));
    }
}

/// Execute a closure with user memory access temporarily enabled.
///
/// This is the preferred way to access user memory from the kernel.
/// It ensures STAC/CLAC are properly paired and the window is minimal.
///
/// # Safety
///
/// - The closure must only access validated user memory
/// - The user pointer must have been range-checked before calling this
/// - The closure should be as short as possible (just the memory access)
///
/// # Example
///
/// ```ignore
/// let value = unsafe {
///     smep_smap::with_user_access(|| {
///         core::ptr::read(validated_user_ptr)
///     })
/// };
/// ```
#[inline(always)]
pub unsafe fn with_user_access<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    // SAFETY: Caller guarantees the closure only accesses validated user memory.
    unsafe { stac(); }
    let result = f();
    // SAFETY: Paired with stac() above.
    unsafe { clac(); }
    result
}

// ---------------------------------------------------------------------------
// Status and diagnostics
// ---------------------------------------------------------------------------

/// SMEP/SMAP/UMIP status for diagnostics.
#[derive(Debug, Clone)]
pub struct SmepSmapStatus {
    /// CPU supports SMEP.
    pub hw_smep: bool,
    /// CPU supports SMAP.
    pub hw_smap: bool,
    /// CPU supports UMIP.
    pub hw_umip: bool,
    /// SMEP currently enabled (CR4.SMEP set on BSP).
    pub smep_active: bool,
    /// SMAP currently enabled (CR4.SMAP set on BSP).
    pub smap_active: bool,
    /// UMIP currently enabled (CR4.UMIP set on BSP).
    pub umip_active: bool,
    /// Total user-access windows opened (STAC calls).
    pub user_access_count: u64,
    /// Current CR4 value (for diagnostics).
    pub cr4: u64,
}

/// Query current SMEP/SMAP/UMIP status.
pub fn status() -> SmepSmapStatus {
    let features = crate::cpu::features();
    let (hw_smep, hw_smap, hw_umip) = features
        .map(|f| (f.smep, f.smap, f.umip))
        .unwrap_or((false, false, false));

    SmepSmapStatus {
        hw_smep,
        hw_smap,
        hw_umip,
        smep_active: SMEP_ENABLED.load(Ordering::Relaxed),
        smap_active: SMAP_ENABLED.load(Ordering::Relaxed),
        umip_active: UMIP_ENABLED.load(Ordering::Relaxed),
        user_access_count: USER_ACCESS_COUNT.load(Ordering::Relaxed),
        cr4: read_cr4(),
    }
}

/// Whether SMEP is currently active.
#[allow(dead_code)]
pub fn smep_active() -> bool {
    SMEP_ENABLED.load(Ordering::Relaxed)
}

/// Whether SMAP is currently active.
#[allow(dead_code)]
pub fn smap_active() -> bool {
    SMAP_ENABLED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Low-level helpers
// ---------------------------------------------------------------------------

/// Read CR4 register.
fn read_cr4() -> u64 {
    let val: u64;
    // SAFETY: Reading CR4 is always safe in ring 0.
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) val, options(nomem, nostack));
    }
    val
}

/// Write CR4 register.
///
/// # Safety
///
/// Caller must ensure the new CR4 value is valid and doesn't disable
/// critical features without proper preparation.
unsafe fn write_cr4(val: u64) {
    // SAFETY: Caller guarantees the value is valid.
    unsafe {
        core::arch::asm!("mov cr4, {}", in(reg) val, options(nomem, nostack));
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for SMEP/SMAP detection and status.
pub fn self_test() {
    serial_println!("[smep_smap] Running self-test...");

    // Test 1: Status query works without panic.
    let s = status();
    serial_println!("[smep_smap]   Status: SMEP hw={}, SMAP hw={}, UMIP hw={}",
        s.hw_smep, s.hw_smap, s.hw_umip);
    serial_println!("[smep_smap]   Active: SMEP={}, SMAP={}, UMIP={}",
        s.smep_active, s.smap_active, s.umip_active);
    serial_println!("[smep_smap]   CR4={:#x}", s.cr4);

    // Test 2: If SMEP is supported, verify CR4.SMEP is set.
    if s.hw_smep {
        assert!(s.cr4 & CR4_SMEP != 0, "CR4.SMEP should be set when SMEP is supported");
        assert!(s.smep_active, "SMEP should be marked active");
        serial_println!("[smep_smap]   SMEP enforcement: VERIFIED (CR4 bit set)");
    } else {
        assert_eq!(s.cr4 & CR4_SMEP, 0, "CR4.SMEP should be clear without support");
        serial_println!("[smep_smap]   SMEP: not available on this CPU");
    }

    // Test 3: SMAP detection (enablement is deferred until user access paths
    // are instrumented with STAC/CLAC).
    if s.hw_smap {
        // SMAP is supported but intentionally NOT enabled yet.
        serial_println!("[smep_smap]   SMAP: supported (deferred — needs user access instrumentation)");
    } else {
        assert_eq!(s.cr4 & CR4_SMAP, 0, "CR4.SMAP should be clear without support");
        serial_println!("[smep_smap]   SMAP: not available on this CPU");
    }

    // Test 3b: If UMIP is supported, verify CR4.UMIP is set.
    if s.hw_umip {
        assert!(s.cr4 & CR4_UMIP != 0, "CR4.UMIP should be set when UMIP is supported");
        assert!(s.umip_active, "UMIP should be marked active");
        serial_println!("[smep_smap]   UMIP enforcement: VERIFIED (CR4 bit set)");
    } else {
        serial_println!("[smep_smap]   UMIP: not available on this CPU");
    }

    // Test 4: STAC/CLAC don't fault (they're safe to execute regardless of SMAP).
    // This just verifies the instructions are encodable and don't trap.
    unsafe {
        stac();
        clac();
    }
    serial_println!("[smep_smap]   STAC/CLAC pair: OK (no fault)");

    // Test 5: with_user_access closure executes and returns value.
    let result = unsafe { with_user_access(|| 42u64) };
    assert_eq!(result, 42);
    serial_println!("[smep_smap]   with_user_access: OK");

    // Test 6: Access count incremented correctly.
    let count_before = USER_ACCESS_COUNT.load(Ordering::Relaxed);
    unsafe { stac(); clac(); }
    let count_after = USER_ACCESS_COUNT.load(Ordering::Relaxed);
    assert_eq!(count_after, count_before + 1);
    serial_println!("[smep_smap]   Access counter: OK");

    serial_println!("[smep_smap] Self-test PASSED");
}
