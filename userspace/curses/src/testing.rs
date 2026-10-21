//! What the tests of several modules share: a locale of their own, so that
//! no test depends on the locale of the process running it; and an allocator
//! that counts, so that a test can show a piece of work allocates nothing.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use crate::addch::{Ctype, MB_LEN_MAX};
use crate::cell::WChar;

/// The test binary's allocator: the system's, counting the calls made while
/// a test has asked to see them -- on that test's thread only, so the tests
/// running beside it count nothing.
pub struct Counting;

thread_local! {
    // `const`-initialised and without a destructor, so reading them never
    // allocates: the allocator itself reads them.
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

/// One call to the allocator, counted if this thread is counting.
fn note() {
    // `try_with`: a thread that is being torn down has no slots left, and
    // what it frees is nobody's to count.
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            let _ = COUNT.try_with(|c| c.set(c.get().saturating_add(1)));
        }
    });
}

// SAFETY: every call is passed to `System` exactly as it came; counting reads
// and writes two thread-local cells and allocates nothing.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller's contract, passed on whole.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note();
        // SAFETY: as `alloc`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: as `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: as `alloc`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// How many times `f` called the allocator -- to allocate, to grow or to
/// free -- on this thread. A free counts as much as an allocation: both
/// take the allocator's lock, which is what a signal handler must not.
pub fn allocations_in(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    COUNT.with(Cell::get)
}

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

    fn wcrtomb(&self, wc: WChar, buf: &mut [u8; MB_LEN_MAX]) -> Option<usize> {
        char::from_u32(wc.cast_unsigned()).map(|c| c.encode_utf8(buf).len())
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

    fn wcrtomb(&self, wc: WChar, buf: &mut [u8; MB_LEN_MAX]) -> Option<usize> {
        let b = u8::try_from(wc).ok().filter(|b| *b < 0x80)?;
        buf[0] = b;
        Some(1)
    }

    fn unicode_locale(&self) -> bool {
        false
    }

    fn legacy_locale(&self) -> bool {
        true
    }
}
