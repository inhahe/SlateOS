//! `<uapi/asm/papr_pdsm.h>` — PAPR NVDIMM platform-driver constants.
//!
//! PAPR PDSM (Platform Driver Specific Methods) interface used by
//! PowerVM logical NVDIMMs. The `ndctl` userspace tool sends these
//! command codes over `/dev/ndctlN` to query device health and
//! firmware version on POWER systems.

// ---------------------------------------------------------------------------
// Command codes (struct nd_papr_pdsm_health.cmd / pdsm.cmd)
// ---------------------------------------------------------------------------

/// Reserved sentinel.
pub const PAPR_PDSM_MIN: u32 = 0;
/// Retrieve device health status.
pub const PAPR_PDSM_HEALTH: u32 = 1;
/// Retrieve smart-event log size and contents.
pub const PAPR_PDSM_SMART_INJECT: u32 = 2;
/// Maximum currently defined PDSM.
pub const PAPR_PDSM_MAX: u32 = 3;

// ---------------------------------------------------------------------------
// Health status flag bits (struct nd_papr_pdsm_health.extension_flags)
// ---------------------------------------------------------------------------

/// PDSM extension flags include shutdown_count.
pub const PDSM_DIMM_HEALTH_RUN_GAUGE_VALID: u32 = 1 << 0;
/// DIMM is unable to persist memory.
pub const PDSM_DIMM_DSC_VALID: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Health byte codes (struct nd_papr_pdsm_health.dimm_health)
// ---------------------------------------------------------------------------

/// DIMM is healthy.
pub const PAPR_PDSM_DIMM_HEALTHY: u8 = 0;
/// DIMM is reporting non-critical warnings.
pub const PAPR_PDSM_DIMM_UNHEALTHY: u8 = 1;
/// DIMM is critically degraded.
pub const PAPR_PDSM_DIMM_CRITICAL: u8 = 2;
/// DIMM is fatally failed.
pub const PAPR_PDSM_DIMM_FATAL: u8 = 3;

// ---------------------------------------------------------------------------
// SMART injection flag bits (struct nd_papr_pdsm_smart_inject.flags)
// ---------------------------------------------------------------------------

/// Inject UNSAFE-shutdown signal.
pub const PDSM_SMART_INJECT_HEALTH_FATAL: u32 = 1 << 0;
/// Inject SHUTDOWN-state signal.
pub const PDSM_SMART_INJECT_BAD_SHUTDOWN: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_codes_distinct_and_within_range() {
        let cmds = [PAPR_PDSM_HEALTH, PAPR_PDSM_SMART_INJECT];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
            assert!(cmds[i] > PAPR_PDSM_MIN);
            assert!(cmds[i] < PAPR_PDSM_MAX);
        }
    }

    #[test]
    fn test_extension_flags_distinct_powers_of_two() {
        let flags = [PDSM_DIMM_HEALTH_RUN_GAUGE_VALID, PDSM_DIMM_DSC_VALID];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        assert_ne!(flags[0], flags[1]);
    }

    #[test]
    fn test_health_codes_ordered_by_severity() {
        // Health-byte ordering must be monotonically increasing in
        // severity so userspace can use a simple comparison.
        assert!(PAPR_PDSM_DIMM_HEALTHY < PAPR_PDSM_DIMM_UNHEALTHY);
        assert!(PAPR_PDSM_DIMM_UNHEALTHY < PAPR_PDSM_DIMM_CRITICAL);
        assert!(PAPR_PDSM_DIMM_CRITICAL < PAPR_PDSM_DIMM_FATAL);
    }

    #[test]
    fn test_smart_inject_flags_distinct_powers_of_two() {
        let flags = [PDSM_SMART_INJECT_HEALTH_FATAL, PDSM_SMART_INJECT_BAD_SHUTDOWN];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        assert_ne!(flags[0], flags[1]);
    }
}
