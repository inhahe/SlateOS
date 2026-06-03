//! `<linux/perf_event.h>` — perf mmap ring buffer page constants.
//!
//! perf events deliver sample data to userspace via a memory-mapped
//! ring buffer. The first page contains metadata (header), followed
//! by data pages. These constants define header field values and
//! record types found in the ring buffer.

// ---------------------------------------------------------------------------
// perf_event_mmap_page constants
// ---------------------------------------------------------------------------

/// Version of the mmap page structure.
pub const PERF_MMAP_PAGE_VERSION: u32 = 0;
/// Compatibility version for aux area.
pub const PERF_MMAP_PAGE_AUX_VERSION: u32 = 0;

// ---------------------------------------------------------------------------
// Ring buffer record types (perf_event_header.type)
// ---------------------------------------------------------------------------

/// Mmap event (executable mapped).
pub const PERF_RECORD_MMAP: u32 = 1;
/// Lost event records (overflow).
pub const PERF_RECORD_LOST: u32 = 2;
/// Comm (command name change / exec).
pub const PERF_RECORD_COMM: u32 = 3;
/// Exit event (process/thread exit).
pub const PERF_RECORD_EXIT: u32 = 4;
/// Throttle event (sampling throttled).
pub const PERF_RECORD_THROTTLE: u32 = 5;
/// Unthrottle event.
pub const PERF_RECORD_UNTHROTTLE: u32 = 6;
/// Fork event (new process/thread).
pub const PERF_RECORD_FORK: u32 = 7;
/// Read event (counter value).
pub const PERF_RECORD_READ: u32 = 8;
/// Sample event (the main data record).
pub const PERF_RECORD_SAMPLE: u32 = 9;
/// Mmap2 event (extended mmap info).
pub const PERF_RECORD_MMAP2: u32 = 10;
/// AUX record (hardware trace data).
pub const PERF_RECORD_AUX: u32 = 11;
/// ITRACE start.
pub const PERF_RECORD_ITRACE_START: u32 = 12;
/// Lost samples (sampling overflow).
pub const PERF_RECORD_LOST_SAMPLES: u32 = 13;
/// Context switch.
pub const PERF_RECORD_SWITCH: u32 = 14;
/// Context switch (with CPU info).
pub const PERF_RECORD_SWITCH_CPU_WIDE: u32 = 15;
/// Namespace info.
pub const PERF_RECORD_NAMESPACES: u32 = 16;
/// Ksymbol (kernel symbol event).
pub const PERF_RECORD_KSYMBOL: u32 = 17;
/// BPF event.
pub const PERF_RECORD_BPF_EVENT: u32 = 18;
/// Cgroup event.
pub const PERF_RECORD_CGROUP: u32 = 19;
/// Text poke (kernel code modification).
pub const PERF_RECORD_TEXT_POKE: u32 = 20;
/// AUX output HW ID.
pub const PERF_RECORD_AUX_OUTPUT_HW_ID: u32 = 21;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_types_distinct() {
        let types = [
            PERF_RECORD_MMAP,
            PERF_RECORD_LOST,
            PERF_RECORD_COMM,
            PERF_RECORD_EXIT,
            PERF_RECORD_THROTTLE,
            PERF_RECORD_UNTHROTTLE,
            PERF_RECORD_FORK,
            PERF_RECORD_READ,
            PERF_RECORD_SAMPLE,
            PERF_RECORD_MMAP2,
            PERF_RECORD_AUX,
            PERF_RECORD_ITRACE_START,
            PERF_RECORD_LOST_SAMPLES,
            PERF_RECORD_SWITCH,
            PERF_RECORD_SWITCH_CPU_WIDE,
            PERF_RECORD_NAMESPACES,
            PERF_RECORD_KSYMBOL,
            PERF_RECORD_BPF_EVENT,
            PERF_RECORD_CGROUP,
            PERF_RECORD_TEXT_POKE,
            PERF_RECORD_AUX_OUTPUT_HW_ID,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_mmap_is_one() {
        assert_eq!(PERF_RECORD_MMAP, 1);
    }

    #[test]
    fn test_sample_is_nine() {
        assert_eq!(PERF_RECORD_SAMPLE, 9);
    }

    #[test]
    fn test_mmap_page_version() {
        assert_eq!(PERF_MMAP_PAGE_VERSION, 0);
    }
}
