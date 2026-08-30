//! `<linux/sysinfo.h>` — sysinfo() system information constants.
//!
//! sysinfo() returns a snapshot of system resource usage: uptime,
//! load averages, total/free/shared/buffer RAM, swap usage, and
//! number of running processes. Used by `free`, `uptime`, `top`,
//! and system monitoring tools.

// ---------------------------------------------------------------------------
// sysinfo structure field indices (conceptual)
// ---------------------------------------------------------------------------

/// Load average measurement interval 1 (1 minute).
pub const SI_LOAD_1: u32 = 0;
/// Load average measurement interval 2 (5 minutes).
pub const SI_LOAD_5: u32 = 1;
/// Load average measurement interval 3 (15 minutes).
pub const SI_LOAD_15: u32 = 2;

// ---------------------------------------------------------------------------
// Load average scaling
// ---------------------------------------------------------------------------

/// Load average fixed-point shift (result = raw >> SI_LOAD_SHIFT).
pub const SI_LOAD_SHIFT: u32 = 16;

// ---------------------------------------------------------------------------
// Memory unit
// ---------------------------------------------------------------------------

/// Memory amounts in sysinfo are in mem_unit-byte units.
/// When mem_unit is 1, values are in bytes.
pub const SI_MEM_UNIT_DEFAULT: u32 = 1;

// ---------------------------------------------------------------------------
// System limits visible through sysinfo
// ---------------------------------------------------------------------------

/// Maximum hostname length.
pub const HOST_NAME_MAX: u32 = 64;
/// Maximum domain name length.
pub const DOMAIN_NAME_MAX: u32 = 64;

// ---------------------------------------------------------------------------
// Uptime-related constants
// ---------------------------------------------------------------------------

/// Seconds per minute.
pub const SECS_PER_MIN: u32 = 60;
/// Seconds per hour.
pub const SECS_PER_HOUR: u32 = 3600;
/// Seconds per day.
pub const SECS_PER_DAY: u32 = 86400;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_indices_distinct() {
        assert_ne!(SI_LOAD_1, SI_LOAD_5);
        assert_ne!(SI_LOAD_5, SI_LOAD_15);
        assert_ne!(SI_LOAD_1, SI_LOAD_15);
    }

    #[test]
    fn test_load_shift() {
        assert_eq!(SI_LOAD_SHIFT, 16);
        assert_eq!(1u32 << SI_LOAD_SHIFT, 65536);
    }

    #[test]
    fn test_time_constants() {
        assert_eq!(SECS_PER_MIN, 60);
        assert_eq!(SECS_PER_HOUR, 60 * 60);
        assert_eq!(SECS_PER_DAY, 60 * 60 * 24);
    }

    #[test]
    fn test_name_limits() {
        assert!(HOST_NAME_MAX > 0);
        assert!(DOMAIN_NAME_MAX > 0);
    }
}
