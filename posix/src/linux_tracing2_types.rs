//! `<linux/trace_events.h>` — Additional tracing constants.
//!
//! Supplementary tracing constants covering trace event types,
//! ring buffer flags, and trace filter operations.

// ---------------------------------------------------------------------------
// Trace event types
// ---------------------------------------------------------------------------

/// Function entry.
pub const TRACE_FN_ENTRY: u32 = 0;
/// Function return.
pub const TRACE_FN_RETURN: u32 = 1;
/// Context switch.
pub const TRACE_CTX: u32 = 2;
/// Wakeup event.
pub const TRACE_WAKE: u32 = 3;
/// Stack trace.
pub const TRACE_STACK: u32 = 4;
/// Print (printk-style).
pub const TRACE_PRINT: u32 = 5;
/// Bprint (binary print).
pub const TRACE_BPRINT: u32 = 6;
/// MMIO read/write.
pub const TRACE_MMIO_RW: u32 = 7;
/// MMIO map.
pub const TRACE_MMIO_MAP: u32 = 8;
/// Branch trace.
pub const TRACE_BRANCH: u32 = 9;
/// Graph entry.
pub const TRACE_GRAPH_ENT: u32 = 10;
/// Graph return.
pub const TRACE_GRAPH_RET: u32 = 11;
/// User stack.
pub const TRACE_USER_STACK: u32 = 12;
/// BLK IO trace.
pub const TRACE_BLK: u32 = 13;
/// Bputs.
pub const TRACE_BPUTS: u32 = 14;
/// Hwlat trace.
pub const TRACE_HWLAT: u32 = 15;
/// Raw data.
pub const TRACE_RAW_DATA: u32 = 16;
/// Osnoise trace.
pub const TRACE_OSNOISE: u32 = 17;
/// Timerlat trace.
pub const TRACE_TIMERLAT: u32 = 18;

// ---------------------------------------------------------------------------
// Trace ring buffer flags
// ---------------------------------------------------------------------------

/// Event is padding.
pub const RINGBUF_TYPE_PADDING: u32 = 29;
/// Time extend.
pub const RINGBUF_TYPE_TIME_EXTEND: u32 = 30;
/// Time stamp.
pub const RINGBUF_TYPE_TIME_STAMP: u32 = 31;
/// Data type (0-28).
pub const RINGBUF_TYPE_DATA_TYPE_LEN_MAX: u32 = 28;

// ---------------------------------------------------------------------------
// Trace filter operations
// ---------------------------------------------------------------------------

/// Filter: equal.
pub const FILTER_OP_EQ: u32 = 0;
/// Filter: not equal.
pub const FILTER_OP_NE: u32 = 1;
/// Filter: less than.
pub const FILTER_OP_LT: u32 = 2;
/// Filter: less or equal.
pub const FILTER_OP_LE: u32 = 3;
/// Filter: greater than.
pub const FILTER_OP_GT: u32 = 4;
/// Filter: greater or equal.
pub const FILTER_OP_GE: u32 = 5;
/// Filter: bitwise AND.
pub const FILTER_OP_BAND: u32 = 6;
/// Filter: glob match.
pub const FILTER_OP_GLOB: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let types = [
            TRACE_FN_ENTRY,
            TRACE_FN_RETURN,
            TRACE_CTX,
            TRACE_WAKE,
            TRACE_STACK,
            TRACE_PRINT,
            TRACE_BPRINT,
            TRACE_MMIO_RW,
            TRACE_MMIO_MAP,
            TRACE_BRANCH,
            TRACE_GRAPH_ENT,
            TRACE_GRAPH_RET,
            TRACE_USER_STACK,
            TRACE_BLK,
            TRACE_BPUTS,
            TRACE_HWLAT,
            TRACE_RAW_DATA,
            TRACE_OSNOISE,
            TRACE_TIMERLAT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ringbuf_types_distinct() {
        let types = [
            RINGBUF_TYPE_PADDING,
            RINGBUF_TYPE_TIME_EXTEND,
            RINGBUF_TYPE_TIME_STAMP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_filter_ops_distinct() {
        let ops = [
            FILTER_OP_EQ,
            FILTER_OP_NE,
            FILTER_OP_LT,
            FILTER_OP_LE,
            FILTER_OP_GT,
            FILTER_OP_GE,
            FILTER_OP_BAND,
            FILTER_OP_GLOB,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_data_type_len_max() {
        assert!(RINGBUF_TYPE_DATA_TYPE_LEN_MAX < RINGBUF_TYPE_PADDING);
    }
}
