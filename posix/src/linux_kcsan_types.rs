//! `<linux/kcsan.h>` — Kernel Concurrency Sanitizer constants.
//!
//! Constants for KCSAN — the kernel concurrency sanitizer that
//! detects data races through compiler-instrumented memory accesses.
//! Userspace test harnesses use these to configure access-type masks
//! when injecting accesses.

// ---------------------------------------------------------------------------
// Access-type flag bits (kcsan_check_access type argument)
// ---------------------------------------------------------------------------

/// Access is a write.
pub const KCSAN_ACCESS_WRITE: u32 = 0x1;
/// Access is intended to be atomic (LOCK-prefixed / xchg / etc.).
pub const KCSAN_ACCESS_ATOMIC: u32 = 0x2;
/// Access is a compound (read-modify-write) operation.
pub const KCSAN_ACCESS_COMPOUND: u32 = 0x4;
/// Access is "assert-only" — kcsan should report but not treat as a hit.
pub const KCSAN_ACCESS_ASSERT: u32 = 0x8;
/// Access is implicit — should not be reported alone.
pub const KCSAN_ACCESS_SCOPED: u32 = 0x10;

// ---------------------------------------------------------------------------
// Report types (struct kcsan_report.type)
// ---------------------------------------------------------------------------

/// Standard race — two concurrent accesses without synchronisation.
pub const KCSAN_REPORT_RACE_UNKNOWN: u32 = 0;
/// Single-access race report with stack and value mismatch.
pub const KCSAN_REPORT_RACE_SIGNAL: u32 = 1;
/// kcsan_check_access detected an unexpected value change.
pub const KCSAN_REPORT_RACE_VALUE_CHANGED: u32 = 2;
/// Assertion-only finding.
pub const KCSAN_REPORT_RACE_ASSERT: u32 = 3;

// ---------------------------------------------------------------------------
// Threshold defaults (matching the kernel default config)
// ---------------------------------------------------------------------------

/// Default skip-watch ratio (every N accesses gets a watchpoint).
pub const KCSAN_DEFAULT_SKIP_WATCH: u32 = 4000;
/// Default udelay for delaying a watchpoint observation (interrupt context).
pub const KCSAN_DEFAULT_UDELAY_INTERRUPT: u32 = 20;
/// Default udelay for delaying a watchpoint observation (task context).
pub const KCSAN_DEFAULT_UDELAY_TASK: u32 = 80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_flags_distinct_bits() {
        let flags = [
            KCSAN_ACCESS_WRITE,
            KCSAN_ACCESS_ATOMIC,
            KCSAN_ACCESS_COMPOUND,
            KCSAN_ACCESS_ASSERT,
            KCSAN_ACCESS_SCOPED,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two(), "{f:#x} not single-bit");
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_report_types_distinct() {
        let types = [
            KCSAN_REPORT_RACE_UNKNOWN,
            KCSAN_REPORT_RACE_SIGNAL,
            KCSAN_REPORT_RACE_VALUE_CHANGED,
            KCSAN_REPORT_RACE_ASSERT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_defaults_in_reasonable_range() {
        assert!(KCSAN_DEFAULT_SKIP_WATCH >= 100);
        assert!(KCSAN_DEFAULT_UDELAY_INTERRUPT >= 1);
        assert!(KCSAN_DEFAULT_UDELAY_TASK >= KCSAN_DEFAULT_UDELAY_INTERRUPT);
    }
}
