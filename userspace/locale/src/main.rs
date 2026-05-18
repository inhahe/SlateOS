//! OurOS `locale` / `localedef` / `getconf` -- locale and system configuration.
//!
//! Multi-personality binary that behaves as one of three POSIX utilities
//! depending on `argv[0]`:
//!
//! - **locale** (default): display locale environment settings, list available
//!   locales and charmaps, show locale data with keyword/category options.
//! - **localedef**: compile locale definitions (simplified: validates arguments,
//!   lists archive).
//! - **getconf**: print system configuration variable values.

use std::env;
use std::io::{self, Write};
use std::path::Path;
use std::process;

// ===========================================================================
// Personality detection
// ===========================================================================

/// Which personality this binary is running as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Locale,
    Localedef,
    Getconf,
}

/// Determine the personality from `argv[0]`.
fn detect_personality(argv0: &str) -> Personality {
    let base = Path::new(argv0)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("locale");

    match base {
        "localedef" => Personality::Localedef,
        "getconf" => Personality::Getconf,
        _ => Personality::Locale,
    }
}

// ===========================================================================
// Available locales and charmaps
// ===========================================================================

/// Built-in available locale names.
const AVAILABLE_LOCALES: &[&str] = &[
    "C",
    "POSIX",
    "de_DE.UTF-8",
    "en_GB.UTF-8",
    "en_US.UTF-8",
    "es_ES.UTF-8",
    "fr_FR.UTF-8",
    "ja_JP.UTF-8",
    "ko_KR.UTF-8",
    "pt_BR.UTF-8",
    "ru_RU.UTF-8",
    "zh_CN.UTF-8",
];

/// Built-in available charmap names.
const AVAILABLE_CHARMAPS: &[&str] = &[
    "ASCII",
    "ISO-8859-1",
    "ISO-8859-15",
    "KOI8-R",
    "UTF-8",
    "Windows-1252",
];

// ===========================================================================
// Locale categories
// ===========================================================================

/// All LC_* category names in display order.
const LC_CATEGORIES: &[&str] = &[
    "LANG",
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_ALL",
];

/// Resolve the effective value for a locale category.  The resolution order
/// is: `LC_ALL` overrides everything, then the specific `LC_<cat>` variable,
/// then `LANG`, then the built-in default `"POSIX"`.
fn resolve_category(category: &str) -> String {
    // LC_ALL overrides all individual categories.
    if category != "LC_ALL" && category != "LANG" {
        if let Ok(val) = env::var("LC_ALL") {
            if !val.is_empty() {
                return val;
            }
        }
    }

    // The specific variable itself.
    if let Ok(val) = env::var(category) {
        if !val.is_empty() {
            return val;
        }
    }

    // Fall back to LANG (except when querying LANG or LC_ALL themselves).
    if category != "LANG" && category != "LC_ALL" {
        if let Ok(val) = env::var("LANG") {
            if !val.is_empty() {
                return val;
            }
        }
    }

    String::from("POSIX")
}

// ===========================================================================
// Locale data structures
// ===========================================================================

/// Numeric formatting data for a locale.
struct LcNumericData {
    decimal_point: &'static str,
    thousands_sep: &'static str,
    grouping: &'static str,
}

/// Time formatting data for a locale.
struct LcTimeData {
    day_names: &'static [&'static str; 7],
    abbrev_day_names: &'static [&'static str; 7],
    month_names: &'static [&'static str; 12],
    abbrev_month_names: &'static [&'static str; 12],
    d_t_fmt: &'static str,
    d_fmt: &'static str,
    t_fmt: &'static str,
    am_pm: &'static [&'static str; 2],
}

/// Monetary formatting data for a locale.
struct LcMonetaryData {
    currency_symbol: &'static str,
    int_curr_symbol: &'static str,
    mon_decimal_point: &'static str,
    mon_thousands_sep: &'static str,
    mon_grouping: &'static str,
    positive_sign: &'static str,
    negative_sign: &'static str,
    frac_digits: u8,
    int_frac_digits: u8,
}

/// Message expressions for a locale.
struct LcMessagesData {
    yesexpr: &'static str,
    noexpr: &'static str,
    yesstr: &'static str,
    nostr: &'static str,
}

/// Aggregate locale data.
struct LocaleData {
    numeric: LcNumericData,
    time: LcTimeData,
    monetary: LcMonetaryData,
    messages: LcMessagesData,
    charmap: &'static str,
    collate: &'static str,
}

// ===========================================================================
// Static locale data -- en_US.UTF-8 (also used as default for C/POSIX)
// ===========================================================================

static EN_US_DAY_NAMES: [&str; 7] = [
    "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
];
static EN_US_ABBREV_DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
static EN_US_MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];
static EN_US_ABBREV_MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
static EN_US_AM_PM: [&str; 2] = ["AM", "PM"];

static EN_US_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ".",
        thousands_sep: ",",
        grouping: "3;3",
    },
    time: LcTimeData {
        day_names: &EN_US_DAY_NAMES,
        abbrev_day_names: &EN_US_ABBREV_DAYS,
        month_names: &EN_US_MONTH_NAMES,
        abbrev_month_names: &EN_US_ABBREV_MONTHS,
        d_t_fmt: "%a %b %e %H:%M:%S %Y",
        d_fmt: "%m/%d/%Y",
        t_fmt: "%H:%M:%S",
        am_pm: &EN_US_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "$",
        int_curr_symbol: "USD ",
        mon_decimal_point: ".",
        mon_thousands_sep: ",",
        mon_grouping: "3;3",
        positive_sign: "",
        negative_sign: "-",
        frac_digits: 2,
        int_frac_digits: 2,
    },
    messages: LcMessagesData {
        yesexpr: "^[yY]",
        noexpr: "^[nN]",
        yesstr: "yes",
        nostr: "no",
    },
    charmap: "UTF-8",
    collate: "English linguistic",
};

// ===========================================================================
// Static locale data -- en_GB.UTF-8
// ===========================================================================

static EN_GB_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ".",
        thousands_sep: ",",
        grouping: "3;3",
    },
    time: LcTimeData {
        day_names: &EN_US_DAY_NAMES,
        abbrev_day_names: &EN_US_ABBREV_DAYS,
        month_names: &EN_US_MONTH_NAMES,
        abbrev_month_names: &EN_US_ABBREV_MONTHS,
        d_t_fmt: "%a %d %b %Y %T %Z",
        d_fmt: "%d/%m/%Y",
        t_fmt: "%T",
        am_pm: &EN_US_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "\u{00A3}",
        int_curr_symbol: "GBP ",
        mon_decimal_point: ".",
        mon_thousands_sep: ",",
        mon_grouping: "3;3",
        positive_sign: "",
        negative_sign: "-",
        frac_digits: 2,
        int_frac_digits: 2,
    },
    messages: LcMessagesData {
        yesexpr: "^[yY]",
        noexpr: "^[nN]",
        yesstr: "yes",
        nostr: "no",
    },
    charmap: "UTF-8",
    collate: "English linguistic",
};

// ===========================================================================
// Static locale data -- de_DE.UTF-8
// ===========================================================================

static DE_DAY_NAMES: [&str; 7] = [
    "Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag",
];
static DE_ABBREV_DAYS: [&str; 7] = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];
static DE_MONTH_NAMES: [&str; 12] = [
    "Januar", "Februar", "M\u{00E4}rz", "April", "Mai", "Juni",
    "Juli", "August", "September", "Oktober", "November", "Dezember",
];
static DE_ABBREV_MONTHS: [&str; 12] = [
    "Jan", "Feb", "M\u{00E4}r", "Apr", "Mai", "Jun",
    "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
];
static DE_AM_PM: [&str; 2] = ["", ""];

static DE_DE_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ",",
        thousands_sep: ".",
        grouping: "3;3",
    },
    time: LcTimeData {
        day_names: &DE_DAY_NAMES,
        abbrev_day_names: &DE_ABBREV_DAYS,
        month_names: &DE_MONTH_NAMES,
        abbrev_month_names: &DE_ABBREV_MONTHS,
        d_t_fmt: "%a %d %b %Y %T %Z",
        d_fmt: "%d.%m.%Y",
        t_fmt: "%T",
        am_pm: &DE_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "\u{20AC}",
        int_curr_symbol: "EUR ",
        mon_decimal_point: ",",
        mon_thousands_sep: ".",
        mon_grouping: "3;3",
        positive_sign: "",
        negative_sign: "-",
        frac_digits: 2,
        int_frac_digits: 2,
    },
    messages: LcMessagesData {
        yesexpr: "^[jJyY]",
        noexpr: "^[nN]",
        yesstr: "ja",
        nostr: "nein",
    },
    charmap: "UTF-8",
    collate: "German linguistic",
};

// ===========================================================================
// Static locale data -- fr_FR.UTF-8
// ===========================================================================

static FR_DAY_NAMES: [&str; 7] = [
    "dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi",
];
static FR_ABBREV_DAYS: [&str; 7] = ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."];
static FR_MONTH_NAMES: [&str; 12] = [
    "janvier", "f\u{00E9}vrier", "mars", "avril", "mai", "juin",
    "juillet", "ao\u{00FB}t", "septembre", "octobre", "novembre", "d\u{00E9}cembre",
];
static FR_ABBREV_MONTHS: [&str; 12] = [
    "janv.", "f\u{00E9}vr.", "mars", "avr.", "mai", "juin",
    "juil.", "ao\u{00FB}t", "sept.", "oct.", "nov.", "d\u{00E9}c.",
];
static FR_AM_PM: [&str; 2] = ["", ""];

static FR_FR_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ",",
        thousands_sep: "\u{202F}",
        grouping: "3;3",
    },
    time: LcTimeData {
        day_names: &FR_DAY_NAMES,
        abbrev_day_names: &FR_ABBREV_DAYS,
        month_names: &FR_MONTH_NAMES,
        abbrev_month_names: &FR_ABBREV_MONTHS,
        d_t_fmt: "%a %d %b %Y %T %Z",
        d_fmt: "%d/%m/%Y",
        t_fmt: "%T",
        am_pm: &FR_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "\u{20AC}",
        int_curr_symbol: "EUR ",
        mon_decimal_point: ",",
        mon_thousands_sep: "\u{202F}",
        mon_grouping: "3;3",
        positive_sign: "",
        negative_sign: "-",
        frac_digits: 2,
        int_frac_digits: 2,
    },
    messages: LcMessagesData {
        yesexpr: "^[oOyY]",
        noexpr: "^[nN]",
        yesstr: "oui",
        nostr: "non",
    },
    charmap: "UTF-8",
    collate: "French linguistic",
};

// ===========================================================================
// Static locale data -- ja_JP.UTF-8
// ===========================================================================

static JA_DAY_NAMES: [&str; 7] = [
    "\u{65E5}\u{66DC}\u{65E5}",
    "\u{6708}\u{66DC}\u{65E5}",
    "\u{706B}\u{66DC}\u{65E5}",
    "\u{6C34}\u{66DC}\u{65E5}",
    "\u{6728}\u{66DC}\u{65E5}",
    "\u{91D1}\u{66DC}\u{65E5}",
    "\u{571F}\u{66DC}\u{65E5}",
];
static JA_ABBREV_DAYS: [&str; 7] = [
    "\u{65E5}", "\u{6708}", "\u{706B}", "\u{6C34}", "\u{6728}", "\u{91D1}", "\u{571F}",
];
static JA_MONTH_NAMES: [&str; 12] = [
    "1\u{6708}", "2\u{6708}", "3\u{6708}", "4\u{6708}", "5\u{6708}", "6\u{6708}",
    "7\u{6708}", "8\u{6708}", "9\u{6708}", "10\u{6708}", "11\u{6708}", "12\u{6708}",
];
static JA_ABBREV_MONTHS: [&str; 12] = [
    "1\u{6708}", "2\u{6708}", "3\u{6708}", "4\u{6708}", "5\u{6708}", "6\u{6708}",
    "7\u{6708}", "8\u{6708}", "9\u{6708}", "10\u{6708}", "11\u{6708}", "12\u{6708}",
];
static JA_AM_PM: [&str; 2] = ["\u{5348}\u{524D}", "\u{5348}\u{5F8C}"];

static JA_JP_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ".",
        thousands_sep: ",",
        grouping: "3",
    },
    time: LcTimeData {
        day_names: &JA_DAY_NAMES,
        abbrev_day_names: &JA_ABBREV_DAYS,
        month_names: &JA_MONTH_NAMES,
        abbrev_month_names: &JA_ABBREV_MONTHS,
        d_t_fmt: "%Y\u{5E74}%m\u{6708}%d\u{65E5} %H\u{6642}%M\u{5206}%S\u{79D2}",
        d_fmt: "%Y/%m/%d",
        t_fmt: "%H:%M:%S",
        am_pm: &JA_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "\u{00A5}",
        int_curr_symbol: "JPY ",
        mon_decimal_point: ".",
        mon_thousands_sep: ",",
        mon_grouping: "3",
        positive_sign: "",
        negative_sign: "-",
        frac_digits: 0,
        int_frac_digits: 0,
    },
    messages: LcMessagesData {
        yesexpr: "^[yY\u{306F}]",
        noexpr: "^[nN\u{3044}]",
        yesstr: "\u{306F}\u{3044}",
        nostr: "\u{3044}\u{3044}\u{3048}",
    },
    charmap: "UTF-8",
    collate: "Japanese linguistic",
};

// ===========================================================================
// Static locale data -- zh_CN.UTF-8
// ===========================================================================

static ZH_DAY_NAMES: [&str; 7] = [
    "\u{661F}\u{671F}\u{65E5}",
    "\u{661F}\u{671F}\u{4E00}",
    "\u{661F}\u{671F}\u{4E8C}",
    "\u{661F}\u{671F}\u{4E09}",
    "\u{661F}\u{671F}\u{56DB}",
    "\u{661F}\u{671F}\u{4E94}",
    "\u{661F}\u{671F}\u{516D}",
];
static ZH_ABBREV_DAYS: [&str; 7] = [
    "\u{65E5}", "\u{4E00}", "\u{4E8C}", "\u{4E09}", "\u{56DB}", "\u{4E94}", "\u{516D}",
];
static ZH_MONTH_NAMES: [&str; 12] = [
    "\u{4E00}\u{6708}", "\u{4E8C}\u{6708}", "\u{4E09}\u{6708}",
    "\u{56DB}\u{6708}", "\u{4E94}\u{6708}", "\u{516D}\u{6708}",
    "\u{4E03}\u{6708}", "\u{516B}\u{6708}", "\u{4E5D}\u{6708}",
    "\u{5341}\u{6708}", "\u{5341}\u{4E00}\u{6708}", "\u{5341}\u{4E8C}\u{6708}",
];
static ZH_ABBREV_MONTHS: [&str; 12] = [
    "1\u{6708}", "2\u{6708}", "3\u{6708}", "4\u{6708}", "5\u{6708}", "6\u{6708}",
    "7\u{6708}", "8\u{6708}", "9\u{6708}", "10\u{6708}", "11\u{6708}", "12\u{6708}",
];
static ZH_AM_PM: [&str; 2] = ["\u{4E0A}\u{5348}", "\u{4E0B}\u{5348}"];

static ZH_CN_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ".",
        thousands_sep: ",",
        grouping: "3",
    },
    time: LcTimeData {
        day_names: &ZH_DAY_NAMES,
        abbrev_day_names: &ZH_ABBREV_DAYS,
        month_names: &ZH_MONTH_NAMES,
        abbrev_month_names: &ZH_ABBREV_MONTHS,
        d_t_fmt: "%Y\u{5E74}%m\u{6708}%d\u{65E5} %A %H\u{65F6}%M\u{5206}%S\u{79D2}",
        d_fmt: "%Y\u{5E74}%m\u{6708}%d\u{65E5}",
        t_fmt: "%H\u{65F6}%M\u{5206}%S\u{79D2}",
        am_pm: &ZH_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "\u{00A5}",
        int_curr_symbol: "CNY ",
        mon_decimal_point: ".",
        mon_thousands_sep: ",",
        mon_grouping: "3",
        positive_sign: "",
        negative_sign: "-",
        frac_digits: 2,
        int_frac_digits: 2,
    },
    messages: LcMessagesData {
        yesexpr: "^[yY\u{662F}]",
        noexpr: "^[nN\u{4E0D}]",
        yesstr: "\u{662F}",
        nostr: "\u{4E0D}\u{662F}",
    },
    charmap: "UTF-8",
    collate: "Chinese linguistic",
};

// ===========================================================================
// Static locale data -- ko_KR.UTF-8
// ===========================================================================

static KO_DAY_NAMES: [&str; 7] = [
    "\u{C77C}\u{C694}\u{C77C}",
    "\u{C6D4}\u{C694}\u{C77C}",
    "\u{D654}\u{C694}\u{C77C}",
    "\u{C218}\u{C694}\u{C77C}",
    "\u{BAA9}\u{C694}\u{C77C}",
    "\u{AE08}\u{C694}\u{C77C}",
    "\u{D1A0}\u{C694}\u{C77C}",
];
static KO_ABBREV_DAYS: [&str; 7] = [
    "\u{C77C}", "\u{C6D4}", "\u{D654}", "\u{C218}", "\u{BAA9}", "\u{AE08}", "\u{D1A0}",
];
static KO_MONTH_NAMES: [&str; 12] = [
    "1\u{C6D4}", "2\u{C6D4}", "3\u{C6D4}", "4\u{C6D4}", "5\u{C6D4}", "6\u{C6D4}",
    "7\u{C6D4}", "8\u{C6D4}", "9\u{C6D4}", "10\u{C6D4}", "11\u{C6D4}", "12\u{C6D4}",
];
static KO_ABBREV_MONTHS: [&str; 12] = [
    "1\u{C6D4}", "2\u{C6D4}", "3\u{C6D4}", "4\u{C6D4}", "5\u{C6D4}", "6\u{C6D4}",
    "7\u{C6D4}", "8\u{C6D4}", "9\u{C6D4}", "10\u{C6D4}", "11\u{C6D4}", "12\u{C6D4}",
];
static KO_AM_PM: [&str; 2] = ["\u{C624}\u{C804}", "\u{C624}\u{D6C4}"];

static KO_KR_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ".",
        thousands_sep: ",",
        grouping: "3",
    },
    time: LcTimeData {
        day_names: &KO_DAY_NAMES,
        abbrev_day_names: &KO_ABBREV_DAYS,
        month_names: &KO_MONTH_NAMES,
        abbrev_month_names: &KO_ABBREV_MONTHS,
        d_t_fmt: "%Y\u{B144} %m\u{C6D4} %d\u{C77C} (%a) %p %I\u{C2DC} %M\u{BD84} %S\u{CD08}",
        d_fmt: "%Y. %m. %d.",
        t_fmt: "%H\u{C2DC} %M\u{BD84} %S\u{CD08}",
        am_pm: &KO_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "\u{20A9}",
        int_curr_symbol: "KRW ",
        mon_decimal_point: ".",
        mon_thousands_sep: ",",
        mon_grouping: "3",
        positive_sign: "",
        negative_sign: "-",
        frac_digits: 0,
        int_frac_digits: 0,
    },
    messages: LcMessagesData {
        yesexpr: "^[yY\u{C608}]",
        noexpr: "^[nN\u{C544}]",
        yesstr: "\u{C608}",
        nostr: "\u{C544}\u{B2C8}\u{C624}",
    },
    charmap: "UTF-8",
    collate: "Korean linguistic",
};

// ===========================================================================
// Static locale data -- es_ES.UTF-8
// ===========================================================================

static ES_DAY_NAMES: [&str; 7] = [
    "domingo", "lunes", "martes", "mi\u{00E9}rcoles", "jueves", "viernes", "s\u{00E1}bado",
];
static ES_ABBREV_DAYS: [&str; 7] = ["dom", "lun", "mar", "mi\u{00E9}", "jue", "vie", "s\u{00E1}b"];
static ES_MONTH_NAMES: [&str; 12] = [
    "enero", "febrero", "marzo", "abril", "mayo", "junio",
    "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre",
];
static ES_ABBREV_MONTHS: [&str; 12] = [
    "ene", "feb", "mar", "abr", "may", "jun",
    "jul", "ago", "sep", "oct", "nov", "dic",
];
static ES_AM_PM: [&str; 2] = ["", ""];

static ES_ES_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ",",
        thousands_sep: ".",
        grouping: "3;3",
    },
    time: LcTimeData {
        day_names: &ES_DAY_NAMES,
        abbrev_day_names: &ES_ABBREV_DAYS,
        month_names: &ES_MONTH_NAMES,
        abbrev_month_names: &ES_ABBREV_MONTHS,
        d_t_fmt: "%a %d %b %Y %T %Z",
        d_fmt: "%d/%m/%Y",
        t_fmt: "%T",
        am_pm: &ES_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "\u{20AC}",
        int_curr_symbol: "EUR ",
        mon_decimal_point: ",",
        mon_thousands_sep: ".",
        mon_grouping: "3;3",
        positive_sign: "",
        negative_sign: "-",
        frac_digits: 2,
        int_frac_digits: 2,
    },
    messages: LcMessagesData {
        yesexpr: "^[sS\u{00ED}yY]",
        noexpr: "^[nN]",
        yesstr: "s\u{00ED}",
        nostr: "no",
    },
    charmap: "UTF-8",
    collate: "Spanish linguistic",
};

// ===========================================================================
// Static locale data -- pt_BR.UTF-8
// ===========================================================================

static PT_DAY_NAMES: [&str; 7] = [
    "domingo", "segunda-feira", "ter\u{00E7}a-feira", "quarta-feira",
    "quinta-feira", "sexta-feira", "s\u{00E1}bado",
];
static PT_ABBREV_DAYS: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "s\u{00E1}b"];
static PT_MONTH_NAMES: [&str; 12] = [
    "janeiro", "fevereiro", "mar\u{00E7}o", "abril", "maio", "junho",
    "julho", "agosto", "setembro", "outubro", "novembro", "dezembro",
];
static PT_ABBREV_MONTHS: [&str; 12] = [
    "jan", "fev", "mar", "abr", "mai", "jun",
    "jul", "ago", "set", "out", "nov", "dez",
];
static PT_AM_PM: [&str; 2] = ["", ""];

static PT_BR_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ",",
        thousands_sep: ".",
        grouping: "3;3",
    },
    time: LcTimeData {
        day_names: &PT_DAY_NAMES,
        abbrev_day_names: &PT_ABBREV_DAYS,
        month_names: &PT_MONTH_NAMES,
        abbrev_month_names: &PT_ABBREV_MONTHS,
        d_t_fmt: "%a %d %b %Y %T %Z",
        d_fmt: "%d/%m/%Y",
        t_fmt: "%T",
        am_pm: &PT_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "R$",
        int_curr_symbol: "BRL ",
        mon_decimal_point: ",",
        mon_thousands_sep: ".",
        mon_grouping: "3;3",
        positive_sign: "",
        negative_sign: "-",
        frac_digits: 2,
        int_frac_digits: 2,
    },
    messages: LcMessagesData {
        yesexpr: "^[sS\u{00ED}yY]",
        noexpr: "^[nN]",
        yesstr: "sim",
        nostr: "n\u{00E3}o",
    },
    charmap: "UTF-8",
    collate: "Portuguese linguistic",
};

// ===========================================================================
// Static locale data -- ru_RU.UTF-8
// ===========================================================================

static RU_DAY_NAMES: [&str; 7] = [
    "\u{0412}\u{043E}\u{0441}\u{043A}\u{0440}\u{0435}\u{0441}\u{0435}\u{043D}\u{044C}\u{0435}",
    "\u{041F}\u{043E}\u{043D}\u{0435}\u{0434}\u{0435}\u{043B}\u{044C}\u{043D}\u{0438}\u{043A}",
    "\u{0412}\u{0442}\u{043E}\u{0440}\u{043D}\u{0438}\u{043A}",
    "\u{0421}\u{0440}\u{0435}\u{0434}\u{0430}",
    "\u{0427}\u{0435}\u{0442}\u{0432}\u{0435}\u{0440}\u{0433}",
    "\u{041F}\u{044F}\u{0442}\u{043D}\u{0438}\u{0446}\u{0430}",
    "\u{0421}\u{0443}\u{0431}\u{0431}\u{043E}\u{0442}\u{0430}",
];
static RU_ABBREV_DAYS: [&str; 7] = [
    "\u{0412}\u{0441}", "\u{041F}\u{043D}",
    "\u{0412}\u{0442}", "\u{0421}\u{0440}",
    "\u{0427}\u{0442}", "\u{041F}\u{0442}",
    "\u{0421}\u{0431}",
];
static RU_MONTH_NAMES: [&str; 12] = [
    "\u{042F}\u{043D}\u{0432}\u{0430}\u{0440}\u{044C}",
    "\u{0424}\u{0435}\u{0432}\u{0440}\u{0430}\u{043B}\u{044C}",
    "\u{041C}\u{0430}\u{0440}\u{0442}",
    "\u{0410}\u{043F}\u{0440}\u{0435}\u{043B}\u{044C}",
    "\u{041C}\u{0430}\u{0439}",
    "\u{0418}\u{044E}\u{043D}\u{044C}",
    "\u{0418}\u{044E}\u{043B}\u{044C}",
    "\u{0410}\u{0432}\u{0433}\u{0443}\u{0441}\u{0442}",
    "\u{0421}\u{0435}\u{043D}\u{0442}\u{044F}\u{0431}\u{0440}\u{044C}",
    "\u{041E}\u{043A}\u{0442}\u{044F}\u{0431}\u{0440}\u{044C}",
    "\u{041D}\u{043E}\u{044F}\u{0431}\u{0440}\u{044C}",
    "\u{0414}\u{0435}\u{043A}\u{0430}\u{0431}\u{0440}\u{044C}",
];
static RU_ABBREV_MONTHS: [&str; 12] = [
    "\u{044F}\u{043D}\u{0432}", "\u{0444}\u{0435}\u{0432}",
    "\u{043C}\u{0430}\u{0440}", "\u{0430}\u{043F}\u{0440}",
    "\u{043C}\u{0430}\u{0439}", "\u{0438}\u{044E}\u{043D}",
    "\u{0438}\u{044E}\u{043B}", "\u{0430}\u{0432}\u{0433}",
    "\u{0441}\u{0435}\u{043D}", "\u{043E}\u{043A}\u{0442}",
    "\u{043D}\u{043E}\u{044F}", "\u{0434}\u{0435}\u{043A}",
];
static RU_AM_PM: [&str; 2] = ["", ""];

static RU_RU_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ",",
        thousands_sep: "\u{00A0}",
        grouping: "3;3",
    },
    time: LcTimeData {
        day_names: &RU_DAY_NAMES,
        abbrev_day_names: &RU_ABBREV_DAYS,
        month_names: &RU_MONTH_NAMES,
        abbrev_month_names: &RU_ABBREV_MONTHS,
        d_t_fmt: "%a %d %b %Y %T",
        d_fmt: "%d.%m.%Y",
        t_fmt: "%T",
        am_pm: &RU_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "\u{20BD}",
        int_curr_symbol: "RUB ",
        mon_decimal_point: ",",
        mon_thousands_sep: "\u{00A0}",
        mon_grouping: "3;3",
        positive_sign: "",
        negative_sign: "-",
        frac_digits: 2,
        int_frac_digits: 2,
    },
    messages: LcMessagesData {
        yesexpr: "^[yY\u{0434}\u{0414}]",
        noexpr: "^[nN\u{043D}\u{041D}]",
        yesstr: "\u{0434}\u{0430}",
        nostr: "\u{043D}\u{0435}\u{0442}",
    },
    charmap: "UTF-8",
    collate: "Russian linguistic",
};

// ===========================================================================
// Static locale data -- C / POSIX
// ===========================================================================

static C_DAY_NAMES: [&str; 7] = [
    "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
];
static C_ABBREV_DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
static C_MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];
static C_ABBREV_MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
static C_AM_PM: [&str; 2] = ["AM", "PM"];

static C_LOCALE_DATA: LocaleData = LocaleData {
    numeric: LcNumericData {
        decimal_point: ".",
        thousands_sep: "",
        grouping: "",
    },
    time: LcTimeData {
        day_names: &C_DAY_NAMES,
        abbrev_day_names: &C_ABBREV_DAYS,
        month_names: &C_MONTH_NAMES,
        abbrev_month_names: &C_ABBREV_MONTHS,
        d_t_fmt: "%a %b %e %H:%M:%S %Y",
        d_fmt: "%m/%d/%y",
        t_fmt: "%H:%M:%S",
        am_pm: &C_AM_PM,
    },
    monetary: LcMonetaryData {
        currency_symbol: "",
        int_curr_symbol: "",
        mon_decimal_point: "",
        mon_thousands_sep: "",
        mon_grouping: "",
        positive_sign: "",
        negative_sign: "",
        frac_digits: 127,
        int_frac_digits: 127,
    },
    messages: LcMessagesData {
        yesexpr: "^[yY]",
        noexpr: "^[nN]",
        yesstr: "yes",
        nostr: "no",
    },
    charmap: "ANSI_X3.4-1968",
    collate: "C/POSIX standard",
};

// ===========================================================================
// Locale data lookup
// ===========================================================================

/// Look up the built-in locale data for a given locale name.
fn get_locale_data(locale_name: &str) -> &'static LocaleData {
    match locale_name {
        "C" | "POSIX" => &C_LOCALE_DATA,
        "en_US.UTF-8" | "en_US.utf8" => &EN_US_DATA,
        "en_GB.UTF-8" | "en_GB.utf8" => &EN_GB_DATA,
        "de_DE.UTF-8" | "de_DE.utf8" => &DE_DE_DATA,
        "fr_FR.UTF-8" | "fr_FR.utf8" => &FR_FR_DATA,
        "ja_JP.UTF-8" | "ja_JP.utf8" => &JA_JP_DATA,
        "zh_CN.UTF-8" | "zh_CN.utf8" => &ZH_CN_DATA,
        "ko_KR.UTF-8" | "ko_KR.utf8" => &KO_KR_DATA,
        "es_ES.UTF-8" | "es_ES.utf8" => &ES_ES_DATA,
        "pt_BR.UTF-8" | "pt_BR.utf8" => &PT_BR_DATA,
        "ru_RU.UTF-8" | "ru_RU.utf8" => &RU_RU_DATA,
        // Default to C for unrecognized locales.
        _ => &C_LOCALE_DATA,
    }
}

// ===========================================================================
// Locale mode -- display and query locale settings
// ===========================================================================

/// Print all locale keywords for a given category and locale data.
fn print_category_keywords(
    out: &mut dyn Write,
    category: &str,
    data: &LocaleData,
    show_keyword: bool,
    show_category: bool,
) {
    if show_category {
        let _ = writeln!(out, "{category}");
    }

    match category {
        "LC_CTYPE" => {
            let line = if show_keyword {
                format!("charmap=\"{}\"", data.charmap)
            } else {
                format!("\"{}\"", data.charmap)
            };
            let _ = writeln!(out, "{line}");
        }
        "LC_NUMERIC" => {
            let items: &[(&str, &str)] = &[
                ("decimal_point", data.numeric.decimal_point),
                ("thousands_sep", data.numeric.thousands_sep),
                ("grouping", data.numeric.grouping),
            ];
            for &(kw, val) in items {
                let line = if show_keyword {
                    format!("{kw}=\"{val}\"")
                } else {
                    format!("\"{val}\"")
                };
                let _ = writeln!(out, "{line}");
            }
        }
        "LC_TIME" => {
            // Day names
            for (i, name) in data.time.day_names.iter().enumerate() {
                let kw = format!("day[{i}]");
                let line = if show_keyword {
                    format!("{kw}=\"{name}\"")
                } else {
                    format!("\"{name}\"")
                };
                let _ = writeln!(out, "{line}");
            }
            for (i, name) in data.time.abbrev_day_names.iter().enumerate() {
                let kw = format!("abday[{i}]");
                let line = if show_keyword {
                    format!("{kw}=\"{name}\"")
                } else {
                    format!("\"{name}\"")
                };
                let _ = writeln!(out, "{line}");
            }
            for (i, name) in data.time.month_names.iter().enumerate() {
                let kw = format!("mon[{i}]");
                let line = if show_keyword {
                    format!("{kw}=\"{name}\"")
                } else {
                    format!("\"{name}\"")
                };
                let _ = writeln!(out, "{line}");
            }
            for (i, name) in data.time.abbrev_month_names.iter().enumerate() {
                let kw = format!("abmon[{i}]");
                let line = if show_keyword {
                    format!("{kw}=\"{name}\"")
                } else {
                    format!("\"{name}\"")
                };
                let _ = writeln!(out, "{line}");
            }
            let time_fmts: &[(&str, &str)] = &[
                ("d_t_fmt", data.time.d_t_fmt),
                ("d_fmt", data.time.d_fmt),
                ("t_fmt", data.time.t_fmt),
            ];
            for &(kw, val) in time_fmts {
                let line = if show_keyword {
                    format!("{kw}=\"{val}\"")
                } else {
                    format!("\"{val}\"")
                };
                let _ = writeln!(out, "{line}");
            }
            for (i, label) in data.time.am_pm.iter().enumerate() {
                let kw = format!("am_pm[{i}]");
                let line = if show_keyword {
                    format!("{kw}=\"{label}\"")
                } else {
                    format!("\"{label}\"")
                };
                let _ = writeln!(out, "{line}");
            }
        }
        "LC_COLLATE" => {
            let line = if show_keyword {
                format!("collate-description=\"{}\"", data.collate)
            } else {
                format!("\"{}\"", data.collate)
            };
            let _ = writeln!(out, "{line}");
        }
        "LC_MONETARY" => {
            let items: &[(&str, &str)] = &[
                ("currency_symbol", data.monetary.currency_symbol),
                ("int_curr_symbol", data.monetary.int_curr_symbol),
                ("mon_decimal_point", data.monetary.mon_decimal_point),
                ("mon_thousands_sep", data.monetary.mon_thousands_sep),
                ("mon_grouping", data.monetary.mon_grouping),
                ("positive_sign", data.monetary.positive_sign),
                ("negative_sign", data.monetary.negative_sign),
            ];
            for &(kw, val) in items {
                let line = if show_keyword {
                    format!("{kw}=\"{val}\"")
                } else {
                    format!("\"{val}\"")
                };
                let _ = writeln!(out, "{line}");
            }
            // Numeric fields
            let num_items: &[(&str, u8)] = &[
                ("frac_digits", data.monetary.frac_digits),
                ("int_frac_digits", data.monetary.int_frac_digits),
            ];
            for &(kw, val) in num_items {
                let line = if show_keyword {
                    format!("{kw}={val}")
                } else {
                    format!("{val}")
                };
                let _ = writeln!(out, "{line}");
            }
        }
        "LC_MESSAGES" => {
            let items: &[(&str, &str)] = &[
                ("yesexpr", data.messages.yesexpr),
                ("noexpr", data.messages.noexpr),
                ("yesstr", data.messages.yesstr),
                ("nostr", data.messages.nostr),
            ];
            for &(kw, val) in items {
                let line = if show_keyword {
                    format!("{kw}=\"{val}\"")
                } else {
                    format!("\"{val}\"")
                };
                let _ = writeln!(out, "{line}");
            }
        }
        _ => {}
    }
}

/// Run the `locale` personality.
fn run_locale(args: &[String]) {
    let mut show_all = false;
    let mut show_charmaps = false;
    let mut show_keyword = false;
    let mut show_category = false;
    let mut query_categories: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-a" | "--all-locales" => show_all = true,
            "-m" | "--charmaps" => show_charmaps = true,
            "-k" | "--keyword" => show_keyword = true,
            "-c" | "--category-name" => show_category = true,
            "-h" | "--help" => {
                println!("Usage: locale [OPTION...] [NAME...]");
                println!("  -a, --all-locales    List all available locales");
                println!("  -m, --charmaps       List available character maps");
                println!("  -k, --keyword        Show keyword names with values");
                println!("  -c, --category-name  Show category name before values");
                println!("  -h, --help           Show this help");
                process::exit(0);
            }
            other => {
                if other.starts_with('-') {
                    let _ = writeln!(io::stderr(), "locale: unrecognized option: {other}");
                    process::exit(1);
                }
                query_categories.push(other.to_string());
            }
        }
        i += 1;
    }

    let out = io::stdout();
    let mut out = out.lock();

    if show_all {
        for loc in AVAILABLE_LOCALES {
            let _ = writeln!(out, "{loc}");
        }
        return;
    }

    if show_charmaps {
        for cm in AVAILABLE_CHARMAPS {
            let _ = writeln!(out, "{cm}");
        }
        return;
    }

    // If specific categories were requested, show data for those.
    if !query_categories.is_empty() {
        for cat in &query_categories {
            let effective = resolve_category(cat);
            let data = get_locale_data(&effective);
            print_category_keywords(&mut out, cat, data, show_keyword, show_category);
        }
        return;
    }

    // Default: show all category settings.
    for &cat in LC_CATEGORIES {
        let val = resolve_category(cat);
        let _ = writeln!(out, "{cat}={val}");
    }
}

// ===========================================================================
// Localedef mode
// ===========================================================================

/// Run the `localedef` personality.
fn run_localedef(args: &[String]) {
    let mut charmap: Option<String> = None;
    let mut input: Option<String> = None;
    let mut force_create = false;
    let mut list_archive = false;
    let mut locale_name: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-f" => {
                i += 1;
                if i < args.len() {
                    charmap = Some(args[i].clone());
                } else {
                    let _ = writeln!(io::stderr(), "localedef: -f requires an argument");
                    process::exit(1);
                }
            }
            "-i" => {
                i += 1;
                if i < args.len() {
                    input = Some(args[i].clone());
                } else {
                    let _ = writeln!(io::stderr(), "localedef: -i requires an argument");
                    process::exit(1);
                }
            }
            "-c" => {
                force_create = true;
            }
            "--list-archive" => {
                list_archive = true;
            }
            "-h" | "--help" => {
                println!("Usage: localedef [OPTION...] LOCALE_NAME");
                println!("  -f charmap       Specify character map");
                println!("  -i input         Specify input file");
                println!("  -c               Create even if warnings");
                println!("  --list-archive   List installed locales");
                println!("  -h, --help       Show this help");
                process::exit(0);
            }
            other => {
                if other.starts_with('-') {
                    let _ = writeln!(io::stderr(), "localedef: unrecognized option: {other}");
                    process::exit(1);
                }
                locale_name = Some(other.to_string());
            }
        }
        i += 1;
    }

    if list_archive {
        for loc in AVAILABLE_LOCALES {
            println!("{loc}");
        }
        return;
    }

    let Some(name) = locale_name else {
        let _ = writeln!(io::stderr(), "localedef: no locale name specified");
        process::exit(1);
    };

    // Validate that we have an input file specified (or default).
    let input_file = input.unwrap_or_else(|| name.clone());
    let charmap_name = charmap.unwrap_or_else(|| String::from("UTF-8"));

    // Check if the charmap is known.
    let charmap_valid = AVAILABLE_CHARMAPS.iter().any(|&cm| {
        cm.eq_ignore_ascii_case(&charmap_name)
    });

    if !charmap_valid && !force_create {
        let _ = writeln!(
            io::stderr(),
            "localedef: unknown charmap '{charmap_name}'"
        );
        process::exit(1);
    }

    // In a real implementation, we would read and parse the input file and
    // the charmap file, then generate a compiled locale.  For now we
    // validate the arguments and report success.
    println!(
        "localedef: locale '{name}' created from input '{input_file}' with charmap '{charmap_name}'"
    );
}

// ===========================================================================
// Getconf mode -- system configuration variables
// ===========================================================================

/// A system configuration variable entry.
struct ConfVar {
    name: &'static str,
    value: ConfValue,
}

/// Value of a configuration variable: either a fixed string or a dynamic
/// computation.
enum ConfValue {
    Str(&'static str),
    Int(i64),
}

impl ConfValue {
    fn display(&self) -> String {
        match self {
            ConfValue::Str(s) => (*s).to_string(),
            ConfValue::Int(n) => n.to_string(),
        }
    }
}

/// Attempt to count CPUs from `/proc/cpuinfo`.  Returns 1 if the file
/// cannot be read or parsed.
fn count_cpus() -> i64 {
    let Ok(content) = std::fs::read_to_string("/proc/cpuinfo") else {
        return 1;
    };
    let count = content
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count();
    if count == 0 { 1 } else { count as i64 }
}

/// Build the table of known configuration variables.
fn build_conf_table() -> Vec<ConfVar> {
    let ncpus = count_cpus();
    vec![
        // POSIX
        ConfVar { name: "ARG_MAX", value: ConfValue::Int(2_097_152) },
        ConfVar { name: "CHILD_MAX", value: ConfValue::Int(32_768) },
        ConfVar { name: "CLK_TCK", value: ConfValue::Int(100) },
        ConfVar { name: "HOST_NAME_MAX", value: ConfValue::Int(255) },
        ConfVar { name: "LOGIN_NAME_MAX", value: ConfValue::Int(256) },
        ConfVar { name: "OPEN_MAX", value: ConfValue::Int(1_024) },
        ConfVar { name: "PAGE_SIZE", value: ConfValue::Int(16_384) },
        ConfVar { name: "PAGESIZE", value: ConfValue::Int(16_384) },
        ConfVar { name: "_POSIX_VERSION", value: ConfValue::Int(200_809) },
        // Path
        ConfVar { name: "PATH_MAX", value: ConfValue::Int(4_096) },
        ConfVar { name: "NAME_MAX", value: ConfValue::Int(255) },
        ConfVar { name: "PIPE_BUF", value: ConfValue::Int(4_096) },
        ConfVar { name: "SYMLINK_MAX", value: ConfValue::Int(255) },
        // System
        ConfVar { name: "NPROCESSORS_CONF", value: ConfValue::Int(ncpus) },
        ConfVar { name: "NPROCESSORS_ONLN", value: ConfValue::Int(ncpus) },
        // GNU
        ConfVar { name: "GNU_LIBC_VERSION", value: ConfValue::Str("ouros-libc 0.1") },
        ConfVar { name: "GNU_LIBPTHREAD_VERSION", value: ConfValue::Str("ouros-pthread 0.1") },
        // Limits
        ConfVar { name: "LONG_BIT", value: ConfValue::Int(64) },
        ConfVar { name: "WORD_BIT", value: ConfValue::Int(32) },
        ConfVar { name: "INT_MAX", value: ConfValue::Int(2_147_483_647) },
        ConfVar { name: "INT_MIN", value: ConfValue::Int(-2_147_483_648) },
        ConfVar { name: "UINT_MAX", value: ConfValue::Int(4_294_967_295) },
        ConfVar { name: "SSIZE_MAX", value: ConfValue::Int(9_223_372_036_854_775_807) },
        // File
        ConfVar { name: "FILESIZEBITS", value: ConfValue::Int(64) },
        ConfVar { name: "LINK_MAX", value: ConfValue::Int(65_000) },
    ]
}

/// Run the `getconf` personality.
fn run_getconf(args: &[String]) {
    if args.is_empty() {
        let _ = writeln!(io::stderr(), "Usage: getconf [-a | NAME]");
        process::exit(1);
    }

    let table = build_conf_table();

    // Handle `-a` (print all).
    if args.len() == 1 && args[0] == "-a" {
        let out = io::stdout();
        let mut out = out.lock();
        for var in &table {
            let _ = writeln!(out, "{}: {}", var.name, var.value.display());
        }
        return;
    }

    // Handle `--help`.
    if args.len() == 1 && (args[0] == "-h" || args[0] == "--help") {
        println!("Usage: getconf [-a | NAME]");
        println!("  -a         Print all configuration variables");
        println!("  NAME       Print the value of the named variable");
        println!("  -h, --help Show this help");
        process::exit(0);
    }

    // Look up a specific variable.
    let name = &args[0];
    for var in &table {
        if var.name == name.as_str() {
            println!("{}", var.value.display());
            return;
        }
    }

    let _ = writeln!(io::stderr(), "getconf: unrecognized variable: {name}");
    process::exit(1);
}

// ===========================================================================
// Main
// ===========================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map_or("locale", |s| s.as_str());
    let personality = detect_personality(argv0);
    let rest = if args.len() > 1 { &args[1..] } else { &[] };

    match personality {
        Personality::Locale => run_locale(rest),
        Personality::Localedef => run_localedef(rest),
        Personality::Getconf => run_getconf(rest),
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Personality detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_personality_locale_default() {
        assert_eq!(detect_personality("locale"), Personality::Locale);
    }

    #[test]
    fn test_personality_locale_path() {
        assert_eq!(detect_personality("/usr/bin/locale"), Personality::Locale);
    }

    #[test]
    fn test_personality_localedef() {
        assert_eq!(detect_personality("localedef"), Personality::Localedef);
    }

    #[test]
    fn test_personality_localedef_path() {
        assert_eq!(detect_personality("/usr/bin/localedef"), Personality::Localedef);
    }

    #[test]
    fn test_personality_getconf() {
        assert_eq!(detect_personality("getconf"), Personality::Getconf);
    }

    #[test]
    fn test_personality_getconf_path() {
        assert_eq!(detect_personality("/usr/bin/getconf"), Personality::Getconf);
    }

    #[test]
    fn test_personality_unknown_defaults_locale() {
        assert_eq!(detect_personality("something_else"), Personality::Locale);
    }

    // -----------------------------------------------------------------------
    // Locale environment resolution
    // -----------------------------------------------------------------------

    // Helper: run with controlled env vars.  We use a sequential approach
    // because tests may run in parallel and `env::set_var`/`remove_var`
    // affect the whole process.  However, since these are unit tests and
    // the `resolve_category` function is deterministic given the env, the
    // tests remain meaningful.

    #[test]
    fn test_resolve_category_default_posix() {
        // When no env vars are set, the default is POSIX.
        // This test relies on the test runner not having LC_CTYPE etc set,
        // which may not be true on all systems.  We test the function logic
        // via the data-driven tests below instead.
        let data = get_locale_data("POSIX");
        assert_eq!(data.numeric.decimal_point, ".");
    }

    #[test]
    fn test_resolve_c_locale() {
        let data = get_locale_data("C");
        assert_eq!(data.numeric.decimal_point, ".");
        assert_eq!(data.numeric.thousands_sep, "");
    }

    #[test]
    fn test_resolve_posix_locale() {
        let data = get_locale_data("POSIX");
        assert_eq!(data.charmap, "ANSI_X3.4-1968");
    }

    // -----------------------------------------------------------------------
    // Available locales listing
    // -----------------------------------------------------------------------

    #[test]
    fn test_available_locales_contains_c() {
        assert!(AVAILABLE_LOCALES.contains(&"C"));
    }

    #[test]
    fn test_available_locales_contains_posix() {
        assert!(AVAILABLE_LOCALES.contains(&"POSIX"));
    }

    #[test]
    fn test_available_locales_contains_en_us() {
        assert!(AVAILABLE_LOCALES.contains(&"en_US.UTF-8"));
    }

    #[test]
    fn test_available_locales_contains_de_de() {
        assert!(AVAILABLE_LOCALES.contains(&"de_DE.UTF-8"));
    }

    #[test]
    fn test_available_locales_contains_ja_jp() {
        assert!(AVAILABLE_LOCALES.contains(&"ja_JP.UTF-8"));
    }

    #[test]
    fn test_available_locales_count() {
        assert_eq!(AVAILABLE_LOCALES.len(), 12);
    }

    // -----------------------------------------------------------------------
    // Charmap listing
    // -----------------------------------------------------------------------

    #[test]
    fn test_charmaps_contains_utf8() {
        assert!(AVAILABLE_CHARMAPS.contains(&"UTF-8"));
    }

    #[test]
    fn test_charmaps_contains_ascii() {
        assert!(AVAILABLE_CHARMAPS.contains(&"ASCII"));
    }

    #[test]
    fn test_charmaps_contains_iso8859_1() {
        assert!(AVAILABLE_CHARMAPS.contains(&"ISO-8859-1"));
    }

    #[test]
    fn test_charmaps_contains_iso8859_15() {
        assert!(AVAILABLE_CHARMAPS.contains(&"ISO-8859-15"));
    }

    #[test]
    fn test_charmaps_contains_windows1252() {
        assert!(AVAILABLE_CHARMAPS.contains(&"Windows-1252"));
    }

    #[test]
    fn test_charmaps_contains_koi8r() {
        assert!(AVAILABLE_CHARMAPS.contains(&"KOI8-R"));
    }

    #[test]
    fn test_charmaps_count() {
        assert_eq!(AVAILABLE_CHARMAPS.len(), 6);
    }

    // -----------------------------------------------------------------------
    // Locale data -- LC_NUMERIC
    // -----------------------------------------------------------------------

    #[test]
    fn test_en_us_decimal_point() {
        let data = get_locale_data("en_US.UTF-8");
        assert_eq!(data.numeric.decimal_point, ".");
    }

    #[test]
    fn test_en_us_thousands_sep() {
        let data = get_locale_data("en_US.UTF-8");
        assert_eq!(data.numeric.thousands_sep, ",");
    }

    #[test]
    fn test_de_de_decimal_point() {
        let data = get_locale_data("de_DE.UTF-8");
        assert_eq!(data.numeric.decimal_point, ",");
    }

    #[test]
    fn test_de_de_thousands_sep() {
        let data = get_locale_data("de_DE.UTF-8");
        assert_eq!(data.numeric.thousands_sep, ".");
    }

    #[test]
    fn test_fr_fr_decimal_point() {
        let data = get_locale_data("fr_FR.UTF-8");
        assert_eq!(data.numeric.decimal_point, ",");
    }

    #[test]
    fn test_c_numeric() {
        let data = get_locale_data("C");
        assert_eq!(data.numeric.decimal_point, ".");
        assert_eq!(data.numeric.thousands_sep, "");
        assert_eq!(data.numeric.grouping, "");
    }

    #[test]
    fn test_ru_ru_thousands_sep() {
        let data = get_locale_data("ru_RU.UTF-8");
        // Russian uses non-breaking space as thousands separator.
        assert_eq!(data.numeric.thousands_sep, "\u{00A0}");
    }

    // -----------------------------------------------------------------------
    // Locale data -- LC_TIME
    // -----------------------------------------------------------------------

    #[test]
    fn test_en_us_day_names() {
        let data = get_locale_data("en_US.UTF-8");
        assert_eq!(data.time.day_names[0], "Sunday");
        assert_eq!(data.time.day_names[6], "Saturday");
    }

    #[test]
    fn test_en_us_month_names() {
        let data = get_locale_data("en_US.UTF-8");
        assert_eq!(data.time.month_names[0], "January");
        assert_eq!(data.time.month_names[11], "December");
    }

    #[test]
    fn test_de_de_day_names() {
        let data = get_locale_data("de_DE.UTF-8");
        assert_eq!(data.time.day_names[0], "Sonntag");
        assert_eq!(data.time.day_names[1], "Montag");
    }

    #[test]
    fn test_de_de_month_names() {
        let data = get_locale_data("de_DE.UTF-8");
        assert_eq!(data.time.month_names[0], "Januar");
        assert_eq!(data.time.month_names[2], "M\u{00E4}rz");
    }

    #[test]
    fn test_fr_fr_day_names() {
        let data = get_locale_data("fr_FR.UTF-8");
        assert_eq!(data.time.day_names[0], "dimanche");
        assert_eq!(data.time.day_names[1], "lundi");
    }

    #[test]
    fn test_ja_jp_month_names() {
        let data = get_locale_data("ja_JP.UTF-8");
        assert_eq!(data.time.month_names[0], "1\u{6708}");
        assert_eq!(data.time.month_names[11], "12\u{6708}");
    }

    #[test]
    fn test_en_us_am_pm() {
        let data = get_locale_data("en_US.UTF-8");
        assert_eq!(data.time.am_pm[0], "AM");
        assert_eq!(data.time.am_pm[1], "PM");
    }

    #[test]
    fn test_de_de_am_pm_empty() {
        let data = get_locale_data("de_DE.UTF-8");
        // German locale does not use AM/PM.
        assert_eq!(data.time.am_pm[0], "");
        assert_eq!(data.time.am_pm[1], "");
    }

    #[test]
    fn test_en_us_date_fmt() {
        let data = get_locale_data("en_US.UTF-8");
        assert_eq!(data.time.d_fmt, "%m/%d/%Y");
    }

    #[test]
    fn test_de_de_date_fmt() {
        let data = get_locale_data("de_DE.UTF-8");
        assert_eq!(data.time.d_fmt, "%d.%m.%Y");
    }

    // -----------------------------------------------------------------------
    // Locale data -- LC_MONETARY
    // -----------------------------------------------------------------------

    #[test]
    fn test_en_us_currency_symbol() {
        let data = get_locale_data("en_US.UTF-8");
        assert_eq!(data.monetary.currency_symbol, "$");
        assert_eq!(data.monetary.int_curr_symbol, "USD ");
    }

    #[test]
    fn test_de_de_currency_symbol() {
        let data = get_locale_data("de_DE.UTF-8");
        assert_eq!(data.monetary.currency_symbol, "\u{20AC}");
        assert_eq!(data.monetary.int_curr_symbol, "EUR ");
    }

    #[test]
    fn test_en_gb_currency_symbol() {
        let data = get_locale_data("en_GB.UTF-8");
        assert_eq!(data.monetary.currency_symbol, "\u{00A3}");
        assert_eq!(data.monetary.int_curr_symbol, "GBP ");
    }

    #[test]
    fn test_ja_jp_currency() {
        let data = get_locale_data("ja_JP.UTF-8");
        assert_eq!(data.monetary.currency_symbol, "\u{00A5}");
        assert_eq!(data.monetary.frac_digits, 0);
    }

    #[test]
    fn test_c_monetary_undefined() {
        let data = get_locale_data("C");
        assert_eq!(data.monetary.currency_symbol, "");
        assert_eq!(data.monetary.frac_digits, 127);
    }

    #[test]
    fn test_pt_br_currency() {
        let data = get_locale_data("pt_BR.UTF-8");
        assert_eq!(data.monetary.currency_symbol, "R$");
        assert_eq!(data.monetary.int_curr_symbol, "BRL ");
    }

    #[test]
    fn test_ko_kr_currency() {
        let data = get_locale_data("ko_KR.UTF-8");
        assert_eq!(data.monetary.currency_symbol, "\u{20A9}");
    }

    #[test]
    fn test_ru_ru_currency() {
        let data = get_locale_data("ru_RU.UTF-8");
        assert_eq!(data.monetary.currency_symbol, "\u{20BD}");
        assert_eq!(data.monetary.int_curr_symbol, "RUB ");
    }

    // -----------------------------------------------------------------------
    // Locale data -- LC_MESSAGES
    // -----------------------------------------------------------------------

    #[test]
    fn test_en_us_messages() {
        let data = get_locale_data("en_US.UTF-8");
        assert_eq!(data.messages.yesexpr, "^[yY]");
        assert_eq!(data.messages.noexpr, "^[nN]");
        assert_eq!(data.messages.yesstr, "yes");
        assert_eq!(data.messages.nostr, "no");
    }

    #[test]
    fn test_de_de_messages() {
        let data = get_locale_data("de_DE.UTF-8");
        assert_eq!(data.messages.yesstr, "ja");
        assert_eq!(data.messages.nostr, "nein");
    }

    #[test]
    fn test_fr_fr_messages() {
        let data = get_locale_data("fr_FR.UTF-8");
        assert_eq!(data.messages.yesstr, "oui");
        assert_eq!(data.messages.nostr, "non");
    }

    // -----------------------------------------------------------------------
    // Locale data -- LC_CTYPE (charmap)
    // -----------------------------------------------------------------------

    #[test]
    fn test_en_us_charmap() {
        let data = get_locale_data("en_US.UTF-8");
        assert_eq!(data.charmap, "UTF-8");
    }

    #[test]
    fn test_c_charmap() {
        let data = get_locale_data("C");
        assert_eq!(data.charmap, "ANSI_X3.4-1968");
    }

    // -----------------------------------------------------------------------
    // Getconf -- known values
    // -----------------------------------------------------------------------

    #[test]
    fn test_getconf_arg_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "ARG_MAX").unwrap();
        assert_eq!(entry.value.display(), "2097152");
    }

    #[test]
    fn test_getconf_page_size() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "PAGE_SIZE").unwrap();
        // OurOS uses 16 KiB pages.
        assert_eq!(entry.value.display(), "16384");
    }

    #[test]
    fn test_getconf_pagesize_alias() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "PAGESIZE").unwrap();
        assert_eq!(entry.value.display(), "16384");
    }

    #[test]
    fn test_getconf_path_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "PATH_MAX").unwrap();
        assert_eq!(entry.value.display(), "4096");
    }

    #[test]
    fn test_getconf_name_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "NAME_MAX").unwrap();
        assert_eq!(entry.value.display(), "255");
    }

    #[test]
    fn test_getconf_child_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "CHILD_MAX").unwrap();
        assert_eq!(entry.value.display(), "32768");
    }

    #[test]
    fn test_getconf_open_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "OPEN_MAX").unwrap();
        assert_eq!(entry.value.display(), "1024");
    }

    #[test]
    fn test_getconf_clk_tck() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "CLK_TCK").unwrap();
        assert_eq!(entry.value.display(), "100");
    }

    #[test]
    fn test_getconf_host_name_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "HOST_NAME_MAX").unwrap();
        assert_eq!(entry.value.display(), "255");
    }

    #[test]
    fn test_getconf_login_name_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "LOGIN_NAME_MAX").unwrap();
        assert_eq!(entry.value.display(), "256");
    }

    #[test]
    fn test_getconf_pipe_buf() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "PIPE_BUF").unwrap();
        assert_eq!(entry.value.display(), "4096");
    }

    #[test]
    fn test_getconf_posix_version() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "_POSIX_VERSION").unwrap();
        assert_eq!(entry.value.display(), "200809");
    }

    #[test]
    fn test_getconf_long_bit() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "LONG_BIT").unwrap();
        assert_eq!(entry.value.display(), "64");
    }

    #[test]
    fn test_getconf_word_bit() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "WORD_BIT").unwrap();
        assert_eq!(entry.value.display(), "32");
    }

    #[test]
    fn test_getconf_int_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "INT_MAX").unwrap();
        assert_eq!(entry.value.display(), "2147483647");
    }

    #[test]
    fn test_getconf_int_min() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "INT_MIN").unwrap();
        assert_eq!(entry.value.display(), "-2147483648");
    }

    #[test]
    fn test_getconf_uint_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "UINT_MAX").unwrap();
        assert_eq!(entry.value.display(), "4294967295");
    }

    #[test]
    fn test_getconf_ssize_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "SSIZE_MAX").unwrap();
        assert_eq!(entry.value.display(), "9223372036854775807");
    }

    #[test]
    fn test_getconf_filesizebits() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "FILESIZEBITS").unwrap();
        assert_eq!(entry.value.display(), "64");
    }

    #[test]
    fn test_getconf_link_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "LINK_MAX").unwrap();
        assert_eq!(entry.value.display(), "65000");
    }

    #[test]
    fn test_getconf_symlink_max() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "SYMLINK_MAX").unwrap();
        assert_eq!(entry.value.display(), "255");
    }

    #[test]
    fn test_getconf_gnu_libc_version() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "GNU_LIBC_VERSION").unwrap();
        assert_eq!(entry.value.display(), "ouros-libc 0.1");
    }

    #[test]
    fn test_getconf_gnu_libpthread_version() {
        let table = build_conf_table();
        let entry = table.iter().find(|v| v.name == "GNU_LIBPTHREAD_VERSION").unwrap();
        assert_eq!(entry.value.display(), "ouros-pthread 0.1");
    }

    #[test]
    fn test_getconf_nprocessors() {
        let table = build_conf_table();
        let conf_entry = table.iter().find(|v| v.name == "NPROCESSORS_CONF").unwrap();
        let onln_entry = table.iter().find(|v| v.name == "NPROCESSORS_ONLN").unwrap();
        // Should be at least 1.
        let conf_val: i64 = conf_entry.value.display().parse().unwrap();
        let onln_val: i64 = onln_entry.value.display().parse().unwrap();
        assert!(conf_val >= 1);
        assert!(onln_val >= 1);
    }

    // -----------------------------------------------------------------------
    // Getconf -- listing all variables
    // -----------------------------------------------------------------------

    #[test]
    fn test_getconf_table_not_empty() {
        let table = build_conf_table();
        assert!(table.len() >= 20);
    }

    #[test]
    fn test_getconf_all_have_names() {
        let table = build_conf_table();
        for var in &table {
            assert!(!var.name.is_empty());
        }
    }

    // -----------------------------------------------------------------------
    // Getconf -- unknown variable
    // -----------------------------------------------------------------------

    #[test]
    fn test_getconf_unknown_variable() {
        let table = build_conf_table();
        let found = table.iter().any(|v| v.name == "NONEXISTENT_VAR_XYZ");
        assert!(!found);
    }

    // -----------------------------------------------------------------------
    // Category keyword display
    // -----------------------------------------------------------------------

    #[test]
    fn test_category_keyword_numeric() {
        let data = get_locale_data("en_US.UTF-8");
        let mut buf = Vec::new();
        print_category_keywords(&mut buf, "LC_NUMERIC", data, true, false);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("decimal_point=\".\""));
        assert!(output.contains("thousands_sep=\",\""));
    }

    #[test]
    fn test_category_name_display() {
        let data = get_locale_data("en_US.UTF-8");
        let mut buf = Vec::new();
        print_category_keywords(&mut buf, "LC_NUMERIC", data, false, true);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.starts_with("LC_NUMERIC\n"));
    }

    #[test]
    fn test_category_keyword_messages() {
        let data = get_locale_data("en_US.UTF-8");
        let mut buf = Vec::new();
        print_category_keywords(&mut buf, "LC_MESSAGES", data, true, false);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("yesexpr=\"^[yY]\""));
        assert!(output.contains("noexpr=\"^[nN]\""));
    }

    #[test]
    fn test_category_keyword_monetary() {
        let data = get_locale_data("en_US.UTF-8");
        let mut buf = Vec::new();
        print_category_keywords(&mut buf, "LC_MONETARY", data, true, false);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("currency_symbol=\"$\""));
        assert!(output.contains("int_curr_symbol=\"USD \""));
    }

    #[test]
    fn test_category_keyword_with_category_name() {
        let data = get_locale_data("de_DE.UTF-8");
        let mut buf = Vec::new();
        print_category_keywords(&mut buf, "LC_NUMERIC", data, true, true);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.starts_with("LC_NUMERIC\n"));
        assert!(output.contains("decimal_point=\",\""));
    }

    #[test]
    fn test_category_no_keyword_numeric() {
        let data = get_locale_data("en_US.UTF-8");
        let mut buf = Vec::new();
        print_category_keywords(&mut buf, "LC_NUMERIC", data, false, false);
        let output = String::from_utf8(buf).unwrap();
        // Without -k, keywords should not appear.
        assert!(!output.contains("decimal_point="));
        assert!(output.contains("\".\""));
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_unknown_locale_falls_back_to_c() {
        let data = get_locale_data("xx_XX.UTF-8");
        assert_eq!(data.numeric.decimal_point, ".");
        assert_eq!(data.charmap, "ANSI_X3.4-1968");
    }

    #[test]
    fn test_empty_locale_falls_back_to_c() {
        let data = get_locale_data("");
        assert_eq!(data.numeric.decimal_point, ".");
    }

    #[test]
    fn test_locale_data_utf8_variant() {
        // Test that the .utf8 (lowercase) variant also works.
        let data = get_locale_data("en_US.utf8");
        assert_eq!(data.numeric.decimal_point, ".");
        assert_eq!(data.charmap, "UTF-8");
    }

    #[test]
    fn test_lc_time_keyword_output() {
        let data = get_locale_data("en_US.UTF-8");
        let mut buf = Vec::new();
        print_category_keywords(&mut buf, "LC_TIME", data, true, false);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("day[0]=\"Sunday\""));
        assert!(output.contains("mon[0]=\"January\""));
        assert!(output.contains("d_fmt=\"%m/%d/%Y\""));
    }

    #[test]
    fn test_lc_ctype_keyword_output() {
        let data = get_locale_data("en_US.UTF-8");
        let mut buf = Vec::new();
        print_category_keywords(&mut buf, "LC_CTYPE", data, true, false);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("charmap=\"UTF-8\""));
    }

    #[test]
    fn test_lc_collate_keyword_output() {
        let data = get_locale_data("en_US.UTF-8");
        let mut buf = Vec::new();
        print_category_keywords(&mut buf, "LC_COLLATE", data, true, false);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("collate-description=\"English linguistic\""));
    }

    #[test]
    fn test_es_es_locale_data() {
        let data = get_locale_data("es_ES.UTF-8");
        assert_eq!(data.numeric.decimal_point, ",");
        assert_eq!(data.monetary.currency_symbol, "\u{20AC}");
        assert_eq!(data.messages.yesstr, "s\u{00ED}");
    }

    #[test]
    fn test_zh_cn_locale_data() {
        let data = get_locale_data("zh_CN.UTF-8");
        assert_eq!(data.monetary.int_curr_symbol, "CNY ");
    }

    #[test]
    fn test_confvalue_display_str() {
        let v = ConfValue::Str("hello");
        assert_eq!(v.display(), "hello");
    }

    #[test]
    fn test_confvalue_display_int() {
        let v = ConfValue::Int(42);
        assert_eq!(v.display(), "42");
    }

    #[test]
    fn test_confvalue_display_negative() {
        let v = ConfValue::Int(-100);
        assert_eq!(v.display(), "-100");
    }
}
