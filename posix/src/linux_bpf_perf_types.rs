//! `<linux/bpf_perf_event.h>` — BPF perf event integration constants.
//!
//! BPF programs can attach to perf events to process performance
//! monitoring data in-kernel. This avoids the overhead of copying
//! every sample to userspace. BPF perf programs can filter events,
//! aggregate data into maps, and send summaries to userspace via
//! ring buffers. This integration enables tools like bpftrace and
//! BCC to efficiently analyze hardware performance counters, software
//! events, and tracepoints.

// ---------------------------------------------------------------------------
// BPF perf event program types (contexts)
// ---------------------------------------------------------------------------

/// Perf event program (generic perf attachment).
pub const BPF_PERF_EVENT: u32 = 0;
/// Tracepoint program.
pub const BPF_PERF_TRACEPOINT: u32 = 1;
/// Kprobe program.
pub const BPF_PERF_KPROBE: u32 = 2;
/// Uprobe program.
pub const BPF_PERF_UPROBE: u32 = 3;

// ---------------------------------------------------------------------------
// BPF perf output flags (bpf_perf_event_output)
// ---------------------------------------------------------------------------

/// Use current CPU's perf buffer.
pub const BPF_F_CURRENT_CPU: u64 = 0xFFFF_FFFF;
/// Index is specified (use specific CPU buffer).
pub const BPF_F_INDEX_MASK: u64 = 0xFFFF_FFFF;
/// Compute CTC delta for Intel PT.
pub const BPF_F_CTC_DELTA: u64 = 1 << 32;

// ---------------------------------------------------------------------------
// BPF perf event read flags
// ---------------------------------------------------------------------------

/// Read hardware PMC value.
pub const BPF_PERF_READ_VALUE: u32 = 0;
/// Read running time.
pub const BPF_PERF_READ_RUNNING: u32 = 1;
/// Read enabled time.
pub const BPF_PERF_READ_ENABLED: u32 = 2;

// ---------------------------------------------------------------------------
// BPF perf ring buffer flags
// ---------------------------------------------------------------------------

/// Ring buffer: output notification.
pub const BPF_RB_NOTIFY: u32 = 0;
/// Ring buffer: no notification (batch mode).
pub const BPF_RB_NO_WAKEUP: u32 = 1 << 0;
/// Ring buffer: force wakeup (even if below watermark).
pub const BPF_RB_FORCE_WAKEUP: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// BPF perf helper return values
// ---------------------------------------------------------------------------

/// Helper succeeded.
pub const BPF_PERF_OK: i32 = 0;
/// Helper failed: buffer full.
pub const BPF_PERF_ERR_FULL: i32 = -1;
/// Helper failed: invalid argument.
pub const BPF_PERF_ERR_INVAL: i32 = -2;
/// Helper failed: no memory.
pub const BPF_PERF_ERR_NOMEM: i32 = -3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_program_types_distinct() {
        let types = [
            BPF_PERF_EVENT,
            BPF_PERF_TRACEPOINT,
            BPF_PERF_KPROBE,
            BPF_PERF_UPROBE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_output_flags() {
        assert_eq!(BPF_F_CURRENT_CPU, 0xFFFF_FFFF);
        assert_eq!(BPF_F_INDEX_MASK, 0xFFFF_FFFF);
    }

    #[test]
    fn test_read_flags_distinct() {
        let flags = [
            BPF_PERF_READ_VALUE,
            BPF_PERF_READ_RUNNING,
            BPF_PERF_READ_ENABLED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_rb_flags_no_overlap() {
        let flags = [BPF_RB_NO_WAKEUP, BPF_RB_FORCE_WAKEUP];
        assert_eq!(flags[0] & flags[1], 0);
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_error_codes_distinct() {
        let codes = [
            BPF_PERF_OK,
            BPF_PERF_ERR_FULL,
            BPF_PERF_ERR_INVAL,
            BPF_PERF_ERR_NOMEM,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }
}
