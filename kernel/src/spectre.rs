//! Speculative execution vulnerability mitigations.
//!
//! This module manages hardware mitigations for Spectre, Meltdown, and
//! related transient-execution attacks:
//!
//! ## Spectre v1 (Bounds Check Bypass)
//!
//! Mitigated primarily by the compiler (LLVM speculative load hardening)
//! and careful kernel coding (index masking).  No runtime MSR needed.
//!
//! ## Spectre v2 (Branch Target Injection)
//!
//! Mitigated by IBRS (Indirect Branch Restricted Speculation):
//! - When IA32_SPEC_CTRL.IBRS = 1, indirect branches in kernel mode
//!   cannot be influenced by predictions from a less-privileged mode.
//! - This protects against an attacker in user-mode training the
//!   kernel's indirect branch predictor.
//! - Performance cost: ~1-5% depending on workload (mitigated on newer
//!   CPUs where "enhanced IBRS" is free in hardware).
//!
//! Additionally, IBPB (Indirect Branch Prediction Barrier) flushes the
//! predictor when switching between security domains (e.g., process switch).
//!
//! ## Spectre v4 (Speculative Store Bypass)
//!
//! Mitigated by SSBD (Speculative Store Bypass Disable):
//! - When IA32_SPEC_CTRL.SSBD = 1, speculative loads cannot bypass
//!   preceding stores to the same address.
//! - Prevents an attacker from speculatively reading stale values.
//! - Performance cost: 2-8% (significant for memory-intensive workloads).
//!   We enable it only for sensitive contexts.
//!
//! ## STIBP (Single Thread Indirect Branch Predictors)
//!
//! On SMT (hyperthreaded) CPUs, one logical core's branch predictions can
//! influence another's (same physical core).  STIBP prevents cross-thread
//! prediction poisoning.  Enabled when SMT is active.
//!
//! ## MSRs
//!
//! - `IA32_SPEC_CTRL` (0x48): IBRS (bit 0), STIBP (bit 1), SSBD (bit 2)
//! - `IA32_PRED_CMD` (0x49): IBPB (bit 0) — write-only, triggers barrier
//! - `IA32_ARCH_CAPABILITIES` (0x10A): read-only, describes inherent
//!   immunity to certain attacks (e.g., RDCL_NO = Meltdown-immune)
//!
//! ## References
//!
//! - Intel: "Speculative Execution Side Channel Mitigations" (spec update)
//! - AMD: "Software Techniques for Managing Speculation on AMD Processors"
//! - Linux: arch/x86/kernel/cpu/bugs.c

use crate::serial_println;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// MSR addresses
// ---------------------------------------------------------------------------

/// IA32_SPEC_CTRL — Speculation Control (R/W).
/// Bits: [0] IBRS, [1] STIBP, [2] SSBD.
const IA32_SPEC_CTRL: u32 = 0x48;

/// IA32_PRED_CMD — Prediction Command (W/O).
/// Bit [0]: IBPB — flush all indirect branch predictors.
const IA32_PRED_CMD: u32 = 0x49;

/// IA32_ARCH_CAPABILITIES — Enumeration of architectural MDS/TAA mitigations.
const IA32_ARCH_CAPABILITIES: u32 = 0x10A;

// ---------------------------------------------------------------------------
// IA32_SPEC_CTRL bits
// ---------------------------------------------------------------------------

/// IBRS: Indirect Branch Restricted Speculation.
const SPEC_CTRL_IBRS: u64 = 1 << 0;
/// STIBP: Single Thread Indirect Branch Predictors.
const SPEC_CTRL_STIBP: u64 = 1 << 1;
/// SSBD: Speculative Store Bypass Disable.
const SPEC_CTRL_SSBD: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// IA32_ARCH_CAPABILITIES bits (read-only, describes CPU's inherent immunity)
// ---------------------------------------------------------------------------

/// RDCL_NO: CPU is not susceptible to Meltdown (Rogue Data Cache Load).
const ARCH_CAP_RDCL_NO: u64 = 1 << 0;
/// IBRS_ALL: IBRS is always on (enhanced IBRS) — no performance penalty.
const ARCH_CAP_IBRS_ALL: u64 = 1 << 1;
/// SKIP_L1DFL_VMENTRY: No L1 data flush needed on VM entry.
#[allow(dead_code)]
const ARCH_CAP_SKIP_L1DFL: u64 = 1 << 3;
/// SSB_NO: CPU not susceptible to Speculative Store Bypass.
const ARCH_CAP_SSB_NO: u64 = 1 << 4;
/// MDS_NO: CPU not susceptible to Microarchitectural Data Sampling.
const ARCH_CAP_MDS_NO: u64 = 1 << 5;
/// TAA_NO: CPU not susceptible to TSX Asynchronous Abort.
#[allow(dead_code)]
const ARCH_CAP_TAA_NO: u64 = 1 << 8;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether IBRS is currently enabled.
static IBRS_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether STIBP is currently enabled.
static STIBP_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether SSBD is currently enabled.
static SSBD_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Number of IBPB barriers issued (for diagnostics).
static IBPB_COUNT: AtomicU64 = AtomicU64::new(0);
/// Cached IA32_ARCH_CAPABILITIES value (0 if MSR not available).
static ARCH_CAPS: AtomicU64 = AtomicU64::new(0);
/// Whether the CPU is immune to Meltdown (RDCL_NO or non-Intel).
static MELTDOWN_IMMUNE: AtomicBool = AtomicBool::new(false);
/// Whether enhanced IBRS is available (IBRS_ALL: always-on, no perf cost).
static ENHANCED_IBRS: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Detect and enable speculative execution mitigations on the BSP.
///
/// Called during early boot after `cpu::detect_features()`.
/// Each AP also calls [`init_ap()`] to set its own MSRs.
pub fn init() {
    let Some(features) = crate::cpu::features() else {
        serial_println!("[spectre] CPU features not detected — skipping");
        return;
    };

    serial_println!("[spectre] Speculative execution mitigation detection:");

    // Read IA32_ARCH_CAPABILITIES if available.
    if features.arch_capabilities {
        let caps = unsafe { crate::cpu::rdmsr(IA32_ARCH_CAPABILITIES) };
        ARCH_CAPS.store(caps, Ordering::Release);

        let meltdown_immune = caps & ARCH_CAP_RDCL_NO != 0;
        let enhanced_ibrs = caps & ARCH_CAP_IBRS_ALL != 0;
        let ssb_immune = caps & ARCH_CAP_SSB_NO != 0;
        let mds_immune = caps & ARCH_CAP_MDS_NO != 0;

        MELTDOWN_IMMUNE.store(meltdown_immune, Ordering::Release);
        ENHANCED_IBRS.store(enhanced_ibrs, Ordering::Release);

        serial_println!("[spectre]   ARCH_CAPABILITIES={:#x}", caps);
        serial_println!("[spectre]   Meltdown-immune (RDCL_NO): {}",
            if meltdown_immune { "yes" } else { "NO — KPTI needed" });
        serial_println!("[spectre]   Enhanced IBRS (IBRS_ALL): {}",
            if enhanced_ibrs { "yes (zero-cost)" } else { "no" });
        serial_println!("[spectre]   SSB-immune (SSB_NO): {}",
            if ssb_immune { "yes" } else { "no" });
        serial_println!("[spectre]   MDS-immune (MDS_NO): {}",
            if mds_immune { "yes" } else { "no" });
    } else {
        serial_println!("[spectre]   IA32_ARCH_CAPABILITIES: not available");
        // Assume vulnerable (conservative).
    }

    // Determine what to enable.
    let has_ibrs = features.ibrs_ibpb || features.amd_ibrs;
    let has_stibp = features.stibp || features.amd_stibp;
    let has_ssbd = features.ssbd || features.amd_ssbd;

    // Build the SPEC_CTRL value to write.
    let mut spec_ctrl: u64 = 0;

    // Enable IBRS if available.
    // On CPUs with enhanced IBRS (IBRS_ALL), this is free.
    // On older CPUs, it has a small performance cost (~1-5%).
    if has_ibrs {
        spec_ctrl |= SPEC_CTRL_IBRS;
        IBRS_ACTIVE.store(true, Ordering::Release);
        serial_println!("[spectre]   Enabling IBRS (indirect branch restricted speculation)");
    } else {
        serial_println!("[spectre]   IBRS not available — retpoline is the fallback");
    }

    // Enable STIBP if SMT could be present.
    // On single-threaded CPUs this is a no-op but harmless.
    if has_stibp {
        spec_ctrl |= SPEC_CTRL_STIBP;
        STIBP_ACTIVE.store(true, Ordering::Release);
        serial_println!("[spectre]   Enabling STIBP (cross-thread prediction isolation)");
    } else {
        serial_println!("[spectre]   STIBP not available");
    }

    // Enable SSBD only if the CPU is actually vulnerable.
    // CPUs with SSB_NO are immune and don't need it.
    let ssb_immune = ARCH_CAPS.load(Ordering::Relaxed) & ARCH_CAP_SSB_NO != 0;
    if has_ssbd && !ssb_immune {
        spec_ctrl |= SPEC_CTRL_SSBD;
        SSBD_ACTIVE.store(true, Ordering::Release);
        serial_println!("[spectre]   Enabling SSBD (speculative store bypass disable)");
    } else if ssb_immune {
        serial_println!("[spectre]   SSBD not needed (CPU immune to SSB)");
    } else {
        serial_println!("[spectre]   SSBD not available");
    }

    // Write IA32_SPEC_CTRL if we have anything to set.
    if spec_ctrl != 0 && (has_ibrs || has_stibp || has_ssbd) {
        // SAFETY: MSR is available (CPUID confirmed IBRS/STIBP/SSBD support).
        // Writing SPEC_CTRL only restricts speculation — it cannot cause faults.
        unsafe { crate::cpu::wrmsr(IA32_SPEC_CTRL, spec_ctrl); }
        serial_println!("[spectre]   IA32_SPEC_CTRL = {:#x}", spec_ctrl);
    }

    // Issue an initial IBPB to flush any stale predictions from boot.
    if features.ibrs_ibpb || features.amd_ibpb {
        // SAFETY: Writing IBPB just flushes the branch predictor.
        unsafe { crate::cpu::wrmsr(IA32_PRED_CMD, 1); }
        IBPB_COUNT.fetch_add(1, Ordering::Relaxed);
        serial_println!("[spectre]   Initial IBPB barrier issued");
    }

    serial_println!("[spectre] Mitigation setup complete");
}

/// Apply speculation mitigations on an Application Processor.
///
/// Replicates the BSP's IA32_SPEC_CTRL settings.  Each CPU has its own
/// MSR instance, so each AP must write independently.
pub fn init_ap() {
    let Some(features) = crate::cpu::features() else { return };

    let has_ibrs = features.ibrs_ibpb || features.amd_ibrs;
    let has_stibp = features.stibp || features.amd_stibp;
    let has_ssbd = features.ssbd || features.amd_ssbd;

    let mut spec_ctrl: u64 = 0;
    if has_ibrs && IBRS_ACTIVE.load(Ordering::Relaxed) {
        spec_ctrl |= SPEC_CTRL_IBRS;
    }
    if has_stibp && STIBP_ACTIVE.load(Ordering::Relaxed) {
        spec_ctrl |= SPEC_CTRL_STIBP;
    }
    if has_ssbd && SSBD_ACTIVE.load(Ordering::Relaxed) {
        spec_ctrl |= SPEC_CTRL_SSBD;
    }

    if spec_ctrl != 0 {
        // SAFETY: Same as BSP — MSR is confirmed available.
        unsafe { crate::cpu::wrmsr(IA32_SPEC_CTRL, spec_ctrl); }
    }

    // IBPB on AP startup to ensure clean predictor state.
    if features.ibrs_ibpb || features.amd_ibpb {
        // SAFETY: Writing IBPB flushes the branch predictor.
        unsafe { crate::cpu::wrmsr(IA32_PRED_CMD, 1); }
        IBPB_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Runtime operations
// ---------------------------------------------------------------------------

/// Issue an Indirect Branch Prediction Barrier (IBPB).
///
/// Flushes all indirect branch predictions.  Should be called on
/// security-sensitive context switches (e.g., between different
/// processes/security domains) to prevent cross-process branch
/// injection attacks.
///
/// No-op if IBPB is not supported.
#[inline]
pub fn ibpb_barrier() {
    let Some(features) = crate::cpu::features() else { return };
    if features.ibrs_ibpb || features.amd_ibpb {
        // SAFETY: IBPB is a write-only command that flushes predictions.
        // Cannot fault, has no side effects beyond clearing the predictor.
        unsafe { crate::cpu::wrmsr(IA32_PRED_CMD, 1); }
        IBPB_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Status and diagnostics
// ---------------------------------------------------------------------------

/// Speculation mitigation status.
#[derive(Debug, Clone)]
pub struct SpectreStatus {
    /// IBRS is active.
    pub ibrs_active: bool,
    /// STIBP is active.
    pub stibp_active: bool,
    /// SSBD is active.
    pub ssbd_active: bool,
    /// Enhanced IBRS (hardware always-on, zero cost).
    pub enhanced_ibrs: bool,
    /// CPU is immune to Meltdown (no KPTI needed).
    pub meltdown_immune: bool,
    /// Number of IBPB barriers issued since boot.
    pub ibpb_count: u64,
    /// Raw IA32_ARCH_CAPABILITIES value.
    pub arch_caps: u64,
    /// Hardware supports IBRS/IBPB.
    pub hw_ibrs: bool,
    /// Hardware supports STIBP.
    pub hw_stibp: bool,
    /// Hardware supports SSBD.
    pub hw_ssbd: bool,
}

/// Query current speculation mitigation status.
pub fn status() -> SpectreStatus {
    let features = crate::cpu::features();
    let (hw_ibrs, hw_stibp, hw_ssbd) = features
        .map(|f| (
            f.ibrs_ibpb || f.amd_ibrs,
            f.stibp || f.amd_stibp,
            f.ssbd || f.amd_ssbd,
        ))
        .unwrap_or((false, false, false));

    SpectreStatus {
        ibrs_active: IBRS_ACTIVE.load(Ordering::Relaxed),
        stibp_active: STIBP_ACTIVE.load(Ordering::Relaxed),
        ssbd_active: SSBD_ACTIVE.load(Ordering::Relaxed),
        enhanced_ibrs: ENHANCED_IBRS.load(Ordering::Relaxed),
        meltdown_immune: MELTDOWN_IMMUNE.load(Ordering::Relaxed),
        ibpb_count: IBPB_COUNT.load(Ordering::Relaxed),
        arch_caps: ARCH_CAPS.load(Ordering::Relaxed),
        hw_ibrs,
        hw_stibp,
        hw_ssbd,
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test for speculation mitigations.
pub fn self_test() {
    serial_println!("[spectre] Running self-test...");

    // Test 1: Status query works.
    let s = status();
    serial_println!("[spectre]   IBRS: hw={}, active={}", s.hw_ibrs, s.ibrs_active);
    serial_println!("[spectre]   STIBP: hw={}, active={}", s.hw_stibp, s.stibp_active);
    serial_println!("[spectre]   SSBD: hw={}, active={}", s.hw_ssbd, s.ssbd_active);

    // Test 2: If IBRS is active, verify IA32_SPEC_CTRL has IBRS bit set.
    if s.ibrs_active {
        let val = unsafe { crate::cpu::rdmsr(IA32_SPEC_CTRL) };
        assert!(
            val & SPEC_CTRL_IBRS != 0,
            "IBRS reported active but IA32_SPEC_CTRL.IBRS=0"
        );
        serial_println!("[spectre]   IA32_SPEC_CTRL readback: IBRS bit set: OK");
    }

    // Test 3: If STIBP is active, verify it's set.
    if s.stibp_active {
        let val = unsafe { crate::cpu::rdmsr(IA32_SPEC_CTRL) };
        assert!(
            val & SPEC_CTRL_STIBP != 0,
            "STIBP reported active but IA32_SPEC_CTRL.STIBP=0"
        );
        serial_println!("[spectre]   IA32_SPEC_CTRL readback: STIBP bit set: OK");
    }

    // Test 4: IBPB count is at least 1 (initial barrier).
    if s.hw_ibrs {
        assert!(
            s.ibpb_count >= 1,
            "IBPB supported but no barriers issued"
        );
        serial_println!("[spectre]   IBPB barriers: {} (initial + APs): OK", s.ibpb_count);
    }

    // Test 5: ibpb_barrier() doesn't fault.
    ibpb_barrier();
    serial_println!("[spectre]   ibpb_barrier(): OK (no fault)");

    serial_println!("[spectre] Self-test PASSED");
}
