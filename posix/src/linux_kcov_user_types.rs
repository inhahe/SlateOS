//! `<linux/kcov.h>` — kernel coverage harness user ABI.
//!
//! `kcov` is the kernel fuzzer's coverage feedback channel. Syzkaller,
//! AFL-on-the-kernel, and every userspace harness reading
//! `/sys/kernel/debug/kcov` for kernel-edge coverage uses the ioctl
//! sequence INIT_TRACE → mmap → ENABLE → run target → DISABLE → read.

// ---------------------------------------------------------------------------
// Device path
// ---------------------------------------------------------------------------

/// Debugfs path opened to obtain a kcov fd.
pub const KCOV_DEV_PATH: &str = "/sys/kernel/debug/kcov";

// ---------------------------------------------------------------------------
// ioctl numbers (raw — magic 'c' = 0x63)
// ---------------------------------------------------------------------------

/// `KCOV_INIT_TRACE` — size argument in `unsigned long`, returns 0/-1.
pub const KCOV_INIT_TRACE: u32 = 0x6308;
/// `KCOV_ENABLE` — start coverage collection for current task.
pub const KCOV_ENABLE: u32 = 0x6364;
/// `KCOV_DISABLE` — stop coverage collection.
pub const KCOV_DISABLE: u32 = 0x6365;
/// `KCOV_REMOTE_ENABLE` — enable remote (per-handle) coverage (Linux 5.5+).
pub const KCOV_REMOTE_ENABLE: u32 = 0x4040_6366;

// ---------------------------------------------------------------------------
// Coverage modes (argument to KCOV_ENABLE)
// ---------------------------------------------------------------------------

/// Trace only basic blocks (8-byte PC entries).
pub const KCOV_TRACE_PC: u32 = 0;
/// Trace comparisons for KCOV-COMP (16-byte entries: type, arg1, arg2, pc).
pub const KCOV_TRACE_CMP: u32 = 1;

// ---------------------------------------------------------------------------
// Comparison trace flags (bit 0 of the type field is "const operand")
// ---------------------------------------------------------------------------

pub const KCOV_CMP_CONST: u64 = 1 << 0;
pub const KCOV_CMP_SIZE_MASK: u64 = 6;
pub const KCOV_CMP_SIZE1: u64 = 0 << 1;
pub const KCOV_CMP_SIZE2: u64 = 1 << 1;
pub const KCOV_CMP_SIZE4: u64 = 2 << 1;
pub const KCOV_CMP_SIZE8: u64 = 3 << 1;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Bytes in a single PC entry (one `unsigned long`).
pub const KCOV_PC_ENTRY_SIZE: usize = 8;
/// Bytes in a single comparison entry (type, arg1, arg2, pc — all 8 bytes).
pub const KCOV_CMP_ENTRY_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_path() {
        // KCOV opens the debugfs node; syzkaller hard-codes this string.
        assert_eq!(KCOV_DEV_PATH, "/sys/kernel/debug/kcov");
    }

    #[test]
    fn test_ioctl_magic_byte_is_c() {
        // 'c' magic = 0x63 in bits 8..15 (or low byte for _IO without args).
        assert_eq!(KCOV_INIT_TRACE >> 8 & 0xFF, 0x63);
        // ENABLE/DISABLE are _IO('c', n) so bits 8..15 = 'c'.
        assert_eq!(KCOV_ENABLE >> 8 & 0xFF, 0x63);
        assert_eq!(KCOV_DISABLE >> 8 & 0xFF, 0x63);
        // REMOTE_ENABLE is _IOW('c', 102, ...) so type byte is also 'c'.
        assert_eq!(KCOV_REMOTE_ENABLE >> 8 & 0xFF, 0x63);
    }

    #[test]
    fn test_trace_modes_distinct() {
        assert_ne!(KCOV_TRACE_PC, KCOV_TRACE_CMP);
        assert_eq!(KCOV_TRACE_PC, 0);
        assert_eq!(KCOV_TRACE_CMP, 1);
    }

    #[test]
    fn test_cmp_flag_layout() {
        // CONST bit is bit 0.
        assert_eq!(KCOV_CMP_CONST, 1);
        // SIZE field occupies bits 1..2.
        assert_eq!(KCOV_CMP_SIZE_MASK, 0b110);
        // The four size encodings cover exactly the mask.
        let sizes = [
            KCOV_CMP_SIZE1,
            KCOV_CMP_SIZE2,
            KCOV_CMP_SIZE4,
            KCOV_CMP_SIZE8,
        ];
        for s in sizes {
            assert_eq!(s & KCOV_CMP_SIZE_MASK, s);
            assert_eq!(s & KCOV_CMP_CONST, 0);
        }
        // Values are monotonic.
        assert!(KCOV_CMP_SIZE1 < KCOV_CMP_SIZE2);
        assert!(KCOV_CMP_SIZE2 < KCOV_CMP_SIZE4);
        assert!(KCOV_CMP_SIZE4 < KCOV_CMP_SIZE8);
    }

    #[test]
    fn test_entry_sizes() {
        // 8 bytes per PC = one unsigned long on 64-bit kernels.
        assert_eq!(KCOV_PC_ENTRY_SIZE, 8);
        // 32 = 4 × 8 (type, arg1, arg2, pc).
        assert_eq!(KCOV_CMP_ENTRY_SIZE, 4 * KCOV_PC_ENTRY_SIZE);
    }
}
