//! IRQ storm detection and mitigation.
//!
//! Monitors per-IRQ interrupt rates and detects "storms" — situations
//! where a single IRQ line fires at an excessive rate, monopolizing
//! CPU time.  Common causes:
//!
//! - Broken/misconfigured hardware generating spurious interrupts.
//! - Level-triggered IRQ not properly acknowledged by the device driver.
//! - Faulty PCI card holding its interrupt line asserted.
//!
//! ## Detection
//!
//! Each IRQ has a per-second counter.  Every 100 ticks (1 second), the
//! BSP checks all counters.  If any exceeds [`STORM_THRESHOLD`], the IRQ
//! is considered storming.
//!
//! ## Mitigation
//!
//! 1. First offense: log a warning (rate-limited).
//! 2. After [`STORM_STRIKES`] consecutive offenses: mask the IRQ line
//!    via IOAPIC, preventing further interrupts.
//! 3. After [`COOLDOWN_SECONDS`]: unmask and observe.  If the storm
//!    resumes, re-mask with exponential backoff.
//!
//! ## Why This Matters
//!
//! An IRQ storm at 100,000+ interrupts/sec can consume 100% of a CPU
//! core in ISR handling, starving all other work.  Detection and masking
//! limits the damage to a brief burst before the problematic line is
//! silenced.
//!
//! ## References
//!
//! - Linux `kernel/irq/spurious.c` — `note_interrupt()`, `__report_bad_irq()`
//! - Linux `CONFIG_IRQ_FORCED_THREADING` — threaded IRQ mitigation
//! - FreeBSD `sys/kern/kern_intr.c` — interrupt storm detection

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of IRQ lines to monitor.
const MAX_IRQS: usize = 24;

/// Interrupt rate (per second) above which an IRQ is considered "storming."
///
/// 10,000 IRQs/sec is already aggressive for most devices.  A keyboard
/// generates ~100/sec, disk I/O ~1000/sec, network at full load ~50,000/sec.
/// We set the threshold high enough to avoid false positives on loaded NICs
/// but low enough to catch true storms from misconfigured hardware.
const STORM_THRESHOLD: u32 = 50_000;

/// Number of consecutive storm-detection intervals required before masking.
///
/// Avoids masking on brief transient bursts (e.g., network packet storm
/// during a large transfer).  Two consecutive seconds of excessive rate
/// means it's not transient.
const STORM_STRIKES: u32 = 3;

/// Seconds to keep a masked IRQ disabled before attempting unmask.
///
/// After masking, we wait this long before trying to unmask.  If the
/// storm resumes, we re-mask with doubled cooldown (exponential backoff,
/// capped at 60 seconds).
const COOLDOWN_SECONDS: u32 = 5;

/// Maximum cooldown (exponential backoff cap).
const MAX_COOLDOWN: u32 = 60;

// ---------------------------------------------------------------------------
// Per-IRQ state
// ---------------------------------------------------------------------------

/// Per-IRQ storm detection state.
struct IrqStormState {
    /// Interrupt count in the current 1-second window.
    window_count: AtomicU32,
    /// Number of consecutive windows exceeding the threshold.
    strike_count: AtomicU32,
    /// Whether this IRQ is currently masked due to a storm.
    masked: AtomicBool,
    /// Tick count when the IRQ was masked (for cooldown timing).
    masked_at_tick: AtomicU64,
    /// Current cooldown duration (seconds, doubles on repeated storms).
    cooldown_secs: AtomicU32,
    /// Total number of storms detected on this IRQ since boot.
    total_storms: AtomicU32,
    /// Total interrupts suppressed by masking (estimated from rate × mask time).
    total_suppressed: AtomicU64,
}

impl IrqStormState {
    const fn new() -> Self {
        Self {
            window_count: AtomicU32::new(0),
            strike_count: AtomicU32::new(0),
            masked: AtomicBool::new(false),
            masked_at_tick: AtomicU64::new(0),
            cooldown_secs: AtomicU32::new(COOLDOWN_SECONDS),
            total_storms: AtomicU32::new(0),
            total_suppressed: AtomicU64::new(0),
        }
    }
}

/// Per-IRQ storm detection array.
static IRQ_STORM: [IrqStormState; MAX_IRQS] = [const { IrqStormState::new() }; MAX_IRQS];

/// Global flag: storm detection enabled (can be disabled via sysctl/kshell).
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Total storms detected since boot (across all IRQs).
static TOTAL_STORMS: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Public API — ISR path (must be ultra-fast)
// ---------------------------------------------------------------------------

/// Record an interrupt on the given IRQ line.
///
/// Called from the device ISR handler.  Cost: one atomic increment (~3 cycles).
/// The check-and-mask logic runs separately in the periodic scan.
#[inline]
pub fn record_irq(irq: u32) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(state) = IRQ_STORM.get(irq as usize) {
        state.window_count.fetch_add(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Public API — periodic scan (called from timer softirq, BSP only)
// ---------------------------------------------------------------------------

/// Periodic storm check — call once per second from the BSP.
///
/// Examines each IRQ's count for the previous window, detects storms,
/// manages mask/unmask lifecycle.
pub fn periodic_check() {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let current_tick = crate::apic::tick_count();

    for irq in 0..MAX_IRQS {
        let state = &IRQ_STORM[irq];

        // Read and reset the window counter.
        let count = state.window_count.swap(0, Ordering::Relaxed);

        if state.masked.load(Ordering::Relaxed) {
            // IRQ is masked — check if cooldown has expired.
            let masked_at = state.masked_at_tick.load(Ordering::Relaxed);
            let cooldown = state.cooldown_secs.load(Ordering::Relaxed);
            #[allow(clippy::arithmetic_side_effects)]
            let cooldown_ticks = u64::from(cooldown) * 100; // 100 Hz timer

            if current_tick.saturating_sub(masked_at) >= cooldown_ticks {
                // Cooldown expired — try to unmask.
                state.masked.store(false, Ordering::Relaxed);
                state.strike_count.store(0, Ordering::Relaxed);

                // SAFETY: irq < MAX_IRQS < 256, IOAPIC is initialized.
                #[allow(clippy::cast_possible_truncation)]
                unsafe {
                    crate::ioapic::unmask_irq(irq as u8);
                }

                crate::serial_println!(
                    "[irq-storm] IRQ {} unmasked after {}s cooldown (observing...)",
                    irq, cooldown
                );
            } else {
                // Still in cooldown — estimate suppressed interrupts.
                // Use the last known rate (count should be 0 while masked,
                // but account for any that slipped through during unmask).
                state.total_suppressed.fetch_add(
                    u64::from(STORM_THRESHOLD), Ordering::Relaxed
                );
            }
            continue;
        }

        // IRQ is active — check rate.
        if count >= STORM_THRESHOLD {
            // Over threshold — record a strike.
            let strikes = state.strike_count.fetch_add(1, Ordering::Relaxed)
                .saturating_add(1);

            if strikes >= STORM_STRIKES {
                // Storm confirmed — mask the IRQ.
                state.masked.store(true, Ordering::Relaxed);
                state.masked_at_tick.store(current_tick, Ordering::Relaxed);
                state.total_storms.fetch_add(1, Ordering::Relaxed);
                TOTAL_STORMS.fetch_add(1, Ordering::Relaxed);

                // Double the cooldown for repeated storms (exponential backoff).
                let prev_cooldown = state.cooldown_secs.load(Ordering::Relaxed);
                let new_cooldown = prev_cooldown.saturating_mul(2).min(MAX_COOLDOWN);
                state.cooldown_secs.store(new_cooldown, Ordering::Relaxed);

                // Mask via IOAPIC.
                #[allow(clippy::cast_possible_truncation)]
                crate::ioapic::mask_irq(irq as u8);

                crate::serial_println!(
                    "[irq-storm] IRQ {} MASKED: {} IRQs/sec for {}s (storm #{}, cooldown {}s)",
                    irq, count, strikes, state.total_storms.load(Ordering::Relaxed),
                    new_cooldown
                );

                // Log to kwarn for kshell visibility.
                crate::kwarn::warn(
                    "IRQ storm detected and masked",
                    "irq_storm.rs",
                    line!(),
                );
            }
        } else {
            // Rate is normal — decay the strike count.
            let current_strikes = state.strike_count.load(Ordering::Relaxed);
            if current_strikes > 0 {
                state.strike_count.store(
                    current_strikes.saturating_sub(1), Ordering::Relaxed
                );
            }

            // Reset cooldown on sustained good behavior.
            if current_strikes == 0 {
                state.cooldown_secs.store(COOLDOWN_SECONDS, Ordering::Relaxed);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// Per-IRQ storm statistics.
#[derive(Debug, Clone, Copy)]
pub struct IrqStormStats {
    /// IRQ line number.
    pub irq: usize,
    /// Current strike count.
    pub strikes: u32,
    /// Whether currently masked.
    pub masked: bool,
    /// Total storms detected.
    pub total_storms: u32,
    /// Current cooldown setting (seconds).
    pub cooldown_secs: u32,
}

/// Get storm stats for all IRQs.
pub fn stats() -> alloc::vec::Vec<IrqStormStats> {
    let mut result = alloc::vec::Vec::new();
    for irq in 0..MAX_IRQS {
        let state = &IRQ_STORM[irq];
        let storms = state.total_storms.load(Ordering::Relaxed);
        let strikes = state.strike_count.load(Ordering::Relaxed);
        let masked = state.masked.load(Ordering::Relaxed);
        // Only include IRQs that have had activity.
        if storms > 0 || strikes > 0 || masked {
            result.push(IrqStormStats {
                irq,
                strikes,
                masked,
                total_storms: storms,
                cooldown_secs: state.cooldown_secs.load(Ordering::Relaxed),
            });
        }
    }
    result
}

/// Get the total number of storms detected since boot.
#[must_use]
pub fn total_storms() -> u32 {
    TOTAL_STORMS.load(Ordering::Relaxed)
}

/// Enable or disable storm detection.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if storm detection is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Manually unmask an IRQ that was storm-masked (for debugging).
///
/// Resets the storm state for that IRQ.
pub fn force_unmask(irq: usize) {
    if let Some(state) = IRQ_STORM.get(irq) {
        state.masked.store(false, Ordering::Relaxed);
        state.strike_count.store(0, Ordering::Relaxed);
        state.cooldown_secs.store(COOLDOWN_SECONDS, Ordering::Relaxed);

        #[allow(clippy::cast_possible_truncation)]
        unsafe {
            crate::ioapic::unmask_irq(irq as u8);
        }
    }
}

extern crate alloc;
