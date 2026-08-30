//! `<linux/uprobes.h>` — User-space probes (uprobes) constants.
//!
//! Uprobes allow dynamic instrumentation of userspace programs at
//! arbitrary instruction addresses. Like kprobes for the kernel,
//! uprobes replace a userspace instruction with a breakpoint (int3
//! on x86), trap into the kernel, execute a handler, then single-step
//! the original instruction. Used by perf, bpftrace, and SystemTap
//! for application-level tracing without recompilation. Uretprobes
//! trace function returns.

// ---------------------------------------------------------------------------
// Uprobe types
// ---------------------------------------------------------------------------

/// Standard uprobe (breakpoint at instruction).
pub const UPROBE_TYPE_STANDARD: u32 = 0;
/// Uretprobe (trace function return).
pub const UPROBE_TYPE_RETURN: u32 = 1;

// ---------------------------------------------------------------------------
// Uprobe states
// ---------------------------------------------------------------------------

/// Probe registered (in uprobe tree).
pub const UPROBE_STATE_REGISTERED: u32 = 0;
/// Probe is active (inserted in at least one process).
pub const UPROBE_STATE_ACTIVE: u32 = 1;
/// Probe is being deleted.
pub const UPROBE_STATE_DELETING: u32 = 2;

// ---------------------------------------------------------------------------
// Uprobe flags (for registration)
// ---------------------------------------------------------------------------

/// Probe fires only on function entry (filter calls).
pub const UPROBE_FLAG_ENTRY: u32 = 1 << 0;
/// Probe fires on function return.
pub const UPROBE_FLAG_RETURN: u32 = 1 << 1;
/// Probe is a "multi" probe (attach to multiple locations).
pub const UPROBE_FLAG_MULTI: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Uprobe handler return values
// ---------------------------------------------------------------------------

/// Continue execution normally.
pub const UPROBE_HANDLER_CONTINUE: u32 = 0;
/// Remove probe after this hit.
pub const UPROBE_HANDLER_REMOVE: u32 = 1;

// ---------------------------------------------------------------------------
// perf_event uprobe attributes
// ---------------------------------------------------------------------------

/// perf uprobe type (function entry).
pub const PERF_UPROBE_TYPE_ENTRY: u32 = 0;
/// perf uretprobe type (function return).
pub const PERF_UPROBE_TYPE_RETURN: u32 = 1;
/// Reference counter offset for SDT semaphores.
pub const UPROBE_REF_CTR_OFFSET_SHIFT: u32 = 32;

// ---------------------------------------------------------------------------
// Uprobe event types (in tracing subsystem)
// ---------------------------------------------------------------------------

/// Uprobe hit event.
pub const UPROBE_EVENT_HIT: u32 = 0;
/// Uretprobe return event.
pub const UPROBE_EVENT_RETURN: u32 = 1;
/// Uprobe exception (handler faulted).
pub const UPROBE_EVENT_EXCEPTION: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        assert_ne!(UPROBE_TYPE_STANDARD, UPROBE_TYPE_RETURN);
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            UPROBE_STATE_REGISTERED,
            UPROBE_STATE_ACTIVE,
            UPROBE_STATE_DELETING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [UPROBE_FLAG_ENTRY, UPROBE_FLAG_RETURN, UPROBE_FLAG_MULTI];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_handler_returns_distinct() {
        assert_ne!(UPROBE_HANDLER_CONTINUE, UPROBE_HANDLER_REMOVE);
    }

    #[test]
    fn test_perf_types_distinct() {
        assert_ne!(PERF_UPROBE_TYPE_ENTRY, PERF_UPROBE_TYPE_RETURN);
    }

    #[test]
    fn test_event_types_distinct() {
        let events = [
            UPROBE_EVENT_HIT,
            UPROBE_EVENT_RETURN,
            UPROBE_EVENT_EXCEPTION,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
