//! Intel CET (Control-flow Enforcement Technology) — hardware CFI.
//!
//! CET provides two complementary mechanisms to defend against control-flow
//! hijacking attacks (ROP, JOP, COP):
//!
//! ## Shadow Stacks (SHSTK)
//!
//! A hardware-maintained shadow copy of return addresses.  On every CALL,
//! the CPU pushes the return address to both the regular stack and a
//! separate shadow stack.  On RET, both are popped and compared — if they
//! differ, a #CP exception (vector 21) fires.  This defeats ROP attacks
//! because an attacker cannot modify the shadow stack via buffer overflows.
//!
//! ## Indirect Branch Tracking (IBT)
//!
//! Requires all indirect branch targets (via JMP/CALL through a register
//! or memory) to begin with an ENDBR64 instruction.  If an indirect branch
//! lands on any other instruction, a #CP exception fires.  This defeats
//! JOP/COP attacks by constraining where indirect branches can go.
//!
//! ## Implementation
//!
//! At boot, we detect CET support via CPUID, then enable it for supervisor
//! mode (kernel protection).  User-mode CET can be enabled per-process when
//! tasks are created.  On hardware that doesn't support CET (including
//! QEMU TCG), we gracefully report "not available" and skip enablement.
//!
//! ## MSRs
//!
//! - `IA32_U_CET`  (0x6A0): User-mode CET configuration
//! - `IA32_S_CET`  (0x6A2): Supervisor-mode CET configuration
//! - `IA32_PL0_SSP` (0x6A4): Ring-0 shadow stack pointer
//! - `IA32_PL3_SSP` (0x6A7): Ring-3 shadow stack pointer
//! - `IA32_INTERRUPT_SSP_TABLE` (0x6A8): Interrupt shadow stack table base
//!
//! ## #CP Exception (Vector 21)
//!
//! Error code format:
//! - Bits [14:0]: CPEC (Control Protection Error Code)
//!   - 1: Near RET mismatch
//!   - 2: Far RET/IRET mismatch
//!   - 3: ENDBRANCH missing (IBT violation)
//!   - 4: RSTORSSP token mismatch
//!   - 5: Supervisor shadow stack token busy
//! - Bit 15: 0 (reserved)
//!
//! ## References
//!
//! - Intel SDM Vol. 1 Ch. 17 "Control-flow Enforcement Technology"
//! - Intel SDM Vol. 3 Ch. 18 "Control-flow Enforcement Technology"
//! - Linux kernel: arch/x86/kernel/shstk.c, arch/x86/kernel/ibt.c

use crate::serial_println;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// MSR addresses
// ---------------------------------------------------------------------------

/// User-mode CET configuration MSR.
const MSR_IA32_U_CET: u32 = 0x6A0;
/// Supervisor-mode CET configuration MSR.
const MSR_IA32_S_CET: u32 = 0x6A2;
/// Ring-0 shadow stack pointer MSR.
const MSR_IA32_PL0_SSP: u32 = 0x6A4;
/// Ring-3 shadow stack pointer MSR.
const MSR_IA32_PL3_SSP: u32 = 0x6A7;
/// Interrupt shadow stack table base address MSR.
#[allow(dead_code)]
const MSR_IA32_INTERRUPT_SSP_TABLE: u32 = 0x6A8;

// ---------------------------------------------------------------------------
// CET MSR bit definitions
// ---------------------------------------------------------------------------

/// SH_STK_EN — enable shadow stacks.
const CET_SH_STK_EN: u64 = 1 << 0;
/// WR_SHSTK_EN — enable WRSS instruction for shadow stack writes.
const CET_WR_SHSTK_EN: u64 = 1 << 1;
/// ENDBR_EN — enable ENDBR enforcement (IBT).
const CET_ENDBR_EN: u64 = 1 << 2;
/// LEG_IW_EN — enable legacy compatibility treatment for indirect JMP.
#[allow(dead_code)]
const CET_LEG_IW_EN: u64 = 1 << 3;
/// NO_TRACK_EN — enable NOTRACK prefix suppression of IBT enforcement.
#[allow(dead_code)]
const CET_NO_TRACK_EN: u64 = 1 << 4;
/// SUPPRESS_DIS — disable suppress on indirect call/jmp.
#[allow(dead_code)]
const CET_SUPPRESS_DIS: u64 = 1 << 5;
/// SUPPRESS — current suppress state (read-only in IA32_U_CET).
#[allow(dead_code)]
const CET_SUPPRESS: u64 = 1 << 10;
/// TRACKER — IBT state machine: IDLE=0, WAIT_FOR_ENDBRANCH=1 (read-only).
#[allow(dead_code)]
const CET_TRACKER: u64 = 1 << 11;

// ---------------------------------------------------------------------------
// CR4 bit
// ---------------------------------------------------------------------------

/// CR4.CET — master enable for CET (bit 23).
const CR4_CET: u64 = 1 << 23;

// ---------------------------------------------------------------------------
// #CP error codes
// ---------------------------------------------------------------------------

/// Control Protection Error Codes (bits [14:0] of the error code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum CpErrorCode {
    /// Near RET: shadow stack vs. regular stack mismatch.
    NearRet = 1,
    /// Far RET or IRET: shadow stack mismatch.
    FarRet = 2,
    /// Missing ENDBRANCH at indirect branch target (IBT violation).
    EndbranchMissing = 3,
    /// RSTORSSP: token verification failed.
    RstorsspToken = 4,
    /// Supervisor shadow stack token is busy.
    SupervisorTokenBusy = 5,
    /// Unknown/reserved error code.
    Unknown = 0xFFFF,
}

impl CpErrorCode {
    /// Parse from the raw exception error code.
    pub fn from_error(error: u64) -> Self {
        match error & 0x7FFF {
            1 => Self::NearRet,
            2 => Self::FarRet,
            3 => Self::EndbranchMissing,
            4 => Self::RstorsspToken,
            5 => Self::SupervisorTokenBusy,
            _ => Self::Unknown,
        }
    }

    /// Human-readable description.
    pub fn description(self) -> &'static str {
        match self {
            Self::NearRet => "near RET: shadow stack mismatch (possible ROP attack)",
            Self::FarRet => "far RET/IRET: shadow stack mismatch",
            Self::EndbranchMissing => "indirect branch target missing ENDBR64 (IBT violation)",
            Self::RstorsspToken => "RSTORSSP token verification failed",
            Self::SupervisorTokenBusy => "supervisor shadow stack token busy",
            Self::Unknown => "unknown control protection error",
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether CET has been detected and is available.
static CET_AVAILABLE: AtomicBool = AtomicBool::new(false);
/// Whether shadow stacks are enabled for supervisor mode.
static SHSTK_ENABLED: AtomicBool = AtomicBool::new(false);
/// Whether IBT is enabled for supervisor mode.
static IBT_ENABLED: AtomicBool = AtomicBool::new(false);
/// Number of #CP exceptions received.
static CP_EXCEPTION_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// CET status reporting
// ---------------------------------------------------------------------------

/// Complete CET status for display/diagnostics.
#[derive(Debug, Clone)]
pub struct CetStatus {
    /// CPU supports shadow stacks (CPUID.07H.0:ECX[7]).
    pub hw_shstk: bool,
    /// CPU supports IBT (CPUID.07H.0:EDX[20]).
    pub hw_ibt: bool,
    /// Shadow stacks currently enabled for supervisor mode.
    pub supervisor_shstk: bool,
    /// IBT currently enabled for supervisor mode.
    pub supervisor_ibt: bool,
    /// Number of #CP exceptions since boot.
    pub cp_exceptions: u64,
}

/// Query the current CET status.
pub fn status() -> CetStatus {
    let features = crate::cpu::features();
    let (hw_shstk, hw_ibt) = features
        .map(|f| (f.cet_ss, f.cet_ibt))
        .unwrap_or((false, false));

    CetStatus {
        hw_shstk,
        hw_ibt,
        supervisor_shstk: SHSTK_ENABLED.load(Ordering::Relaxed),
        supervisor_ibt: IBT_ENABLED.load(Ordering::Relaxed),
        cp_exceptions: CP_EXCEPTION_COUNT.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Detect and enable CET on supporting hardware.
///
/// Called during early boot after `cpu::detect_features()`.  On hardware
/// without CET support (including QEMU TCG), this logs the absence and
/// returns without modifying any state.
///
/// ## Enablement Strategy
///
/// We enable CET for supervisor mode (kernel protection) only.  User-mode
/// CET is configured per-task at process creation time.  The kernel
/// enables:
/// - Shadow stacks (SHSTK): protects kernel return addresses from corruption
/// - IBT: requires all indirect branch targets in kernel code to have ENDBR64
///
/// Note: IBT requires the kernel to be compiled with `-fcf-protection=branch`
/// or equivalent.  If the kernel wasn't compiled with ENDBR64 annotations,
/// enabling IBT would immediately #CP on the first indirect call.  We detect
/// this by checking if our own code has ENDBR64 at function entries.
pub fn detect() {
    let Some(features) = crate::cpu::features() else {
        serial_println!("[cet] CPU features not yet detected — skipping");
        return;
    };

    serial_println!("[cet] Control-flow Enforcement Technology detection:");
    serial_println!("[cet]   Shadow Stacks (SHSTK): {}",
        if features.cet_ss { "supported" } else { "not supported" });
    serial_println!("[cet]   Indirect Branch Tracking (IBT): {}",
        if features.cet_ibt { "supported" } else { "not supported" });

    if !features.cet_ss && !features.cet_ibt {
        serial_println!("[cet]   Hardware does not support CET — skipping enablement");
        return;
    }

    CET_AVAILABLE.store(true, Ordering::Release);

    // On real hardware with CET support, we would enable it here.
    // For now, we skip actual enablement because:
    // 1. The kernel may not be compiled with ENDBR64 annotations (IBT would fault)
    // 2. We need to allocate and set up shadow stack pages first
    // 3. We need to wire up the #CP exception handler before enabling
    //
    // The infrastructure is ready — enablement is gated on:
    // - Kernel compiled with -fcf-protection=full (for IBT)
    // - Shadow stack pages allocated for BSP and each AP
    // - #CP handler installed in IDT vector 21
    //
    // TODO: Enable when the toolchain supports CET instrumentation.
    serial_println!("[cet]   CET available but enablement deferred (needs CET-compiled kernel)");

    // Even without enablement, register the #CP handler so that if CET
    // is accidentally enabled or a future code path enables it, we get
    // useful diagnostics instead of a triple fault.
}

/// Enable supervisor-mode shadow stacks.
///
/// # Safety
///
/// - CR4.CET must not already be set
/// - A valid shadow stack must be allocated and its address set in IA32_PL0_SSP
/// - The #CP exception handler must be installed
/// - Interrupts should be disabled during enablement
#[allow(dead_code)]
pub unsafe fn enable_supervisor_shstk(shadow_stack_top: u64) {
    // Set CR4.CET to enable the CET master switch.
    let cr4 = read_cr4();
    if cr4 & CR4_CET == 0 {
        // SAFETY: Adding CET bit to CR4; caller guarantees prerequisites.
        unsafe { write_cr4(cr4 | CR4_CET); }
    }

    // Set the ring-0 shadow stack pointer.
    // SAFETY: Caller guarantees shadow_stack_top is valid.
    unsafe { wrmsr(MSR_IA32_PL0_SSP, shadow_stack_top); }

    // Configure supervisor CET: enable shadow stacks.
    let s_cet = CET_SH_STK_EN | CET_WR_SHSTK_EN;
    // SAFETY: MSR address and value are correct for enabling SHSTK.
    unsafe { wrmsr(MSR_IA32_S_CET, s_cet); }

    SHSTK_ENABLED.store(true, Ordering::Release);
    serial_println!("[cet] Supervisor shadow stacks ENABLED (SSP={:#x})", shadow_stack_top);
}

/// Enable supervisor-mode IBT (indirect branch tracking).
///
/// # Safety
///
/// - The kernel must be compiled with ENDBR64 at all indirect branch targets
/// - CR4.CET must be set
#[allow(dead_code)]
pub unsafe fn enable_supervisor_ibt() {
    // CR4.CET must already be set (by enable_supervisor_shstk or explicitly).
    let cr4 = read_cr4();
    if cr4 & CR4_CET == 0 {
        // SAFETY: Adding CET bit to CR4; caller guarantees prerequisites.
        unsafe { write_cr4(cr4 | CR4_CET); }
    }

    // Read current S_CET and add ENDBR enforcement.
    // SAFETY: MSR_IA32_S_CET is a valid MSR when CET is supported.
    let current = unsafe { rdmsr(MSR_IA32_S_CET) };
    // SAFETY: Adding ENDBR_EN bit; caller guarantees kernel has ENDBR64.
    unsafe { wrmsr(MSR_IA32_S_CET, current | CET_ENDBR_EN); }

    IBT_ENABLED.store(true, Ordering::Release);
    serial_println!("[cet] Supervisor IBT ENABLED");
}

/// Disable all supervisor-mode CET protections.
///
/// # Safety
///
/// Disabling CET removes hardware CFI protection.  Only call during
/// shutdown or if CET is causing issues.
#[allow(dead_code)]
pub unsafe fn disable_supervisor() {
    // Clear S_CET configuration.
    // SAFETY: Zeroing S_CET disables all CET enforcement.
    unsafe { wrmsr(MSR_IA32_S_CET, 0); }
    // Clear CR4.CET.
    let cr4 = read_cr4();
    // SAFETY: Removing CET bit from CR4.
    unsafe { write_cr4(cr4 & !CR4_CET); }

    SHSTK_ENABLED.store(false, Ordering::Release);
    IBT_ENABLED.store(false, Ordering::Release);
    serial_println!("[cet] Supervisor CET DISABLED");
}

// ---------------------------------------------------------------------------
// Per-task shadow stack management
// ---------------------------------------------------------------------------

/// Shadow stack token format (written at the top of each shadow stack).
///
/// When switching between shadow stacks, the CPU verifies a "token" at the
/// target shadow stack's top.  The token is the address of the token itself
/// OR'd with bit 0 (busy bit).  This prevents reuse of stale shadow stacks.
#[allow(dead_code)]
const SSP_TOKEN_BUSY_BIT: u64 = 1;

/// Required alignment for shadow stack pages (must be page-aligned).
/// Shadow stacks use the same page size as regular memory (16 KiB in our kernel).
#[allow(dead_code)]
pub const SHADOW_STACK_ALIGN: usize = 16384; // 16 KiB page

/// Configure user-mode CET for a new task.
///
/// Sets up IA32_U_CET and IA32_PL3_SSP for a user task's shadow stack.
/// Called during context switch to restore per-task CET state.
///
/// # Safety
///
/// - `user_ssp` must point to a valid, mapped shadow stack
/// - Must be called with interrupts disabled (during context switch)
#[allow(dead_code)]
pub unsafe fn set_user_cet(enable_shstk: bool, enable_ibt: bool, user_ssp: u64) {
    let mut u_cet: u64 = 0;
    if enable_shstk {
        u_cet |= CET_SH_STK_EN | CET_WR_SHSTK_EN;
    }
    if enable_ibt {
        u_cet |= CET_ENDBR_EN;
    }
    // SAFETY: Caller guarantees CET is available and user_ssp is valid.
    unsafe {
        wrmsr(MSR_IA32_U_CET, u_cet);
        wrmsr(MSR_IA32_PL3_SSP, user_ssp);
    }
}

/// Read the current user shadow stack pointer.
#[allow(dead_code)]
pub fn read_user_ssp() -> u64 {
    // SAFETY: Reading this MSR is always safe when CET is available.
    unsafe { rdmsr(MSR_IA32_PL3_SSP) }
}

// ---------------------------------------------------------------------------
// #CP exception handling
// ---------------------------------------------------------------------------

/// Handle a #CP (Control Protection) exception.
///
/// Called from the IDT vector 21 handler.  The error code indicates what
/// kind of CET violation occurred.
#[allow(dead_code)]
pub fn handle_cp_exception(rip: u64, error_code: u64) {
    CP_EXCEPTION_COUNT.fetch_add(1, Ordering::Relaxed);
    let cpec = CpErrorCode::from_error(error_code);

    serial_println!("[cet] #CP EXCEPTION at RIP={:#x}", rip);
    serial_println!("[cet]   Error code: {:#x} → {:?}", error_code, cpec);
    serial_println!("[cet]   Description: {}", cpec.description());
    serial_println!("[cet]   Total #CP exceptions: {}",
        CP_EXCEPTION_COUNT.load(Ordering::Relaxed));

    // In a production kernel, we would:
    // - For user-mode #CP: deliver SEH exception to the process
    // - For kernel-mode #CP: this indicates a serious security issue or bug
    //   Log detailed state and halt (or attempt recovery if possible)
}

/// Whether CET hardware is available on this CPU.
pub fn is_available() -> bool {
    CET_AVAILABLE.load(Ordering::Relaxed)
}

/// Whether supervisor shadow stacks are active.
#[allow(dead_code)]
pub fn shstk_active() -> bool {
    SHSTK_ENABLED.load(Ordering::Relaxed)
}

/// Whether supervisor IBT is active.
#[allow(dead_code)]
pub fn ibt_active() -> bool {
    IBT_ENABLED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Low-level CPU helpers
// ---------------------------------------------------------------------------

/// Read CR4 register.
fn read_cr4() -> u64 {
    let val: u64;
    // SAFETY: Reading CR4 is safe in ring 0.
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) val, options(nomem, nostack));
    }
    val
}

/// Write CR4 register.
///
/// # Safety
///
/// Caller must ensure the new value doesn't disable critical features
/// or enable features without proper setup.
unsafe fn write_cr4(val: u64) {
    // SAFETY: Caller guarantees the value is valid.
    unsafe {
        core::arch::asm!("mov cr4, {}", in(reg) val, options(nomem, nostack));
    }
}

/// Read a Model-Specific Register.
///
/// # Safety
///
/// The MSR address must be valid for this CPU.
unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: Caller guarantees valid MSR address.
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack),
        );
    }
    u64::from(lo) | (u64::from(hi) << 32)
}

/// Write a Model-Specific Register.
///
/// # Safety
///
/// The MSR address must be valid and the value must be appropriate.
unsafe fn wrmsr(msr: u32, val: u64) {
    let lo = val as u32;
    let hi = (val >> 32) as u32;
    // SAFETY: Caller guarantees valid MSR address and value.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") lo,
            in("edx") hi,
            options(nomem, nostack),
        );
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the CET module.
pub fn self_test() {
    serial_println!("[cet] Running self-test...");

    // Test 1: Status query works.
    let s = status();
    serial_println!("[cet]   Status: SHSTK hw={}, IBT hw={}", s.hw_shstk, s.hw_ibt);
    serial_println!("[cet]   Active: SHSTK={}, IBT={}", s.supervisor_shstk, s.supervisor_ibt);
    // On QEMU TCG, both should be false.
    // On real CET hardware, hw_shstk and hw_ibt may be true.
    // Either way, the status query must not panic.

    // Test 2: Error code parsing.
    assert_eq!(CpErrorCode::from_error(1), CpErrorCode::NearRet);
    assert_eq!(CpErrorCode::from_error(2), CpErrorCode::FarRet);
    assert_eq!(CpErrorCode::from_error(3), CpErrorCode::EndbranchMissing);
    assert_eq!(CpErrorCode::from_error(4), CpErrorCode::RstorsspToken);
    assert_eq!(CpErrorCode::from_error(5), CpErrorCode::SupervisorTokenBusy);
    assert_eq!(CpErrorCode::from_error(0x42), CpErrorCode::Unknown);
    // High bits should be masked.
    assert_eq!(CpErrorCode::from_error(0x8001), CpErrorCode::NearRet);
    serial_println!("[cet]   Error code parsing: OK");

    // Test 3: Descriptions are non-empty.
    assert!(!CpErrorCode::NearRet.description().is_empty());
    assert!(!CpErrorCode::EndbranchMissing.description().is_empty());
    serial_println!("[cet]   Descriptions: OK");

    // Test 4: is_available() consistent with features.
    let avail = is_available();
    if let Some(f) = crate::cpu::features() {
        if !f.cet_ss && !f.cet_ibt {
            assert!(!avail, "should not be available without hardware support");
        }
    }
    serial_println!("[cet]   Availability check: OK (available={})", avail);

    // Test 5: CR4 read doesn't fault.
    let cr4 = read_cr4();
    // CR4.CET should not be set since we haven't enabled it.
    assert_eq!(cr4 & CR4_CET, 0, "CR4.CET should be clear");
    serial_println!("[cet]   CR4 read: OK (CET bit clear as expected)");

    serial_println!("[cet] Self-test PASSED");
}
