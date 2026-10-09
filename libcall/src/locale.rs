//! The C library's locale: selecting one from the environment, asking which
//! is selected and what its codeset is, and the character functions that
//! follow it -- `mbrtowc`, `wcrtomb`, `wcwidth`, `iswprint`, `isprint`,
//! `iscntrl`.
//!
//! Through the C library rather than tables of our own (design-decisions
//! §768), because these answers are the locale's: a curses program decides
//! what is printable, how many columns a character takes and how to write
//! it from them, and has to decide as every C program on the same system
//! does. glibc answers from its locale files; SlateOS's C library answers
//! `wcwidth` from the one width table (`userspace/charwidth`) and the rest
//! from its own tables.
//!
//! Nothing here allocates: a name comes back copied into the caller's
//! buffer, because `setlocale`'s own is static storage the next call
//! overwrites.
//!
//! On a host with no C library of ours (the Windows build of the unit
//! tests) the locale is always `C`: ASCII, one byte to a character, and
//! nothing past 0x7f valid or printable -- glibc's `C` locale.

/// `LC_CTYPE`.
pub const LC_CTYPE: i32 = 0;
/// `LC_ALL`.
pub const LC_ALL: i32 = 6;
/// `CODESET`, `nl_langinfo`'s item for the codeset's name.
pub const CODESET: i32 = 14;

/// `MB_LEN_MAX` in glibc: the most bytes one character takes in any locale.
pub const MB_LEN_MAX: usize = 16;

/// `mbstate_t`: the state of a conversion between multibyte and wide
/// characters, 8 bytes in glibc and in ours. Start from [`MbState::new`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MbState {
    opaque: [u8; 8],
}

impl MbState {
    /// The initial state.
    #[must_use]
    pub const fn new() -> Self {
        Self { opaque: [0; 8] }
    }
}

/// What `mbrtowc` made of some bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mb {
    /// A whole character, from this many bytes (0 for the NUL character).
    Char(i32, usize),
    /// The bytes begin a character but do not finish it (`(size_t) -2`).
    Incomplete,
    /// The bytes are no character (`(size_t) -1`, `EILSEQ`).
    Invalid,
}

#[cfg(unix)]
mod sys {
    use super::MbState;

    unsafe extern "C" {
        pub fn setlocale(category: i32, locale: *const u8) -> *const u8;
        pub fn nl_langinfo(item: i32) -> *const u8;
        pub fn mbrtowc(pwc: *mut i32, s: *const u8, n: usize, ps: *mut MbState) -> usize;
        pub fn wcrtomb(s: *mut u8, wc: i32, ps: *mut MbState) -> usize;
        pub fn wcwidth(wc: i32) -> i32;
        pub fn iswprint(wc: u32) -> i32;
        pub fn isprint(c: i32) -> i32;
        pub fn iscntrl(c: i32) -> i32;
        pub fn wctob(wc: u32) -> i32;
        pub fn btowc(c: i32) -> u32;
        pub fn __ctype_get_mb_cur_max() -> usize;
    }
}

/// A C string the library returned, copied into `buf` without its NUL:
/// the length, or `None` when it was NULL or does not fit.
#[cfg(unix)]
fn copy_c_string(p: *const u8, buf: &mut [u8]) -> Option<usize> {
    if p.is_null() {
        return None;
    }
    // SAFETY: the C library returned a NUL-terminated string, which stays
    // valid until the next call that may overwrite it; it is copied out
    // before anything else is called.
    let s = unsafe { core::ffi::CStr::from_ptr(p.cast()) }.to_bytes();
    let dst = buf.get_mut(..s.len())?;
    dst.copy_from_slice(s);
    Some(s.len())
}

/// `setlocale (category, "")`: the locale the environment names selected
/// for `category` -- `LC_ALL`, the category's own variable, `LANG`, in that
/// order -- and its name copied into `buf`. `None` when the named locale does
/// not exist, which leaves the selection as it was.
pub fn select_from_env(category: i32, buf: &mut [u8]) -> Option<usize> {
    #[cfg(unix)]
    {
        // SAFETY: an empty C string asks for the environment's selection;
        // the result is copied out at once.
        let p = unsafe { sys::setlocale(category, c"".as_ptr().cast()) };
        copy_c_string(p, buf)
    }
    #[cfg(not(unix))]
    {
        let _ = category;
        name_c(buf)
    }
}

/// `setlocale (category, NULL)`: the name of the locale selected for
/// `category`, copied into `buf`.
pub fn current(category: i32, buf: &mut [u8]) -> Option<usize> {
    #[cfg(unix)]
    {
        // SAFETY: a NULL locale only asks; the result is copied out at once.
        let p = unsafe { sys::setlocale(category, core::ptr::null()) };
        copy_c_string(p, buf)
    }
    #[cfg(not(unix))]
    {
        let _ = category;
        name_c(buf)
    }
}

/// `"C"`, the only locale the host build has.
#[cfg(not(unix))]
fn name_c(buf: &mut [u8]) -> Option<usize> {
    let dst = buf.get_mut(..1)?;
    dst.copy_from_slice(b"C");
    Some(1)
}

/// `nl_langinfo (CODESET)`: the selected `LC_CTYPE`'s codeset, copied
/// into `buf` -- `UTF-8`, or `ANSI_X3.4-1968` for glibc's `C`.
pub fn codeset(buf: &mut [u8]) -> Option<usize> {
    #[cfg(unix)]
    {
        // SAFETY: `CODESET` is a valid item; the result is copied out at once.
        let p = unsafe { sys::nl_langinfo(CODESET) };
        copy_c_string(p, buf)
    }
    #[cfg(not(unix))]
    {
        const ASCII: &[u8] = b"ANSI_X3.4-1968";
        let dst = buf.get_mut(..ASCII.len())?;
        dst.copy_from_slice(ASCII);
        Some(ASCII.len())
    }
}

/// `MB_CUR_MAX`: the most bytes a character takes in the selected locale.
#[must_use]
pub fn mb_cur_max() -> usize {
    #[cfg(unix)]
    {
        // SAFETY: no arguments; it reads the selected locale.
        unsafe { sys::__ctype_get_mb_cur_max() }
    }
    #[cfg(not(unix))]
    {
        1
    }
}

/// `mbrtowc (&wc, s, s.len (), state)`.
pub fn mbrtowc(s: &[u8], state: &mut MbState) -> Mb {
    #[cfg(unix)]
    {
        let mut wc: i32 = 0;
        // SAFETY: `s` is readable for its length, which is what is passed;
        // `wc` and `state` are ours and writable.
        let n = unsafe { sys::mbrtowc(&raw mut wc, s.as_ptr(), s.len(), state) };
        match n {
            usize::MAX => Mb::Invalid,
            n if n == usize::MAX - 1 => Mb::Incomplete,
            n => Mb::Char(wc, n),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = state;
        match s.first() {
            None => Mb::Incomplete,
            Some(&0) => Mb::Char(0, 0),
            Some(&b) if b < 0x80 => Mb::Char(i32::from(b), 1),
            Some(_) => Mb::Invalid,
        }
    }
}

/// `wcrtomb (buf, wc, state)`: the character's bytes in `buf`, how many,
/// or `None` when the locale cannot write it (`EILSEQ`).
pub fn wcrtomb(wc: i32, state: &mut MbState, buf: &mut [u8; MB_LEN_MAX]) -> Option<usize> {
    #[cfg(unix)]
    {
        // SAFETY: `buf` holds `MB_LEN_MAX` bytes, the most any character
        // takes; `state` is ours and writable.
        let n = unsafe { sys::wcrtomb(buf.as_mut_ptr(), wc, state) };
        if n == usize::MAX { None } else { Some(n) }
    }
    #[cfg(not(unix))]
    {
        let _ = state;
        let b = u8::try_from(wc).ok().filter(|b| *b < 0x80)?;
        buf[0] = b;
        Some(1)
    }
}

/// `wcwidth (wc)`: the columns the character takes, -1 for none it can show.
#[must_use]
pub fn wcwidth(wc: i32) -> i32 {
    #[cfg(unix)]
    {
        // SAFETY: a plain value in, a plain value out.
        unsafe { sys::wcwidth(wc) }
    }
    #[cfg(not(unix))]
    {
        match wc {
            0 => 0,
            0x20..=0x7e => 1,
            _ => -1,
        }
    }
}

/// `iswprint ((wint_t) wc)`.
#[must_use]
pub fn iswprint(wc: i32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: a plain value in, a plain value out.
        unsafe { sys::iswprint(wc.cast_unsigned()) != 0 }
    }
    #[cfg(not(unix))]
    {
        (0x20..=0x7e).contains(&wc)
    }
}

/// `isprint (c)`, for `c` an `unsigned char` or `EOF`.
#[must_use]
pub fn isprint(c: i32) -> bool {
    if !(-1..=255).contains(&c) {
        // The C function's behaviour is undefined there; nothing is printable.
        return false;
    }
    #[cfg(unix)]
    {
        // SAFETY: `c` is in the range the function is defined for.
        unsafe { sys::isprint(c) != 0 }
    }
    #[cfg(not(unix))]
    {
        (0x20..=0x7e).contains(&c)
    }
}

/// `iscntrl (c)`, for `c` an `unsigned char` or `EOF`.
#[must_use]
pub fn iscntrl(c: i32) -> bool {
    if !(-1..=255).contains(&c) {
        return false;
    }
    #[cfg(unix)]
    {
        // SAFETY: `c` is in the range the function is defined for.
        unsafe { sys::iscntrl(c) != 0 }
    }
    #[cfg(not(unix))]
    {
        (0..0x20).contains(&c) || c == 0x7f
    }
}

/// `EOF`.
pub const EOF: i32 = -1;
/// `WEOF`.
pub const WEOF: u32 = u32::MAX;

/// `wctob ((wint_t) wc)`: the one byte that is the character in the
/// selected locale, or [`EOF`] when it takes more or none.
#[must_use]
pub fn wctob(wc: i32) -> i32 {
    #[cfg(unix)]
    {
        // SAFETY: a plain value in, a plain value out.
        unsafe { sys::wctob(wc.cast_unsigned()) }
    }
    #[cfg(not(unix))]
    {
        if (0..0x80).contains(&wc) { wc } else { EOF }
    }
}

/// `btowc (c)`: the character the one byte `c` is in the selected locale,
/// or [`WEOF`] when it is none on its own.
#[must_use]
pub fn btowc(c: i32) -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: a plain value in, a plain value out.
        unsafe { sys::btowc(c) }
    }
    #[cfg(not(unix))]
    {
        if (0..0x80).contains(&c) {
            c.cast_unsigned()
        } else {
            WEOF
        }
    }
}

#[cfg(test)]
mod tests {
    // A test that fails should say where; the defensive lints are for code
    // that runs on a user's data.
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn the_numbers_are_the_librarys() {
        assert_eq!(LC_CTYPE, posix::locale::LC_CTYPE);
        assert_eq!(LC_ALL, posix::locale::LC_ALL);
        assert_eq!(CODESET, posix::langinfo::CODESET);
        assert_eq!(
            core::mem::size_of::<MbState>(),
            core::mem::size_of::<posix::wchar::MbstateT>()
        );
    }

    /// Without `setlocale (LC_ALL, "")` a program is in the `C` locale:
    /// ASCII alone, one byte to a character.
    #[test]
    fn a_program_starts_in_c() {
        let mut name = [0u8; 64];
        let n = current(LC_CTYPE, &mut name).expect("a name");
        assert_eq!(name.get(..n), Some(&b"C"[..]));
        let mut state = MbState::new();
        assert_eq!(mbrtowc(b"A", &mut state), Mb::Char(0x41, 1));
        assert_eq!(mbrtowc(b"\0", &mut state), Mb::Char(0, 0));
        assert_eq!(mbrtowc(b"\xe9", &mut MbState::new()), Mb::Invalid);
        assert_eq!(mb_cur_max(), 1);
        let mut buf = [0u8; MB_LEN_MAX];
        assert_eq!(wcrtomb(0x41, &mut MbState::new(), &mut buf), Some(1));
        assert_eq!(buf[0], 0x41);
        assert_eq!(wcrtomb(0xe9, &mut MbState::new(), &mut buf), None);
        assert_eq!(wcwidth(0x41), 1);
        assert!(iswprint(0x41) && !iswprint(0x7));
        assert!(isprint(0x41) && !isprint(0x7) && !isprint(0xe9));
        assert!(iscntrl(0x7) && iscntrl(0x7f) && !iscntrl(0x41));
        assert!(!isprint(300) && !iscntrl(-2));
        assert_eq!((wctob(0x41), wctob(0xe9)), (0x41, EOF));
        assert_eq!((btowc(0x41), btowc(0xe9)), (0x41, WEOF));
    }
}
