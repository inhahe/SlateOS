//! `<linux/pm_domain.h>` — Generic Power Domain (genpd) constants.
//!
//! Generic power domains represent hardware power islands on SoCs.
//! When all devices in a power domain are idle, the entire domain
//! can be powered off to save energy. Genpd manages domain power
//! state transitions, ensuring proper sequencing (enable clocks →
//! deassert resets → restore state) and respecting parent-child
//! domain hierarchies (child domain can't be on if parent is off).

// ---------------------------------------------------------------------------
// Power domain states
// ---------------------------------------------------------------------------

/// Domain is powered on.
pub const GENPD_STATE_ON: u32 = 0;
/// Domain is powered off.
pub const GENPD_STATE_OFF: u32 = 1;

// ---------------------------------------------------------------------------
// Power domain flags
// ---------------------------------------------------------------------------

/// Domain is always on (cannot be powered off).
pub const GENPD_FLAG_ALWAYS_ON: u32 = 1 << 0;
/// Domain is active wakeup (can wake system from sleep).
pub const GENPD_FLAG_ACTIVE_WAKEUP: u32 = 1 << 1;
/// Domain supports runtime PM.
pub const GENPD_FLAG_RPM_ALWAYS_ON: u32 = 1 << 2;
/// Domain is an IRQ safe domain (can be managed in IRQ context).
pub const GENPD_FLAG_IRQ_SAFE: u32 = 1 << 3;
/// Domain has a single device (optimize for single-device domains).
pub const GENPD_FLAG_CPU_DOMAIN: u32 = 1 << 4;
/// Domain should not account idle time.
pub const GENPD_FLAG_MIN_RESIDENCY: u32 = 1 << 5;
/// OPP (performance state) supported.
pub const GENPD_FLAG_OPP_TABLE: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Power domain performance states
// ---------------------------------------------------------------------------

/// No performance state constraint.
pub const GENPD_PERF_STATE_NONE: u32 = 0;
/// Low performance state (power saving).
pub const GENPD_PERF_STATE_LOW: u32 = 1;
/// Medium performance state.
pub const GENPD_PERF_STATE_MEDIUM: u32 = 2;
/// High performance state.
pub const GENPD_PERF_STATE_HIGH: u32 = 3;
/// Maximum performance state (turbo).
pub const GENPD_PERF_STATE_MAX: u32 = 4;

// ---------------------------------------------------------------------------
// Power domain suspend modes
// ---------------------------------------------------------------------------

/// Suspend: deep power off (full context lost).
pub const GENPD_SUSPEND_DEEP: u32 = 0;
/// Suspend: retention (domain off, context retained in SRAM).
pub const GENPD_SUSPEND_RETENTION: u32 = 1;
/// Suspend: standby (domain on but idle).
pub const GENPD_SUSPEND_STANDBY: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        assert_ne!(GENPD_STATE_ON, GENPD_STATE_OFF);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            GENPD_FLAG_ALWAYS_ON,
            GENPD_FLAG_ACTIVE_WAKEUP,
            GENPD_FLAG_RPM_ALWAYS_ON,
            GENPD_FLAG_IRQ_SAFE,
            GENPD_FLAG_CPU_DOMAIN,
            GENPD_FLAG_MIN_RESIDENCY,
            GENPD_FLAG_OPP_TABLE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_perf_states_distinct() {
        let states = [
            GENPD_PERF_STATE_NONE,
            GENPD_PERF_STATE_LOW,
            GENPD_PERF_STATE_MEDIUM,
            GENPD_PERF_STATE_HIGH,
            GENPD_PERF_STATE_MAX,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_suspend_modes_distinct() {
        let modes = [
            GENPD_SUSPEND_DEEP,
            GENPD_SUSPEND_RETENTION,
            GENPD_SUSPEND_STANDBY,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
