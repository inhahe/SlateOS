//! `<linux/kprobes.h>` — Kernel probes (kprobes) constants.
//!
//! Kprobes allow dynamic instrumentation of almost any kernel function
//! or instruction without recompilation. A kprobe replaces the target
//! instruction with a breakpoint (int3 on x86), executes a handler,
//! then single-steps the original instruction. Kretprobes trace
//! function returns. Kprobes are the foundation for many tracing tools
//! (SystemTap, bpftrace, perf probe). They enable live debugging and
//! performance analysis on production systems.

// ---------------------------------------------------------------------------
// Kprobe types
// ---------------------------------------------------------------------------

/// Standard kprobe (breakpoint at instruction).
pub const KPROBE_TYPE_STANDARD: u32 = 0;
/// Kretprobe (trace function return).
pub const KPROBE_TYPE_RETURN: u32 = 1;
/// Jprobe (deprecated, was for function entry args).
pub const KPROBE_TYPE_JPROBE: u32 = 2;

// ---------------------------------------------------------------------------
// Kprobe states
// ---------------------------------------------------------------------------

/// Probe is registered but not armed.
pub const KPROBE_STATE_REGISTERED: u32 = 0;
/// Probe is armed (breakpoint inserted).
pub const KPROBE_STATE_ARMED: u32 = 1;
/// Probe is disabled (temporarily inactive).
pub const KPROBE_STATE_DISABLED: u32 = 2;
/// Probe is being removed.
pub const KPROBE_STATE_REMOVING: u32 = 3;
/// Probe hit (currently executing handler).
pub const KPROBE_STATE_HIT: u32 = 4;

// ---------------------------------------------------------------------------
// Kprobe flags
// ---------------------------------------------------------------------------

/// Probe is on a ftrace-able function (optimized path).
pub const KPROBE_FLAG_FTRACE: u32 = 1 << 0;
/// Probe has been optimized (uses jump, not int3).
pub const KPROBE_FLAG_OPTIMIZED: u32 = 1 << 1;
/// Probe is disabled.
pub const KPROBE_FLAG_DISABLED: u32 = 1 << 2;
/// Probe is gone (removed, being cleaned up).
pub const KPROBE_FLAG_GONE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Kretprobe special values
// ---------------------------------------------------------------------------

/// Max number of concurrent kretprobe instances.
pub const KRETPROBE_MAX_INSTANCES: u32 = 1024;
/// Instance missed (too many concurrent calls).
pub const KRETPROBE_MISSED: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Kprobe blacklist reasons
// ---------------------------------------------------------------------------

/// Function is not traceable (inline, notrace).
pub const KPROBE_BLACKLIST_NOTRACE: u32 = 0;
/// Function is critical (exception handlers, etc.).
pub const KPROBE_BLACKLIST_CRITICAL: u32 = 1;
/// Function is in kprobe infrastructure itself.
pub const KPROBE_BLACKLIST_SELF: u32 = 2;
/// Function uses non-standard calling convention.
pub const KPROBE_BLACKLIST_NONSTANDARD: u32 = 3;

// ---------------------------------------------------------------------------
// perf_event kprobe types (for perf_event_open)
// ---------------------------------------------------------------------------

/// perf kprobe type (attach to function entry).
pub const PERF_KPROBE_TYPE_ENTRY: u32 = 0;
/// perf kretprobe type (attach to function return).
pub const PERF_KPROBE_TYPE_RETURN: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [KPROBE_TYPE_STANDARD, KPROBE_TYPE_RETURN, KPROBE_TYPE_JPROBE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            KPROBE_STATE_REGISTERED,
            KPROBE_STATE_ARMED,
            KPROBE_STATE_DISABLED,
            KPROBE_STATE_REMOVING,
            KPROBE_STATE_HIT,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            KPROBE_FLAG_FTRACE,
            KPROBE_FLAG_OPTIMIZED,
            KPROBE_FLAG_DISABLED,
            KPROBE_FLAG_GONE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_blacklist_reasons_distinct() {
        let reasons = [
            KPROBE_BLACKLIST_NOTRACE,
            KPROBE_BLACKLIST_CRITICAL,
            KPROBE_BLACKLIST_SELF,
            KPROBE_BLACKLIST_NONSTANDARD,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_perf_types_distinct() {
        assert_ne!(PERF_KPROBE_TYPE_ENTRY, PERF_KPROBE_TYPE_RETURN);
    }

    #[test]
    fn test_kretprobe_limits() {
        assert!(KRETPROBE_MAX_INSTANCES > 0);
        assert!(KRETPROBE_MAX_INSTANCES.is_power_of_two());
    }
}
