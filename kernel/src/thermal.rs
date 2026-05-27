//! CPU thermal monitoring — temperature reading and throttle detection.
//!
//! Reads CPU package temperature via `IA32_THERM_STATUS` and
//! `IA32_PACKAGE_THERM_STATUS` MSRs.  These are available on all modern
//! Intel CPUs and AMD Zen+ processors.
//!
//! ## How x86 Thermal MSRs Work
//!
//! The CPU reports temperature as a *distance from Tj_max* (thermal
//! junction maximum — the maximum safe operating temperature, typically
//! 100°C for desktop CPUs).  The actual temperature is:
//!
//! ```text
//! T_current = Tj_max - digital_readout
//! ```
//!
//! The `digital_readout` field in `IA32_THERM_STATUS` (bits [22:16])
//! gives the offset below Tj_max in degrees Celsius.  If Tj_max is
//! 100°C and the readout is 30, the current temperature is 70°C.
//!
//! ## Tj_max Detection
//!
//! - Intel: Read from `MSR_TEMPERATURE_TARGET` (0x1A2) bits [23:16].
//! - AMD Zen: Typically 95°C or from BIOS tables.
//! - Fallback: Assume 100°C (safe assumption for modern desktop CPUs).
//!
//! ## Monitoring
//!
//! A periodic check (every 5 seconds from the timer softirq) reads the
//! temperature and updates the running statistics.  If temperature exceeds
//! thresholds, warnings are logged:
//!
//! - **Warning** (85°C default): log a warning, potentially reduce CPU freq.
//! - **Critical** (95°C default): force maximum throttle, emergency warning.
//!
//! ## References
//!
//! - Intel SDM Vol. 3B §15.5: Thermal Monitoring and Protection
//! - Intel SDM Vol. 4: MSR tables (IA32_THERM_STATUS, MSR_TEMPERATURE_TARGET)
//! - AMD PPR: THM::TCTL (temperature control register)
//! - Linux `drivers/hwmon/coretemp.c`

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// MSR addresses
// ---------------------------------------------------------------------------

/// IA32_THERM_STATUS: Per-core thermal status (read-only).
/// Bit 31: Reading valid.  Bits [22:16]: Digital readout (offset from Tj_max).
const MSR_THERM_STATUS: u32 = 0x19C;

/// IA32_PACKAGE_THERM_STATUS: Package-level thermal status.
/// Same format as IA32_THERM_STATUS but for the whole package.
const MSR_PKG_THERM_STATUS: u32 = 0x1B1;

/// MSR_TEMPERATURE_TARGET: Tj_max in bits [23:16].
const MSR_TEMPERATURE_TARGET: u32 = 0x1A2;

/// IA32_THERM_INTERRUPT: Thermal interrupt enable/threshold config.
const MSR_THERM_INTERRUPT: u32 = 0x19B;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Default Tj_max if we can't read it from MSR (conservative for modern CPUs).
const DEFAULT_TJ_MAX: u8 = 100;

/// Temperature warning threshold (°C).
const WARN_THRESHOLD: u8 = 85;

/// Temperature critical threshold (°C).
const CRITICAL_THRESHOLD: u8 = 95;

/// How often to sample temperature (in 100Hz timer ticks = every 5 seconds).
const SAMPLE_INTERVAL_TICKS: u64 = 500;

/// Number of temperature history samples to keep.
const HISTORY_SIZE: usize = 64;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether thermal monitoring is supported.
static SUPPORTED: AtomicBool = AtomicBool::new(false);

/// Whether thermal monitoring is enabled.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Tj_max (thermal junction maximum temperature, °C).
static TJ_MAX: AtomicU8 = AtomicU8::new(DEFAULT_TJ_MAX);

/// Current package temperature (°C).
static CURRENT_TEMP: AtomicU8 = AtomicU8::new(0);

/// Maximum temperature seen since boot (°C).
static MAX_TEMP: AtomicU8 = AtomicU8::new(0);

/// Minimum temperature seen since boot (°C, starts at 255).
static MIN_TEMP: AtomicU8 = AtomicU8::new(255);

/// Sum of all temperature samples (for mean calculation).
static TEMP_SUM: AtomicU64 = AtomicU64::new(0);

/// Number of temperature samples taken.
static SAMPLE_COUNT: AtomicU32 = AtomicU32::new(0);

/// Whether the CPU is currently thermally throttled.
static THROTTLED: AtomicBool = AtomicBool::new(false);

/// Number of times thermal throttle was detected.
static THROTTLE_COUNT: AtomicU32 = AtomicU32::new(0);

/// Number of temperature warnings logged.
static WARN_COUNT: AtomicU32 = AtomicU32::new(0);

/// Number of critical temperature events.
static CRITICAL_COUNT: AtomicU32 = AtomicU32::new(0);

/// Temperature history ring buffer (last N samples).
static TEMP_HISTORY: [AtomicU8; HISTORY_SIZE] = [const { AtomicU8::new(0) }; HISTORY_SIZE];

/// Write index for temperature history.
static HISTORY_IDX: AtomicU16 = AtomicU16::new(0);

/// Tick counter for sampling interval.
static TICK_COUNTER: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize thermal monitoring.
///
/// Detects thermal MSR support via CPUID and reads Tj_max.
/// Call during boot after CPUID feature detection.
pub fn init() {
    // Check if Digital Thermal Sensor is supported: CPUID.06H:EAX[0].
    let eax = cpuid_leaf6_eax();
    if eax & 1 == 0 {
        serial_println!("[thermal] Digital Thermal Sensor not supported");
        return;
    }

    SUPPORTED.store(true, Ordering::Relaxed);

    // Try to read Tj_max from MSR_TEMPERATURE_TARGET.
    let tj_max = read_tj_max();
    TJ_MAX.store(tj_max, Ordering::Relaxed);

    // Take an initial temperature reading.
    if let Some(temp) = read_package_temp() {
        CURRENT_TEMP.store(temp, Ordering::Relaxed);
        MAX_TEMP.store(temp, Ordering::Relaxed);
        MIN_TEMP.store(temp, Ordering::Relaxed);
        serial_println!(
            "[thermal] Initialized: Tj_max={}°C, current={}°C",
            tj_max, temp
        );
    } else {
        serial_println!("[thermal] Initialized: Tj_max={}°C (initial read failed)", tj_max);
    }
}

// ---------------------------------------------------------------------------
// Periodic sampling
// ---------------------------------------------------------------------------

/// Periodic temperature check.
///
/// Called from the timer softirq on the BSP.  Samples at
/// `SAMPLE_INTERVAL_TICKS` rate and updates statistics.
pub fn periodic_check() {
    if !SUPPORTED.load(Ordering::Relaxed) || !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let ticks = TICK_COUNTER.fetch_add(1, Ordering::Relaxed);
    if !ticks.is_multiple_of(SAMPLE_INTERVAL_TICKS) {
        return;
    }

    let Some(temp) = read_package_temp() else {
        return;
    };

    // Update current temperature.
    CURRENT_TEMP.store(temp, Ordering::Relaxed);

    // Update statistics.
    let prev_max = MAX_TEMP.load(Ordering::Relaxed);
    if temp > prev_max {
        MAX_TEMP.store(temp, Ordering::Relaxed);
    }
    let prev_min = MIN_TEMP.load(Ordering::Relaxed);
    if temp < prev_min {
        MIN_TEMP.store(temp, Ordering::Relaxed);
    }
    TEMP_SUM.fetch_add(u64::from(temp), Ordering::Relaxed);
    SAMPLE_COUNT.fetch_add(1, Ordering::Relaxed);

    // Record in history ring.
    let idx = HISTORY_IDX.fetch_add(1, Ordering::Relaxed) as usize % HISTORY_SIZE;
    TEMP_HISTORY[idx].store(temp, Ordering::Relaxed);

    // Check throttle status from THERM_STATUS bit 0 (PROCHOT).
    let throttled = is_prochot_active();
    let was_throttled = THROTTLED.swap(throttled, Ordering::Relaxed);
    if throttled && !was_throttled {
        THROTTLE_COUNT.fetch_add(1, Ordering::Relaxed);
        serial_println!("[thermal] WARNING: PROCHOT asserted (CPU thermally throttled)");
    }

    // Temperature threshold checks (rate-limited).
    if temp >= CRITICAL_THRESHOLD {
        let prev_critical = CRITICAL_COUNT.fetch_add(1, Ordering::Relaxed);
        if prev_critical.is_multiple_of(10) {
            serial_println!(
                "[thermal] CRITICAL: Package temperature {}°C (>= {}°C threshold)",
                temp, CRITICAL_THRESHOLD
            );
            crate::kwarn::warn(
                "CPU temperature critical",
                "thermal.rs",
                line!(),
            );
        }
    } else if temp >= WARN_THRESHOLD {
        let prev_warn = WARN_COUNT.fetch_add(1, Ordering::Relaxed);
        if prev_warn.is_multiple_of(60) {
            // Warn once per ~5 minutes at sustained high temp.
            serial_println!(
                "[thermal] Warning: Package temperature {}°C (>= {}°C threshold)",
                temp, WARN_THRESHOLD
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Temperature reading
// ---------------------------------------------------------------------------

/// Read the current package temperature in °C.
///
/// Returns None if the reading is invalid.
fn read_package_temp() -> Option<u8> {
    // Try package-level MSR first (gives overall chip temp).
    let status = unsafe { rdmsr(MSR_PKG_THERM_STATUS) };

    // Check if reading is valid (bit 31).
    if status & (1 << 31) == 0 {
        // Fall back to per-core thermal status.
        let core_status = unsafe { rdmsr(MSR_THERM_STATUS) };
        if core_status & (1 << 31) == 0 {
            return None;
        }
        let digital_readout = ((core_status >> 16) & 0x7F) as u8;
        let tj_max = TJ_MAX.load(Ordering::Relaxed);
        return Some(tj_max.saturating_sub(digital_readout));
    }

    let digital_readout = ((status >> 16) & 0x7F) as u8;
    let tj_max = TJ_MAX.load(Ordering::Relaxed);
    Some(tj_max.saturating_sub(digital_readout))
}

/// Read Tj_max from MSR_TEMPERATURE_TARGET.
fn read_tj_max() -> u8 {
    // SAFETY: MSR 0x1A2 is read-only and available when DTS is supported.
    let val = unsafe { rdmsr(MSR_TEMPERATURE_TARGET) };
    let tj = ((val >> 16) & 0xFF) as u8;

    // Sanity check: Tj_max should be between 50°C and 120°C.
    if tj >= 50 && tj <= 120 {
        tj
    } else {
        DEFAULT_TJ_MAX
    }
}

/// Check if PROCHOT is currently asserted (hardware thermal throttle).
fn is_prochot_active() -> bool {
    let status = unsafe { rdmsr(MSR_THERM_STATUS) };
    // Bit 0: Thermal status (PROCHOT active).
    status & 1 != 0
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// Thermal monitoring snapshot.
#[derive(Debug, Clone, Copy)]
pub struct ThermalInfo {
    /// Whether thermal monitoring is supported.
    pub supported: bool,
    /// Tj_max (maximum junction temperature, °C).
    pub tj_max: u8,
    /// Current package temperature (°C).
    pub current_temp: u8,
    /// Maximum temperature since boot (°C).
    pub max_temp: u8,
    /// Minimum temperature since boot (°C).
    pub min_temp: u8,
    /// Mean temperature (°C, from all samples).
    pub mean_temp: u8,
    /// Number of temperature samples taken.
    pub sample_count: u32,
    /// Whether CPU is currently thermally throttled.
    pub throttled: bool,
    /// Total throttle events since boot.
    pub throttle_count: u32,
    /// Total warning events.
    pub warn_count: u32,
    /// Total critical events.
    pub critical_count: u32,
}

/// Get a snapshot of thermal monitoring state.
#[must_use]
pub fn info() -> ThermalInfo {
    let sample_count = SAMPLE_COUNT.load(Ordering::Relaxed);
    let sum = TEMP_SUM.load(Ordering::Relaxed);
    #[allow(clippy::cast_possible_truncation)]
    let mean = if sample_count > 0 {
        (sum / u64::from(sample_count)) as u8
    } else {
        0
    };

    let min = MIN_TEMP.load(Ordering::Relaxed);

    ThermalInfo {
        supported: SUPPORTED.load(Ordering::Relaxed),
        tj_max: TJ_MAX.load(Ordering::Relaxed),
        current_temp: CURRENT_TEMP.load(Ordering::Relaxed),
        max_temp: MAX_TEMP.load(Ordering::Relaxed),
        min_temp: if min == 255 { 0 } else { min },
        mean_temp: mean,
        sample_count,
        throttled: THROTTLED.load(Ordering::Relaxed),
        throttle_count: THROTTLE_COUNT.load(Ordering::Relaxed),
        warn_count: WARN_COUNT.load(Ordering::Relaxed),
        critical_count: CRITICAL_COUNT.load(Ordering::Relaxed),
    }
}

/// Get recent temperature history (most recent first).
///
/// Returns up to `count` recent samples.
pub fn history(count: usize) -> alloc::vec::Vec<u8> {
    let total = SAMPLE_COUNT.load(Ordering::Relaxed) as usize;
    let available = total.min(HISTORY_SIZE);
    let n = count.min(available);

    if n == 0 {
        return alloc::vec::Vec::new();
    }

    let write_pos = HISTORY_IDX.load(Ordering::Relaxed) as usize;
    let mut result = alloc::vec::Vec::with_capacity(n);

    for i in 0..n {
        let idx = write_pos.wrapping_sub(1).wrapping_sub(i) % HISTORY_SIZE;
        let temp = TEMP_HISTORY[idx].load(Ordering::Relaxed);
        if temp > 0 {
            result.push(temp);
        }
    }

    result
}

/// Enable/disable thermal monitoring.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if thermal monitoring is supported on this CPU.
#[must_use]
pub fn is_supported() -> bool {
    SUPPORTED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// CPUID / MSR helpers
// ---------------------------------------------------------------------------

/// CPUID leaf 6 EAX: Thermal and Power Management features.
fn cpuid_leaf6_eax() -> u32 {
    let max_leaf: u32;
    // SAFETY: CPUID leaf 0 always valid.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "xor eax, eax",
            "cpuid",
            "pop rbx",
            lateout("eax") max_leaf,
            out("ecx") _,
            out("edx") _,
            options(nomem, nostack),
        );
    }
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

/// Read a Model-Specific Register.
///
/// # Safety
///
/// MSR must be valid and readable in current CPU mode.
#[inline]
unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
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

extern crate alloc;
