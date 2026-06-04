//! `<linux/bug.h>` — Kernel BUG/WARN infrastructure constants (user view).
//!
//! These macros expand to `ud2` (or an architecture equivalent) plus
//! metadata in a `.bug_table` section. Userspace tooling (and kernel
//! debuggers) decode the bug-table entry to recover file/line and the
//! kind of failure.

// ---------------------------------------------------------------------------
// Bug-table entry flags (`bug_entry.flags`)
// ---------------------------------------------------------------------------

/// `BUGFLAG_WARNING` — entry is a WARN, not a fatal BUG.
pub const BUGFLAG_WARNING: u16 = 1 << 0;

/// `BUGFLAG_ONCE` — fires at most once per location.
pub const BUGFLAG_ONCE: u16 = 1 << 1;

/// `BUGFLAG_DONE` — already-fired sentinel (set by the handler).
pub const BUGFLAG_DONE: u16 = 1 << 2;

/// `BUGFLAG_NO_CUT_HERE` — suppress the "------------[ cut here ]" banner.
pub const BUGFLAG_NO_CUT_HERE: u16 = 1 << 3;

// ---------------------------------------------------------------------------
// Combined query masks
// ---------------------------------------------------------------------------

/// Mask of mutable runtime flags (set by the handler after firing).
pub const BUGFLAG_RUNTIME_MASK: u16 = BUGFLAG_DONE;

/// Mask of build-time flags (set by the WARN_ONCE/BUG_ON macros).
pub const BUGFLAG_BUILDTIME_MASK: u16 =
    BUGFLAG_WARNING | BUGFLAG_ONCE | BUGFLAG_NO_CUT_HERE;

// ---------------------------------------------------------------------------
// Bug-table entry field offsets (relative bug_addr, file, line, flags)
// ---------------------------------------------------------------------------

pub const BUG_ENTRY_OFF_BUG_ADDR: usize = 0;
pub const BUG_ENTRY_OFF_FILE: usize = 4;
pub const BUG_ENTRY_OFF_LINE: usize = 8;
pub const BUG_ENTRY_OFF_FLAGS: usize = 10;
pub const BUG_ENTRY_SIZE: usize = 12;

// ---------------------------------------------------------------------------
// Default kernel oops behavior
// ---------------------------------------------------------------------------

/// Default value of `panic_on_oops` sysctl (0 = oops, do not panic).
pub const PANIC_ON_OOPS_DEFAULT: u32 = 0;

/// Default `oops_limit` — after this many oopses the kernel panics.
pub const OOPS_LIMIT_DEFAULT: u32 = 10_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bug_flags_distinct_single_bits() {
        let f = [
            BUGFLAG_WARNING,
            BUGFLAG_ONCE,
            BUGFLAG_DONE,
            BUGFLAG_NO_CUT_HERE,
        ];
        for &v in &f {
            assert!(v.is_power_of_two());
        }
        for (i, &a) in f.iter().enumerate() {
            for &b in &f[i + 1..] {
                assert_eq!(a & b, 0);
            }
        }
    }

    #[test]
    fn test_masks_partition_flag_space() {
        // Runtime and build-time masks together cover all defined bits…
        let all = BUGFLAG_RUNTIME_MASK | BUGFLAG_BUILDTIME_MASK;
        assert_eq!(
            all,
            BUGFLAG_WARNING | BUGFLAG_ONCE | BUGFLAG_DONE | BUGFLAG_NO_CUT_HERE
        );
        // …and they do not overlap.
        assert_eq!(BUGFLAG_RUNTIME_MASK & BUGFLAG_BUILDTIME_MASK, 0);
    }

    #[test]
    fn test_bug_entry_layout_packed() {
        // bug_addr: 4-byte relative pointer at offset 0.
        assert_eq!(BUG_ENTRY_OFF_BUG_ADDR, 0);
        // file: 4-byte relative pointer at offset 4.
        assert_eq!(BUG_ENTRY_OFF_FILE, 4);
        // line: 2-byte u16 at offset 8.
        assert_eq!(BUG_ENTRY_OFF_LINE, 8);
        // flags: 2-byte u16 at offset 10.
        assert_eq!(BUG_ENTRY_OFF_FLAGS, 10);
        // Total size: 12 bytes.
        assert_eq!(BUG_ENTRY_SIZE, 12);
        // Consecutive 4-byte fields then two 2-byte fields.
        assert_eq!(BUG_ENTRY_OFF_FILE - BUG_ENTRY_OFF_BUG_ADDR, 4);
        assert_eq!(BUG_ENTRY_OFF_LINE - BUG_ENTRY_OFF_FILE, 4);
        assert_eq!(BUG_ENTRY_OFF_FLAGS - BUG_ENTRY_OFF_LINE, 2);
    }

    #[test]
    fn test_oops_defaults() {
        // Default: do not panic on oops.
        assert_eq!(PANIC_ON_OOPS_DEFAULT, 0);
        // 10k oopses is the default ceiling before panicking.
        assert_eq!(OOPS_LIMIT_DEFAULT, 10_000);
        // Comfortably above any plausible legitimate count.
        assert!(OOPS_LIMIT_DEFAULT > 1_000);
    }
}
