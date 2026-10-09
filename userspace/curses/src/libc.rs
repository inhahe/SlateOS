//! The locale, as the C library answers for it: the [`Ctype`] a screen set
//! up by [`crate::initscr`] uses.
//!
//! Through `libcall::locale`, so that what a curses program counts as
//! printable, how wide it takes a character to be and how it writes one is
//! what every C program in the same locale would.

use crate::addch::Ctype;
use crate::cell::WChar;
use libcall::locale::{self, Mb, MbState};

/// The C library's answers.
#[derive(Clone, Copy, Debug, Default)]
pub struct Libc;

impl Ctype for Libc {
    fn mbrtowc(&self, bytes: &[u8]) -> (i32, WChar) {
        // `init_mb (state)`: every call starts from the initial state.
        match locale::mbrtowc(bytes, &mut MbState::new()) {
            Mb::Char(wc, n) => (i32::try_from(n).unwrap_or(i32::MAX), wc),
            Mb::Incomplete => (-2, 0),
            Mb::Invalid => (-1, 0),
        }
    }

    fn wcwidth(&self, wc: WChar) -> i32 {
        locale::wcwidth(wc)
    }

    fn iswprint(&self, wc: WChar) -> bool {
        locale::iswprint(wc)
    }

    fn isprint(&self, c: i32) -> bool {
        locale::isprint(c)
    }

    fn iscntrl(&self, c: i32) -> bool {
        locale::iscntrl(c)
    }

    fn wctob(&self, wc: WChar) -> i32 {
        locale::wctob(wc)
    }

    fn btowc(&self, c: i32) -> u32 {
        locale::btowc(c)
    }

    fn wcrtomb(&self, wc: WChar) -> Option<Vec<u8>> {
        let mut buf = [0u8; locale::MB_LEN_MAX];
        let n = locale::wcrtomb(wc, &mut MbState::new(), &mut buf)?;
        buf.get(..n).map(<[u8]>::to_vec)
    }

    /// `nl_langinfo (CODESET)` is `UTF-8`.
    fn unicode_locale(&self) -> bool {
        let mut buf = [0u8; 64];
        locale::codeset(&mut buf).is_some_and(|n| buf.get(..n) == Some(&b"UTF-8"[..]))
    }

    /// `_nc_get_locale ()` -- `setlocale (LC_CTYPE, NULL)` -- compared as
    /// `_nc_setupscreen` compares it.
    fn legacy_locale(&self) -> bool {
        let mut buf = [0u8; 256];
        match locale::current(locale::LC_CTYPE, &mut buf) {
            None => true,
            Some(n) => matches!(buf.get(..n), Some(b"C" | b"POSIX")),
        }
    }
}
