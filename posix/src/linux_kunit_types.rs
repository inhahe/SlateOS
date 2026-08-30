//! `<kunit/test.h>` (selected user-visible bits) — KUnit kernel-test
//! framework constants.
//!
//! KUnit is the in-kernel test framework used since 5.5. Userspace
//! `kunit.py` invokes a kernel test build, reads the TAP-like
//! `KTAP` output, and matches the level/status strings below.

// ---------------------------------------------------------------------------
// Test status (mirrors enum kunit_status / KTAP "ok"/"not ok"/"skip")
// ---------------------------------------------------------------------------

/// Test passed.
pub const KUNIT_SUCCESS: u32 = 0;
/// Test failed.
pub const KUNIT_FAILURE: u32 = 1;
/// Test was skipped.
pub const KUNIT_SKIPPED: u32 = 2;

// ---------------------------------------------------------------------------
// Speed annotations (kunit_speed)
// ---------------------------------------------------------------------------

/// Default speed (no annotation supplied).
pub const KUNIT_SPEED_UNSET: u32 = 0;
/// Normal-speed test (default category).
pub const KUNIT_SPEED_NORMAL: u32 = 1;
/// Slow test (only run when --slow_tests=true).
pub const KUNIT_SPEED_SLOW: u32 = 2;
/// Very-slow test (manual opt-in).
pub const KUNIT_SPEED_VERY_SLOW: u32 = 3;

// ---------------------------------------------------------------------------
// Maximums (chosen by the framework; userspace tooling enforces them)
// ---------------------------------------------------------------------------

/// Maximum length of a suite or test name in KTAP output.
pub const KUNIT_NAME_MAX: u32 = 256;
/// Maximum number of attributes ever attached to a KUnit object.
pub const KUNIT_MAX_ATTR: u32 = 4;
/// Indent step (spaces) for KTAP nested diagnostics.
pub const KUNIT_INDENT: u32 = 4;

// ---------------------------------------------------------------------------
// KTAP version emitted by kunit_tool / parsed by userspace
// ---------------------------------------------------------------------------

/// KTAP protocol major version supported.
pub const KUNIT_KTAP_VERSION_MAJOR: u32 = 1;

// ---------------------------------------------------------------------------
// Well-known KTAP directive strings
// ---------------------------------------------------------------------------

/// "SKIP" directive (test was skipped at runtime).
pub const KUNIT_KTAP_SKIP: &str = "SKIP";
/// "TODO" directive (expected-failure annotation).
pub const KUNIT_KTAP_TODO: &str = "TODO";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_codes_distinct_and_success_zero() {
        let s = [KUNIT_SUCCESS, KUNIT_FAILURE, KUNIT_SKIPPED];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
        // Success must be 0 so a zeroed report is a passing test.
        assert_eq!(KUNIT_SUCCESS, 0);
    }

    #[test]
    fn test_speed_categories_distinct_and_ordered() {
        // UNSET < NORMAL < SLOW < VERY_SLOW so comparing speeds at the
        // tool level uses normal integer ordering.
        assert!(KUNIT_SPEED_UNSET < KUNIT_SPEED_NORMAL);
        assert!(KUNIT_SPEED_NORMAL < KUNIT_SPEED_SLOW);
        assert!(KUNIT_SPEED_SLOW < KUNIT_SPEED_VERY_SLOW);
    }

    #[test]
    fn test_limits_sane() {
        assert!(KUNIT_NAME_MAX.is_power_of_two());
        assert!(KUNIT_MAX_ATTR >= 1);
        assert_eq!(KUNIT_INDENT, 4);
        assert_eq!(KUNIT_KTAP_VERSION_MAJOR, 1);
    }

    #[test]
    fn test_directive_strings_uppercase_ascii() {
        for d in [KUNIT_KTAP_SKIP, KUNIT_KTAP_TODO] {
            for b in d.bytes() {
                assert!(b.is_ascii_uppercase());
            }
        }
        assert_ne!(KUNIT_KTAP_SKIP, KUNIT_KTAP_TODO);
    }
}
