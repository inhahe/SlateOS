//! `<linux/prctl.h>` — PR_SET_MM sub-operations for memory map manipulation.
//!
//! `prctl(PR_SET_MM, ...)` allows a process to modify its own
//! memory map parameters (e.g., brk, stack boundaries, auxv)
//! typically used by process checkpoint/restore (CRIU).

// ---------------------------------------------------------------------------
// PR_SET_MM sub-operations
// ---------------------------------------------------------------------------

/// Set the start of the code segment.
pub const PR_SET_MM_START_CODE: u32 = 1;
/// Set the end of the code segment.
pub const PR_SET_MM_END_CODE: u32 = 2;
/// Set the start of the data segment.
pub const PR_SET_MM_START_DATA: u32 = 3;
/// Set the end of the data segment.
pub const PR_SET_MM_END_DATA: u32 = 4;
/// Set the start of the stack.
pub const PR_SET_MM_START_STACK: u32 = 5;
/// Set the start of the brk (heap).
pub const PR_SET_MM_START_BRK: u32 = 6;
/// Set the current brk value.
pub const PR_SET_MM_BRK: u32 = 7;
/// Set the start of the argument strings.
pub const PR_SET_MM_ARG_START: u32 = 8;
/// Set the end of the argument strings.
pub const PR_SET_MM_ARG_END: u32 = 9;
/// Set the start of the environment strings.
pub const PR_SET_MM_ENV_START: u32 = 10;
/// Set the end of the environment strings.
pub const PR_SET_MM_ENV_END: u32 = 11;
/// Set the auxiliary vector.
pub const PR_SET_MM_AUXV: u32 = 12;
/// Set the exe file link (/proc/pid/exe).
pub const PR_SET_MM_EXE_FILE: u32 = 13;
/// Set all mm map fields at once via struct.
pub const PR_SET_MM_MAP: u32 = 14;
/// Same as MAP but with size parameter.
pub const PR_SET_MM_MAP_SIZE: u32 = 15;

// ---------------------------------------------------------------------------
// PR_SET_MM_MAP struct field count
// ---------------------------------------------------------------------------

/// Number of fields in prctl_mm_map struct.
pub const PRCTL_MM_MAP_FIELD_COUNT: u32 = 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_distinct() {
        let ops = [
            PR_SET_MM_START_CODE,
            PR_SET_MM_END_CODE,
            PR_SET_MM_START_DATA,
            PR_SET_MM_END_DATA,
            PR_SET_MM_START_STACK,
            PR_SET_MM_START_BRK,
            PR_SET_MM_BRK,
            PR_SET_MM_ARG_START,
            PR_SET_MM_ARG_END,
            PR_SET_MM_ENV_START,
            PR_SET_MM_ENV_END,
            PR_SET_MM_AUXV,
            PR_SET_MM_EXE_FILE,
            PR_SET_MM_MAP,
            PR_SET_MM_MAP_SIZE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_sequential_numbering() {
        assert_eq!(PR_SET_MM_START_CODE, 1);
        assert_eq!(PR_SET_MM_MAP_SIZE, 15);
    }

    #[test]
    fn test_field_count() {
        assert_eq!(PRCTL_MM_MAP_FIELD_COUNT, 15);
    }

    #[test]
    fn test_code_segment_order() {
        assert!(PR_SET_MM_START_CODE < PR_SET_MM_END_CODE);
    }

    #[test]
    fn test_data_segment_order() {
        assert!(PR_SET_MM_START_DATA < PR_SET_MM_END_DATA);
    }
}
