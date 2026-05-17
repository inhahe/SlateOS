//! `<linux/kcov.h>` — Kernel code coverage (KCOV) constants.
//!
//! KCOV enables code coverage collection from userspace-triggered
//! kernel execution paths. It is used by fuzzers (syzkaller, etc.)
//! to guide test generation by tracking which kernel code branches
//! are exercised by specific syscall sequences.

// ---------------------------------------------------------------------------
// KCOV ioctl commands
// ---------------------------------------------------------------------------

/// Initialize KCOV buffer (argument: buffer size in entries).
pub const KCOV_INIT_TRACE: u32 = 0xC008_6401;
/// Enable KCOV tracing for the current thread.
pub const KCOV_ENABLE: u32 = 0x6464;
/// Disable KCOV tracing for the current thread.
pub const KCOV_DISABLE: u32 = 0x6465;

// ---------------------------------------------------------------------------
// KCOV tracing modes (argument to KCOV_ENABLE)
// ---------------------------------------------------------------------------

/// Trace PC (program counter) coverage.
pub const KCOV_TRACE_PC: u32 = 0;
/// Trace comparison operands (for guided fuzzing).
pub const KCOV_TRACE_CMP: u32 = 1;

// ---------------------------------------------------------------------------
// KCOV comparison size encoding
// ---------------------------------------------------------------------------

/// Comparison size: 1 byte.
pub const KCOV_CMP_SIZE_1: u32 = 0;
/// Comparison size: 2 bytes.
pub const KCOV_CMP_SIZE_2: u32 = 1;
/// Comparison size: 4 bytes.
pub const KCOV_CMP_SIZE_4: u32 = 2;
/// Comparison size: 8 bytes.
pub const KCOV_CMP_SIZE_8: u32 = 3;
/// Mask for extracting comparison size bits.
pub const KCOV_CMP_SIZE_MASK: u32 = 3;
/// Bit indicating comparison is a constant.
pub const KCOV_CMP_CONST: u32 = 4;

// ---------------------------------------------------------------------------
// KCOV buffer entry layout
// ---------------------------------------------------------------------------

/// Maximum number of entries in a KCOV coverage buffer.
pub const KCOV_MAX_ENTRIES: u32 = 1 << 24;
/// Entry size in bytes (u64 per entry).
pub const KCOV_ENTRY_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [KCOV_INIT_TRACE, KCOV_ENABLE, KCOV_DISABLE];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_trace_modes_distinct() {
        assert_ne!(KCOV_TRACE_PC, KCOV_TRACE_CMP);
    }

    #[test]
    fn test_cmp_sizes_sequential() {
        assert_eq!(KCOV_CMP_SIZE_1, 0);
        assert_eq!(KCOV_CMP_SIZE_2, 1);
        assert_eq!(KCOV_CMP_SIZE_4, 2);
        assert_eq!(KCOV_CMP_SIZE_8, 3);
    }

    #[test]
    fn test_cmp_size_mask() {
        assert_eq!(KCOV_CMP_SIZE_1 & KCOV_CMP_SIZE_MASK, KCOV_CMP_SIZE_1);
        assert_eq!(KCOV_CMP_SIZE_8 & KCOV_CMP_SIZE_MASK, KCOV_CMP_SIZE_8);
        assert_eq!(KCOV_CMP_CONST & KCOV_CMP_SIZE_MASK, 0);
    }

    #[test]
    fn test_max_entries() {
        assert!(KCOV_MAX_ENTRIES > 0);
        assert!(KCOV_MAX_ENTRIES.is_power_of_two());
    }

    #[test]
    fn test_entry_size() {
        assert_eq!(KCOV_ENTRY_SIZE, 8);
    }
}
