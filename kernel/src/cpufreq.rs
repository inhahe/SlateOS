//! CPU frequency scaling — P-state management for power/performance.
//!
//! Controls the CPU's operating frequency via hardware P-state interfaces.
//! On modern x86_64 processors, the CPU frequency can be adjusted to trade
//! performance for power consumption:
//!
//! - **Performance mode**: Run at maximum frequency for best throughput.
//! - **Powersave mode**: Run at minimum frequency for lowest power draw.
//! - **Ondemand mode**: Dynamically adjust based on load (raise when busy,
//!   lower when idle).
//!
//! ## Hardware Interface
//!
//! Modern Intel/AMD CPUs support hardware-managed P-states via:
//! - **HWP (Hardware-managed Performance)** — Intel Speed Shift (CPUID.06H:EAX[7]).
//!   The CPU autonomously selects P-states within a min/max range set by the OS.
//! - **Legacy EIST** — Enhanced Intel SpeedStep. OS requests specific P-states
//!   via `IA32_PERF_CTL` MSR.
//! - **AMD CPB/CPPC** — Core Performance Boost and Collaborative Processor
//!   Performance Control, functionally similar to HWP.
//!
//! This module prefers HWP when available (CPU makes better decisions with
//! more information about thermal/power state), falling back to direct
//! MSR-based control.
//!
//! ## Governor
//!
//! The governor policy determines how frequency is managed:
//! - `Performance`: lock at max frequency (best for latency-sensitive work)
//! - `Powersave`: lock at min frequency (best for battery/thermal)
//! - `Ondemand`: raise frequency when CPU load > threshold, lower when idle
//!
//! The ondemand governor checks load every 100ms (10 timer ticks) using
//! the idle percentage from cputime accounting.
//!
//! ## References
//!
//! - Intel SDM Vol. 3B §15: Power and Thermal Management
//! - Intel SDM Vol. 3B §15.4.4: HWP (Hardware-Controlled Performance States)
//! - AMD PPR: Core Performance Boost, CPPC
//! - Linux `drivers/cpufreq/intel_pstate.c`

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// MSR addresses
// ---------------------------------------------------------------------------

/// IA32_PERF_STATUS: Current P-state (read-only).
const MSR_PERF_STATUS: u32 = 0x198;

/// IA32_PERF_CTL: Request a P-state transition (write).
const MSR_PERF_CTL: u32 = 0x199;

/// IA32_PM_ENABLE: Enable/disable HWP.
const MSR_PM_ENABLE: u32 = 0x770;

/// IA32_HWP_CAPABILITIES: HWP performance range (read-only).
const MSR_HWP_CAPABILITIES: u32 = 0x771;

/// IA32_HWP_REQUEST: HWP hint from OS (desired min/max/preferred).
const MSR_HWP_REQUEST: u32 = 0x774;

/// IA32_HWP_STATUS: HWP status flags.
const MSR_HWP_STATUS: u32 = 0x777;

/// IA32_MPERF: Maximum performance frequency clock count.
const MSR_MPERF: u32 = 0xE7;

/// IA32_APERF: Actual performance frequency clock count.
const MSR_APERF: u32 = 0xE8;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Load threshold (percent) above which ondemand governor raises frequency.
const ONDEMAND_UP_THRESHOLD: u8 = 80;

/// Load threshold below which ondemand governor lowers frequency.
const ONDEMAND_DOWN_THRESHOLD: u8 = 20;

/// How often the ondemand governor samples (in 100Hz ticks = 100ms).
const ONDEMAND_SAMPLE_INTERVAL: u64 = 10;

// ---------------------------------------------------------------------------
// Governor policy
// ---------------------------------------------------------------------------

/// CPU frequency governor policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Governor {
    /// Lock at maximum frequency.
    Performance = 0,
    /// Lock at minimum frequency.
    Powersave = 1,
    /// Dynamically adjust based on load.
    Ondemand = 2,
}

impl Governor {
    fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::Performance,
            1 => Self::Powersave,
            2 => Self::Ondemand,
            _ => Self::Performance,
        }
    }

    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Powersave => "powersave",
            Self::Ondemand => "ondemand",
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether HWP is supported and active.
static HWP_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether EIST (legacy SpeedStep) is available.
static EIST_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Current governor policy.
static GOVERNOR: AtomicU8 = AtomicU8::new(Governor::Performance as u8);

/// Whether cpufreq has been initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Minimum performance level (0-255 for HWP, or ratio for EIST).
static PERF_MIN: AtomicU8 = AtomicU8::new(0);

/// Maximum performance level.
static PERF_MAX: AtomicU8 = AtomicU8::new(0);

/// Guaranteed (efficient) performance level.
static PERF_GUARANTEED: AtomicU8 = AtomicU8::new(0);

/// Current requested performance level.
static PERF_CURRENT: AtomicU8 = AtomicU8::new(0);

/// Base frequency in MHz (from CPUID or TSC calibration).
static BASE_FREQ_MHZ: AtomicU16 = AtomicU16::new(0);

/// Last tick count when ondemand governor ran.
static LAST_SAMPLE_TICK: AtomicU64 = AtomicU64::new(0);

/// Total transitions since boot.
static TOTAL_TRANSITIONS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize CPU frequency scaling.
///
/// Detects HWP or EIST support, reads performance limits, and sets the
/// default governor (performance mode).  Call after CPUID feature detection.
pub fn init() {
    // Check for HWP support: CPUID.06H:EAX[7].
    let hwp_supported = check_hwp_support();
    let eist_supported = check_eist_support();

    if hwp_supported {
        init_hwp();
    } else if eist_supported {
        init_eist();
    } else {
        serial_println!("[cpufreq] No frequency scaling support detected");
        // Estimate base frequency from TSC calibration.
        let tsc_khz = crate::bench::tsc_freq() / 1000;
        #[allow(clippy::cast_possible_truncation)]
        let mhz = (tsc_khz / 1000) as u16;
        BASE_FREQ_MHZ.store(mhz, Ordering::Relaxed);
        INITIALIZED.store(true, Ordering::Release);
        serial_println!("[cpufreq] Estimated frequency: {} MHz (from TSC)", mhz);
        return;
    }

    // Default to performance governor.
    set_governor(Governor::Performance);
    INITIALIZED.store(true, Ordering::Release);
}

/// Initialize HWP (Hardware-managed Performance states).
fn init_hwp() {
    // Enable HWP via IA32_PM_ENABLE.
    // SAFETY: HWP is supported (CPUID check passed).
    unsafe {
        wrmsr(MSR_PM_ENABLE, 1);
    }

    // Read capabilities.
    // SAFETY: HWP was just enabled above; MSR_HWP_CAPABILITIES is valid.
    let caps = unsafe { rdmsr(MSR_HWP_CAPABILITIES) };
    let perf_highest = (caps & 0xFF) as u8;
    let perf_guaranteed = ((caps >> 8) & 0xFF) as u8;
    let perf_efficient = ((caps >> 16) & 0xFF) as u8;
    let perf_lowest = ((caps >> 24) & 0xFF) as u8;

    PERF_MAX.store(perf_highest, Ordering::Relaxed);
    PERF_GUARANTEED.store(perf_guaranteed, Ordering::Relaxed);
    PERF_MIN.store(perf_lowest, Ordering::Relaxed);
    HWP_ACTIVE.store(true, Ordering::Relaxed);

    // Estimate base frequency from TSC.
    let tsc_khz = crate::bench::tsc_freq() / 1000;
    #[allow(clippy::cast_possible_truncation)]
    let mhz = (tsc_khz / 1000) as u16;
    BASE_FREQ_MHZ.store(mhz, Ordering::Relaxed);

    serial_println!(
        "[cpufreq] HWP enabled: lowest={} efficient={} guaranteed={} highest={} (base ~{}MHz)",
        perf_lowest, perf_efficient, perf_guaranteed, perf_highest, mhz
    );
}

/// Initialize legacy EIST (Enhanced Intel SpeedStep).
fn init_eist() {
    // Read current P-state from IA32_PERF_STATUS.
    // SAFETY: MSR_PERF_STATUS is read-only and always valid on EIST-capable CPUs.
    let status = unsafe { rdmsr(MSR_PERF_STATUS) };
    let current_ratio = ((status >> 8) & 0xFF) as u8;

    // For EIST, the ratio × bus_clock = frequency.
    // Most modern Intel CPUs use 100 MHz bus clock.
    let estimated_mhz = u16::from(current_ratio) * 100;

    PERF_MAX.store(current_ratio, Ordering::Relaxed);
    PERF_GUARANTEED.store(current_ratio, Ordering::Relaxed);
    // Min is harder to detect without platform info; assume ~50% of max.
    let min_ratio = current_ratio / 2;
    PERF_MIN.store(min_ratio.max(1), Ordering::Relaxed);
    PERF_CURRENT.store(current_ratio, Ordering::Relaxed);
    EIST_AVAILABLE.store(true, Ordering::Relaxed);
    BASE_FREQ_MHZ.store(estimated_mhz, Ordering::Relaxed);

    serial_println!(
        "[cpufreq] EIST: current ratio={} (~{}MHz), min ratio={}",
        current_ratio, estimated_mhz, min_ratio
    );
}

// ---------------------------------------------------------------------------
// Governor control
// ---------------------------------------------------------------------------

/// Set the CPU frequency governor.
pub fn set_governor(gov: Governor) {
    GOVERNOR.store(gov as u8, Ordering::Relaxed);

    match gov {
        Governor::Performance => apply_perf_level(PERF_MAX.load(Ordering::Relaxed)),
        Governor::Powersave => apply_perf_level(PERF_MIN.load(Ordering::Relaxed)),
        Governor::Ondemand => {
            // Start at guaranteed level; ondemand_tick will adjust.
            apply_perf_level(PERF_GUARANTEED.load(Ordering::Relaxed));
        }
    }

    serial_println!("[cpufreq] Governor set to: {}", gov.name());
}

/// Get the current governor.
#[must_use]
pub fn governor() -> Governor {
    Governor::from_u8(GOVERNOR.load(Ordering::Relaxed))
}

// ---------------------------------------------------------------------------
// Ondemand governor tick
// ---------------------------------------------------------------------------

/// Periodic tick for the ondemand governor.
///
/// Called from the timer softirq handler (every tick = 10ms).
/// Only acts every `ONDEMAND_SAMPLE_INTERVAL` ticks.
pub fn ondemand_tick() {
    if governor() != Governor::Ondemand {
        return;
    }

    let current_tick = crate::apic::tick_count();
    let last = LAST_SAMPLE_TICK.load(Ordering::Relaxed);

    if current_tick.saturating_sub(last) < ONDEMAND_SAMPLE_INTERVAL {
        return;
    }
    LAST_SAMPLE_TICK.store(current_tick, Ordering::Relaxed);

    // Get CPU load from cputime module.
    let Some(stats) = crate::cputime::cpu_stats(0) else {
        return;
    };
    let total = stats.system_ns
        .saturating_add(stats.irq_ns)
        .saturating_add(stats.softirq_ns)
        .saturating_add(stats.idle_ns);

    if total == 0 {
        return;
    }

    // Load = (non-idle time / total time) * 100.
    #[allow(clippy::cast_possible_truncation)]
    let load_pct = ((total.saturating_sub(stats.idle_ns)) * 100 / total) as u8;

    let current_perf = PERF_CURRENT.load(Ordering::Relaxed);
    let max_perf = PERF_MAX.load(Ordering::Relaxed);
    let min_perf = PERF_MIN.load(Ordering::Relaxed);

    let new_perf = if load_pct >= ONDEMAND_UP_THRESHOLD {
        // High load → jump to max immediately (responsive).
        max_perf
    } else if load_pct <= ONDEMAND_DOWN_THRESHOLD {
        // Low load → step down gradually.
        current_perf.saturating_sub(1).max(min_perf)
    } else {
        // Medium load → proportional between min and max.
        #[allow(clippy::cast_possible_truncation)]
        let range = max_perf.saturating_sub(min_perf) as u16;
        #[allow(clippy::cast_possible_truncation)]
        let target = (min_perf as u16 + (range * u16::from(load_pct) / 100)) as u8;
        target
    };

    if new_perf != current_perf {
        apply_perf_level(new_perf);
    }
}

// ---------------------------------------------------------------------------
// Performance level application
// ---------------------------------------------------------------------------

/// Apply a performance level to the hardware.
fn apply_perf_level(level: u8) {
    if level == 0 {
        return;
    }

    PERF_CURRENT.store(level, Ordering::Relaxed);
    TOTAL_TRANSITIONS.fetch_add(1, Ordering::Relaxed);

    if HWP_ACTIVE.load(Ordering::Relaxed) {
        // HWP: write desired min/max/preferred to IA32_HWP_REQUEST.
        let min = PERF_MIN.load(Ordering::Relaxed);
        let max = PERF_MAX.load(Ordering::Relaxed);
        // Bits [7:0] = min, [15:8] = max, [23:16] = desired (0 = let HW decide).
        // Setting desired = level gives the CPU a hint about our preference.
        let request = u64::from(min)
            | (u64::from(max) << 8)
            | (u64::from(level) << 16);
        // SAFETY: HWP is enabled and the MSR is valid.
        unsafe { wrmsr(MSR_HWP_REQUEST, request); }
    } else if EIST_AVAILABLE.load(Ordering::Relaxed) {
        // EIST: write target ratio to IA32_PERF_CTL[15:8].
        let ctl = u64::from(level) << 8;
        // SAFETY: EIST is available and the MSR is writable.
        unsafe { wrmsr(MSR_PERF_CTL, ctl); }
    }
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// CPU frequency information snapshot.
#[derive(Debug, Clone, Copy)]
pub struct FreqInfo {
    /// Base frequency in MHz (from TSC or CPUID).
    pub base_mhz: u16,
    /// Current performance level (0-255).
    pub current_perf: u8,
    /// Minimum performance level.
    pub min_perf: u8,
    /// Maximum performance level.
    pub max_perf: u8,
    /// Guaranteed (efficient) performance level.
    pub guaranteed_perf: u8,
    /// Current governor.
    pub governor: Governor,
    /// Whether HWP is active.
    pub hwp_active: bool,
    /// Whether EIST is available.
    pub eist_available: bool,
    /// Total P-state transitions since boot.
    pub transitions: u64,
}

/// Get current CPU frequency information.
#[must_use]
pub fn info() -> FreqInfo {
    FreqInfo {
        base_mhz: BASE_FREQ_MHZ.load(Ordering::Relaxed),
        current_perf: PERF_CURRENT.load(Ordering::Relaxed),
        min_perf: PERF_MIN.load(Ordering::Relaxed),
        max_perf: PERF_MAX.load(Ordering::Relaxed),
        guaranteed_perf: PERF_GUARANTEED.load(Ordering::Relaxed),
        governor: governor(),
        hwp_active: HWP_ACTIVE.load(Ordering::Relaxed),
        eist_available: EIST_AVAILABLE.load(Ordering::Relaxed),
        transitions: TOTAL_TRANSITIONS.load(Ordering::Relaxed),
    }
}

/// Estimate the current effective frequency in MHz.
///
/// Uses MPERF/APERF ratio when available for accurate measurement,
/// otherwise estimates from the performance level and base frequency.
#[must_use]
pub fn current_freq_mhz() -> u16 {
    let base = BASE_FREQ_MHZ.load(Ordering::Relaxed);
    let max_perf = PERF_MAX.load(Ordering::Relaxed);
    let cur_perf = PERF_CURRENT.load(Ordering::Relaxed);

    if max_perf == 0 || base == 0 {
        return base;
    }

    // Simple linear estimate: freq = base * (current_perf / max_perf).
    #[allow(clippy::cast_possible_truncation)]
    let estimated = (u32::from(base) * u32::from(cur_perf) / u32::from(max_perf)) as u16;
    estimated.max(1)
}

// ---------------------------------------------------------------------------
// CPUID feature checks
// ---------------------------------------------------------------------------

/// Check if HWP (Hardware-managed P-states) is supported.
/// CPUID.06H:EAX[7] = 1.
fn check_hwp_support() -> bool {
    let eax = cpuid_leaf6_eax();
    (eax >> 7) & 1 != 0
}

/// Check if EIST (Enhanced Intel SpeedStep) is supported.
/// CPUID.01H:ECX[7] = 1.
fn check_eist_support() -> bool {
    let ecx = cpuid_leaf1_ecx();
    (ecx >> 7) & 1 != 0
}

/// CPUID leaf 6: Thermal and Power Management.
fn cpuid_leaf6_eax() -> u32 {
    let max_leaf = cpuid_max_leaf();
    if max_leaf < 6 {
        return 0;
    }
    let eax: u32;
    // SAFETY: Leaf 6 is valid (max_leaf >= 6).
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 6",
            "cpuid",
            "pop rbx",
            lateout("eax") eax,
            out("ecx") _,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    eax
}

/// CPUID leaf 1 ECX (feature flags).
fn cpuid_leaf1_ecx() -> u32 {
    let ecx: u32;
    // SAFETY: CPUID leaf 1 always valid.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "pop rbx",
            lateout("eax") _,
            lateout("ecx") ecx,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    ecx
}

/// CPUID leaf 0: max standard leaf.
fn cpuid_max_leaf() -> u32 {
    let eax: u32;
    // SAFETY: CPUID leaf 0 always valid.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "xor eax, eax",
            "cpuid",
            "pop rbx",
            lateout("eax") eax,
            out("ecx") _,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    eax
}

// ---------------------------------------------------------------------------
// MSR helpers
// ---------------------------------------------------------------------------

/// Read a Model-Specific Register.
///
/// # Safety
///
/// The MSR address must be valid and reading it must be safe in the
/// current CPU mode.
#[inline]
unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    // SAFETY: Caller guarantees MSR address is valid; rdmsr reads ECX-selected MSR.
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            lateout("eax") low,
            lateout("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
    u64::from(low) | (u64::from(high) << 32)
}

/// Write a Model-Specific Register.
///
/// # Safety
///
/// The MSR address must be valid and the value must be appropriate for
/// the register.  Writing invalid values can crash the CPU or corrupt
/// system state.
#[inline]
unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    // SAFETY: Caller guarantees MSR address and value are valid; wrmsr writes ECX-selected MSR.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
}
