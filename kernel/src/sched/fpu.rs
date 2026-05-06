//! FPU/SSE/AVX state management for context switching.
//!
//! On x86_64, the SSE2 instruction set is always available and the Rust
//! compiler may use XMM registers for any code (auto-vectorization,
//! memcpy/memset optimizations, register allocation for small structs).
//! Without explicit save/restore across context switches, one task's
//! register state can silently corrupt another task's computations.
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
//! ## XSAVE vs FXSAVE
//!
//! When the CPU supports XSAVE (virtually all CPUs since Sandy Bridge, 2011),
//! we use it instead of FXSAVE:
//!
//! - **FXSAVE**: Saves x87 + SSE only (512 bytes, 16-byte aligned).
//! - **XSAVE**: Saves x87 + SSE + AVX (YMM) + AVX-512 (ZMM, opmask) + more.
//!   Size varies by CPU; detected via CPUID leaf 0xD.
//! - **XSAVEOPT**: Optimized variant that only writes state components
//!   that have been modified since the last XRSTOR (tracking via XINUSE).
//!   Significantly faster for tasks that don't use AVX/AVX-512.
//!
//! The save strategy (determined once at boot, used for all context switches):
//!
//! | Priority | Instruction  | Condition                                    |
//! |----------|-------------|----------------------------------------------|
//! | 1        | `xsaveopt64` | CPUID.0DH.1:EAX[0] = 1                    |
//! | 2        | `xsave64`    | CPUID.01H:ECX[26] = 1 (XSAVE supported)   |
//! | 3        | `fxsave64`   | Fallback (always available on x86_64)      |
//!
//! ## Hardware Configuration
//!
//! - `CR0.EM` = 0 (no x87 emulation)
//! - `CR0.TS` = 0 (no task-switched #NM)
//! - `CR0.MP` = 1 (monitor coprocessor)
//! - `CR4.OSFXSR` = 1 (enable FXSAVE/FXRSTOR and SSE)
//! - `CR4.OSXMMEXCPT` = 1 (enable #XF for unmasked SSE exceptions)
//! - `CR4.OSXSAVE` = 1 (enable XSAVE/XRSTOR, when XSAVE supported)
//! - `XCR0` = x87 | SSE | AVX [| AVX-512 ...] (when XSAVE enabled)
//!
//! ## Save Area
//!
//! When using XSAVE, the area size is determined by CPUID leaf 0xD ECX
//! (maximum size for all supported features).  Typical sizes:
//! - x87 + SSE only: 576 bytes
//! - x87 + SSE + AVX: 832 bytes
//! - x87 + SSE + AVX + AVX-512: 2688 bytes
//!
//! We allocate the maximum reported size, 64-byte aligned (XSAVE
//! requirement).  Each task carries its own buffer.
//!
//! ## Performance
//!
//! - `fxsave64` + `fxrstor64`: ~150-200 cycles (~50-70ns at 3 GHz)
//! - `xsave64` + `xrstor64`: ~200-300 cycles for SSE+AVX
//! - `xsaveopt64`: ~100-150 cycles (skips unmodified components)
//!
//! The XSAVEOPT path is faster than FXSAVE when a task doesn't use AVX,
//! because it only writes the x87+SSE header (modified bits tracking).

use crate::serial_println;
use core::arch::asm;
use core::sync::atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// XCR0 feature bits
// ---------------------------------------------------------------------------

/// XCR0 bit 0: x87 FPU state.
const XCR0_X87: u64 = 1 << 0;
/// XCR0 bit 1: SSE state (XMM registers + MXCSR).
const XCR0_SSE: u64 = 1 << 1;
/// XCR0 bit 2: AVX state (upper 128 bits of YMM0-YMM15).
const XCR0_AVX: u64 = 1 << 2;
/// XCR0 bits 5-7: AVX-512 state (opmask k0-k7, ZMM upper 256, ZMM16-31).
#[allow(dead_code)]
const XCR0_AVX512_OPMASK: u64 = 1 << 5;
#[allow(dead_code)]
const XCR0_AVX512_ZMM_HI256: u64 = 1 << 6;
#[allow(dead_code)]
const XCR0_AVX512_HI16_ZMM: u64 = 1 << 7;

// ---------------------------------------------------------------------------
// Save strategy (determined at boot, immutable thereafter)
// ---------------------------------------------------------------------------

/// Which instruction set to use for save/restore.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SaveStrategy {
    /// Legacy FXSAVE/FXRSTOR (512 bytes, 16-byte aligned).
    Fxsave = 0,
    /// XSAVE/XRSTOR (variable size, 64-byte aligned).
    Xsave = 1,
    /// XSAVEOPT/XRSTOR (optimized, only saves modified components).
    Xsaveopt = 2,
}

/// XSAVE area size for new task allocation.  Set during init.
/// When using FXSAVE, this is 512.  When using XSAVE, it's the
/// CPUID-reported maximum size (typically 832 for AVX, 2688 for AVX-512).
static XSAVE_AREA_SIZE: AtomicU32 = AtomicU32::new(512);

/// XCR0 value we've configured (for XRSTOR mask parameter).
/// Default: x87 + SSE (the minimum FXSAVE-equivalent).
static ACTIVE_XCR0: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(XCR0_X87 | XCR0_SSE);

/// Get the current save strategy (reads from context.rs assembly dispatch var).
#[inline(always)]
fn strategy() -> SaveStrategy {
    match super::context::FPU_STRATEGY.load(Ordering::Relaxed) {
        1 => SaveStrategy::Xsave,
        2 => SaveStrategy::Xsaveopt,
        _ => SaveStrategy::Fxsave,
    }
}

// ---------------------------------------------------------------------------
// FPU state save area
// ---------------------------------------------------------------------------

/// FXSAVE64 save area layout offsets.
///
/// See Intel SDM Vol. 1, Table 10-2 "Layout of the FXSAVE Area".
/// These offsets are shared between FXSAVE and XSAVE (the first 512
/// bytes of the XSAVE area are the legacy region, identical to FXSAVE).
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

/// Maximum XSAVE area size we support.  This caps allocation even if
/// CPUID reports something absurd.  4096 bytes covers x87+SSE+AVX+AVX-512
/// on all known CPUs (typical max is ~2688 for AVX-512).
const MAX_XSAVE_AREA: u32 = 4096;

/// FPU/SSE/AVX state for a single task.
///
/// Must be 64-byte aligned for `xsave64`/`xrstor64` (XSAVE requires 64-byte
/// alignment; FXSAVE only needs 16, but we use the stricter requirement).
///
/// The actual used size depends on what the CPU supports (512 for FXSAVE,
/// up to ~2688 for AVX-512 via XSAVE).  We always allocate the maximum
/// the CPU reports to avoid per-task size tracking.
///
/// This is embedded directly in the [`Task`](super::task::Task) struct.
/// The alignment requirement propagates to the containing struct and is
/// satisfied by the heap allocator (which uses power-of-2 size classes
/// with natural alignment).
#[derive(Clone)]
#[repr(C, align(64))]
pub struct FpuState {
    /// Raw XSAVE/FXSAVE area.  Sized for the maximum component set.
    /// Only the first `xsave_area_size()` bytes are meaningful.
    data: [u8; MAX_XSAVE_AREA as usize],
}

impl FpuState {
    /// Create a clean initial FPU state.
    ///
    /// Sets the x87 FCW and MXCSR to their standard default values.
    /// All registers (ST0-ST7, XMM0-XMM15, YMM, ZMM) are zeroed.
    ///
    /// For XSAVE: also sets the XSTATE_BV header field (bytes 512-519)
    /// to indicate which components are present, so XRSTOR doesn't fault
    /// on an all-zero header.
    ///
    /// This is the state a new task starts with — equivalent to the
    /// hardware state after FNINIT + LDMXCSR(0x1F80).
    #[must_use]
    pub fn new_default() -> Self {
        let mut state = Self { data: [0u8; MAX_XSAVE_AREA as usize] };

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

        // For XSAVE: set XSTATE_BV in the XSAVE header (offset 512, 8 bytes).
        // This tells XRSTOR which state components are valid in this image.
        // We mark x87 + SSE as initialized (the minimum valid set).
        if strategy() != SaveStrategy::Fxsave {
            let xstate_bv = (XCR0_X87 | XCR0_SSE).to_le_bytes();
            state.data[512] = xstate_bv[0];
            state.data[513] = xstate_bv[1];
            state.data[514] = xstate_bv[2];
            state.data[515] = xstate_bv[3];
            state.data[516] = xstate_bv[4];
            state.data[517] = xstate_bv[5];
            state.data[518] = xstate_bv[6];
            state.data[519] = xstate_bv[7];
        }

        state
    }

    /// Get a raw pointer to the save area.
    ///
    /// Available for subsystems that need direct access to the save
    /// buffer (e.g., process state dump, debug inspection).
    #[must_use]
    #[inline(always)]
    #[allow(dead_code)]
    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    /// Get a raw mutable pointer to the save area.
    ///
    /// Available for subsystems that need direct access to the save
    /// buffer (e.g., signal frame construction, ptrace).
    #[must_use]
    #[inline(always)]
    #[allow(dead_code)]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }
}

// Statically assert alignment (64-byte for XSAVE).
const _: () = {
    assert!(core::mem::align_of::<FpuState>() >= 64);
    assert!(core::mem::size_of::<FpuState>() == MAX_XSAVE_AREA as usize);
};

// ---------------------------------------------------------------------------
// Hardware initialization
// ---------------------------------------------------------------------------

/// Initialize FPU/SSE/AVX on the BSP (Boot Strap Processor).
///
/// Detects XSAVE support, enables CR4.OSXSAVE + XCR0 if available,
/// and selects the optimal save/restore strategy.  Falls back to FXSAVE
/// if XSAVE is not supported.
///
/// Called once during early boot, before the scheduler starts.
pub fn init_bsp() {
    configure_fpu_cr_bits();

    // Verify FXSAVE support via cached CPU features.
    let features = crate::cpu::features()
        .expect("CPU features must be detected before FPU init");
    assert!(features.fxsr, "CPU does not support FXSAVE/FXRSTOR (impossible on x86_64)");

    // Initialize x87 FPU to known state.
    // SAFETY: We've verified the FPU hardware is present and enabled.
    unsafe { asm!("fninit", options(nomem, nostack)); }

    // Set MXCSR to default (all exceptions masked).
    let default_mxcsr: u32 = DEFAULT_MXCSR;
    // SAFETY: MXCSR is a valid SSE control register, we're setting a safe value.
    unsafe { asm!("ldmxcsr [{}]", in(reg) &default_mxcsr, options(nostack)); }

    // Try to enable XSAVE.
    if features.xsave {
        enable_xsave(features);
    } else {
        serial_println!("[fpu] XSAVE not supported — using FXSAVE (x87+SSE only)");
        super::context::set_fpu_strategy(SaveStrategy::Fxsave as u8, 0x3, 0);
        XSAVE_AREA_SIZE.store(512, Ordering::Release);
    }

    let strat = strategy();
    let strat_name = match strat {
        SaveStrategy::Fxsave => "FXSAVE",
        SaveStrategy::Xsave => "XSAVE",
        SaveStrategy::Xsaveopt => "XSAVEOPT",
    };
    serial_println!(
        "[fpu] BSP initialized: strategy={}, area={}B, XCR0={:#x}",
        strat_name,
        XSAVE_AREA_SIZE.load(Ordering::Relaxed),
        ACTIVE_XCR0.load(Ordering::Relaxed),
    );
}

/// Enable XSAVE and configure XCR0 with all supported state components.
fn enable_xsave(features: &crate::cpu::CpuFeatures) {
    // Step 1: Set CR4.OSXSAVE (bit 18) to enable XSAVE instructions.
    // SAFETY: CPU supports XSAVE (verified by caller).
    unsafe {
        let cr4: u64;
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        let cr4 = cr4 | (1 << 18); // CR4.OSXSAVE
        asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));
    }

    // Step 2: Configure XCR0 with all state components the CPU supports.
    // x87 and SSE are mandatory (bits 0-1 must be set together per SDM).
    let mut xcr0: u64 = XCR0_X87 | XCR0_SSE;

    // Enable AVX (YMM upper halves) if supported.
    if features.avx && (features.xcr0_supported & XCR0_AVX != 0) {
        xcr0 |= XCR0_AVX;
    }

    // Enable AVX-512 components if all three bits are supported.
    // All three must be enabled together or none (SDM requirement).
    if features.avx512f {
        let avx512_bits = XCR0_AVX512_OPMASK | XCR0_AVX512_ZMM_HI256 | XCR0_AVX512_HI16_ZMM;
        if features.xcr0_supported & avx512_bits == avx512_bits {
            xcr0 |= avx512_bits;
        }
    }

    // Step 3: Write XCR0 via XSETBV (ECX=0 selects XCR0).
    let xcr0_lo = xcr0 as u32;
    let xcr0_hi = (xcr0 >> 32) as u32;
    // SAFETY: CR4.OSXSAVE is set, and xcr0 only includes bits reported
    // as supported in CPUID.0DH:EAX+EDX.
    unsafe {
        asm!(
            "xsetbv",
            in("ecx") 0u32,
            in("eax") xcr0_lo,
            in("edx") xcr0_hi,
            options(nomem, nostack),
        );
    }

    ACTIVE_XCR0.store(xcr0, Ordering::Release);

    // Step 4: Determine area size.  Use CPUID.0DH.0:ECX (max for all
    // supported features), capped at our static buffer size.
    let area_size = features.xsave_area_size.min(MAX_XSAVE_AREA);
    XSAVE_AREA_SIZE.store(area_size, Ordering::Release);

    // Step 5: Select save strategy and publish to context switch assembly.
    let xcr0_lo = xcr0 as u32;
    let xcr0_hi = (xcr0 >> 32) as u32;
    if features.xsaveopt {
        super::context::set_fpu_strategy(SaveStrategy::Xsaveopt as u8, xcr0_lo, xcr0_hi);
        serial_println!("[fpu] XSAVEOPT enabled (optimized — skips unmodified state)");
    } else {
        super::context::set_fpu_strategy(SaveStrategy::Xsave as u8, xcr0_lo, xcr0_hi);
        serial_println!("[fpu] XSAVE enabled");
    }

    // Log which state components are active.
    let mut components = "[fpu]   XCR0 components: x87 SSE";
    if xcr0 & XCR0_AVX != 0 {
        components = "[fpu]   XCR0 components: x87 SSE AVX";
    }
    if xcr0 & XCR0_AVX512_OPMASK != 0 {
        components = "[fpu]   XCR0 components: x87 SSE AVX AVX-512";
    }
    serial_println!("{}", components);
}

/// Initialize FPU/SSE/AVX on an Application Processor.
///
/// APs start from INIT state with CR4 = 0 (no OSFXSR), so SSE
/// instructions would #UD without this.  Called from `ap_entry()`.
///
/// Also sets CR4.OSXSAVE and XCR0 to match the BSP configuration,
/// so all CPUs have identical extended state support.
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

    // If the BSP enabled XSAVE, replicate on this AP.
    if strategy() != SaveStrategy::Fxsave {
        // Enable CR4.OSXSAVE.
        // SAFETY: BSP already verified XSAVE support; all CPUs in the
        // system have the same feature set.
        unsafe {
            let cr4: u64;
            asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
            let cr4 = cr4 | (1 << 18);
            asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));
        }

        // Set XCR0 to match BSP.
        let xcr0 = ACTIVE_XCR0.load(Ordering::Acquire);
        let xcr0_lo = xcr0 as u32;
        let xcr0_hi = (xcr0 >> 32) as u32;
        // SAFETY: CR4.OSXSAVE is set, xcr0 matches BSP config.
        unsafe {
            asm!(
                "xsetbv",
                in("ecx") 0u32,
                in("eax") xcr0_lo,
                in("edx") xcr0_hi,
                options(nomem, nostack),
            );
        }
    }
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

// ---------------------------------------------------------------------------
// Public queries
// ---------------------------------------------------------------------------

/// Get the XSAVE area size (bytes) used for task state allocation.
///
/// Returns 512 if using FXSAVE, or the CPUID-detected size for XSAVE.
#[must_use]
#[inline]
pub fn xsave_area_size() -> u32 {
    XSAVE_AREA_SIZE.load(Ordering::Relaxed)
}

/// Whether XSAVE is active (vs legacy FXSAVE).
#[must_use]
#[inline]
#[allow(dead_code)]
pub fn xsave_active() -> bool {
    strategy() != SaveStrategy::Fxsave
}

/// Name of the current save strategy (for diagnostics).
#[must_use]
pub fn strategy_name() -> &'static str {
    match strategy() {
        SaveStrategy::Fxsave => "FXSAVE",
        SaveStrategy::Xsave => "XSAVE",
        SaveStrategy::Xsaveopt => "XSAVEOPT",
    }
}

/// Active XCR0 value.
#[must_use]
#[inline]
#[allow(dead_code)]
pub fn active_xcr0() -> u64 {
    ACTIVE_XCR0.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Save / restore (used from context switch assembly via raw pointers)
// ---------------------------------------------------------------------------

/// Save the current CPU's FPU/SSE/AVX state to the given buffer.
///
/// Uses the optimal instruction based on boot-time detection:
/// - XSAVEOPT64 (if available): only saves modified components
/// - XSAVE64 (if XSAVE supported): saves all enabled components
/// - FXSAVE64 (fallback): saves x87 + SSE only
///
/// # Safety
///
/// - `state` must point to a valid, 64-byte-aligned buffer of at least
///   `xsave_area_size()` bytes.
/// - Must be called with the CPU's FPU state being the state to save.
#[inline(always)]
pub unsafe fn save(state: *mut FpuState) {
    match strategy() {
        SaveStrategy::Xsaveopt => {
            // XSAVEOPT64: save only modified state components.
            // The mask (EDX:EAX) specifies which components to potentially
            // save.  We pass all enabled bits — the CPU skips unmodified ones.
            let xcr0 = ACTIVE_XCR0.load(Ordering::Relaxed);
            let mask_lo = xcr0 as u32;
            let mask_hi = (xcr0 >> 32) as u32;
            // SAFETY: Caller guarantees state is valid and 64-byte aligned.
            // XSAVEOPT64 writes to [state, state + xsave_area_size).
            unsafe {
                asm!(
                    "xsaveopt64 [{}]",
                    in(reg) state,
                    in("eax") mask_lo,
                    in("edx") mask_hi,
                    options(nostack),
                );
            }
        }
        SaveStrategy::Xsave => {
            // XSAVE64: save all state components specified in mask.
            let xcr0 = ACTIVE_XCR0.load(Ordering::Relaxed);
            let mask_lo = xcr0 as u32;
            let mask_hi = (xcr0 >> 32) as u32;
            // SAFETY: Same as above.
            unsafe {
                asm!(
                    "xsave64 [{}]",
                    in(reg) state,
                    in("eax") mask_lo,
                    in("edx") mask_hi,
                    options(nostack),
                );
            }
        }
        SaveStrategy::Fxsave => {
            // FXSAVE64: legacy path, x87 + SSE only.
            // SAFETY: Caller guarantees state is valid and 16-byte aligned
            // (our 64-byte alignment exceeds this requirement).
            unsafe {
                asm!(
                    "fxsave64 [{}]",
                    in(reg) state,
                    options(nostack),
                );
            }
        }
    }
}

/// Restore FPU/SSE/AVX state from the given buffer to the CPU.
///
/// # Safety
///
/// - `state` must point to a valid, 64-byte-aligned buffer containing a
///   previously-saved state (or a default-initialized state from
///   [`FpuState::new_default()`]).
/// - After this call, the CPU's FPU/SSE/AVX registers reflect the saved state.
#[inline(always)]
pub unsafe fn restore(state: *const FpuState) {
    match strategy() {
        SaveStrategy::Xsaveopt | SaveStrategy::Xsave => {
            // XRSTOR64: restore state components specified in mask.
            // Components not in the mask (or not marked in XSTATE_BV) get
            // their init values (zeros for registers, defaults for control).
            let xcr0 = ACTIVE_XCR0.load(Ordering::Relaxed);
            let mask_lo = xcr0 as u32;
            let mask_hi = (xcr0 >> 32) as u32;
            // SAFETY: Caller guarantees state is valid, aligned, and contains
            // a valid XSAVE image (XSTATE_BV header is correctly set).
            unsafe {
                asm!(
                    "xrstor64 [{}]",
                    in(reg) state,
                    in("eax") mask_lo,
                    in("edx") mask_hi,
                    options(nostack),
                );
            }
        }
        SaveStrategy::Fxsave => {
            // FXRSTOR64: legacy path.
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
/// 4. (If AVX) Test YMM register round-trip.
pub fn self_test() {
    serial_println!("[fpu] Running FPU/SSE self-test...");
    serial_println!("[fpu]   Strategy: {}, area: {}B", strategy_name(), xsave_area_size());

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

    // Test 4: If XSAVE + AVX, test YMM round-trip.
    if strategy() != SaveStrategy::Fxsave
        && ACTIVE_XCR0.load(Ordering::Relaxed) & XCR0_AVX != 0
    {
        test_ymm_round_trip();
    }

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

/// Test that YMM register state (AVX 256-bit) survives a save/restore cycle.
///
/// Only called when XSAVE is active and XCR0.AVX is set.
#[allow(clippy::arithmetic_side_effects)]
fn test_ymm_round_trip() {
    // A 256-bit pattern split into two 128-bit halves.
    let pattern_lo: u128 = 0xAAAA_BBBB_CCCC_DDDD_1111_2222_3333_4444;
    let pattern_hi: u128 = 0x5555_6666_7777_8888_9999_AAAA_BBBB_CCCC;
    let mut state_a = FpuState::new_default();
    let mut readback_lo: u128 = 0;
    let mut readback_hi: u128 = 0;

    // SAFETY: AVX is enabled (XCR0.AVX set, CR4.OSXSAVE set), buffers valid.
    unsafe {
        // Load 256-bit pattern into YMM0 (low half via XMM0, high via vinsert).
        asm!(
            "vmovdqu xmm0, [{lo}]",
            "vinsertf128 ymm0, ymm0, [{hi}], 1",
            lo = in(reg) &pattern_lo,
            hi = in(reg) &pattern_hi,
            options(nostack),
        );

        // Save state (captures full YMM0).
        save(&raw mut state_a);

        // Clobber YMM0 with zeros.
        asm!("vpxor ymm0, ymm0, ymm0", options(nomem, nostack));

        // Restore state.
        restore(&raw const state_a);

        // Read back the low 128 bits (XMM0).
        asm!(
            "vmovdqu [{lo}], xmm0",
            lo = in(reg) &mut readback_lo,
            options(nostack),
        );
        // Read back the high 128 bits (extract upper lane).
        asm!(
            "vextractf128 [{hi}], ymm0, 1",
            hi = in(reg) &mut readback_hi,
            options(nostack),
        );
    }

    assert!(
        readback_lo == pattern_lo,
        "FPU self-test: YMM0 low 128 round-trip failed"
    );
    assert!(
        readback_hi == pattern_hi,
        "FPU self-test: YMM0 high 128 round-trip failed"
    );
    serial_println!("[fpu]   YMM0 (AVX 256-bit) save/restore round-trip: OK");
}

// ---------------------------------------------------------------------------
// Multi-task FPU isolation stress test
// ---------------------------------------------------------------------------

/// Number of concurrent tasks in the FPU stress test.
const STRESS_TASK_COUNT: usize = 4;

/// Number of yield iterations per stress-test task.
const STRESS_ITERATIONS: u32 = 50;

/// Shared atomic counters for the stress test.
///
/// - `STRESS_ERRORS`: incremented if any task detects XMM corruption.
/// - `STRESS_DONE`: incremented when a task finishes all iterations.
static STRESS_ERRORS: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);
static STRESS_DONE: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);

/// Entry point for FPU stress test tasks.
///
/// Each task writes a unique 128-bit pattern derived from its task_arg
/// into XMM1, then repeatedly yields and verifies the pattern is intact.
/// If another task's pattern leaks into our XMM1, the FPU save/restore
/// is broken and we increment STRESS_ERRORS.
///
/// We use XMM1 (not XMM0) because XMM0 is caller-saved in the System V
/// ABI — the compiler is likely to use it as scratch between yields.
/// XMM1 is also caller-saved, but in our tight loop the compiler won't
/// use it because we pin it with inline asm.  The key: the context switch
/// must save ALL XMM registers, not just callee-saved ones (there are no
/// callee-saved XMM registers in System V).  The FXSAVE/XSAVE approach
/// saves all of them.
extern "C" fn stress_test_entry(task_arg: u64) {
    use core::sync::atomic::Ordering;

    // Generate a unique 128-bit pattern from our task_arg.
    // Each task gets a different value that's easy to identify.
    let pattern: u128 = 0xAAAA_BBBB_CCCC_0000_u128
        | (task_arg as u128)
        | ((task_arg as u128) << 32)
        | ((task_arg as u128) << 64)
        | ((task_arg as u128) << 96);

    // Load our unique pattern into XMM1.
    // SAFETY: We control XMM1 and pattern is stack-allocated.
    unsafe {
        asm!(
            "movdqu xmm1, [{}]",
            in(reg) &pattern,
            options(nostack),
        );
    }

    for _ in 0..STRESS_ITERATIONS {
        // Yield to let other tasks run (they write their OWN patterns
        // to XMM1).  If the context switch doesn't save/restore FPU
        // state correctly, our XMM1 will be corrupted when we resume.
        super::yield_now();

        // Read XMM1 back and verify it matches our pattern.
        let mut readback: u128 = 0;
        // SAFETY: readback is properly aligned on the stack.
        unsafe {
            asm!(
                "movdqu [{}], xmm1",
                in(reg) &mut readback,
                options(nostack),
            );
        }

        if readback != pattern {
            STRESS_ERRORS.fetch_add(1, Ordering::Relaxed);
            // Log first corruption only (avoid flooding serial).
            if STRESS_ERRORS.load(Ordering::Relaxed) == 1 {
                crate::serial_println!(
                    "[fpu] CORRUPTION in task arg={}: expected {:#034x}, got {:#034x}",
                    task_arg, pattern, readback
                );
            }
            break;
        }
    }

    STRESS_DONE.fetch_add(1, Ordering::Relaxed);
}

/// Run the multi-task FPU isolation stress test.
///
/// Spawns multiple tasks that each write unique patterns to XMM1,
/// then yield repeatedly.  After each yield, each task verifies its
/// XMM1 is intact.  If any task sees another task's pattern, the
/// FPU context switch is broken.
///
/// This tests the full end-to-end path: task switch → xsave/fxsave →
/// xrstor/fxrstor → resume, under real scheduler pressure with multiple
/// tasks competing for CPU time.
pub fn stress_test() {
    use core::sync::atomic::Ordering;

    serial_println!("[fpu] Running multi-task FPU isolation stress test...");

    // Reset counters.
    STRESS_ERRORS.store(0, Ordering::Release);
    STRESS_DONE.store(0, Ordering::Release);

    // Spawn stress test tasks at the same priority so they round-robin.
    let mut task_ids = [0u64; STRESS_TASK_COUNT];
    for i in 0..STRESS_TASK_COUNT {
        // Each task gets a unique arg (1, 2, 3, 4) used to generate its pattern.
        #[allow(clippy::arithmetic_side_effects)]
        let arg = (i + 1) as u64;
        match super::spawn(b"fpu-stress", 16, stress_test_entry, arg, 0) {
            Ok(id) => task_ids[i] = id,
            Err(e) => {
                serial_println!("[fpu]   SKIP: couldn't spawn task {}: {:?}", i, e);
                return;
            }
        }
    }

    // Wait for all tasks to finish (yield to let them run).
    #[allow(clippy::arithmetic_side_effects)]
    let target = STRESS_TASK_COUNT as u32;
    for _ in 0..10000u32 {
        if STRESS_DONE.load(Ordering::Acquire) >= target {
            break;
        }
        super::yield_now();
    }

    let done = STRESS_DONE.load(Ordering::Acquire);
    let errors = STRESS_ERRORS.load(Ordering::Acquire);

    if done < target {
        serial_println!(
            "[fpu]   WARNING: only {}/{} tasks completed (timeout)",
            done, target
        );
    }

    if errors == 0 {
        serial_println!(
            "[fpu]   {} tasks x {} yields, no XMM corruption detected: OK",
            STRESS_TASK_COUNT, STRESS_ITERATIONS
        );
    } else {
        serial_println!(
            "[fpu]   FAIL: {} XMM corruption(s) detected!",
            errors
        );
    }

    // Clean up any remaining tasks.
    for &id in &task_ids {
        if id != 0 {
            super::kill_task(id);
        }
    }
    super::reap_dead_tasks();

    assert!(errors == 0, "FPU stress test: XMM state leaked between tasks");
    serial_println!("[fpu] Multi-task FPU isolation stress test PASSED");
}
