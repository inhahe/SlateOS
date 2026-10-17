//! `<locale.h>`: the program's locale, and POSIX's locale objects.
//!
//! Two locales exist here, as in musl (design-decisions §1190, the operator's
//! answer to D-Q7):
//!
//! - **"C"** (also "POSIX"), where every program starts: each byte is one
//!   character, nothing is ever an encoding error -- the bytes 0x80-0xFF are
//!   the characters U+DF80-U+DFFF, musl's private range, so they round-trip
//!   -- `MB_CUR_MAX` is 1 and `nl_langinfo(CODESET)` says `ANSI_X3.4-1968`.
//! - **"C.UTF-8"**, the user's setting: text is UTF-8, `MB_CUR_MAX` is 4 and
//!   `CODESET` says `UTF-8`. `setlocale(LC_ALL, "")` gives it unless
//!   `LC_ALL`, the category's own variable or `LANG` names "C" or "POSIX".
//!
//! Only `LC_CTYPE` changes what the library does -- the encoding ([`utf8`]);
//! the other categories are named, set and reported, and behave as "C" in
//! both. A locale is therefore a set of categories that are "C.UTF-8", and a
//! `locale_t` is that set ([`Locale`]): nothing to allocate, so
//! `newlocale`, `duplocale` and `freelocale` cannot fail for want of memory.
//!
//! The calling thread's locale is the one `uselocale` gave it, else the
//! global one `setlocale` sets ([`current`]).
//!
//! Until 2026-10-17 every query answered "C" while the conversions worked
//! in UTF-8, so a program that asked how text was encoded was told ASCII.

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::perprocess::process_global;

/// Locale categories.
pub const LC_CTYPE: i32 = 0;
pub const LC_NUMERIC: i32 = 1;
pub const LC_TIME: i32 = 2;
pub const LC_COLLATE: i32 = 3;
pub const LC_MONETARY: i32 = 4;
pub const LC_MESSAGES: i32 = 5;
pub const LC_ALL: i32 = 6;

/// Numeric formatting information (lconv).
///
/// Matches the POSIX/C `struct lconv` layout.
#[repr(C)]
pub struct Lconv {
    pub decimal_point: *const u8,
    pub thousands_sep: *const u8,
    pub grouping: *const u8,
    pub int_curr_symbol: *const u8,
    pub currency_symbol: *const u8,
    pub mon_decimal_point: *const u8,
    pub mon_thousands_sep: *const u8,
    pub mon_grouping: *const u8,
    pub positive_sign: *const u8,
    pub negative_sign: *const u8,
    pub int_frac_digits: u8,
    pub frac_digits: u8,
    pub p_cs_precedes: u8,
    pub p_sep_by_space: u8,
    pub n_cs_precedes: u8,
    pub n_sep_by_space: u8,
    pub p_sign_posn: u8,
    pub n_sign_posn: u8,
}

// SAFETY: Lconv contains only *const u8 pointing to static string literals
// (which live for the entire program lifetime) and plain u8 values.
// The struct is immutable after initialization and safe to share.
unsafe impl Sync for Lconv {}

/// Static C locale lconv: both locales' numeric and monetary formats are
/// "C"'s.
static C_LCONV: Lconv = Lconv {
    decimal_point: c".".as_ptr().cast::<u8>(),
    thousands_sep: c"".as_ptr().cast::<u8>(),
    grouping: c"".as_ptr().cast::<u8>(),
    int_curr_symbol: c"".as_ptr().cast::<u8>(),
    currency_symbol: c"".as_ptr().cast::<u8>(),
    mon_decimal_point: c"".as_ptr().cast::<u8>(),
    mon_thousands_sep: c"".as_ptr().cast::<u8>(),
    mon_grouping: c"".as_ptr().cast::<u8>(),
    positive_sign: c"".as_ptr().cast::<u8>(),
    negative_sign: c"".as_ptr().cast::<u8>(),
    int_frac_digits: 127, // CHAR_MAX = "not available"
    frac_digits: 127,
    p_cs_precedes: 127,
    p_sep_by_space: 127,
    n_cs_precedes: 127,
    n_sep_by_space: 127,
    p_sign_posn: 127,
    n_sign_posn: 127,
};

// ---------------------------------------------------------------------------
// Locales
// ---------------------------------------------------------------------------

/// The categories `LC_ALL` is: `LC_CTYPE` .. `LC_MESSAGES`, in order.
const CATEGORIES: usize = 6;

/// The categories' environment variables, in category order.
const CATEGORY_VARS: [&[u8]; CATEGORIES] = [
    b"LC_CTYPE",
    b"LC_NUMERIC",
    b"LC_TIME",
    b"LC_COLLATE",
    b"LC_MONETARY",
    b"LC_MESSAGES",
];

/// A locale: the set of categories that are "C.UTF-8", bit `c` for category
/// `c`; the rest are "C".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locale(u8);

impl Locale {
    /// Every category "C": where a program starts.
    pub const C: Self = Self(0);
    /// Every category "C.UTF-8": what `setlocale(LC_ALL, "")` gives.
    pub const UTF8: Self = Self((1 << CATEGORIES) - 1);

    /// Whether category `c` is "C.UTF-8".
    #[must_use]
    pub const fn is_utf8(self, c: usize) -> bool {
        c < CATEGORIES && self.0 & (1 << c) != 0
    }

    /// This locale with category `c` "C.UTF-8" or not.
    #[must_use]
    const fn with(self, c: usize, utf8: bool) -> Self {
        if c >= CATEGORIES {
            self
        } else if utf8 {
            Self(self.0 | (1 << c))
        } else {
            Self(self.0 & !(1 << c))
        }
    }

    /// As a `locale_t`: never 0, which is `newlocale`'s failure, nor
    /// [`LC_GLOBAL_LOCALE`].
    #[must_use]
    pub const fn to_handle(self) -> LocaleT {
        LOCALE_TAG | self.0 as usize
    }

    /// The locale a `locale_t` from [`Locale::to_handle`] is; `None` for
    /// anything else, [`LC_GLOBAL_LOCALE`] included.
    #[must_use]
    pub const fn from_handle(h: LocaleT) -> Option<Self> {
        if h & !LOCALE_BITS == LOCALE_TAG {
            Some(Self((h & LOCALE_BITS) as u8))
        } else {
            None
        }
    }
}

/// A `locale_t`: a [`Locale`] with [`LOCALE_TAG`] set ([`Locale::to_handle`]),
/// or [`LC_GLOBAL_LOCALE`] where `uselocale` and `duplocale` take it.
pub type LocaleT = usize;

/// The global locale (special value for `uselocale`), musl's `(locale_t)-1`.
pub const LC_GLOBAL_LOCALE: LocaleT = usize::MAX;

/// What marks a `locale_t` as one of this library's: a value no pointer to
/// a locale object elsewhere could be, and not 0 or -1.
const LOCALE_TAG: LocaleT = 0x10C0_0000;
/// The bits of a `locale_t` that are its [`Locale`].
const LOCALE_BITS: LocaleT = (1 << CATEGORIES) - 1;

process_global! {
    /// The global locale, as `setlocale` sets it: a [`Locale`]'s bits. Every
    /// program starts in "C".
    fn global_locale() -> AtomicUsize = AtomicUsize::new(0);

    /// What `setlocale` last answered with a composite name, which a caller
    /// may read until the next `setlocale`, as C allows.
    fn composite_name() -> [u8; COMPOSITE_MAX] = [0; COMPOSITE_MAX];
}

/// The longest composite name, "C.UTF-8" six times with five ';' and a NUL.
const COMPOSITE_MAX: usize = 6 * 7 + 5 + 1;

/// The global locale.
fn global() -> Locale {
    // SAFETY: the accessor's pointer is valid for the process (the thread,
    // on the host); the value is only touched atomically.
    let bits = unsafe { (*global_locale()).load(Ordering::Relaxed) };
    Locale((bits & LOCALE_BITS) as u8)
}

/// Make `l` the global locale.
fn set_global(l: Locale) {
    // SAFETY: as in [`global`].
    unsafe { (*global_locale()).store(usize::from(l.0), Ordering::Relaxed) };
}

/// The calling thread's locale: the one `uselocale` gave it, else the
/// global one.
#[must_use]
pub fn current() -> Locale {
    // SAFETY: `perthread::current()` is this thread's block, which only
    // this thread writes.
    let mine = unsafe { (*crate::perthread::current()).locale };
    Locale::from_handle(mine).unwrap_or_else(global)
}

/// Whether text in the calling thread's locale is UTF-8 (`LC_CTYPE` is
/// "C.UTF-8"), as against "C"'s one byte a character: what every
/// multibyte conversion, `MB_CUR_MAX` and `CODESET` follow.
#[must_use]
pub fn utf8() -> bool {
    current().is_utf8(LC_CTYPE as usize)
}

/// The locale `loc` names for an `_l` function: a locale object's, or the
/// global one's for [`LC_GLOBAL_LOCALE`] -- and the calling thread's for
/// anything this library did not make.
#[must_use]
pub fn of_handle(loc: LocaleT) -> Locale {
    if loc == LC_GLOBAL_LOCALE {
        return global();
    }
    Locale::from_handle(loc).unwrap_or_else(current)
}

/// A category's name as `setlocale` takes it, read: `Some(true)` for a
/// UTF-8 locale -- "C.UTF-8", or any name whose codeset is UTF-8, as
/// "en_US.UTF-8" -- `Some(false)` for "C" and "POSIX", and `None` for a
/// locale this library does not have.
fn read_name(name: &[u8]) -> Option<bool> {
    match name {
        b"C" | b"POSIX" => return Some(false),
        b"C.UTF-8" | b"C.utf8" | b"POSIX.UTF-8" => return Some(true),
        _ => {}
    }
    let codeset = name
        .iter()
        .position(|&b| b == b'.')
        .and_then(|dot| name.get(dot.saturating_add(1)..))?;
    let codeset = codeset
        .iter()
        .position(|&b| b == b'@')
        .and_then(|at| codeset.get(..at))
        .unwrap_or(codeset);
    (codeset.eq_ignore_ascii_case(b"UTF-8") || codeset.eq_ignore_ascii_case(b"utf8"))
        .then_some(true)
}

/// What the environment asks for category `c`, as `setlocale(c, "")` reads
/// it: `LC_ALL`, then the category's own variable, then `LANG` -- the first
/// set and not empty. "C.UTF-8" (true) unless it names "C" or "POSIX".
fn from_environment(c: usize) -> bool {
    let var = CATEGORY_VARS.get(c).copied().unwrap_or(b"LC_ALL");
    let value = [b"LC_ALL".as_slice(), var, b"LANG"]
        .into_iter()
        .filter_map(crate::environ::getenv_bytes)
        .find(|v| !v.is_empty());
    !matches!(value, Some(b"C" | b"POSIX"))
}

/// `base` with the categories in `which` (bit `c` for category `c`) set as
/// `name` says: `""` from the environment, a name ([`read_name`]), or -- for
/// all six -- a composite name as `setlocale(LC_ALL, NULL)` gives it, the
/// categories' names in order, separated by ';'. `None` for a name this
/// library has no locale for, which changes nothing.
fn apply(base: Locale, which: u8, name: &[u8]) -> Option<Locale> {
    let mut l = base;
    if name.is_empty() {
        for c in (0..CATEGORIES).filter(|c| which & (1 << c) != 0) {
            l = l.with(c, from_environment(c));
        }
        return Some(l);
    }
    if name.contains(&b';') {
        if which != Locale::UTF8.0 {
            return None;
        }
        let mut parts = name.split(|&b| b == b';');
        for c in 0..CATEGORIES {
            l = l.with(c, read_name(parts.next()?)?);
        }
        return parts.next().is_none().then_some(l);
    }
    let utf8 = read_name(name)?;
    for c in (0..CATEGORIES).filter(|c| which & (1 << c) != 0) {
        l = l.with(c, utf8);
    }
    Some(l)
}

/// The name of category `c`'s locale in `l`.
fn category_name(l: Locale, c: usize) -> &'static core::ffi::CStr {
    if l.is_utf8(c) { c"C.UTF-8" } else { c"C" }
}

/// The name `setlocale` answers for `category` in `l`: the category's, or
/// for `LC_ALL` the one name all six share, else the composite of them,
/// written to the process's buffer ([`composite_name`]).
fn answer(l: Locale, category: usize) -> *const u8 {
    if category < CATEGORIES {
        return category_name(l, category).as_ptr().cast();
    }
    if l == Locale::C || l == Locale::UTF8 {
        return category_name(l, 0).as_ptr().cast();
    }
    // SAFETY: the process's buffer; `setlocale` is not thread-safe, as C
    // allows, so nothing else writes it while this does.
    let buf = unsafe { &mut *composite_name() };
    let mut at = 0usize;
    for c in 0..CATEGORIES {
        if c > 0 {
            if let Some(b) = buf.get_mut(at) {
                *b = b';';
            }
            at = at.saturating_add(1);
        }
        let name = category_name(l, c).to_bytes();
        let end = at.saturating_add(name.len());
        if let Some(dst) = buf.get_mut(at..end) {
            dst.copy_from_slice(name);
        }
        at = end;
    }
    if let Some(b) = buf.get_mut(at) {
        *b = 0;
    }
    buf.as_ptr()
}

/// The bytes of the C string at `p`, without its NUL.
///
/// # Safety
///
/// `p` is NUL-terminated.
unsafe fn c_bytes<'a>(p: *const u8) -> &'a [u8] {
    // SAFETY: the caller's contract.
    unsafe { core::ffi::CStr::from_ptr(p.cast()) }.to_bytes()
}

/// Set or query the program's global locale.
///
/// `locale` NULL asks: the category's name, or `LC_ALL`'s. `""` takes the
/// user's setting from the environment ([`from_environment`]); "C" and
/// "POSIX" are one byte a character; "C.UTF-8" -- or any name whose codeset
/// is UTF-8 -- is UTF-8; `LC_ALL` also takes a composite name back. Anything
/// else, or a category that is not one, is NULL, changing nothing.
///
/// # Safety
///
/// `locale` is NULL or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn setlocale(category: i32, locale: *const u8) -> *const u8 {
    let Ok(c) = usize::try_from(category) else {
        return core::ptr::null();
    };
    if c > CATEGORIES {
        return core::ptr::null();
    }
    let now = global();
    if locale.is_null() {
        return answer(now, c);
    }
    let which = if c == CATEGORIES {
        Locale::UTF8.0
    } else {
        1 << c
    };
    // SAFETY: the caller's contract.
    let name = unsafe { c_bytes(locale) };
    match apply(now, which, name) {
        Some(l) => {
            set_global(l);
            answer(l, c)
        }
        None => core::ptr::null(),
    }
}

/// Get numeric formatting information: "C"'s, in both locales.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn localeconv() -> *const Lconv {
    &raw const C_LCONV
}

// ---------------------------------------------------------------------------
// POSIX 2008 locale objects
// ---------------------------------------------------------------------------

/// A locale object: `base`'s locale -- "C" for `(locale_t)0` -- with the
/// categories in `category_mask` set as `locale` names them (as
/// `setlocale`). 0 with `errno` `ENOENT` for a name this library has no
/// locale for, `EINVAL` for a NULL name or a `base` that is not a locale
/// object. Nothing is allocated, so `base` need not be freed.
///
/// # Safety
///
/// `locale` is NULL or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn newlocale(
    category_mask: i32,
    locale: *const u8,
    base: LocaleT,
) -> LocaleT {
    if locale.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return 0;
    }
    let from = if base == 0 {
        Some(Locale::C)
    } else {
        Locale::from_handle(base)
    };
    let Some(from) = from else {
        crate::errno::set_errno(crate::errno::EINVAL);
        return 0;
    };
    // The six categories' bits; musl's LC_ALL_MASK sets them all, and more.
    let which = (category_mask.cast_unsigned() & u32::from(Locale::UTF8.0)) as u8;
    // SAFETY: the caller's contract.
    let name = unsafe { c_bytes(locale) };
    match apply(from, which, name) {
        Some(l) => l.to_handle(),
        None => {
            crate::errno::set_errno(crate::errno::ENOENT);
            0
        }
    }
}

/// A copy of the locale object `locobj` -- of the global locale for
/// [`LC_GLOBAL_LOCALE`] -- or 0 with `errno` `EINVAL` for anything else.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn duplocale(locobj: LocaleT) -> LocaleT {
    if locobj == LC_GLOBAL_LOCALE {
        return global().to_handle();
    }
    if Locale::from_handle(locobj).is_some() {
        locobj
    } else {
        crate::errno::set_errno(crate::errno::EINVAL);
        0
    }
}

/// Free a locale object: nothing to free (the module's doc).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn freelocale(_locobj: LocaleT) {}

/// Make `newloc` the calling thread's locale -- [`LC_GLOBAL_LOCALE`] for the
/// global one again, 0 to only ask -- and answer the one it had:
/// [`LC_GLOBAL_LOCALE`] when it had none of its own. 0 with `errno`
/// `EINVAL` for anything that is not a locale object, changing nothing.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn uselocale(newloc: LocaleT) -> LocaleT {
    let block = crate::perthread::current();
    // SAFETY: this thread's block, which only this thread writes.
    let mine = unsafe { (*block).locale };
    let previous = if Locale::from_handle(mine).is_some() {
        mine
    } else {
        LC_GLOBAL_LOCALE
    };
    if newloc != 0 {
        let set = if newloc == LC_GLOBAL_LOCALE {
            0
        } else if Locale::from_handle(newloc).is_some() {
            newloc
        } else {
            crate::errno::set_errno(crate::errno::EINVAL);
            return 0;
        };
        // SAFETY: as above.
        unsafe { (*block).locale = set };
    }
    previous
}

/// Make the calling thread's locale UTF-8 until the guard goes: for the
/// tests that are about UTF-8 text, as every program that handles text asks
/// for it with `setlocale(LC_ALL, "")`.
#[cfg(test)]
pub(crate) struct Utf8Guard(LocaleT);

#[cfg(test)]
impl Utf8Guard {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self(uselocale(Locale::UTF8.to_handle()))
    }
}

#[cfg(test)]
impl Drop for Utf8Guard {
    fn drop(&mut self) {
        uselocale(self.0);
    }
}

// ---------------------------------------------------------------------------
// Category masks for newlocale
// ---------------------------------------------------------------------------

/// Mask for `LC_CTYPE`.
pub const LC_CTYPE_MASK: i32 = 1 << LC_CTYPE;
/// Mask for `LC_NUMERIC`.
pub const LC_NUMERIC_MASK: i32 = 1 << LC_NUMERIC;
/// Mask for `LC_TIME`.
pub const LC_TIME_MASK: i32 = 1 << LC_TIME;
/// Mask for `LC_COLLATE`.
pub const LC_COLLATE_MASK: i32 = 1 << LC_COLLATE;
/// Mask for `LC_MONETARY`.
pub const LC_MONETARY_MASK: i32 = 1 << LC_MONETARY;
/// Mask for `LC_MESSAGES`.
pub const LC_MESSAGES_MASK: i32 = 1 << LC_MESSAGES;
/// Every category: musl's `0x7fffffff`, as a C program's `<locale.h>` has it
/// -- all bits, not just the six categories there are (it was `(1 << LC_ALL)
/// - 1`, 63, until 2026-09-27).  `newlocale` takes any mask.
pub const LC_ALL_MASK: i32 = 0x7fff_ffff;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes of the name `setlocale` answered.
    fn named(p: *const u8) -> &'static [u8] {
        assert!(!p.is_null());
        // SAFETY: setlocale answers a NUL-terminated name.
        unsafe { c_bytes(p) }
    }

    fn set(category: i32, name: &core::ffi::CStr) -> *const u8 {
        // SAFETY: a C string.
        unsafe { setlocale(category, name.as_ptr().cast()) }
    }

    fn query(category: i32) -> &'static [u8] {
        // SAFETY: NULL asks.
        named(unsafe { setlocale(category, core::ptr::null()) })
    }

    fn env(name: &core::ffi::CStr, value: Option<&core::ffi::CStr>) {
        // SAFETY: C strings; the host environment is this test thread's.
        let rc = unsafe {
            match value {
                Some(v) => crate::environ::setenv(name.as_ptr().cast(), v.as_ptr().cast(), 1),
                None => crate::environ::unsetenv(name.as_ptr().cast()),
            }
        };
        assert_eq!(rc, 0);
    }

    /// A program starts in "C", one byte a character, until it asks.
    #[test]
    fn a_program_starts_in_c() {
        assert_eq!(query(LC_ALL), b"C");
        assert_eq!(query(LC_CTYPE), b"C");
        assert!(!utf8());
        assert_eq!(current(), Locale::C);
    }

    /// `setlocale(LC_ALL, "")` is "C.UTF-8" unless the environment names
    /// "C" or "POSIX" -- `LC_ALL` first, then the category's variable, then
    /// `LANG`; an empty value is as an unset one.
    #[test]
    fn the_users_setting_is_c_utf8_unless_the_environment_says_c() {
        for v in [c"LC_ALL", c"LC_CTYPE", c"LC_NUMERIC", c"LANG"] {
            env(v, None);
        }
        assert_eq!(named(set(LC_ALL, c"")), b"C.UTF-8");
        assert!(utf8());

        env(c"LANG", Some(c"C"));
        assert_eq!(named(set(LC_ALL, c"")), b"C");
        assert!(!utf8());

        // The category's own variable comes before LANG ...
        env(c"LC_CTYPE", Some(c"en_US.UTF-8"));
        assert_eq!(named(set(LC_ALL, c"")), b"C.UTF-8;C;C;C;C;C");
        assert!(utf8());
        assert_eq!(query(LC_NUMERIC), b"C");

        // ... and LC_ALL before both; empty is unset.
        env(c"LC_ALL", Some(c"POSIX"));
        assert_eq!(named(set(LC_ALL, c"")), b"C");
        env(c"LC_ALL", Some(c""));
        assert_eq!(named(set(LC_CTYPE, c"")), b"C.UTF-8");
        assert!(utf8());

        set(LC_ALL, c"C");
        for v in [c"LC_ALL", c"LC_CTYPE", c"LANG"] {
            env(v, None);
        }
    }

    /// Names: "C" and "POSIX", "C.UTF-8" and any name whose codeset is
    /// UTF-8; anything else is NULL and changes nothing. A composite name
    /// goes back in as it came out.
    #[test]
    fn names_are_read_and_a_composite_round_trips() {
        assert_eq!(named(set(LC_ALL, c"POSIX")), b"C");
        assert_eq!(named(set(LC_ALL, c"C.utf8")), b"C.UTF-8");
        assert_eq!(named(set(LC_ALL, c"de_DE.utf-8@euro")), b"C.UTF-8");
        assert!(set(LC_ALL, c"de_DE.ISO-8859-1").is_null());
        assert!(set(LC_ALL, c"en_US").is_null());
        assert!(set(7, c"C").is_null());
        assert!(set(-1, c"C").is_null());
        assert_eq!(query(LC_ALL), b"C.UTF-8", "a refused name changed it");

        assert_eq!(named(set(LC_NUMERIC, c"C")), b"C");
        let composite = query(LC_ALL).to_vec();
        assert_eq!(composite, b"C.UTF-8;C;C.UTF-8;C.UTF-8;C.UTF-8;C.UTF-8");
        set(LC_ALL, c"C");
        let mut back = composite.clone();
        back.push(0);
        // SAFETY: NUL-terminated.
        assert!(!unsafe { setlocale(LC_ALL, back.as_ptr()) }.is_null());
        assert_eq!(query(LC_ALL), composite.as_slice());
        assert!(set(LC_ALL, c"C;C").is_null());
        assert!(set(LC_CTYPE, c"C;C;C;C;C;C").is_null());
        set(LC_ALL, c"C");
    }

    /// A locale object is the locale it was made as, and `uselocale` makes
    /// it the thread's alone, answering what the thread had.
    #[test]
    fn a_thread_can_have_its_own_locale() {
        // SAFETY: C strings.
        let utf8_obj = unsafe { newlocale(LC_CTYPE_MASK, c"C.UTF-8".as_ptr().cast(), 0) };
        assert_eq!(Locale::from_handle(utf8_obj), Some(Locale::C.with(0, true)));
        assert_eq!(uselocale(utf8_obj), LC_GLOBAL_LOCALE);
        assert!(utf8());
        assert_eq!(
            query(LC_CTYPE),
            b"C",
            "the global locale is not the thread's"
        );
        assert_eq!(uselocale(0), utf8_obj, "0 only asks");
        assert_eq!(uselocale(LC_GLOBAL_LOCALE), utf8_obj);
        assert!(!utf8());

        // From a base, one category at a time; duplocale; the refusals.
        // SAFETY: C strings.
        let both = unsafe { newlocale(LC_TIME_MASK, c"C.UTF-8".as_ptr().cast(), utf8_obj) };
        assert_eq!(
            Locale::from_handle(both),
            Some(Locale::C.with(0, true).with(2, true))
        );
        assert_eq!(duplocale(both), both);
        assert_eq!(duplocale(LC_GLOBAL_LOCALE), Locale::C.to_handle());
        // SAFETY: C strings.
        assert_eq!(
            unsafe { newlocale(LC_ALL_MASK, c"xx_YY".as_ptr().cast(), 0) },
            0
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
        assert_eq!(unsafe { newlocale(LC_ALL_MASK, core::ptr::null(), 0) }, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        assert_eq!(uselocale(12345), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        assert_eq!(duplocale(12345), 0);
        assert_eq!(of_handle(both), Locale::C.with(0, true).with(2, true));
        freelocale(both);
    }

    // -- localeconv --

    #[test]
    fn localeconv_is_cs() {
        let lc = localeconv();
        assert!(!lc.is_null());
        // SAFETY: a static struct.
        let lconv = unsafe { &*lc };
        // SAFETY: static C strings.
        unsafe {
            assert_eq!(*lconv.decimal_point, b'.');
            assert_eq!(*lconv.thousands_sep, 0);
            assert_eq!(*lconv.currency_symbol, 0);
            assert_eq!(*lconv.int_curr_symbol, 0);
        }
        for v in [
            lconv.int_frac_digits,
            lconv.frac_digits,
            lconv.p_cs_precedes,
            lconv.p_sep_by_space,
            lconv.n_cs_precedes,
            lconv.n_sep_by_space,
            lconv.p_sign_posn,
            lconv.n_sign_posn,
        ] {
            assert_eq!(v, 127); // CHAR_MAX: not available
        }
        assert!(core::mem::size_of::<Lconv>() >= 88);
    }

    // -- Constants --

    #[test]
    fn categories_and_masks_are_glibcs_and_musls() {
        assert_eq!(
            [
                LC_CTYPE,
                LC_NUMERIC,
                LC_TIME,
                LC_COLLATE,
                LC_MONETARY,
                LC_MESSAGES,
                LC_ALL
            ],
            [0, 1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            [
                LC_CTYPE_MASK,
                LC_NUMERIC_MASK,
                LC_TIME_MASK,
                LC_COLLATE_MASK,
                LC_MONETARY_MASK,
                LC_MESSAGES_MASK
            ],
            [1, 2, 4, 8, 16, 32]
        );
        assert_eq!(LC_ALL_MASK, 0x7fff_ffff); // musl's, probed 2026-09-27
        assert_eq!(LC_GLOBAL_LOCALE, usize::MAX);
    }
}
