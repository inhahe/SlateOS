//! `kernel/trace/bpf_trace.c` — BPF ↔ tracing-subsystem surface.
//!
//! Kprobes, tracepoints, and uprobes feed BPF programs through a
//! small set of helpers (`bpf_trace_printk`, `bpf_get_stack`,
//! `bpf_seq_*`) and one well-known userspace file
//! (`/sys/kernel/debug/tracing/trace_pipe`).

// ---------------------------------------------------------------------------
// trace_pipe / debugfs paths
// ---------------------------------------------------------------------------

/// Where `bpf_trace_printk()` output appears.
pub const TRACE_PIPE_PATH: &str = "/sys/kernel/debug/tracing/trace_pipe";

/// New (debugfs-less) location used when tracefs is mounted directly.
pub const TRACEFS_TRACE_PIPE_PATH: &str = "/sys/kernel/tracing/trace_pipe";

/// debugfs mount point.
pub const DEBUGFS_MOUNT: &str = "/sys/kernel/debug";

/// tracefs mount point.
pub const TRACEFS_MOUNT: &str = "/sys/kernel/tracing";

// ---------------------------------------------------------------------------
// trace_printk format buffer
// ---------------------------------------------------------------------------

/// Maximum format string size accepted by `bpf_trace_printk()`.
pub const BPF_TRACE_PRINTK_FMT_MAX: usize = 1024;

/// Maximum variadic args (helper ABI limit).
pub const BPF_TRACE_PRINTK_MAX_ARGS: usize = 3;

/// Maximum bytes a single trace_printk line may produce.
pub const BPF_TRACE_PRINTK_MAX_LEN: usize = 2048;

// ---------------------------------------------------------------------------
// Stack-trace capture limits
// ---------------------------------------------------------------------------

/// Maximum frames captured by `bpf_get_stack()`.
pub const BPF_MAX_STACK_DEPTH: u32 = 127;

/// Flag: get user-space stack instead of kernel.
pub const BPF_F_USER_STACK: u32 = 1 << 8;

/// Flag: stack frames are user build-IDs instead of addresses.
pub const BPF_F_USER_BUILD_ID: u32 = 1 << 11;

/// Flag: reuse stackid if same stack already seen.
pub const BPF_F_FAST_STACK_CMP: u32 = 1 << 9;

/// Flag: reuse stackid on hash collision.
pub const BPF_F_REUSE_STACKID: u32 = 1 << 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_pipe_paths_distinct() {
        assert_ne!(TRACE_PIPE_PATH, TRACEFS_TRACE_PIPE_PATH);
        // Both end in /trace_pipe.
        assert!(TRACE_PIPE_PATH.ends_with("/trace_pipe"));
        assert!(TRACEFS_TRACE_PIPE_PATH.ends_with("/trace_pipe"));
        // The debugfs path is the older / longer one.
        assert!(TRACE_PIPE_PATH.contains("/debug/"));
        assert!(!TRACEFS_TRACE_PIPE_PATH.contains("/debug/"));
    }

    #[test]
    fn test_mount_points_align_with_pipe_paths() {
        assert!(TRACE_PIPE_PATH.starts_with(DEBUGFS_MOUNT));
        assert!(TRACEFS_TRACE_PIPE_PATH.starts_with(TRACEFS_MOUNT));
    }

    #[test]
    fn test_trace_printk_limits() {
        assert_eq!(BPF_TRACE_PRINTK_FMT_MAX, 1024);
        assert_eq!(BPF_TRACE_PRINTK_MAX_ARGS, 3);
        assert_eq!(BPF_TRACE_PRINTK_MAX_LEN, 2048);
        // Output buffer is 2x the format buffer (escape/expand budget).
        assert_eq!(BPF_TRACE_PRINTK_MAX_LEN / BPF_TRACE_PRINTK_FMT_MAX, 2);
        assert!(BPF_TRACE_PRINTK_FMT_MAX.is_power_of_two());
        assert!(BPF_TRACE_PRINTK_MAX_LEN.is_power_of_two());
    }

    #[test]
    fn test_stack_depth_bound() {
        // 127 = (1 << 7) - 1.
        assert_eq!(BPF_MAX_STACK_DEPTH, 127);
    }

    #[test]
    fn test_stack_flags_distinct_high_bits() {
        let f = [
            BPF_F_USER_STACK,
            BPF_F_FAST_STACK_CMP,
            BPF_F_REUSE_STACKID,
            BPF_F_USER_BUILD_ID,
        ];
        for &v in &f {
            assert!(v.is_power_of_two());
            // All four flags sit in bits 8..11 — above the helper's
            // stack-depth field in the lower byte.
            assert!(v >= (1 << 8));
            assert!(v < (1 << 12));
        }
        for (i, &a) in f.iter().enumerate() {
            for &b in &f[i + 1..] {
                assert_eq!(a & b, 0);
            }
        }
    }
}
