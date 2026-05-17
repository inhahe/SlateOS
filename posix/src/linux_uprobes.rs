//! `<linux/uprobes.h>` — User-space probes (uprobes) constants.
//!
//! Uprobes allow dynamic instrumentation of user-space code. A probe
//! is placed at a virtual address in a binary; when any process executes
//! that address, the kernel traps, runs the probe handler (BPF program
//! or ftrace callback), then resumes execution. Used by bpftrace, perf,
//! SystemTap, and eBPF tracing tools for application-level observability
//! without recompilation.

// ---------------------------------------------------------------------------
// Uprobe types (registered via tracefs/perf)
// ---------------------------------------------------------------------------

/// Standard uprobe: fires on entry to the probed instruction.
pub const UPROBE_TYPE_NORMAL: u32 = 0;
/// Return probe: fires when the probed function returns.
pub const UPROBE_TYPE_RETURN: u32 = 1;

// ---------------------------------------------------------------------------
// Uprobe register flags
// ---------------------------------------------------------------------------

/// Register as a return probe.
pub const UPROBE_HANDLER_RET: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Uprobe events (perf_event_attr.type for uprobe)
// ---------------------------------------------------------------------------

/// Uprobe event type identifier (used with perf).
pub const PERF_TYPE_MAX: u32 = 6;

// ---------------------------------------------------------------------------
// Uprobe filter flags (for BPF attachment)
// ---------------------------------------------------------------------------

/// Attach to all PIDs (system-wide).
pub const BPF_F_UPROBE_MULTI_ALL: u32 = 1 << 0;
/// Return probe variant for BPF multi-uprobe.
pub const BPF_F_UPROBE_MULTI_RETURN: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Uprobe reference tracking
// ---------------------------------------------------------------------------

/// Maximum number of probes on a single instruction.
pub const MAX_UPROBES_PER_INSN: u32 = 16;
/// Maximum uprobe path length.
pub const UPROBE_PATH_MAX: u32 = 4096;

// ---------------------------------------------------------------------------
// Uprobe status codes
// ---------------------------------------------------------------------------

/// Probe is active (consumer attached).
pub const UPROBE_STATUS_ACTIVE: u32 = 0;
/// Probe is inactive (no consumers).
pub const UPROBE_STATUS_INACTIVE: u32 = 1;
/// Probe failed to install (text not writable, etc).
pub const UPROBE_STATUS_ERROR: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uprobe_types_distinct() {
        assert_ne!(UPROBE_TYPE_NORMAL, UPROBE_TYPE_RETURN);
    }

    #[test]
    fn test_bpf_flags_no_overlap() {
        assert_eq!(BPF_F_UPROBE_MULTI_ALL & BPF_F_UPROBE_MULTI_RETURN, 0);
        assert!(BPF_F_UPROBE_MULTI_ALL.is_power_of_two());
        assert!(BPF_F_UPROBE_MULTI_RETURN.is_power_of_two());
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [UPROBE_STATUS_ACTIVE, UPROBE_STATUS_INACTIVE, UPROBE_STATUS_ERROR];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_limits_positive() {
        assert!(MAX_UPROBES_PER_INSN > 0);
        assert!(UPROBE_PATH_MAX > 0);
    }

    #[test]
    fn test_normal_is_zero() {
        assert_eq!(UPROBE_TYPE_NORMAL, 0);
    }
}
