//! `<linux/vdso.h>` — Additional vDSO constants.
//!
//! Supplementary vDSO constants covering clock source types,
//! vDSO data page layout, and vDSO function indices.

// ---------------------------------------------------------------------------
// vDSO clock source types (VDSO_CLOCKMODE_*)
// ---------------------------------------------------------------------------

/// No vDSO clock (fall back to syscall).
pub const VDSO_CLOCKMODE_NONE: u32 = 0;
/// TSC-based clock source.
pub const VDSO_CLOCKMODE_TSC: u32 = 1;
/// HPET-based clock source.
pub const VDSO_CLOCKMODE_HPET: u32 = 2;
/// pvclock (KVM paravirtual clock).
pub const VDSO_CLOCKMODE_PVCLOCK: u32 = 3;
/// Hyper-V reference TSC.
pub const VDSO_CLOCKMODE_HVCLOCK: u32 = 4;

// ---------------------------------------------------------------------------
// vDSO page types
// ---------------------------------------------------------------------------

/// vDSO data page.
pub const VDSO_PAGE_DATA: u32 = 0;
/// vDSO time data page.
pub const VDSO_PAGE_TIMENS: u32 = 1;
/// vDSO text (code) pages start.
pub const VDSO_PAGE_TEXT: u32 = 2;

// ---------------------------------------------------------------------------
// vDSO function indices (for symbol lookup)
// ---------------------------------------------------------------------------

/// clock_gettime.
pub const VDSO_FN_CLOCK_GETTIME: u32 = 0;
/// gettimeofday.
pub const VDSO_FN_GETTIMEOFDAY: u32 = 1;
/// clock_getres.
pub const VDSO_FN_CLOCK_GETRES: u32 = 2;
/// time.
pub const VDSO_FN_TIME: u32 = 3;
/// getcpu.
pub const VDSO_FN_GETCPU: u32 = 4;
/// clock_gettime64.
pub const VDSO_FN_CLOCK_GETTIME64: u32 = 5;

// ---------------------------------------------------------------------------
// vDSO versioning
// ---------------------------------------------------------------------------

/// vDSO version string: "LINUX_2.6".
pub const VDSO_VERSION_MAJOR: u32 = 2;
/// vDSO version minor.
pub const VDSO_VERSION_MINOR: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_modes_distinct() {
        let modes = [
            VDSO_CLOCKMODE_NONE,
            VDSO_CLOCKMODE_TSC,
            VDSO_CLOCKMODE_HPET,
            VDSO_CLOCKMODE_PVCLOCK,
            VDSO_CLOCKMODE_HVCLOCK,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_page_types_distinct() {
        let pages = [VDSO_PAGE_DATA, VDSO_PAGE_TIMENS, VDSO_PAGE_TEXT];
        for i in 0..pages.len() {
            for j in (i + 1)..pages.len() {
                assert_ne!(pages[i], pages[j]);
            }
        }
    }

    #[test]
    fn test_fn_indices_distinct() {
        let fns = [
            VDSO_FN_CLOCK_GETTIME,
            VDSO_FN_GETTIMEOFDAY,
            VDSO_FN_CLOCK_GETRES,
            VDSO_FN_TIME,
            VDSO_FN_GETCPU,
            VDSO_FN_CLOCK_GETTIME64,
        ];
        for i in 0..fns.len() {
            for j in (i + 1)..fns.len() {
                assert_ne!(fns[i], fns[j]);
            }
        }
    }

    #[test]
    fn test_none_is_zero() {
        assert_eq!(VDSO_CLOCKMODE_NONE, 0);
    }

    #[test]
    fn test_version() {
        assert_eq!(VDSO_VERSION_MAJOR, 2);
        assert_eq!(VDSO_VERSION_MINOR, 6);
    }
}
