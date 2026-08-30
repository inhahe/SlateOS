//! `<langinfo.h>` — Language information constants for nl_langinfo().
//!
//! `nl_langinfo()` returns locale-specific information strings
//! identified by these constants.  They cover date/time formats,
//! character encoding, and locale conventions.

// ---------------------------------------------------------------------------
// Date/time format items
// ---------------------------------------------------------------------------

/// Date and time representation (equivalent of %c).
pub const ABDAY_1: u32 = 0x20000;
/// Abbreviated weekday names (Sun–Sat).
pub const ABDAY_2: u32 = 0x20001;
/// Monday abbreviated.
pub const ABDAY_3: u32 = 0x20002;
/// Tuesday abbreviated.
pub const ABDAY_4: u32 = 0x20003;
/// Wednesday abbreviated.
pub const ABDAY_5: u32 = 0x20004;
/// Thursday abbreviated.
pub const ABDAY_6: u32 = 0x20005;
/// Friday abbreviated.
pub const ABDAY_7: u32 = 0x20006;

/// Full weekday name: Sunday.
pub const DAY_1: u32 = 0x20007;
/// Full weekday name: Monday.
pub const DAY_2: u32 = 0x20008;
/// Full weekday name: Tuesday.
pub const DAY_3: u32 = 0x20009;
/// Full weekday name: Wednesday.
pub const DAY_4: u32 = 0x2000A;
/// Full weekday name: Thursday.
pub const DAY_5: u32 = 0x2000B;
/// Full weekday name: Friday.
pub const DAY_6: u32 = 0x2000C;
/// Full weekday name: Saturday.
pub const DAY_7: u32 = 0x2000D;

/// Abbreviated month name: January.
pub const ABMON_1: u32 = 0x2000E;
/// Abbreviated month name: February.
pub const ABMON_2: u32 = 0x2000F;
/// Abbreviated month name: March.
pub const ABMON_3: u32 = 0x20010;
/// Abbreviated month name: April.
pub const ABMON_4: u32 = 0x20011;
/// Abbreviated month name: May.
pub const ABMON_5: u32 = 0x20012;
/// Abbreviated month name: June.
pub const ABMON_6: u32 = 0x20013;
/// Abbreviated month name: July.
pub const ABMON_7: u32 = 0x20014;
/// Abbreviated month name: August.
pub const ABMON_8: u32 = 0x20015;
/// Abbreviated month name: September.
pub const ABMON_9: u32 = 0x20016;
/// Abbreviated month name: October.
pub const ABMON_10: u32 = 0x20017;
/// Abbreviated month name: November.
pub const ABMON_11: u32 = 0x20018;
/// Abbreviated month name: December.
pub const ABMON_12: u32 = 0x20019;

/// Full month name: January.
pub const MON_1: u32 = 0x2001A;
/// Full month name: February.
pub const MON_2: u32 = 0x2001B;
/// Full month name: March.
pub const MON_3: u32 = 0x2001C;
/// Full month name: April.
pub const MON_4: u32 = 0x2001D;
/// Full month name: May.
pub const MON_5: u32 = 0x2001E;
/// Full month name: June.
pub const MON_6: u32 = 0x2001F;
/// Full month name: July.
pub const MON_7: u32 = 0x20020;
/// Full month name: August.
pub const MON_8: u32 = 0x20021;
/// Full month name: September.
pub const MON_9: u32 = 0x20022;
/// Full month name: October.
pub const MON_10: u32 = 0x20023;
/// Full month name: November.
pub const MON_11: u32 = 0x20024;
/// Full month name: December.
pub const MON_12: u32 = 0x20025;

// ---------------------------------------------------------------------------
// AM/PM and format items
// ---------------------------------------------------------------------------

/// AM string.
pub const AM_STR: u32 = 0x20026;
/// PM string.
pub const PM_STR: u32 = 0x20027;
/// Date and time format string (%c equivalent).
pub const D_T_FMT: u32 = 0x20028;
/// Date format string (%x equivalent).
pub const D_FMT: u32 = 0x20029;
/// Time format string (%X equivalent).
pub const T_FMT: u32 = 0x2002A;
/// 12-hour time format string (%r equivalent).
pub const T_FMT_AMPM: u32 = 0x2002B;

// ---------------------------------------------------------------------------
// Locale information items
// ---------------------------------------------------------------------------

/// Locale's radix character (decimal point).
pub const RADIXCHAR: u32 = 0x10000;
/// Locale's thousands separator.
pub const THOUSEP: u32 = 0x10001;
/// Yes expression for regex matching.
pub const YESEXPR: u32 = 0x50000;
/// No expression for regex matching.
pub const NOEXPR: u32 = 0x50001;
/// Currency symbol.
pub const CRNCYSTR: u32 = 0x40000;

// ---------------------------------------------------------------------------
// Encoding item
// ---------------------------------------------------------------------------

/// Locale's character encoding name.
pub const CODESET: u32 = 14;

// ---------------------------------------------------------------------------
// Alternative date/time format items (glibc extensions)
// ---------------------------------------------------------------------------

/// Era-based date/time format.
pub const ERA: u32 = 0x2002C;
/// Era-based year in date representation.
pub const ERA_D_FMT: u32 = 0x2002E;
/// Era-based date and time representation.
pub const ERA_D_T_FMT: u32 = 0x20030;
/// Era-based time representation.
pub const ERA_T_FMT: u32 = 0x20031;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abday_sequential() {
        assert_eq!(ABDAY_2, ABDAY_1 + 1);
        assert_eq!(ABDAY_3, ABDAY_1 + 2);
        assert_eq!(ABDAY_7, ABDAY_1 + 6);
    }

    #[test]
    fn test_day_sequential() {
        assert_eq!(DAY_2, DAY_1 + 1);
        assert_eq!(DAY_7, DAY_1 + 6);
    }

    #[test]
    fn test_abmon_sequential() {
        assert_eq!(ABMON_2, ABMON_1 + 1);
        assert_eq!(ABMON_12, ABMON_1 + 11);
    }

    #[test]
    fn test_mon_sequential() {
        assert_eq!(MON_2, MON_1 + 1);
        assert_eq!(MON_12, MON_1 + 11);
    }

    #[test]
    fn test_codeset_value() {
        assert_eq!(CODESET, 14);
    }

    #[test]
    fn test_radixchar_value() {
        assert_eq!(RADIXCHAR, 0x10000);
    }

    #[test]
    fn test_am_pm_distinct() {
        assert_ne!(AM_STR, PM_STR);
    }

    #[test]
    fn test_yesno_distinct() {
        assert_ne!(YESEXPR, NOEXPR);
    }

    #[test]
    fn test_format_items_distinct() {
        let fmts = [D_T_FMT, D_FMT, T_FMT, T_FMT_AMPM];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_era_items_distinct() {
        let eras = [ERA, ERA_D_FMT, ERA_D_T_FMT, ERA_T_FMT];
        for i in 0..eras.len() {
            for j in (i + 1)..eras.len() {
                assert_ne!(eras[i], eras[j]);
            }
        }
    }
}
