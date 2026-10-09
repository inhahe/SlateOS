//! What the tests of several modules share: a locale of their own, so that
//! no test depends on the locale of the process running it.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use crate::addch::Ctype;
use crate::cell::WChar;

/// A UTF-8 locale for the tests: Rust's own decoding, ASCII control
/// characters, U+4E00..U+9FFF two columns and U+0300..U+036F none.
pub struct Utf8;

impl Ctype for Utf8 {
    fn mbrtowc(&self, bytes: &[u8]) -> (i32, WChar) {
        match bytes.first() {
            None => (-2, 0),
            Some(0) => (0, 0),
            Some(&b) => {
                let need = match b {
                    0x00..=0x7f => 1,
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => return (-1, 0),
                };
                if bytes.len() < need {
                    return (-2, 0);
                }
                match std::str::from_utf8(&bytes[..need]) {
                    Ok(s) => (
                        i32::try_from(need).unwrap(),
                        u32::from(s.chars().next().unwrap()).cast_signed(),
                    ),
                    Err(_) => (-1, 0),
                }
            }
        }
    }

    fn wcwidth(&self, wc: WChar) -> i32 {
        match wc {
            0 => 0,
            0x300..=0x36f => 0,
            0x4e00..=0x9fff => 2,
            c if c < 0x20 || c == 0x7f => -1,
            _ => 1,
        }
    }

    fn iswprint(&self, wc: WChar) -> bool {
        self.wcwidth(wc) >= 0 && wc >= 0x20
    }

    fn isprint(&self, c: i32) -> bool {
        (0x20..0x7f).contains(&c)
    }

    fn iscntrl(&self, c: i32) -> bool {
        (0..0x20).contains(&c) || c == 0x7f
    }

    fn wctob(&self, wc: WChar) -> i32 {
        if (0..0x80).contains(&wc) { wc } else { -1 }
    }

    fn btowc(&self, c: i32) -> u32 {
        if (0..0x80).contains(&c) {
            c.cast_unsigned()
        } else {
            u32::MAX
        }
    }

    fn wcrtomb(&self, wc: WChar) -> Option<Vec<u8>> {
        char::from_u32(wc.cast_unsigned()).map(|c| c.to_string().into_bytes())
    }

    fn unicode_locale(&self) -> bool {
        true
    }

    fn legacy_locale(&self) -> bool {
        false
    }
}

/// The `C` locale, as glibc answers for it: bytes are characters, and only
/// ASCII is printable.
pub struct CLocale;

impl Ctype for CLocale {
    fn mbrtowc(&self, bytes: &[u8]) -> (i32, WChar) {
        match bytes.first() {
            None => (-2, 0),
            Some(0) => (0, 0),
            // glibc's C locale is ASCII: a byte above 0x7f is no character.
            Some(&b) if b > 0x7f => (-1, 0),
            Some(&b) => (1, WChar::from(b)),
        }
    }

    fn wcwidth(&self, wc: WChar) -> i32 {
        match wc {
            0 => 0,
            0x20..=0x7e => 1,
            _ => -1,
        }
    }

    fn iswprint(&self, wc: WChar) -> bool {
        (0x20..0x7f).contains(&wc)
    }

    fn isprint(&self, c: i32) -> bool {
        (0x20..0x7f).contains(&c)
    }

    fn iscntrl(&self, c: i32) -> bool {
        (0..0x20).contains(&c) || c == 0x7f
    }

    fn wctob(&self, wc: WChar) -> i32 {
        if (0..0x80).contains(&wc) { wc } else { -1 }
    }

    fn btowc(&self, c: i32) -> u32 {
        if (0..0x80).contains(&c) {
            c.cast_unsigned()
        } else {
            u32::MAX
        }
    }

    fn wcrtomb(&self, wc: WChar) -> Option<Vec<u8>> {
        u8::try_from(wc).ok().filter(|b| *b < 0x80).map(|b| vec![b])
    }

    fn unicode_locale(&self) -> bool {
        false
    }

    fn legacy_locale(&self) -> bool {
        true
    }
}
