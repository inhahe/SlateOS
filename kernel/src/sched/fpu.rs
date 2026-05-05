//! FPU/SSE state management for context switching.
//!
//! On x86_64, the SSE2 instruction set is always available and the Rust
//! compiler may use XMM registers for any code (auto-vectorization,
//! memcpy/memset optimizations, register allocation for small structs).
//! Without explicit save/restore across context switches, one task's
//! XMM register state can silently corrupt another task's computations.
//!
//! ## Implementation
//!
//! We use **eager FPU save/restore** (always save on switch-out, restore
//! on switch-in).  This matches modern Linux (since 4.2, 2015) which
//! abandoned lazy FPU switching because:
//!
//! 1. The cost of `fxsave`/`fxrstor` (~100 cycles) is less than the
//!    #NM exception + handler overhead for lazy switching.
//! 2. Lazy switching leaks FPU state across security boundaries
//!    (LazyFP / CVE-2018-3665).
//! 3. Modern kernels use SSE pervasively (memcpy, crypto, checksums).
//!
//! ## Hardware Configuration
//!
//! - `CR0.EM` = 0 (no x87 emulation)
//! - `CR0.TS` = 0 (no task-switched #NM)
//! - `CR0.MP` = 1 (monitor coprocessor)
//! - `CR4.OSFXSR` = 1 (enable FXSAVE/FXRSTOR)
//! - `CR4.OSXMMEXCPT` = 1 (enable #XF for unmasked SSE exceptions)
//!
//! Both `init_bsp()` and `init_ap()` configure these bits.  The Limine
//! bootloader sets them for the BSP, but AP cores start from INIT state
//! (CR4 = 0) so they need explicit setup.
//!
//! ## Save Area
//!
//! FXSAVE/FXRSTOR uses a 512-byte, 16-byte-aligned region containing:
//! - x87 FPU state (FCW, FSW, FTW, FOP, FIP, FDP, ST0-ST7)
//! - SSE state (MXCSR, XMM0-XMM15)
//!
//! Each task carries its own [`FpuState`] buffer.  New tasks start with
//! a clean state (default FCW=0x037F, MXCSR=0x1F80).
//!
//! ## Performance
//!
//! `fxsave64` + `fxrstor64` together cost ~150-200 cycles on modern
//! x86_64 CPUs (~50-70ns at 3 GHz).  This adds < 2% to our context
//! switch budget (target < 5 µs).

use crate::serial_println;
use core::arch::asm;

// ---------------------------------------------------------------------------
// FPU state save area
// ---------------------------------------------------------------------------

/// FXSAVE64 save area layout offsets.
///
/// See Intel SDM Vol. 1, Table 10-2 "Layout of the FXSAVE Area".
const FCW_OFFSET: usize = 0;    // x87 FPU Control Word (16 bits)
const MXCSR_OFFSET: usize = 24; // MXCSR register (32 bits)

/// Default x87 FPU Control Word.
///
/// 0x037F = all x87 exceptions masked, precision=double-extended (64-bit),
/// rounding=round-to-nearest.  This is the value set by `FNINIT`.
const DEFAULT_FCW: u16 = 0x037F;

/// Default MXCSR (SSE control/status register).
///
/// 0x1F80 = all SSE exceptions masked, round-to-nearest, no flags set.
/// This matches the architectural reset value.
const DEFAULT_MXCSR: u32 = 0x1F80;

/// FPU/SSE state for a single task.
///
/// Must be 16-byte aligned for `fxsave64`/`fxrstor64`.
/// Contains the full x87 + SSE register file (512 bytes).
///
/// This is embedded directly in the [`Task`](super::task::Task) struct.
/// The alignment requirement propagates to the containing struct and is
/// satisfied by the heap allocator (which uses power-of-2 size classes
/// with natural alignment).
#[derive(Clone)]
#[repr(C, align(16))]
pub struct FpuState {
    /// Raw 512-byte FXSAVE area.
    data: [u8; 512],
}

impl FpuState {
    /// Create a clean initial FPU state.
    ///
    /// Sets the x87 FCW and MXCSR to their standard default values.
    /// All registers (ST0-ST7, XMM0-XMM15) are zeroed.
    ///
    /// This is the state a new task starts with — equivalent to the
    /// hardware state after FNINIT + LDMXCSR(0x1F80).
    #[must_use]
    pub fn new_default() -> Self {
        let mut state = Self { data: [0u8; 512] };

        // Set FCW at offset 0 (16-bit little-endian).
        let fcw_bytes = DEFAULT_FCW.to_le_bytes();
        state.data[FCW_OFFSET] = fcw_bytes[0];
        state.data[FCW_OFFSET + 1] = fcw_bytes[1];

        // Set MXCSR at offset 24 (32-bit little-endian).
        let mxcsr_bytes = DEFAULT_MXCSR.to_le_bytes();
        state.data[MXCSR_OFFSET] = mxcsr_bytes[0];
        state.data[MXCSR_OFFSET + 1] = mxcsr_bytes[1];
        state.data[MXCSR_OFFSET + 2] = mxcsr_bytes[2];
        state.data[MXCSR_OFFSET + 3] = mxcsr_bytes[3];

        state
    }

    /// Get a raw pointer to the save area.
    ///
    /// Available for subsystems that need direct access to the FXSAVE
    /// buffer (e.g., process state dump, debug inspection).
    #[must_use]
    #[inline(always)]
    #[allow(dead_code)]
    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    /// Get a raw mutable pointer to the save area.
    ///
    /// Available for subsystems that need direct access to the FXSAVE
    /// buffer (e.g., signal frame construction, ptrace).
    #[must_use]
    #[inline(always)]
    #[allow(dead_code)]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }
}

// Statically assert alignment.
const _: () = {
    assert!(core::mem::align_of::<FpuState>() >= 16);
    assert!(core::mem::size_of::<FpuState>() == 512);
};

// ---------------------------------------------------------------------------
// Hardware initialization
// ---------------------------------------------------------------------------

/// Initialize FPU/SSE on the BSP (Boot Strap Processor).
///
/// Ensures CR0 and CR4 are configured for SSE operation and verifies
/// that the hardware supports FXSAVE/FXRSTOR.
///
/// Called once during early boot, before the scheduler starts.
pub fn init_bsp() {
    configure_fpu_cr_bits();

    // Verify FXSAVE support via CPUID.
    let has_fxsr = cpuid_has_fxsr();
    assert!(has_fxsr, "CPU does not support FXSAVE/FXRSTOR (impossible on x86_64)");

    // Initialize x87 FPU to known state.
    // SAFETY: We've verified the FPU hardware is present and enabled.
    unsafe { asm!("fninit", options(nomem, nostack)); }

    // Set MXCSR to default (all exceptions masked).
    let default_mxcsr: u32 = DEFAULT_MXCSR;
    // SAFETY: MXCSR is a valid SSE control register, we're setting a safe value.
    unsafe { asm!("ldmxcsr [{}]", in(reg) &default_mxcsr, options(nostack)); }

    serial_println!("[fpu] BSP FPU/SSE initialized (FXSAVE ready)");
}

/// Initialize FPU/SSE on an Application Processor.
///
/// APs start from INIT state with CR4 = 0 (no OSFXSR), so SSE
/// instructions would #UD without this.  Called from `ap_entry()`.
///
/// # Safety
///
/// Must be called in kernel mode with interrupts disabled (normal
/// AP startup context).
pub unsafe fn init_ap() {
    configure_fpu_cr_bits();

    // Initialize x87 FPU to known state.
    // SAFETY: CR0/CR4 are now configured for FPU/SSE.
    unsafe { asm!("fninit", options(nomem, nostack)); }

    // Set MXCSR to default.
    let default_mxcsr: u32 = DEFAULT_MXCSR;
    // SAFETY: SSE is now enabled via CR4.OSFXSR.
    unsafe { asm!("ldmxcsr [{}]", in(reg) &default_mxcsr, options(nostack)); }
}

/// Configure CR0 and CR4 for FPU/SSE operation.
///
/// Sets:
/// - CR0: clear EM (bit 2), clear TS (bit 3), set MP (bit 1)
/// - CR4: set OSFXSR (bit 9), set OSXMMEXCPT (bit 10)
fn configure_fpu_cr_bits() {
    // SAFETY: We're modifying control registers to enable hardware
    // features.  This is standard OS initialization — no side effects
    // beyond enabling the FPU/SSE hardware.
    unsafe {
        // CR0: clear EM and TS, set MP.
        //   EM=0: Don't trap x87 instructions to software emulator.
        //   TS=0: Don't generate #NM on FPU/SSE use (eager switching).
        //   MP=1: WAIT/FWAIT checks for pending x87 exceptions.
        let cr0: u64;
        asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
        let cr0 = (cr0 & !(1 << 2) & !(1 << 3)) | (1 << 1);
        asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack));

        // CR4: set OSFXSR and OSXMMEXCPT.
        //   OSFXSR (bit 9): Enable FXSAVE/FXRSTOR and SSE instructions.
        //   OSXMMEXCPT (bit 10): Route unmasked SSE exceptions to #XF
        //   (vector 19) instead of #UD.
        let cr4: u64;
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        let cr4 = cr4 | (1 << 9) | (1 << 10);
        asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));
    }
}

/// Check CPUID for FXSAVE/FXRSTOR support (leaf 1, EDX bit 24).
///
/// On x86_64, this is always true (SSE2 is mandatory), but we
/// verify defensively.
fn cpuid_has_fxsr() -> bool {
    let edx: u32;
    // SAFETY: CPUID leaf 1 is always valid on x86_64.
    // We save/restore rbx manually because LLVM reserves it
    // (rbx is the GOT pointer in position-independent code) and
    // won't allow it as an asm operand.
    unsafe {
        asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "pop rbx",
            out("edx") edx,
            out("eax") _,
            out("ecx") _,
            options(nomem, nostack),
        );
    }
    edx & (1 << 24) != 0
}

// ---------------------------------------------------------------------------
// Save / restore (used from context switch assembly via raw pointers)
// ---------------------------------------------------------------------------

/// Save the current CPU's FPU/SSE state to the given buffer.
///
/// # Safety
///
/// - `state` must point to a valid, 16-byte-aligned 512-byte buffer.
/// - Must be called with the CPU's FPU state being the state to save
///   (i.e., no other save/restore has intervened since the target task
///   last ran).
#[inline(always)]
pub unsafe fn save(state: *mut FpuState) {
    // SAFETY: Caller guarantees state is valid and aligned.
    // fxsave64 is the 64-bit variant that saves the full 64-bit
    // FIP/FDP pointers (vs fxsave which only saves 32-bit).
    unsafe {
        asm!(
            "fxsave64 [{}]",
            in(reg) state,
            options(nostack),
        );
    }
}

/// Restore FPU/SSE state from the given buffer to the CPU.
///
/// # Safety
///
/// - `state` must point to a valid, 16-byte-aligned 512-byte buffer
///   containing a previously-saved FPU state (or a default-initialized
///   state from [`FpuState::new_default()`]).
/// - After this call, the CPU's FPU registers reflect the saved state.
#[inline(always)]
pub unsafe fn restore(state: *const FpuState) {
    // SAFETY: Caller guarantees state is valid, aligned, and contains
    // a valid FXSAVE image.
    unsafe {
        asm!(
            "fxrstor64 [{}]",
            in(reg) state,
            options(nostack),
        );
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test for FPU state save/restore.
///
/// Tests:
/// 1. Verify FPU state can be saved and restored.
/// 2. Verify default state has correct FCW/MXCSR values.
/// 3. Write a known pattern to XMM registers, save, modify, restore, verify.
pub fn self_test() {
    serial_println!("[fpu] Running FPU/SSE self-test...");

    // Test 1: Default state has correct control values.
    let state = FpuState::new_default();
    let fcw = u16::from_le_bytes([state.data[FCW_OFFSET], state.data[FCW_OFFSET + 1]]);
    let mxcsr = u32::from_le_bytes([
        state.data[MXCSR_OFFSET],
        state.data[MXCSR_OFFSET + 1],
        state.data[MXCSR_OFFSET + 2],
        state.data[MXCSR_OFFSET + 3],
    ]);
    assert!(fcw == DEFAULT_FCW, "FPU self-test: bad default FCW");
    assert!(mxcsr == DEFAULT_MXCSR, "FPU self-test: bad default MXCSR");
    serial_println!("[fpu]   Default state FCW={:#06x} MXCSR={:#010x}: OK", fcw, mxcsr);

    // Test 2: Save current FPU state, verify it round-trips.
    let mut saved = FpuState::new_default();
    // SAFETY: saved is properly aligned and sized.
    unsafe { save(&raw mut saved); }

    // Verify the saved MXCSR is sane (all exceptions masked).
    let saved_mxcsr = u32::from_le_bytes([
        saved.data[MXCSR_OFFSET],
        saved.data[MXCSR_OFFSET + 1],
        saved.data[MXCSR_OFFSET + 2],
        saved.data[MXCSR_OFFSET + 3],
    ]);
    // At minimum, exception masks (bits 7-12) should be set.
    assert!(
        saved_mxcsr & 0x1F80 == 0x1F80,
        "FPU self-test: MXCSR exception masks not set"
    );
    serial_println!("[fpu]   Save/verify MXCSR={:#010x}: OK", saved_mxcsr);

    // Test 3: Write pattern to XMM0, save, clobber XMM0, restore, verify.
    test_xmm_round_trip();

    serial_println!("[fpu] FPU/SSE self-test PASSED");
}

/// Test that XMM register state survives a save/restore cycle.
#[allow(clippy::arithmetic_side_effects)]
fn test_xmm_round_trip() {
    // Write a known 128-bit pattern to XMM0.
    let pattern: u128 = 0xDEAD_BEEF_CAFE_BABE_1234_5678_9ABC_DEF0;
    let mut state_a = FpuState::new_default();
    let mut readback: u128 = 0;

    // SAFETY: We control XMM0 and the buffers are valid.
    unsafe {
        // Load pattern into XMM0.
        asm!(
            "movdqu xmm0, [{}]",
            in(reg) &pattern,
            options(nostack),
        );

        // Save FPU state (captures XMM0 with our pattern).
        save(&raw mut state_a);

        // Clobber XMM0 with zeros.
        asm!("pxor xmm0, xmm0", options(nomem, nostack));

        // Restore FPU state (should restore XMM0 to our pattern).
        restore(&raw const state_a);

        // Read XMM0 back.
        asm!(
            "movdqu [{}], xmm0",
            in(reg) &mut readback,
            options(nostack),
        );
    }

    assert!(
        readback == pattern,
        "FPU self-test: XMM0 round-trip failed"
    );
    serial_println!("[fpu]   XMM0 save/restore round-trip: OK");
}
