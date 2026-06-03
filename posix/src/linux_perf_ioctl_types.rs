//! `<linux/perf_event.h>` — perf_event ioctl command constants.
//!
//! Once a perf event file descriptor is open, these ioctl commands
//! control it: enable/disable counting, reset counters, refresh
//! overflow counts, set output redirection, and modify filters.

// ---------------------------------------------------------------------------
// perf_event ioctl commands
// ---------------------------------------------------------------------------

/// Enable the event counter.
pub const PERF_EVENT_IOC_ENABLE: u32 = 0x2400;
/// Disable the event counter.
pub const PERF_EVENT_IOC_DISABLE: u32 = 0x2401;
/// Refresh the event (re-enable after overflow).
pub const PERF_EVENT_IOC_REFRESH: u32 = 0x2402;
/// Reset the event counter to zero.
pub const PERF_EVENT_IOC_RESET: u32 = 0x2403;
/// Set the sample period.
pub const PERF_EVENT_IOC_PERIOD: u32 = 0x40082404;
/// Redirect output to another event's ring buffer.
pub const PERF_EVENT_IOC_SET_OUTPUT: u32 = 0x2405;
/// Set filter (e.g., BPF program).
pub const PERF_EVENT_IOC_SET_FILTER: u32 = 0x40082406;
/// Query event ID.
pub const PERF_EVENT_IOC_ID: u32 = 0x80082407;
/// Set BPF program on event.
pub const PERF_EVENT_IOC_SET_BPF: u32 = 0x40042408;
/// Pause/resume output.
pub const PERF_EVENT_IOC_PAUSE_OUTPUT: u32 = 0x40042409;
/// Query supported features.
pub const PERF_EVENT_IOC_QUERY_BPF: u32 = 0xC008240A;
/// Modify event attributes.
pub const PERF_EVENT_IOC_MODIFY_ATTRIBUTES: u32 = 0x4008240B;

// ---------------------------------------------------------------------------
// ioctl flags (for ENABLE/DISABLE)
// ---------------------------------------------------------------------------

/// Apply to event group (not just leader).
pub const PERF_IOC_FLAG_GROUP: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            PERF_EVENT_IOC_ENABLE,
            PERF_EVENT_IOC_DISABLE,
            PERF_EVENT_IOC_REFRESH,
            PERF_EVENT_IOC_RESET,
            PERF_EVENT_IOC_PERIOD,
            PERF_EVENT_IOC_SET_OUTPUT,
            PERF_EVENT_IOC_SET_FILTER,
            PERF_EVENT_IOC_ID,
            PERF_EVENT_IOC_SET_BPF,
            PERF_EVENT_IOC_PAUSE_OUTPUT,
            PERF_EVENT_IOC_QUERY_BPF,
            PERF_EVENT_IOC_MODIFY_ATTRIBUTES,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_enable_disable_sequential() {
        assert_eq!(PERF_EVENT_IOC_ENABLE, 0x2400);
        assert_eq!(PERF_EVENT_IOC_DISABLE, 0x2401);
    }

    #[test]
    fn test_group_flag() {
        assert_eq!(PERF_IOC_FLAG_GROUP, 1);
    }
}
