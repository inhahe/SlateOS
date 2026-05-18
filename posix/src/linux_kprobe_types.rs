//! `<linux/kprobes.h>` — kprobe dynamic instrumentation constants.
//!
//! Kprobes insert breakpoints at arbitrary kernel addresses at
//! runtime. When execution hits the probe, a registered handler runs
//! with access to the register state. This enables dynamic tracing
//! without modifying source code. Kretprobes instrument function
//! returns. BPF programs commonly attach via kprobes.

// ---------------------------------------------------------------------------
// Kprobe states
// ---------------------------------------------------------------------------

/// Probe is registered but not yet armed.
pub const KPROBE_FLAG_GONE: u32 = 1 << 0;
/// Probe is disabled (not firing).
pub const KPROBE_FLAG_DISABLED: u32 = 1 << 1;
/// Probe has been optimized (jump-patched instead of breakpoint).
pub const KPROBE_FLAG_OPTIMIZED: u32 = 1 << 2;
/// Probe is ftrace-based (uses ftrace infrastructure).
pub const KPROBE_FLAG_FTRACE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Kretprobe max instances
// ---------------------------------------------------------------------------

/// Default max active instances for kretprobe.
pub const KRETPROBE_MAX_INSTANCES_DEFAULT: u32 = 16;
/// Maximum configurable active instances.
pub const KRETPROBE_MAX_INSTANCES_MAX: u32 = 65536;

// ---------------------------------------------------------------------------
// Kprobe hit actions (return values from handler)
// ---------------------------------------------------------------------------

/// Continue execution normally.
pub const KPROBE_HIT_ACTIVE: u32 = 0;
/// Probe is being single-stepped.
pub const KPROBE_HIT_SS: u32 = 1;
/// Re-enter: probe hit while handling another probe.
pub const KPROBE_REENTER: u32 = 2;
/// Probe hit in NMI context.
pub const KPROBE_HIT_SSDONE: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            KPROBE_FLAG_GONE, KPROBE_FLAG_DISABLED,
            KPROBE_FLAG_OPTIMIZED, KPROBE_FLAG_FTRACE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_hit_actions_distinct() {
        let actions = [
            KPROBE_HIT_ACTIVE, KPROBE_HIT_SS,
            KPROBE_REENTER, KPROBE_HIT_SSDONE,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_instance_limits() {
        assert!(KRETPROBE_MAX_INSTANCES_DEFAULT > 0);
        assert!(KRETPROBE_MAX_INSTANCES_DEFAULT <= KRETPROBE_MAX_INSTANCES_MAX);
    }
}
