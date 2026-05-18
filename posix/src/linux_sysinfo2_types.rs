//! `<linux/sysinfo.h>` — Additional system information constants.
//!
//! Supplementary sysinfo constants covering memory unit
//! multipliers, load average scales, and system limits.

// ---------------------------------------------------------------------------
// Load average scale
// ---------------------------------------------------------------------------

/// Load average scale factor (1 << 16).
pub const LOAD_SHIFT: u32 = 16;
/// Load average scale denominator.
pub const LOAD_SCALE: u32 = 1 << LOAD_SHIFT;
/// Fixed-point 1.0.
pub const LOAD_INT: u32 = LOAD_SCALE;

// ---------------------------------------------------------------------------
// Load average EXP constants (for 1, 5, 15 min)
// ---------------------------------------------------------------------------

/// EXP factor for 1-minute average.
pub const EXP_1: u32 = 1884;
/// EXP factor for 5-minute average.
pub const EXP_5: u32 = 2014;
/// EXP factor for 15-minute average.
pub const EXP_15: u32 = 2037;

// ---------------------------------------------------------------------------
// System limits
// ---------------------------------------------------------------------------

/// Maximum hostname length.
pub const HOST_NAME_MAX: u32 = 64;
/// Maximum domain name length.
pub const DOMAIN_NAME_MAX: u32 = 64;
/// Maximum login name length.
pub const LOGIN_NAME_MAX: u32 = 256;

// ---------------------------------------------------------------------------
// Memory info fields (for /proc/meminfo parsing)
// ---------------------------------------------------------------------------

/// MemTotal field index.
pub const MEMINFO_TOTAL: u32 = 0;
/// MemFree field index.
pub const MEMINFO_FREE: u32 = 1;
/// MemAvailable field index.
pub const MEMINFO_AVAILABLE: u32 = 2;
/// Buffers field index.
pub const MEMINFO_BUFFERS: u32 = 3;
/// Cached field index.
pub const MEMINFO_CACHED: u32 = 4;
/// SwapTotal field index.
pub const MEMINFO_SWAP_TOTAL: u32 = 5;
/// SwapFree field index.
pub const MEMINFO_SWAP_FREE: u32 = 6;

// ---------------------------------------------------------------------------
// CPU info fields
// ---------------------------------------------------------------------------

/// Number of online CPUs.
pub const CPUINFO_ONLINE: u32 = 0;
/// Number of possible CPUs.
pub const CPUINFO_POSSIBLE: u32 = 1;
/// Number of present CPUs.
pub const CPUINFO_PRESENT: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_scale() {
        assert_eq!(LOAD_SCALE, 1 << 16);
        assert_eq!(LOAD_INT, LOAD_SCALE);
    }

    #[test]
    fn test_exp_values_distinct() {
        let exps = [EXP_1, EXP_5, EXP_15];
        for i in 0..exps.len() {
            for j in (i + 1)..exps.len() {
                assert_ne!(exps[i], exps[j]);
            }
        }
    }

    #[test]
    fn test_exp_ordering() {
        assert!(EXP_1 < EXP_5);
        assert!(EXP_5 < EXP_15);
    }

    #[test]
    fn test_name_limits() {
        assert_eq!(HOST_NAME_MAX, 64);
        assert_eq!(DOMAIN_NAME_MAX, 64);
        assert!(LOGIN_NAME_MAX > HOST_NAME_MAX);
    }

    #[test]
    fn test_meminfo_fields_distinct() {
        let fields = [
            MEMINFO_TOTAL, MEMINFO_FREE, MEMINFO_AVAILABLE,
            MEMINFO_BUFFERS, MEMINFO_CACHED,
            MEMINFO_SWAP_TOTAL, MEMINFO_SWAP_FREE,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_cpuinfo_fields_distinct() {
        let fields = [CPUINFO_ONLINE, CPUINFO_POSSIBLE, CPUINFO_PRESENT];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }
}
