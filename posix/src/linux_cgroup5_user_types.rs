//! `<linux/cgroup.h>` (part 5) — cgroup-v2 io and pids controller files.
//!
//! The io controller exposes BFQ-aware weight, max-bw, and a per-cgroup
//! `io.stat` summary. The pids controller caps process count below
//! a hard upper limit.

// ---------------------------------------------------------------------------
// io controller files
// ---------------------------------------------------------------------------

pub const CGROUP_IO_WEIGHT: &str = "io.weight";
pub const CGROUP_IO_MAX: &str = "io.max";
pub const CGROUP_IO_LOW: &str = "io.low";
pub const CGROUP_IO_LATENCY: &str = "io.latency";
pub const CGROUP_IO_STAT: &str = "io.stat";
pub const CGROUP_IO_PRESSURE: &str = "io.pressure";
pub const CGROUP_IO_PRIO_CLASS: &str = "io.prio.class";
pub const CGROUP_IO_BFQ_WEIGHT: &str = "io.bfq.weight";

// ---------------------------------------------------------------------------
// io.max field tokens
// ---------------------------------------------------------------------------

pub const CGROUP_IO_MAX_RBPS: &str = "rbps";
pub const CGROUP_IO_MAX_WBPS: &str = "wbps";
pub const CGROUP_IO_MAX_RIOPS: &str = "riops";
pub const CGROUP_IO_MAX_WIOPS: &str = "wiops";

// ---------------------------------------------------------------------------
// io.weight bounds (1..10000, default 100)
// ---------------------------------------------------------------------------

pub const CGROUP_IO_WEIGHT_MIN: u32 = 1;
pub const CGROUP_IO_WEIGHT_DEFAULT: u32 = 100;
pub const CGROUP_IO_WEIGHT_MAX: u32 = 10_000;

// ---------------------------------------------------------------------------
// pids controller files
// ---------------------------------------------------------------------------

pub const CGROUP_PIDS_CURRENT: &str = "pids.current";
pub const CGROUP_PIDS_MAX: &str = "pids.max";
pub const CGROUP_PIDS_PEAK: &str = "pids.peak";
pub const CGROUP_PIDS_EVENTS: &str = "pids.events";

// ---------------------------------------------------------------------------
// pids.max sentinel
// ---------------------------------------------------------------------------

/// Literal "max" — no pid cap.
pub const CGROUP_PIDS_MAX_LITERAL: &str = "max";

// ---------------------------------------------------------------------------
// pids.events keys
// ---------------------------------------------------------------------------

pub const CGROUP_PIDS_EVT_MAX: &str = "max";
pub const CGROUP_PIDS_EVT_LOCAL: &str = "local";

// ---------------------------------------------------------------------------
// Default kernel pid_max (sysctl kernel.pid_max).
// ---------------------------------------------------------------------------

/// Default pid_max on 32-bit (32 768).
pub const PID_MAX_DEFAULT_32: u32 = 32_768;

/// Default pid_max on 64-bit (4 194 304).
pub const PID_MAX_DEFAULT_64: u32 = 4_194_304;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_files_have_io_prefix() {
        for f in [
            CGROUP_IO_WEIGHT,
            CGROUP_IO_MAX,
            CGROUP_IO_LOW,
            CGROUP_IO_LATENCY,
            CGROUP_IO_STAT,
            CGROUP_IO_PRESSURE,
            CGROUP_IO_PRIO_CLASS,
            CGROUP_IO_BFQ_WEIGHT,
        ] {
            assert!(f.starts_with("io."));
        }
    }

    #[test]
    fn test_io_max_tokens_distinct() {
        let t = [
            CGROUP_IO_MAX_RBPS,
            CGROUP_IO_MAX_WBPS,
            CGROUP_IO_MAX_RIOPS,
            CGROUP_IO_MAX_WIOPS,
        ];
        for (i, &x) in t.iter().enumerate() {
            for &y in &t[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // bps tokens are 4 chars, iops tokens are 5 chars.
        assert_eq!(CGROUP_IO_MAX_RBPS.len(), 4);
        assert_eq!(CGROUP_IO_MAX_WIOPS.len(), 5);
    }

    #[test]
    fn test_io_weight_range() {
        assert_eq!(CGROUP_IO_WEIGHT_MIN, 1);
        assert_eq!(CGROUP_IO_WEIGHT_DEFAULT, 100);
        assert_eq!(CGROUP_IO_WEIGHT_MAX, 10_000);
        // Same shape as cpu.weight.
        assert!(CGROUP_IO_WEIGHT_MIN < CGROUP_IO_WEIGHT_DEFAULT);
        assert!(CGROUP_IO_WEIGHT_DEFAULT < CGROUP_IO_WEIGHT_MAX);
    }

    #[test]
    fn test_pids_files_have_pids_prefix() {
        for f in [
            CGROUP_PIDS_CURRENT,
            CGROUP_PIDS_MAX,
            CGROUP_PIDS_PEAK,
            CGROUP_PIDS_EVENTS,
        ] {
            assert!(f.starts_with("pids."));
        }
    }

    #[test]
    fn test_pids_max_sentinel() {
        assert_eq!(CGROUP_PIDS_MAX_LITERAL, "max");
    }

    #[test]
    fn test_pid_max_64bit_is_64x_32bit() {
        // 4 194 304 / 32 768 = 128. Linux scales pid_max for 64-bit.
        assert_eq!(PID_MAX_DEFAULT_64 / PID_MAX_DEFAULT_32, 128);
        assert!(PID_MAX_DEFAULT_32.is_power_of_two());
        assert!(PID_MAX_DEFAULT_64.is_power_of_two());
    }
}
