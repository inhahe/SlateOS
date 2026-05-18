//! `<linux/uprobes.h>` — userspace probe (uprobe) constants.
//!
//! Uprobes instrument userspace programs at arbitrary addresses.
//! The kernel inserts a breakpoint at the target instruction in the
//! process's text; when hit, a registered handler runs in kernel
//! context with access to the process registers. Used by BPF, perf,
//! and SystemTap for userspace tracing without recompilation.

// ---------------------------------------------------------------------------
// Uprobe states / flags
// ---------------------------------------------------------------------------

/// Uprobe is registered.
pub const UPROBE_HANDLER_CALLED: u32 = 1 << 0;
/// Uprobe has a return probe (uretprobe).
pub const UPROBE_HANDLER_RET: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Uprobe consumer flags
// ---------------------------------------------------------------------------

/// Consumer wants to single-step over the probed instruction.
pub const UPROBE_SKIP_SSTEP: u32 = 1 << 0;
/// Consumer wants to be called on function return.
pub const UPROBE_RET_HANDLER: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Architecture-specific constants (x86_64)
// ---------------------------------------------------------------------------

/// Breakpoint instruction byte (INT3).
pub const UPROBE_SWBP_INSN: u8 = 0xCC;
/// Breakpoint instruction size (bytes).
pub const UPROBE_SWBP_INSN_SIZE: u32 = 1;
/// Maximum instruction size that can be probed (x86_64).
pub const UPROBE_XOL_SLOT_BYTES: u32 = 128;

// ---------------------------------------------------------------------------
// Uprobe filter types
// ---------------------------------------------------------------------------

/// Apply probe to all tasks.
pub const UPROBE_FILTER_ALL: u32 = 0;
/// Apply probe to specific task.
pub const UPROBE_FILTER_TASK: u32 = 1;
/// Apply probe to specific memory mapping.
pub const UPROBE_FILTER_MMAP: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_flags_no_overlap() {
        assert!(UPROBE_HANDLER_CALLED.is_power_of_two());
        assert!(UPROBE_HANDLER_RET.is_power_of_two());
        assert_eq!(UPROBE_HANDLER_CALLED & UPROBE_HANDLER_RET, 0);
    }

    #[test]
    fn test_consumer_flags_no_overlap() {
        assert!(UPROBE_SKIP_SSTEP.is_power_of_two());
        assert!(UPROBE_RET_HANDLER.is_power_of_two());
        assert_eq!(UPROBE_SKIP_SSTEP & UPROBE_RET_HANDLER, 0);
    }

    #[test]
    fn test_breakpoint_instruction() {
        assert_eq!(UPROBE_SWBP_INSN, 0xCC);
        assert_eq!(UPROBE_SWBP_INSN_SIZE, 1);
    }

    #[test]
    fn test_filter_types_distinct() {
        let filters = [
            UPROBE_FILTER_ALL, UPROBE_FILTER_TASK,
            UPROBE_FILTER_MMAP,
        ];
        for i in 0..filters.len() {
            for j in (i + 1)..filters.len() {
                assert_ne!(filters[i], filters[j]);
            }
        }
    }

    #[test]
    fn test_xol_slot_size() {
        assert!(UPROBE_XOL_SLOT_BYTES > 0);
    }
}
