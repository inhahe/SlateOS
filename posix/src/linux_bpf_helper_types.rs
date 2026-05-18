//! `<linux/bpf.h>` — BPF helper function ID constants.
//!
//! BPF helper functions are kernel-provided functions callable
//! from eBPF programs. Each has a unique ID used in the BPF
//! instruction stream.

// ---------------------------------------------------------------------------
// Core BPF helpers (widely available)
// ---------------------------------------------------------------------------

/// Unspecified helper (reserved).
pub const BPF_FUNC_UNSPEC: u32 = 0;
/// Look up a map element.
pub const BPF_FUNC_MAP_LOOKUP_ELEM: u32 = 1;
/// Update a map element.
pub const BPF_FUNC_MAP_UPDATE_ELEM: u32 = 2;
/// Delete a map element.
pub const BPF_FUNC_MAP_DELETE_ELEM: u32 = 3;
/// Generate a pseudo-random number.
pub const BPF_FUNC_GET_PRANDOM_U32: u32 = 7;
/// Get current time in nanoseconds.
pub const BPF_FUNC_KTIME_GET_NS: u32 = 5;
/// Print formatted debug output.
pub const BPF_FUNC_TRACE_PRINTK: u32 = 6;
/// Get SMP processor ID.
pub const BPF_FUNC_GET_SMP_PROCESSOR_ID: u32 = 8;
/// Get current PID/TGID.
pub const BPF_FUNC_GET_CURRENT_PID_TGID: u32 = 14;
/// Get current UID/GID.
pub const BPF_FUNC_GET_CURRENT_UID_GID: u32 = 15;
/// Get current comm (task name).
pub const BPF_FUNC_GET_CURRENT_COMM: u32 = 16;

// ---------------------------------------------------------------------------
// Networking helpers
// ---------------------------------------------------------------------------

/// Adjust SKB headroom.
pub const BPF_FUNC_SKB_CHANGE_HEAD: u32 = 43;
/// Clone and redirect SKB.
pub const BPF_FUNC_CLONE_REDIRECT: u32 = 13;
/// Redirect packet.
pub const BPF_FUNC_REDIRECT: u32 = 23;
/// Redirect to map entry.
pub const BPF_FUNC_REDIRECT_MAP: u32 = 51;
/// SKB load bytes.
pub const BPF_FUNC_SKB_LOAD_BYTES: u32 = 26;
/// SKB store bytes.
pub const BPF_FUNC_SKB_STORE_BYTES: u32 = 9;
/// Compute incremental checksum.
pub const BPF_FUNC_L3_CSUM_REPLACE: u32 = 10;
/// Compute L4 checksum replacement.
pub const BPF_FUNC_L4_CSUM_REPLACE: u32 = 11;
/// Compute checksum diff.
pub const BPF_FUNC_CSUM_DIFF: u32 = 28;

// ---------------------------------------------------------------------------
// Ring buffer helpers
// ---------------------------------------------------------------------------

/// Reserve space in ring buffer.
pub const BPF_FUNC_RINGBUF_RESERVE: u32 = 131;
/// Submit reserved ring buffer entry.
pub const BPF_FUNC_RINGBUF_SUBMIT: u32 = 132;
/// Discard reserved ring buffer entry.
pub const BPF_FUNC_RINGBUF_DISCARD: u32 = 133;
/// Output to ring buffer.
pub const BPF_FUNC_RINGBUF_OUTPUT: u32 = 130;

// ---------------------------------------------------------------------------
// Tracing helpers
// ---------------------------------------------------------------------------

/// Read user memory.
pub const BPF_FUNC_PROBE_READ_USER: u32 = 112;
/// Read kernel memory.
pub const BPF_FUNC_PROBE_READ_KERNEL: u32 = 113;
/// Read user string.
pub const BPF_FUNC_PROBE_READ_USER_STR: u32 = 114;
/// Read kernel string.
pub const BPF_FUNC_PROBE_READ_KERNEL_STR: u32 = 115;
/// Get current task pointer.
pub const BPF_FUNC_GET_CURRENT_TASK: u32 = 35;
/// Get stack trace.
pub const BPF_FUNC_GET_STACKID: u32 = 27;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_helpers_distinct() {
        let helpers = [
            BPF_FUNC_UNSPEC, BPF_FUNC_MAP_LOOKUP_ELEM,
            BPF_FUNC_MAP_UPDATE_ELEM, BPF_FUNC_MAP_DELETE_ELEM,
            BPF_FUNC_GET_PRANDOM_U32, BPF_FUNC_KTIME_GET_NS,
            BPF_FUNC_TRACE_PRINTK, BPF_FUNC_GET_SMP_PROCESSOR_ID,
            BPF_FUNC_GET_CURRENT_PID_TGID,
            BPF_FUNC_GET_CURRENT_UID_GID,
            BPF_FUNC_GET_CURRENT_COMM,
        ];
        for i in 0..helpers.len() {
            for j in (i + 1)..helpers.len() {
                assert_ne!(helpers[i], helpers[j]);
            }
        }
    }

    #[test]
    fn test_ringbuf_helpers_distinct() {
        let helpers = [
            BPF_FUNC_RINGBUF_RESERVE, BPF_FUNC_RINGBUF_SUBMIT,
            BPF_FUNC_RINGBUF_DISCARD, BPF_FUNC_RINGBUF_OUTPUT,
        ];
        for i in 0..helpers.len() {
            for j in (i + 1)..helpers.len() {
                assert_ne!(helpers[i], helpers[j]);
            }
        }
    }

    #[test]
    fn test_probe_read_helpers_distinct() {
        let helpers = [
            BPF_FUNC_PROBE_READ_USER, BPF_FUNC_PROBE_READ_KERNEL,
            BPF_FUNC_PROBE_READ_USER_STR, BPF_FUNC_PROBE_READ_KERNEL_STR,
        ];
        for i in 0..helpers.len() {
            for j in (i + 1)..helpers.len() {
                assert_ne!(helpers[i], helpers[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(BPF_FUNC_UNSPEC, 0);
    }

    #[test]
    fn test_map_lookup() {
        assert_eq!(BPF_FUNC_MAP_LOOKUP_ELEM, 1);
    }
}
