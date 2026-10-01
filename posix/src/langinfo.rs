//! POSIX `<langinfo.h>` — language information constants.
//!
//! Implements `nl_langinfo` and `nl_langinfo_l` for querying
//! locale-dependent string values.
//!
//! ## Limitations
//!
//! Only the C/POSIX locale is supported.  All returned strings are
//! the C locale defaults defined by POSIX.

// ---------------------------------------------------------------------------
// Item constants — nl_item values
// ---------------------------------------------------------------------------
//
// An item is its category and its index in it, `(category << 16) | index`,
// with musl's categories -- LC_CTYPE 0, LC_NUMERIC 1, LC_TIME 2, LC_MONETARY
// 4, LC_MESSAGES 5 -- which is also glibc's encoding: the two headers agree on
// every item below (probed 2026-09-27).  So `CODESET` is 14 and `ABDAY_1` is
// 0x2_0000.  Until 2026-09-27 these were numbered 0 to 55 in an order of this
// module's own, and a C caller -- whose `<langinfo.h>` is musl's -- asking for
// `CODESET` (14) was answered with ABDAY_1's "Sun": CPython's locale encoding
// and gnulib's `locale_charset`, in every GNU port, among them.

/// Radix character (decimal point).
pub const RADIXCHAR: i32 = 0x1_0000;
/// Date and time format, for `strftime`.
pub const D_T_FMT: i32 = 0x2_0028;
/// Date format.
pub const D_FMT: i32 = 0x2_0029;
/// Time format.
pub const T_FMT: i32 = 0x2_002a;
/// Time format with AM/PM.
pub const T_FMT_AMPM: i32 = 0x2_002b;
/// AM string.
pub const AM_STR: i32 = 0x2_0026;
/// PM string.
pub const PM_STR: i32 = 0x2_0027;

/// Full weekday names, Sunday first.
pub const DAY_1: i32 = 0x2_0007;
pub const DAY_2: i32 = 0x2_0008;
pub const DAY_3: i32 = 0x2_0009;
pub const DAY_4: i32 = 0x2_000a;
pub const DAY_5: i32 = 0x2_000b;
pub const DAY_6: i32 = 0x2_000c;
pub const DAY_7: i32 = 0x2_000d;

/// Abbreviated weekday names, Sunday first.
pub const ABDAY_1: i32 = 0x2_0000;
pub const ABDAY_2: i32 = 0x2_0001;
pub const ABDAY_3: i32 = 0x2_0002;
pub const ABDAY_4: i32 = 0x2_0003;
pub const ABDAY_5: i32 = 0x2_0004;
pub const ABDAY_6: i32 = 0x2_0005;
pub const ABDAY_7: i32 = 0x2_0006;

/// Full month names.
pub const MON_1: i32 = 0x2_001a;
pub const MON_2: i32 = 0x2_001b;
pub const MON_3: i32 = 0x2_001c;
pub const MON_4: i32 = 0x2_001d;
pub const MON_5: i32 = 0x2_001e;
pub const MON_6: i32 = 0x2_001f;
pub const MON_7: i32 = 0x2_0020;
pub const MON_8: i32 = 0x2_0021;
pub const MON_9: i32 = 0x2_0022;
pub const MON_10: i32 = 0x2_0023;
pub const MON_11: i32 = 0x2_0024;
pub const MON_12: i32 = 0x2_0025;

/// Abbreviated month names.
pub const ABMON_1: i32 = 0x2_000e;
pub const ABMON_2: i32 = 0x2_000f;
pub const ABMON_3: i32 = 0x2_0010;
pub const ABMON_4: i32 = 0x2_0011;
pub const ABMON_5: i32 = 0x2_0012;
pub const ABMON_6: i32 = 0x2_0013;
pub const ABMON_7: i32 = 0x2_0014;
pub const ABMON_8: i32 = 0x2_0015;
pub const ABMON_9: i32 = 0x2_0016;
pub const ABMON_10: i32 = 0x2_0017;
pub const ABMON_11: i32 = 0x2_0018;
pub const ABMON_12: i32 = 0x2_0019;

/// Era description (empty in C locale).
pub const ERA: i32 = 0x2_002c;
/// Era date format (empty in C locale).
pub const ERA_D_FMT: i32 = 0x2_002e;
/// Era date-time format (empty in C locale).
pub const ERA_D_T_FMT: i32 = 0x2_0030;
/// Era time format (empty in C locale).
pub const ERA_T_FMT: i32 = 0x2_0031;
/// Alternative digits (empty in C locale).
pub const ALT_DIGITS: i32 = 0x2_002f;

/// glibc's other name for [`RADIXCHAR`]; musl's header has only that one.
pub const DECIMAL_POINT: i32 = RADIXCHAR;
/// Thousands separator.
pub const THOUSEP: i32 = 0x1_0001;
/// Regular expression for a "yes" answer.
pub const YESEXPR: i32 = 0x5_0000;
/// Regular expression for a "no" answer.
pub const NOEXPR: i32 = 0x5_0001;
/// Currency symbol.
pub const CRNCYSTR: i32 = 0x4_000f;

/// Codeset name.
pub const CODESET: i32 = 14;

// ---------------------------------------------------------------------------
// String table — C locale values
// ---------------------------------------------------------------------------

/// C locale decimal point.
static RADIX_STR: &[u8] = b".\0";
/// C locale date-time format: equivalent to `%a %b %e %H:%M:%S %Y`.
static D_T_FMT_STR: &[u8] = b"%a %b %e %H:%M:%S %Y\0";
/// C locale date format: `%m/%d/%y`.
static D_FMT_STR: &[u8] = b"%m/%d/%y\0";
/// C locale time format: `%H:%M:%S`.
static T_FMT_STR: &[u8] = b"%H:%M:%S\0";
/// C locale AM/PM time format: `%I:%M:%S %p`.
static T_FMT_AMPM_STR: &[u8] = b"%I:%M:%S %p\0";
/// AM string.
static AM_STR_VAL: &[u8] = b"AM\0";
/// PM string.
static PM_STR_VAL: &[u8] = b"PM\0";

// Full day names.
static DAY_STRS: [&[u8]; 7] = [
    b"Sunday\0",
    b"Monday\0",
    b"Tuesday\0",
    b"Wednesday\0",
    b"Thursday\0",
    b"Friday\0",
    b"Saturday\0",
];

// Abbreviated day names.
static ABDAY_STRS: [&[u8]; 7] = [
    b"Sun\0", b"Mon\0", b"Tue\0", b"Wed\0", b"Thu\0", b"Fri\0", b"Sat\0",
];

// Full month names.
static MON_STRS: [&[u8]; 12] = [
    b"January\0",
    b"February\0",
    b"March\0",
    b"April\0",
    b"May\0",
    b"June\0",
    b"July\0",
    b"August\0",
    b"September\0",
    b"October\0",
    b"November\0",
    b"December\0",
];

// Abbreviated month names.
static ABMON_STRS: [&[u8]; 12] = [
    b"Jan\0", b"Feb\0", b"Mar\0", b"Apr\0", b"May\0", b"Jun\0", b"Jul\0", b"Aug\0", b"Sep\0",
    b"Oct\0", b"Nov\0", b"Dec\0",
];

/// Empty string for unsupported items.
static EMPTY: &[u8] = b"\0";
/// Yes expression regex.
static YESEXPR_STR: &[u8] = b"^[yY]\0";
/// No expression regex.
static NOEXPR_STR: &[u8] = b"^[nN]\0";
/// Thousands separator (empty in C locale).
static THOUSEP_STR: &[u8] = b"\0";
/// Currency string (empty in C locale).
static CRNCYSTR_STR: &[u8] = b"\0";
/// Codeset in C locale.
static CODESET_STR: &[u8] = b"ANSI_X3.4-1968\0";

// ---------------------------------------------------------------------------
// nl_langinfo
// ---------------------------------------------------------------------------

/// `nl_langinfo` — query language information.
///
/// Returns a pointer to a static null-terminated string for the
/// given `item` constant.  In the C locale, all values are the
/// POSIX-defined defaults.
///
/// Returns a pointer to an empty string for unknown items.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nl_langinfo(item: i32) -> *const u8 {
    langinfo_lookup(item)
}

/// `nl_langinfo_l` — locale-specific language information.
///
/// Stub: ignores the locale parameter and delegates to `nl_langinfo`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nl_langinfo_l(item: i32, _locale: usize) -> *const u8 {
    langinfo_lookup(item)
}

/// Core lookup — maps an `nl_item` to the corresponding C locale string.
fn langinfo_lookup(item: i32) -> *const u8 {
    match item {
        RADIXCHAR => RADIX_STR.as_ptr(),
        D_T_FMT => D_T_FMT_STR.as_ptr(),
        D_FMT => D_FMT_STR.as_ptr(),
        T_FMT => T_FMT_STR.as_ptr(),
        T_FMT_AMPM => T_FMT_AMPM_STR.as_ptr(),
        AM_STR => AM_STR_VAL.as_ptr(),
        PM_STR => PM_STR_VAL.as_ptr(),

        // Full day names.
        DAY_1 => DAY_STRS[0].as_ptr(),
        DAY_2 => DAY_STRS[1].as_ptr(),
        DAY_3 => DAY_STRS[2].as_ptr(),
        DAY_4 => DAY_STRS[3].as_ptr(),
        DAY_5 => DAY_STRS[4].as_ptr(),
        DAY_6 => DAY_STRS[5].as_ptr(),
        DAY_7 => DAY_STRS[6].as_ptr(),

        // Abbreviated day names.
        ABDAY_1 => ABDAY_STRS[0].as_ptr(),
        ABDAY_2 => ABDAY_STRS[1].as_ptr(),
        ABDAY_3 => ABDAY_STRS[2].as_ptr(),
        ABDAY_4 => ABDAY_STRS[3].as_ptr(),
        ABDAY_5 => ABDAY_STRS[4].as_ptr(),
        ABDAY_6 => ABDAY_STRS[5].as_ptr(),
        ABDAY_7 => ABDAY_STRS[6].as_ptr(),

        // Full month names.
        MON_1 => MON_STRS[0].as_ptr(),
        MON_2 => MON_STRS[1].as_ptr(),
        MON_3 => MON_STRS[2].as_ptr(),
        MON_4 => MON_STRS[3].as_ptr(),
        MON_5 => MON_STRS[4].as_ptr(),
        MON_6 => MON_STRS[5].as_ptr(),
        MON_7 => MON_STRS[6].as_ptr(),
        MON_8 => MON_STRS[7].as_ptr(),
        MON_9 => MON_STRS[8].as_ptr(),
        MON_10 => MON_STRS[9].as_ptr(),
        MON_11 => MON_STRS[10].as_ptr(),
        MON_12 => MON_STRS[11].as_ptr(),

        // Abbreviated month names.
        ABMON_1 => ABMON_STRS[0].as_ptr(),
        ABMON_2 => ABMON_STRS[1].as_ptr(),
        ABMON_3 => ABMON_STRS[2].as_ptr(),
        ABMON_4 => ABMON_STRS[3].as_ptr(),
        ABMON_5 => ABMON_STRS[4].as_ptr(),
        ABMON_6 => ABMON_STRS[5].as_ptr(),
        ABMON_7 => ABMON_STRS[6].as_ptr(),
        ABMON_8 => ABMON_STRS[7].as_ptr(),
        ABMON_9 => ABMON_STRS[8].as_ptr(),
        ABMON_10 => ABMON_STRS[9].as_ptr(),
        ABMON_11 => ABMON_STRS[10].as_ptr(),
        ABMON_12 => ABMON_STRS[11].as_ptr(),

        // Era (empty in C locale).
        ERA | ERA_D_FMT | ERA_D_T_FMT | ERA_T_FMT | ALT_DIGITS => EMPTY.as_ptr(),

        THOUSEP => THOUSEP_STR.as_ptr(),
        YESEXPR => YESEXPR_STR.as_ptr(),
        NOEXPR => NOEXPR_STR.as_ptr(),
        CRNCYSTR => CRNCYSTR_STR.as_ptr(),
        CODESET => CODESET_STR.as_ptr(),

        _ => EMPTY.as_ptr(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: read a C string pointer into a byte slice.
    fn cstr_bytes(p: *const u8) -> &'static [u8] {
        assert!(!p.is_null());
        unsafe { core::ffi::CStr::from_ptr(p.cast()) }.to_bytes()
    }

    // -----------------------------------------------------------------------
    // Radix / decimal point
    // -----------------------------------------------------------------------

    #[test]
    fn test_radixchar() {
        assert_eq!(cstr_bytes(nl_langinfo(RADIXCHAR)), b".");
    }

    #[test]
    fn test_decimal_point_alias() {
        assert_eq!(cstr_bytes(nl_langinfo(DECIMAL_POINT)), b".");
    }

    // -----------------------------------------------------------------------
    // Date/time formats
    // -----------------------------------------------------------------------

    #[test]
    fn test_d_t_fmt() {
        assert_eq!(cstr_bytes(nl_langinfo(D_T_FMT)), b"%a %b %e %H:%M:%S %Y");
    }

    #[test]
    fn test_d_fmt() {
        assert_eq!(cstr_bytes(nl_langinfo(D_FMT)), b"%m/%d/%y");
    }

    #[test]
    fn test_t_fmt() {
        assert_eq!(cstr_bytes(nl_langinfo(T_FMT)), b"%H:%M:%S");
    }

    #[test]
    fn test_t_fmt_ampm() {
        assert_eq!(cstr_bytes(nl_langinfo(T_FMT_AMPM)), b"%I:%M:%S %p");
    }

    // -----------------------------------------------------------------------
    // AM/PM
    // -----------------------------------------------------------------------

    #[test]
    fn test_am_str() {
        assert_eq!(cstr_bytes(nl_langinfo(AM_STR)), b"AM");
    }

    #[test]
    fn test_pm_str() {
        assert_eq!(cstr_bytes(nl_langinfo(PM_STR)), b"PM");
    }

    // -----------------------------------------------------------------------
    // Day names
    // -----------------------------------------------------------------------

    #[test]
    fn test_day_names() {
        let expected = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ];
        for (i, name) in expected.iter().enumerate() {
            let item = DAY_1 + i as i32;
            assert_eq!(
                cstr_bytes(nl_langinfo(item)),
                name.as_bytes(),
                "DAY_{} mismatch",
                i + 1
            );
        }
    }

    #[test]
    fn test_abday_names() {
        let expected = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        for (i, name) in expected.iter().enumerate() {
            let item = ABDAY_1 + i as i32;
            assert_eq!(
                cstr_bytes(nl_langinfo(item)),
                name.as_bytes(),
                "ABDAY_{} mismatch",
                i + 1
            );
        }
    }

    // -----------------------------------------------------------------------
    // Month names
    // -----------------------------------------------------------------------

    #[test]
    fn test_month_names() {
        let expected = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        for (i, name) in expected.iter().enumerate() {
            let item = MON_1 + i as i32;
            assert_eq!(
                cstr_bytes(nl_langinfo(item)),
                name.as_bytes(),
                "MON_{} mismatch",
                i + 1
            );
        }
    }

    #[test]
    fn test_abmonth_names() {
        let expected = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        for (i, name) in expected.iter().enumerate() {
            let item = ABMON_1 + i as i32;
            assert_eq!(
                cstr_bytes(nl_langinfo(item)),
                name.as_bytes(),
                "ABMON_{} mismatch",
                i + 1
            );
        }
    }

    // -----------------------------------------------------------------------
    // Era (empty in C locale)
    // -----------------------------------------------------------------------

    #[test]
    fn test_era_empty() {
        assert_eq!(cstr_bytes(nl_langinfo(ERA)), b"");
    }

    #[test]
    fn test_era_d_fmt_empty() {
        assert_eq!(cstr_bytes(nl_langinfo(ERA_D_FMT)), b"");
    }

    #[test]
    fn test_alt_digits_empty() {
        assert_eq!(cstr_bytes(nl_langinfo(ALT_DIGITS)), b"");
    }

    // -----------------------------------------------------------------------
    // Yes/No expressions
    // -----------------------------------------------------------------------

    #[test]
    fn test_yesexpr() {
        assert_eq!(cstr_bytes(nl_langinfo(YESEXPR)), b"^[yY]");
    }

    #[test]
    fn test_noexpr() {
        assert_eq!(cstr_bytes(nl_langinfo(NOEXPR)), b"^[nN]");
    }

    // -----------------------------------------------------------------------
    // Thousands separator (empty in C locale)
    // -----------------------------------------------------------------------

    #[test]
    fn test_thousep() {
        assert_eq!(cstr_bytes(nl_langinfo(THOUSEP)), b"");
    }

    // -----------------------------------------------------------------------
    // Currency
    // -----------------------------------------------------------------------

    #[test]
    fn test_crncystr() {
        assert_eq!(cstr_bytes(nl_langinfo(CRNCYSTR)), b"");
    }

    // -----------------------------------------------------------------------
    // Codeset
    // -----------------------------------------------------------------------

    #[test]
    fn test_codeset() {
        assert_eq!(cstr_bytes(nl_langinfo(CODESET)), b"ANSI_X3.4-1968");
    }

    // -----------------------------------------------------------------------
    // Unknown item returns empty string
    // -----------------------------------------------------------------------

    #[test]
    fn test_unknown_item() {
        assert_eq!(cstr_bytes(nl_langinfo(9999)), b"");
    }

    #[test]
    fn test_negative_item() {
        assert_eq!(cstr_bytes(nl_langinfo(-1)), b"");
    }

    // -----------------------------------------------------------------------
    // nl_langinfo_l delegates to nl_langinfo
    // -----------------------------------------------------------------------

    #[test]
    fn test_nl_langinfo_l_basic() {
        assert_eq!(cstr_bytes(nl_langinfo_l(CODESET, 0)), b"ANSI_X3.4-1968");
    }

    #[test]
    fn test_nl_langinfo_l_day() {
        assert_eq!(cstr_bytes(nl_langinfo_l(DAY_1, 0)), b"Sunday");
    }

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_item_constants_unique() {
        // Verify key constants don't alias each other (except documented aliases).
        let items = [
            RADIXCHAR, D_T_FMT, D_FMT, T_FMT, T_FMT_AMPM, AM_STR, PM_STR, DAY_1, ABDAY_1, MON_1,
            ABMON_1, ERA, YESEXPR, NOEXPR, CODESET,
        ];
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                assert_ne!(
                    items[i], items[j],
                    "items[{i}]={} == items[{j}]={}",
                    items[i], items[j]
                );
            }
        }
    }

    /// The numbers a C caller passes: musl's `<langinfo.h>`, which glibc's
    /// agrees with item for item (both probed 2026-09-27).  The runs --
    /// DAY_1..7 and the rest -- are pinned by the sequence tests below.
    #[test]
    fn the_items_are_musls() {
        assert_eq!(CODESET, 14);
        assert_eq!(
            (RADIXCHAR, THOUSEP, DECIMAL_POINT),
            (0x1_0000, 0x1_0001, 0x1_0000)
        );
        assert_eq!(
            (ABDAY_1, DAY_1, ABMON_1, MON_1),
            (0x2_0000, 0x2_0007, 0x2_000e, 0x2_001a)
        );
        assert_eq!((AM_STR, PM_STR), (0x2_0026, 0x2_0027));
        assert_eq!(
            (D_T_FMT, D_FMT, T_FMT, T_FMT_AMPM),
            (0x2_0028, 0x2_0029, 0x2_002a, 0x2_002b)
        );
        assert_eq!((ERA, ERA_D_FMT, ALT_DIGITS), (0x2_002c, 0x2_002e, 0x2_002f));
        assert_eq!((ERA_D_T_FMT, ERA_T_FMT), (0x2_0030, 0x2_0031));
        assert_eq!(CRNCYSTR, 0x4_000f);
        assert_eq!((YESEXPR, NOEXPR), (0x5_0000, 0x5_0001));
    }

    #[test]
    fn test_day_constants_sequential() {
        assert_eq!(DAY_2, DAY_1 + 1);
        assert_eq!(DAY_7, DAY_1 + 6);
    }

    #[test]
    fn test_mon_constants_sequential() {
        assert_eq!(MON_2, MON_1 + 1);
        assert_eq!(MON_12, MON_1 + 11);
    }

    #[test]
    fn test_abday_constants_sequential() {
        assert_eq!(ABDAY_2, ABDAY_1 + 1);
        assert_eq!(ABDAY_7, ABDAY_1 + 6);
    }

    #[test]
    fn test_abmon_constants_sequential() {
        assert_eq!(ABMON_2, ABMON_1 + 1);
        assert_eq!(ABMON_12, ABMON_1 + 11);
    }
}
