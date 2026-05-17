//! `<linux/trace_seq.h>` / tracefs — ftrace/tracefs event constants.
//!
//! tracefs is the filesystem interface to the Linux tracing
//! infrastructure (ftrace). It exposes tracepoints, kprobes, uprobes,
//! function tracing, and event recording through files under
//! /sys/kernel/tracing/. Events are categorized by subsystem and
//! recorded in per-CPU ring buffers.

// ---------------------------------------------------------------------------
// Trace event types (internal ring buffer record types)
// ---------------------------------------------------------------------------

/// Padding record (fill unused space in ring buffer).
pub const RINGBUF_TYPE_PADDING: u32 = 29;
/// Time extend record (when delta exceeds 27 bits).
pub const RINGBUF_TYPE_TIME_EXTEND: u32 = 30;
/// Time stamp (absolute timestamp marker).
pub const RINGBUF_TYPE_TIMESTAMP: u32 = 31;
/// Data record (normal trace event).
pub const RINGBUF_TYPE_DATA: u32 = 0;

// ---------------------------------------------------------------------------
// Trace flags (per-CPU tracing_on control)
// ---------------------------------------------------------------------------

/// Tracing is enabled for this CPU.
pub const TRACE_FLAG_IRQS_OFF: u32 = 1 << 0;
/// IRQs are disabled at trace point.
pub const TRACE_FLAG_NEED_RESCHED: u32 = 1 << 1;
/// Preemption disabled.
pub const TRACE_FLAG_PREEMPT_RESCHED: u32 = 1 << 2;
/// In hardirq context.
pub const TRACE_FLAG_HARDIRQ: u32 = 1 << 3;
/// In softirq context.
pub const TRACE_FLAG_SOFTIRQ: u32 = 1 << 4;
/// NMI context.
pub const TRACE_FLAG_NMI: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Ftrace function tracer ops flags
// ---------------------------------------------------------------------------

/// Enable function tracing.
pub const FTRACE_OPS_FL_ENABLED: u32 = 1 << 0;
/// Dynamic (can be modified at runtime).
pub const FTRACE_OPS_FL_DYNAMIC: u32 = 1 << 1;
/// Save/restore regs for the callback.
pub const FTRACE_OPS_FL_SAVE_REGS: u32 = 1 << 2;
/// Recursion safe.
pub const FTRACE_OPS_FL_RECURSION: u32 = 1 << 3;
/// Stub (placeholder).
pub const FTRACE_OPS_FL_STUB: u32 = 1 << 4;
/// Initialized.
pub const FTRACE_OPS_FL_INITIALIZED: u32 = 1 << 5;
/// Per-CPU tracing.
pub const FTRACE_OPS_FL_IPMODIFY: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Trace buffer sizes
// ---------------------------------------------------------------------------

/// Default per-CPU ring buffer size (1408 KiB = 1.375 MiB).
pub const TRACE_BUF_SIZE_DEFAULT: u32 = 1408 * 1024;
/// Minimum per-CPU ring buffer size.
pub const TRACE_BUF_SIZE_MIN: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ringbuf_types_distinct() {
        let types = [
            RINGBUF_TYPE_DATA, RINGBUF_TYPE_PADDING,
            RINGBUF_TYPE_TIME_EXTEND, RINGBUF_TYPE_TIMESTAMP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_trace_flags_no_overlap() {
        let flags = [
            TRACE_FLAG_IRQS_OFF, TRACE_FLAG_NEED_RESCHED,
            TRACE_FLAG_PREEMPT_RESCHED, TRACE_FLAG_HARDIRQ,
            TRACE_FLAG_SOFTIRQ, TRACE_FLAG_NMI,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ftrace_ops_flags_no_overlap() {
        let flags = [
            FTRACE_OPS_FL_ENABLED, FTRACE_OPS_FL_DYNAMIC,
            FTRACE_OPS_FL_SAVE_REGS, FTRACE_OPS_FL_RECURSION,
            FTRACE_OPS_FL_STUB, FTRACE_OPS_FL_INITIALIZED,
            FTRACE_OPS_FL_IPMODIFY,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_buf_size_ordering() {
        assert!(TRACE_BUF_SIZE_MIN < TRACE_BUF_SIZE_DEFAULT);
    }
}
