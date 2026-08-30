//! `<sys/times.h>` — Process time accounting constants.
//!
//! `times()` returns process and child time in clock ticks.
//! These constants define the `struct tms` field offsets and
//! related timing constants.

// ---------------------------------------------------------------------------
// struct tms field offsets (bytes, Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of tms_utime (user CPU time of process) in struct tms.
pub const TMS_OFF_UTIME: u32 = 0;
/// Offset of tms_stime (system CPU time of process) in struct tms.
pub const TMS_OFF_STIME: u32 = 8;
/// Offset of tms_cutime (user CPU time of children) in struct tms.
pub const TMS_OFF_CUTIME: u32 = 16;
/// Offset of tms_cstime (system CPU time of children) in struct tms.
pub const TMS_OFF_CSTIME: u32 = 24;

/// Size of struct tms on Linux x86_64 (bytes).
pub const TMS_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// Clock ticks
// ---------------------------------------------------------------------------

/// Standard clock ticks per second (USER_HZ) on Linux.
pub const CLK_TCK: u32 = 100;
/// Kernel internal timer frequency (CONFIG_HZ) — common default.
pub const KERNEL_HZ_DEFAULT: u32 = 250;
/// Alternative kernel HZ for desktop/low-latency kernels.
pub const KERNEL_HZ_DESKTOP: u32 = 1000;
/// Alternative kernel HZ for server kernels.
pub const KERNEL_HZ_SERVER: u32 = 100;

// ---------------------------------------------------------------------------
// times() error return
// ---------------------------------------------------------------------------

/// Error return value from times() (cast from (clock_t)-1).
pub const TIMES_ERROR: i64 = -1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offsets_ascending() {
        let offsets = [TMS_OFF_UTIME, TMS_OFF_STIME, TMS_OFF_CUTIME, TMS_OFF_CSTIME];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(TMS_OFF_CSTIME < TMS_SIZE);
    }

    #[test]
    fn test_struct_size() {
        assert_eq!(TMS_SIZE, 32);
    }

    #[test]
    fn test_utime_at_start() {
        assert_eq!(TMS_OFF_UTIME, 0);
    }

    #[test]
    fn test_clk_tck() {
        assert_eq!(CLK_TCK, 100);
    }

    #[test]
    fn test_kernel_hz_values_distinct() {
        let hz_values = [KERNEL_HZ_DEFAULT, KERNEL_HZ_DESKTOP, KERNEL_HZ_SERVER];
        for i in 0..hz_values.len() {
            for j in (i + 1)..hz_values.len() {
                assert_ne!(hz_values[i], hz_values[j]);
            }
        }
    }

    #[test]
    fn test_times_error() {
        assert_eq!(TIMES_ERROR, -1);
    }

    #[test]
    fn test_clk_tck_positive() {
        assert!(CLK_TCK > 0);
    }
}
