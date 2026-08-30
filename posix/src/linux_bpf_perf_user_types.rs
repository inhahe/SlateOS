//! `<linux/bpf_perf_event.h>` — BPF ↔ perf-event integration.
//!
//! BPF programs of type `BPF_PROG_TYPE_PERF_EVENT` and BPF map type
//! `BPF_MAP_TYPE_PERF_EVENT_ARRAY` exchange data with the perf
//! ringbuffer. This module covers the perf-side ioctls that attach
//! a BPF program to a perf event and the perf cookie/IDs.

// ---------------------------------------------------------------------------
// perf_event ioctls that take a BPF prog fd
// ---------------------------------------------------------------------------

/// `_IOW('$', 8, int)` — attach a BPF prog fd to the perf event.
pub const PERF_EVENT_IOC_SET_BPF: u32 = 0x4004_2408;

/// `_IO('$', 7)` — refresh prepared perf event (used with overflow).
pub const PERF_EVENT_IOC_REFRESH: u32 = 0x2407;

/// `_IO('$', 0)` — enable a perf event.
pub const PERF_EVENT_IOC_ENABLE: u32 = 0x2400;

/// `_IO('$', 1)` — disable a perf event.
pub const PERF_EVENT_IOC_DISABLE: u32 = 0x2401;

/// `_IO('$', 2)` — reset counter to zero.
pub const PERF_EVENT_IOC_RESET: u32 = 0x2403;

// ---------------------------------------------------------------------------
// perf-side magic / cookie sizes
// ---------------------------------------------------------------------------

/// Maximum number of perf-event-array entries (matches NR_CPUS upper bound).
pub const BPF_PERF_EVENT_ARRAY_MAX_ENTRIES: u32 = 4_096;

/// BPF cookie size carried on each perf sample (u64).
pub const BPF_PERF_EVENT_COOKIE_BYTES: usize = 8;

/// Stack-trace bucket maximum depth (max frames captured per sample).
pub const PERF_MAX_STACK_DEPTH: u32 = 127;

// ---------------------------------------------------------------------------
// `bpf_perf_event_value` field offsets (read by `bpf_perf_event_read_value()`)
// ---------------------------------------------------------------------------

pub const BPF_PERF_EVENT_VALUE_OFF_COUNTER: usize = 0;
pub const BPF_PERF_EVENT_VALUE_OFF_ENABLED: usize = 8;
pub const BPF_PERF_EVENT_VALUE_OFF_RUNNING: usize = 16;
pub const BPF_PERF_EVENT_VALUE_SIZE: usize = 24;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_bpf_ioctl_encoded_as_iow() {
        // SET_BPF embeds the int-sized argument in bits 16..30 (size=4)
        // and the direction (_IOC_WRITE = 0x4000_0000) in bits 30..32.
        assert_eq!(PERF_EVENT_IOC_SET_BPF, 0x4004_2408);
        // Type byte is '$' (0x24).
        assert_eq!((PERF_EVENT_IOC_SET_BPF >> 8) & 0xFF, 0x24);
        // Number is 0x08.
        assert_eq!(PERF_EVENT_IOC_SET_BPF & 0xFF, 0x08);
        // Argument size is 4 bytes (sizeof(int)).
        assert_eq!((PERF_EVENT_IOC_SET_BPF >> 16) & 0x3FFF, 4);
    }

    #[test]
    fn test_basic_perf_ioctls_in_dollar_family() {
        for v in [
            PERF_EVENT_IOC_ENABLE,
            PERF_EVENT_IOC_DISABLE,
            PERF_EVENT_IOC_RESET,
            PERF_EVENT_IOC_REFRESH,
        ] {
            // All start with the '$' type byte.
            assert_eq!((v >> 8) & 0xFF, 0x24);
        }
        // ENABLE / DISABLE are adjacent in the table.
        assert_eq!(PERF_EVENT_IOC_DISABLE - PERF_EVENT_IOC_ENABLE, 1);
    }

    #[test]
    fn test_perf_event_array_capacity() {
        assert_eq!(BPF_PERF_EVENT_ARRAY_MAX_ENTRIES, 4_096);
        assert!(BPF_PERF_EVENT_ARRAY_MAX_ENTRIES.is_power_of_two());
    }

    #[test]
    fn test_cookie_and_stack_sizes() {
        assert_eq!(BPF_PERF_EVENT_COOKIE_BYTES, 8);
        // Each cookie is a u64.
        assert_eq!(BPF_PERF_EVENT_COOKIE_BYTES * 8, 64);
        // 127 frames is the hardcoded maximum.
        assert_eq!(PERF_MAX_STACK_DEPTH, 127);
    }

    #[test]
    fn test_perf_event_value_layout() {
        // The structure is three packed u64 fields.
        assert_eq!(BPF_PERF_EVENT_VALUE_OFF_COUNTER, 0);
        assert_eq!(BPF_PERF_EVENT_VALUE_OFF_ENABLED, 8);
        assert_eq!(BPF_PERF_EVENT_VALUE_OFF_RUNNING, 16);
        assert_eq!(BPF_PERF_EVENT_VALUE_SIZE, 24);
        // Each consecutive offset differs by 8 bytes (u64).
        assert_eq!(
            BPF_PERF_EVENT_VALUE_OFF_ENABLED - BPF_PERF_EVENT_VALUE_OFF_COUNTER,
            8
        );
        assert_eq!(
            BPF_PERF_EVENT_VALUE_OFF_RUNNING - BPF_PERF_EVENT_VALUE_OFF_ENABLED,
            8
        );
        // Three u64s = 24 bytes total.
        assert_eq!(BPF_PERF_EVENT_VALUE_SIZE, 3 * 8);
    }
}
