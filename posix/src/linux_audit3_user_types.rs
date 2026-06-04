//! `<linux/audit.h>` continuation — audit filter, action, and field
//! operator constants.
//!
//! `auditctl(8)` rules consist of filter (LIST), action (NEVER/ALWAYS),
//! and per-field comparisons. The numeric encodings live in the
//! kernel uapi and are stable.

// ---------------------------------------------------------------------------
// Filter (rule list)
// ---------------------------------------------------------------------------

pub const AUDIT_FILTER_USER: u32 = 0x00;
pub const AUDIT_FILTER_TASK: u32 = 0x01;
pub const AUDIT_FILTER_ENTRY: u32 = 0x02;
pub const AUDIT_FILTER_WATCH: u32 = 0x03;
pub const AUDIT_FILTER_EXIT: u32 = 0x04;
pub const AUDIT_FILTER_EXCLUDE: u32 = 0x05;
pub const AUDIT_FILTER_FS: u32 = 0x06;
pub const AUDIT_FILTER_URING_EXIT: u32 = 0x07;

pub const AUDIT_NR_FILTERS: u32 = 8;

/// Filter index is stored in the low 3 bits of `rule.flags`.
pub const AUDIT_FILTER_PREPEND: u32 = 0x10;

// ---------------------------------------------------------------------------
// Action (what the rule does once it matches)
// ---------------------------------------------------------------------------

pub const AUDIT_NEVER: u32 = 0;
pub const AUDIT_POSSIBLE: u32 = 1;
pub const AUDIT_ALWAYS: u32 = 2;

// ---------------------------------------------------------------------------
// Field operators — high nibble of the field tag
// ---------------------------------------------------------------------------

pub const AUDIT_BIT_MASK: u32 = 0x0800_0000;
pub const AUDIT_LESS_THAN: u32 = 0x1000_0000;
pub const AUDIT_GREATER_THAN: u32 = 0x2000_0000;
pub const AUDIT_NOT_EQUAL: u32 = 0x3000_0000;
pub const AUDIT_EQUAL: u32 = 0x4000_0000;
pub const AUDIT_BIT_TEST: u32 = AUDIT_BIT_MASK | AUDIT_EQUAL;
pub const AUDIT_LESS_THAN_OR_EQUAL: u32 = AUDIT_LESS_THAN | AUDIT_EQUAL;
pub const AUDIT_GREATER_THAN_OR_EQUAL: u32 = AUDIT_GREATER_THAN | AUDIT_EQUAL;
pub const AUDIT_OPERATORS: u32 = AUDIT_EQUAL | AUDIT_NOT_EQUAL | AUDIT_BIT_MASK;

// ---------------------------------------------------------------------------
// Status mask (audit_status.mask)
// ---------------------------------------------------------------------------

pub const AUDIT_STATUS_ENABLED: u32 = 0x0001;
pub const AUDIT_STATUS_FAILURE: u32 = 0x0002;
pub const AUDIT_STATUS_PID: u32 = 0x0004;
pub const AUDIT_STATUS_RATE_LIMIT: u32 = 0x0008;
pub const AUDIT_STATUS_BACKLOG_LIMIT: u32 = 0x0010;
pub const AUDIT_STATUS_BACKLOG_WAIT_TIME: u32 = 0x0020;
pub const AUDIT_STATUS_LOST: u32 = 0x0040;

// ---------------------------------------------------------------------------
// Failure-mode action (kernel response to backlog overflow)
// ---------------------------------------------------------------------------

pub const AUDIT_FAIL_SILENT: u32 = 0;
pub const AUDIT_FAIL_PRINTK: u32 = 1;
pub const AUDIT_FAIL_PANIC: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_indices_dense_0_to_7() {
        let f = [
            AUDIT_FILTER_USER,
            AUDIT_FILTER_TASK,
            AUDIT_FILTER_ENTRY,
            AUDIT_FILTER_WATCH,
            AUDIT_FILTER_EXIT,
            AUDIT_FILTER_EXCLUDE,
            AUDIT_FILTER_FS,
            AUDIT_FILTER_URING_EXIT,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(AUDIT_NR_FILTERS, f.len() as u32);
        // All fit in the low 3 bits.
        for &v in &f {
            assert!(v < 0x08);
        }
        // PREPEND lives in bit 4 so it never collides with the filter index.
        assert_eq!(AUDIT_FILTER_PREPEND & 0x07, 0);
    }

    #[test]
    fn test_action_codes_dense_0_to_2() {
        assert_eq!(AUDIT_NEVER, 0);
        assert_eq!(AUDIT_POSSIBLE, 1);
        assert_eq!(AUDIT_ALWAYS, 2);
    }

    #[test]
    fn test_field_operators_distinct_nibbles() {
        let ops = [
            AUDIT_BIT_MASK,
            AUDIT_LESS_THAN,
            AUDIT_GREATER_THAN,
            AUDIT_NOT_EQUAL,
            AUDIT_EQUAL,
        ];
        for &v in &ops {
            assert_eq!(v & 0x00FF_FFFF, 0);
        }
        for (i, &a) in ops.iter().enumerate() {
            for &b in &ops[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // Composite ops are bitwise unions of the primitives.
        assert_eq!(AUDIT_BIT_TEST, AUDIT_BIT_MASK | AUDIT_EQUAL);
        assert_eq!(
            AUDIT_LESS_THAN_OR_EQUAL,
            AUDIT_LESS_THAN | AUDIT_EQUAL
        );
        assert_eq!(
            AUDIT_GREATER_THAN_OR_EQUAL,
            AUDIT_GREATER_THAN | AUDIT_EQUAL
        );
        // OPERATORS mask covers EQ, NEQ, BITMASK.
        assert_eq!(
            AUDIT_OPERATORS,
            AUDIT_EQUAL | AUDIT_NOT_EQUAL | AUDIT_BIT_MASK
        );
    }

    #[test]
    fn test_status_mask_bits_are_single_bit() {
        let s = [
            AUDIT_STATUS_ENABLED,
            AUDIT_STATUS_FAILURE,
            AUDIT_STATUS_PID,
            AUDIT_STATUS_RATE_LIMIT,
            AUDIT_STATUS_BACKLOG_LIMIT,
            AUDIT_STATUS_BACKLOG_WAIT_TIME,
            AUDIT_STATUS_LOST,
        ];
        let mut or = 0;
        for &v in &s {
            assert!(v.is_power_of_two());
            or |= v;
        }
        // 7 bits, contiguous low-end mask 0x7F.
        assert_eq!(or, 0x7F);
    }

    #[test]
    fn test_failure_modes_dense_0_to_2() {
        assert_eq!(AUDIT_FAIL_SILENT, 0);
        assert_eq!(AUDIT_FAIL_PRINTK, 1);
        assert_eq!(AUDIT_FAIL_PANIC, 2);
    }
}
