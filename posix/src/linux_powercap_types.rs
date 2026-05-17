//! `<linux/powercap.h>` — Power capping framework constants.
//!
//! The powercap framework exposes hardware power-limiting capabilities
//! via sysfs. It allows userspace to set power budgets on different
//! system components (CPU packages, DRAM, etc.). Intel RAPL (Running
//! Average Power Limit) is the primary backend, exposing per-package
//! and per-domain power limits. The framework supports hierarchical
//! power zones with multiple constraints (short-term and long-term
//! power limits with time windows).

// ---------------------------------------------------------------------------
// Powercap zone types (sysfs structure)
// ---------------------------------------------------------------------------

/// Package-level power zone (entire CPU socket).
pub const POWERCAP_ZONE_PACKAGE: u32 = 0;
/// Core power zone (CPU cores only).
pub const POWERCAP_ZONE_CORE: u32 = 1;
/// Uncore power zone (LLC, memory controller, etc.).
pub const POWERCAP_ZONE_UNCORE: u32 = 2;
/// DRAM power zone.
pub const POWERCAP_ZONE_DRAM: u32 = 3;
/// Platform power zone (entire SoC).
pub const POWERCAP_ZONE_PLATFORM: u32 = 4;

// ---------------------------------------------------------------------------
// Powercap constraint types
// ---------------------------------------------------------------------------

/// Long-term power limit (PL1, sustained).
pub const POWERCAP_CONSTRAINT_LONG_TERM: u32 = 0;
/// Short-term power limit (PL2, turbo burst).
pub const POWERCAP_CONSTRAINT_SHORT_TERM: u32 = 1;
/// Peak power limit (PL4, absolute max).
pub const POWERCAP_CONSTRAINT_PEAK: u32 = 2;

// ---------------------------------------------------------------------------
// Powercap attributes (sysfs files per zone)
// ---------------------------------------------------------------------------

/// Current energy counter (microjoules).
pub const POWERCAP_ATTR_ENERGY_UJ: u32 = 1;
/// Maximum energy counter range (microjoules).
pub const POWERCAP_ATTR_MAX_ENERGY_RANGE_UJ: u32 = 2;
/// Current power (microwatts, if supported).
pub const POWERCAP_ATTR_POWER_UW: u32 = 3;
/// Maximum power (microwatts).
pub const POWERCAP_ATTR_MAX_POWER_UW: u32 = 4;
/// Zone name.
pub const POWERCAP_ATTR_NAME: u32 = 5;
/// Zone enabled (1 = power limiting active).
pub const POWERCAP_ATTR_ENABLED: u32 = 6;

// ---------------------------------------------------------------------------
// Powercap constraint attributes
// ---------------------------------------------------------------------------

/// Power limit (microwatts).
pub const POWERCAP_CATTR_POWER_LIMIT_UW: u32 = 10;
/// Time window (microseconds).
pub const POWERCAP_CATTR_TIME_WINDOW_US: u32 = 11;
/// Maximum power limit (microwatts).
pub const POWERCAP_CATTR_MAX_POWER_UW: u32 = 12;
/// Minimum power limit (microwatts).
pub const POWERCAP_CATTR_MIN_POWER_UW: u32 = 13;
/// Maximum time window (microseconds).
pub const POWERCAP_CATTR_MAX_TIME_WINDOW_US: u32 = 14;
/// Minimum time window (microseconds).
pub const POWERCAP_CATTR_MIN_TIME_WINDOW_US: u32 = 15;
/// Constraint name.
pub const POWERCAP_CATTR_NAME: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zones_distinct() {
        let zones = [
            POWERCAP_ZONE_PACKAGE, POWERCAP_ZONE_CORE,
            POWERCAP_ZONE_UNCORE, POWERCAP_ZONE_DRAM,
            POWERCAP_ZONE_PLATFORM,
        ];
        for i in 0..zones.len() {
            for j in (i + 1)..zones.len() {
                assert_ne!(zones[i], zones[j]);
            }
        }
    }

    #[test]
    fn test_constraints_distinct() {
        let cs = [
            POWERCAP_CONSTRAINT_LONG_TERM,
            POWERCAP_CONSTRAINT_SHORT_TERM,
            POWERCAP_CONSTRAINT_PEAK,
        ];
        for i in 0..cs.len() {
            for j in (i + 1)..cs.len() {
                assert_ne!(cs[i], cs[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            POWERCAP_ATTR_ENERGY_UJ, POWERCAP_ATTR_MAX_ENERGY_RANGE_UJ,
            POWERCAP_ATTR_POWER_UW, POWERCAP_ATTR_MAX_POWER_UW,
            POWERCAP_ATTR_NAME, POWERCAP_ATTR_ENABLED,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_constraint_attrs_distinct() {
        let cattrs = [
            POWERCAP_CATTR_POWER_LIMIT_UW, POWERCAP_CATTR_TIME_WINDOW_US,
            POWERCAP_CATTR_MAX_POWER_UW, POWERCAP_CATTR_MIN_POWER_UW,
            POWERCAP_CATTR_MAX_TIME_WINDOW_US, POWERCAP_CATTR_MIN_TIME_WINDOW_US,
            POWERCAP_CATTR_NAME,
        ];
        for i in 0..cattrs.len() {
            for j in (i + 1)..cattrs.len() {
                assert_ne!(cattrs[i], cattrs[j]);
            }
        }
    }
}
