//! Memory streams: `fmemopen`, `open_memstream`, `open_wmemstream`.
//!
//! Each is a cookie stream (`stdio::fopencookie`) whose far end is memory,
//! which is how glibc builds `fmemopen` too: the stream's own buffer sits in
//! front, so what is written reaches the memory at `fflush` and `fclose`, as
//! it does in glibc.
//!
//! The rules are glibc's (`libio/fmemopen.c`, `memstream.c`,
//! `wmemstream.c`), which differ from musl's in the details a program can
//! see -- where a NUL is written, where `a` starts, what `*sizeloc` counts:
//!
//! - **`fmemopen`** reads and writes a caller's buffer of `size` bytes (or
//!   its own, freed at close, for a NULL buffer).  Reading stops at the
//!   written length -- all of `size` for `r` -- and writing at `size`, after
//!   which a write fails with `ENOSPC`.  A write that ends before the buffer
//!   does ends with a NUL if there is room for one; a write that fills the
//!   buffer, in any stream not opened `a`, has its last byte made one
//!   (glibc's rule -- its comment says "for update", its code applies it to
//!   `w` too).  `a` starts at the
//!   first NUL, `w+` empties the buffer, seeking is allowed from 0 to `size`.
//! - **`open_memstream`** writes a buffer it grows, publishing it and the
//!   length through `*bufloc` and `*sizeloc` at `fflush` and `fclose` (and,
//!   here, at every write and seek, never less often).  The length is the
//!   stream's *position*, as glibc counts it: seek back and flush, and the
//!   length is shorter.  A seek past the end fills the gap with zeros.  At
//!   `fclose` the buffer is trimmed to the position and terminated there.
//! - **`open_wmemstream`** is the same in wide characters: the stream is
//!   wide, the buffer holds `wchar_t`, the length counts characters.  It is
//!   unbuffered, so its positions stay in characters (a buffer in front would
//!   hold UTF-8 bytes whose count means nothing to it).

use core::ffi::c_void;

use crate::errno;
use crate::stdio::{CookieIoFunctions, fopencookie};
use crate::wchar::WcharT;

// ---------------------------------------------------------------------------
// fmemopen
// ---------------------------------------------------------------------------

/// `fmemopen`'s state: glibc's `fmemopen_cookie_t`.
struct Fmem {
    buffer: *mut u8,
    /// The buffer is ours, to free at close.
    mine: bool,
    append: bool,
    size: usize,
    pos: usize,
    /// How far the buffer holds data: where reading stops.
    maxpos: usize,
}

unsafe extern "C" fn fmem_read(c: *mut c_void, b: *mut u8, s: usize) -> isize {
    // SAFETY: `fmemopen` made the cookie.
    let c = unsafe { &mut *c.cast::<Fmem>() };
    let n = if c.pos >= c.maxpos { 0 } else { s.min(c.maxpos.wrapping_sub(c.pos)) };
    // SAFETY: `pos + n <= maxpos <= size`, and `b` holds `s >= n`.
    unsafe { core::ptr::copy_nonoverlapping(c.buffer.add(c.pos), b, n) };
    c.pos = c.pos.wrapping_add(n);
    isize::try_from(n).unwrap_or(isize::MAX)
}

unsafe extern "C" fn fmem_write(c: *mut c_void, b: *const u8, s: usize) -> isize {
    // SAFETY: `fmemopen` made the cookie; `b` holds `s` bytes.
    let (c, last) = unsafe { (&mut *c.cast::<Fmem>(), if s == 0 { 1 } else { *b.add(s.wrapping_sub(1)) }) };
    let pos = if c.append { c.maxpos } else { c.pos };
    let add_nul = s == 0 || last != 0;
    let mut s = s;
    if pos.saturating_add(s) > c.size {
        // glibc tests the stream's position here, not the append point.
        if c.pos.saturating_add(usize::from(add_nul)) >= c.size {
            errno::set_errno(errno::ENOSPC);
            return 0;
        }
        s = c.size.saturating_sub(pos);
    }
    // SAFETY: `pos + s <= size`, inside the buffer.
    unsafe { core::ptr::copy_nonoverlapping(b, c.buffer.add(pos), s) };
    c.pos = pos.wrapping_add(s);
    if c.pos > c.maxpos {
        c.maxpos = c.pos;
        if c.maxpos < c.size && add_nul {
            // SAFETY: `maxpos < size`.
            unsafe { *c.buffer.add(c.maxpos) = 0 };
        } else if !c.append && add_nul && c.size > 0 {
            // glibc's comment reads "A null byte is written in a stream
            // open for update iff it fits"; its code makes the last byte one
            // for any stream not opened `a`, and so does this.
            // SAFETY: `size > 0`.
            unsafe { *c.buffer.add(c.size.wrapping_sub(1)) = 0 };
        }
    }
    isize::try_from(s).unwrap_or(isize::MAX)
}

unsafe extern "C" fn fmem_seek(c: *mut c_void, p: *mut i64, whence: i32) -> i32 {
    // SAFETY: `fmemopen` made the cookie; `p` is the stream's.
    let (c, off) = unsafe { (&mut *c.cast::<Fmem>(), *p) };
    let base = match whence {
        crate::stdio::SEEK_SET => 0i64,
        crate::stdio::SEEK_CUR => i64::try_from(c.pos).unwrap_or(i64::MAX),
        crate::stdio::SEEK_END => i64::try_from(c.maxpos).unwrap_or(i64::MAX),
        _ => return -1,
    };
    let Some(np) = base.checked_add(off) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    match usize::try_from(np) {
        Ok(n) if n <= c.size => {
            c.pos = n;
            // SAFETY: the stream's.
            unsafe { *p = np };
            0
        }
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

unsafe extern "C" fn fmem_close(c: *mut c_void) -> i32 {
    // SAFETY: `fmemopen` made the cookie, and nothing uses it after this.
    unsafe {
        let st = c.cast::<Fmem>();
        if (*st).mine {
            crate::malloc::free((*st).buffer);
        }
        crate::malloc::free(st.cast());
    }
    0
}

/// A stream over `size` bytes at `buf` -- or over a buffer of its own, freed
/// at close, if `buf` is NULL -- as glibc's `fmemopen` (see the module doc).
/// A buffer that would wrap the address space is `EINVAL`; the mode is
/// `fopencookie`'s.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fmemopen(buf: *mut c_void, size: usize, mode: *const u8) -> *mut u8 {
    if mode.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: a C string by C's contract; each byte read only past a non-NUL.
    let (m0, m1) = unsafe {
        let m0 = *mode;
        (m0, if m0 == 0 { 0 } else { *mode.add(1) })
    };
    let st = crate::malloc::calloc(1, core::mem::size_of::<Fmem>()).cast::<Fmem>();
    if st.is_null() {
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    }
    let mine = buf.is_null();
    let buffer = if mine {
        let b = crate::malloc::malloc(size.max(1));
        if b.is_null() {
            // SAFETY: ours.
            unsafe { crate::malloc::free(st.cast()) };
            errno::set_errno(errno::ENOMEM);
            return core::ptr::null_mut();
        }
        // SAFETY: at least one byte.
        unsafe { *b = 0 };
        b
    } else {
        if size > usize::MAX.wrapping_sub(buf as usize) {
            // SAFETY: ours.
            unsafe { crate::malloc::free(st.cast()) };
            errno::set_errno(errno::EINVAL);
            return core::ptr::null_mut();
        }
        let b = buf.cast::<u8>();
        if m0 == b'w' && m1 == b'+' && size > 0 {
            // POSIX: `w+` empties the buffer.
            // SAFETY: the caller's buffer holds `size > 0` bytes.
            unsafe { *b = 0 };
        }
        b
    };
    let maxpos = if m0 == b'r' {
        size
    } else if m0 == b'a' && !mine {
        // SAFETY: the caller's `size` bytes.
        let bytes = unsafe { core::slice::from_raw_parts(buffer, size) };
        bytes.iter().position(|&x| x == 0).unwrap_or(size)
    } else {
        0
    };
    let append = m0 == b'a';
    // SAFETY: `st` holds one zeroed `Fmem`.
    unsafe {
        st.write(Fmem { buffer, mine, append, size, pos: if append { maxpos } else { 0 }, maxpos });
    }
    let io = CookieIoFunctions {
        read: Some(fmem_read),
        write: Some(fmem_write),
        seek: Some(fmem_seek),
        close: Some(fmem_close),
    };
    // SAFETY: the callbacks take this cookie.
    let f = unsafe { fopencookie(st.cast(), mode, io) };
    if f.is_null() {
        // SAFETY: never handed out.
        unsafe {
            if mine {
                crate::malloc::free(buffer);
            }
            crate::malloc::free(st.cast());
        }
    }
    f
}

// ---------------------------------------------------------------------------
// open_memstream and open_wmemstream
// ---------------------------------------------------------------------------

/// A growing buffer of `T` (a byte or a `wchar_t`) and the caller's two
/// places to publish it.
struct Grow<T> {
    bufp: *mut *mut T,
    sizep: *mut usize,
    buf: *mut T,
    /// Elements the buffer holds, a terminator's room included.
    space: usize,
    /// The high-water mark: elements written.
    len: usize,
    pos: usize,
}

impl<T: Copy + Default> Grow<T> {
    /// Room for `need` elements and a terminator, the new part zeroed.
    fn reserve(&mut self, need: usize) -> bool {
        let Some(want) = need.checked_add(1) else {
            return false;
        };
        if want <= self.space {
            return true;
        }
        let grow = want.max(self.space.saturating_mul(2));
        let Some(bytes) = grow.checked_mul(core::mem::size_of::<T>()) else {
            return false;
        };
        // SAFETY: `buf` is this stream's `malloc` allocation.
        let nb = unsafe { crate::malloc::realloc(self.buf.cast(), bytes) }.cast::<T>();
        if nb.is_null() {
            return false;
        }
        for i in self.space..grow {
            // SAFETY: inside the new allocation.
            unsafe { nb.add(i).write(T::default()) };
        }
        self.buf = nb;
        self.space = grow;
        true
    }

    /// `*bufloc` and `*sizeloc`, now.
    fn publish(&self) {
        // SAFETY: the caller's two places, by `open_memstream`'s contract.
        unsafe {
            *self.bufp = self.buf;
            *self.sizep = self.pos;
        }
    }

    /// Store `items` at the position, growing as needed; `false` if memory
    /// ran out.
    fn put(&mut self, items: &[T]) -> bool {
        let Some(end) = self.pos.checked_add(items.len()) else {
            return false;
        };
        if !self.reserve(end.max(self.len)) {
            return false;
        }
        // SAFETY: `reserve` made room for `end` elements.
        unsafe { core::ptr::copy_nonoverlapping(items.as_ptr(), self.buf.add(self.pos), items.len()) };
        self.pos = end;
        if end > self.len {
            self.len = end;
            // SAFETY: `reserve` left room for the terminator.
            unsafe { self.buf.add(end).write(T::default()) };
        }
        self.publish();
        true
    }

    /// glibc's `_IO_str_seekoff` for a write stream: from 0, the position or
    /// the end; a new position past the end fills the gap with zeros.
    fn seek(&mut self, off: i64, whence: i32) -> Option<i64> {
        let base = match whence {
            crate::stdio::SEEK_SET => 0,
            crate::stdio::SEEK_CUR => i64::try_from(self.pos).ok()?,
            crate::stdio::SEEK_END => i64::try_from(self.len).ok()?,
            _ => return None,
        };
        let np = usize::try_from(base.checked_add(off)?).ok()?;
        if isize::try_from(np).is_err() {
            return None;
        }
        if np > self.len {
            if !self.reserve(np) {
                return None;
            }
            self.len = np;
        }
        self.pos = np;
        self.publish();
        i64::try_from(np).ok()
    }

    /// glibc's `_IO_mem_finish`: the buffer trimmed to the position and
    /// terminated there, and published.
    fn finish(&mut self) {
        let keep = self.pos.saturating_add(1);
        if let Some(bytes) = keep.checked_mul(core::mem::size_of::<T>()) {
            // SAFETY: this stream's allocation; a failed shrink keeps the old.
            let nb = unsafe { crate::malloc::realloc(self.buf.cast(), bytes) }.cast::<T>();
            if !nb.is_null() {
                self.buf = nb;
                self.space = keep;
            }
        }
        // SAFETY: at least `pos + 1` elements.
        unsafe { self.buf.add(self.pos).write(T::default()) };
        self.publish();
    }
}

/// `open_memstream`'s cookie.
type MemStream = Grow<u8>;

unsafe extern "C" fn ms_write(c: *mut c_void, b: *const u8, s: usize) -> isize {
    // SAFETY: `open_memstream` made the cookie; `b` holds `s` bytes.
    let (m, bytes) = unsafe { (&mut *c.cast::<MemStream>(), core::slice::from_raw_parts(b, s)) };
    if m.put(bytes) {
        isize::try_from(s).unwrap_or(isize::MAX)
    } else {
        errno::set_errno(errno::ENOMEM);
        -1
    }
}

unsafe extern "C" fn ms_seek(c: *mut c_void, p: *mut i64, whence: i32) -> i32 {
    // SAFETY: as above; `p` is the stream's.
    unsafe {
        match (*c.cast::<MemStream>()).seek(*p, whence) {
            Some(np) => {
                *p = np;
                0
            }
            None => {
                errno::set_errno(errno::EINVAL);
                -1
            }
        }
    }
}

unsafe extern "C" fn ms_close(c: *mut c_void) -> i32 {
    // SAFETY: as above; the buffer is the caller's from here, the cookie ours.
    unsafe {
        (*c.cast::<MemStream>()).finish();
        crate::malloc::free(c.cast());
    }
    0
}

/// Make a growing stream's state with a first buffer of `first` elements.
fn new_grow<T: Copy + Default>(bufp: *mut *mut T, sizep: *mut usize, first: usize) -> *mut Grow<T> {
    let st = crate::malloc::calloc(1, core::mem::size_of::<Grow<T>>()).cast::<Grow<T>>();
    if st.is_null() {
        return st;
    }
    let buf = crate::malloc::calloc(first, core::mem::size_of::<T>()).cast::<T>();
    if buf.is_null() {
        // SAFETY: ours.
        unsafe { crate::malloc::free(st.cast()) };
        return core::ptr::null_mut();
    }
    // SAFETY: `st` holds one zeroed `Grow<T>`.
    unsafe {
        st.write(Grow { bufp, sizep, buf, space: first, len: 0, pos: 0 });
        (*st).publish();
    }
    st
}

/// A write-only stream into a buffer that grows (glibc's `open_memstream`;
/// see the module doc).  `*bufloc` and `*sizeloc` hold the buffer and the
/// length after every `fflush` and at `fclose`; the buffer is the caller's to
/// `free` after `fclose`.  NULL `bufloc` or `sizeloc` is `EFAULT`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn open_memstream(bufloc: *mut *mut u8, sizeloc: *mut usize) -> *mut u8 {
    if bufloc.is_null() || sizeloc.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    let st = new_grow(bufloc, sizeloc, 8192);
    if st.is_null() {
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    }
    let io = CookieIoFunctions { read: None, write: Some(ms_write), seek: Some(ms_seek), close: Some(ms_close) };
    // SAFETY: the callbacks take this cookie.
    let f = unsafe { fopencookie(st.cast(), c"w".as_ptr().cast(), io) };
    if f.is_null() {
        // SAFETY: never handed out.
        unsafe {
            crate::malloc::free((*st).buf);
            crate::malloc::free(st.cast());
        }
    }
    f
}

/// `open_wmemstream`'s cookie: the wide buffer and the UTF-8 sequence cut off
/// at the end of the last write, if any.
struct WideMem {
    grow: Grow<WcharT>,
    partial: [u8; 4],
    have: usize,
}

/// Decode the UTF-8 in `bytes` (after what `w` carried over) into wide
/// characters and store them: `false` for a sequence that is no character.
fn wide_put(w: &mut WideMem, bytes: &[u8]) -> bool {
    for &b in bytes {
        if let Some(slot) = w.partial.get_mut(w.have) {
            *slot = b;
        }
        w.have = w.have.wrapping_add(1);
        let seq = w.partial.get(..w.have).unwrap_or(&[]);
        match core::str::from_utf8(seq) {
            Ok(s) => {
                let Some(c) = s.chars().next() else {
                    w.have = 0;
                    continue;
                };
                w.have = 0;
                let Ok(wc) = WcharT::try_from(u32::from(c)) else {
                    return false;
                };
                if !w.grow.put(&[wc]) {
                    return false;
                }
            }
            // An incomplete sequence: wait for more.
            Err(e) if e.error_len().is_none() && w.have < 4 => {}
            Err(_) => return false,
        }
    }
    true
}

unsafe extern "C" fn wms_write(c: *mut c_void, b: *const u8, s: usize) -> isize {
    // SAFETY: `open_wmemstream` made the cookie; `b` holds `s` bytes.
    let (w, bytes) = unsafe { (&mut *c.cast::<WideMem>(), core::slice::from_raw_parts(b, s)) };
    if wide_put(w, bytes) {
        isize::try_from(s).unwrap_or(isize::MAX)
    } else {
        w.have = 0;
        errno::set_errno(errno::EILSEQ);
        -1
    }
}

unsafe extern "C" fn wms_seek(c: *mut c_void, p: *mut i64, whence: i32) -> i32 {
    // SAFETY: as above.
    unsafe {
        let w = &mut *c.cast::<WideMem>();
        w.have = 0;
        match w.grow.seek(*p, whence) {
            Some(np) => {
                *p = np;
                0
            }
            None => {
                errno::set_errno(errno::EINVAL);
                -1
            }
        }
    }
}

unsafe extern "C" fn wms_close(c: *mut c_void) -> i32 {
    // SAFETY: as in `ms_close`.
    unsafe {
        (*c.cast::<WideMem>()).grow.finish();
        crate::malloc::free(c.cast());
    }
    0
}

/// A wide, write-only stream into a `wchar_t` buffer that grows (glibc's
/// `open_wmemstream`): `open_memstream` in characters.  NULL `bufloc` or
/// `sizeloc` is `EFAULT`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn open_wmemstream(bufloc: *mut *mut WcharT, sizeloc: *mut usize) -> *mut u8 {
    if bufloc.is_null() || sizeloc.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    let grow = new_grow(bufloc, sizeloc, 1024);
    if grow.is_null() {
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    }
    let st = crate::malloc::calloc(1, core::mem::size_of::<WideMem>()).cast::<WideMem>();
    if st.is_null() {
        // SAFETY: ours.
        unsafe {
            crate::malloc::free((*grow).buf.cast());
            crate::malloc::free(grow.cast());
        }
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    }
    // SAFETY: `st` holds one zeroed `WideMem`; `grow` is moved into it.
    unsafe {
        st.write(WideMem { grow: grow.read(), partial: [0; 4], have: 0 });
        crate::malloc::free(grow.cast());
    }
    let io = CookieIoFunctions { read: None, write: Some(wms_write), seek: Some(wms_seek), close: Some(wms_close) };
    // SAFETY: the callbacks take this cookie.
    let f = unsafe { fopencookie(st.cast(), c"w".as_ptr().cast(), io) };
    if f.is_null() {
        // SAFETY: never handed out.
        unsafe {
            crate::malloc::free((*st).grow.buf.cast());
            crate::malloc::free(st.cast());
        }
        return f;
    }
    // Wide from the start, as glibc's is, and unbuffered, so that positions
    // stay in characters.
    let _ = crate::stdio::fwide(f, 1); // a new stream takes it
    let _ = crate::stdio::setvbuf(f, core::ptr::null_mut(), crate::stdio::_IONBF, 0); // as above
    f
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;
    use crate::stdio::{fclose, fflush, fgetc, fputc, fputs, fread, fseek, ftell, SEEK_CUR, SEEK_END, SEEK_SET};

    fn s(c: &core::ffi::CStr) -> *const u8 {
        c.as_ptr().cast()
    }

    #[test]
    fn fmemopen_reads_what_the_buffer_holds() {
        let mut buf = *b"hello\0world";
        let f = unsafe { fmemopen(buf.as_mut_ptr().cast(), buf.len(), s(c"r")) };
        assert!(!f.is_null());
        let mut out = [0u8; 16];
        // `r` reads the whole size, NULs included.
        assert_eq!(unsafe { fread(out.as_mut_ptr(), 1, 16, f) }, 11);
        assert_eq!(&out[..11], b"hello\0world");
        assert_eq!(fgetc(f), crate::stdio::EOF);
        assert_eq!(fclose(f), 0);
    }

    #[test]
    fn fmemopen_writes_end_with_a_nul_when_there_is_room() {
        let mut buf = [b'#'; 8];
        let f = unsafe { fmemopen(buf.as_mut_ptr().cast(), buf.len(), s(c"w")) };
        assert_eq!(unsafe { fputs(s(c"abc"), f) }, 1);
        assert_eq!(buf, [b'#'; 8], "buffered until flushed, as in glibc");
        assert_eq!(fflush(f), 0);
        assert_eq!(&buf[..5], b"abc\0#");
        assert_eq!(fclose(f), 0);
    }

    #[test]
    fn fmemopen_fills_to_the_end_and_then_says_enospc() {
        let mut buf = [0u8; 4];
        let f = unsafe { fmemopen(buf.as_mut_ptr().cast(), buf.len(), s(c"w")) };
        assert_eq!(unsafe { fputs(s(c"abcdef"), f) }, 1, "buffered");
        assert_eq!(fflush(f), crate::stdio::EOF, "the far end took four and then refused");
        assert_eq!(crate::stdio::ferror(f), 1);
        assert_eq!(&buf, b"abc\0", "filling the buffer makes the last byte a NUL, as glibc does");
        assert_eq!(fclose(f), 0);
    }

    #[test]
    fn fmemopen_update_stream_makes_the_last_byte_a_nul_when_full() {
        let mut buf = [b'x'; 4];
        let f = unsafe { fmemopen(buf.as_mut_ptr().cast(), buf.len(), s(c"w+")) };
        assert_eq!(buf[0], 0, "w+ empties the buffer");
        let four = b"wxyz";
        assert_eq!(unsafe { crate::stdio::fwrite(four.as_ptr(), 1, 4, f) }, 4);
        assert_eq!(fflush(f), 0);
        assert_eq!(&buf, b"wxy\0", "glibc: the NUL goes in iff it fits, over the last byte");
        assert_eq!(fclose(f), 0);
    }

    #[test]
    fn fmemopen_appends_at_the_first_nul() {
        let mut buf = *b"ab\0\0\0\0";
        let f = unsafe { fmemopen(buf.as_mut_ptr().cast(), buf.len(), s(c"a")) };
        assert_eq!(ftell(f), 2);
        assert_eq!(unsafe { fputs(s(c"cd"), f) }, 1);
        assert_eq!(fclose(f), 0);
        assert_eq!(&buf[..5], b"abcd\0");
    }

    #[test]
    fn fmemopen_seeks_within_the_size() {
        let mut buf = *b"0123456789";
        let f = unsafe { fmemopen(buf.as_mut_ptr().cast(), buf.len(), s(c"r")) };
        assert_eq!(fseek(f, 7, SEEK_SET), 0);
        assert_eq!(fgetc(f), i32::from(b'7'));
        assert_eq!(fseek(f, -2, SEEK_END), 0);
        assert_eq!(fgetc(f), i32::from(b'8'));
        assert_eq!(fseek(f, 11, SEEK_SET), -1, "past the size");
        assert_eq!(fseek(f, -1, SEEK_SET), -1);
        assert_eq!(fclose(f), 0);
    }

    #[test]
    fn fmemopen_with_a_null_buffer_has_its_own() {
        let f = unsafe { fmemopen(core::ptr::null_mut(), 16, s(c"w+")) };
        assert!(!f.is_null());
        assert_eq!(unsafe { fputs(s(c"mine"), f) }, 1);
        assert_eq!(fseek(f, 0, SEEK_SET), 0);
        let mut out = [0u8; 8];
        assert_eq!(unsafe { fread(out.as_mut_ptr(), 1, 8, f) }, 4, "reading stops at what was written");
        assert_eq!(&out[..4], b"mine");
        assert_eq!(fclose(f), 0);
    }

    #[test]
    fn fmemopen_refuses_a_bad_mode_and_a_wrapping_buffer() {
        let mut buf = [0u8; 4];
        errno::set_errno(0);
        assert!(unsafe { fmemopen(buf.as_mut_ptr().cast(), 4, s(c"q")) }.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        assert!(unsafe { fmemopen(usize::MAX as *mut c_void, 2, s(c"r")) }.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        assert!(unsafe { fmemopen(buf.as_mut_ptr().cast(), 4, core::ptr::null()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn open_memstream_publishes_at_fflush_and_trims_at_fclose() {
        let mut p: *mut u8 = core::ptr::null_mut();
        let mut n: usize = 99;
        let f = unsafe { open_memstream(&raw mut p, &raw mut n) };
        assert!(!f.is_null());
        assert_eq!(n, 0);
        assert_eq!(unsafe { fputs(s(c"hello, world"), f) }, 1);
        assert_eq!(fflush(f), 0);
        assert_eq!(n, 12);
        assert_eq!(unsafe { core::slice::from_raw_parts(p, 13) }, b"hello, world\0");
        // The length is the position, as glibc counts it.
        assert_eq!(fseek(f, 5, SEEK_SET), 0);
        assert_eq!(fflush(f), 0);
        assert_eq!(n, 5);
        assert_eq!(fseek(f, 0, SEEK_END), 0);
        assert_eq!(n, 12, "the data after the position was kept");
        // Past the end: zeros fill the gap.
        assert_eq!(fseek(f, 3, SEEK_CUR), 0);
        assert_eq!(fputc(i32::from(b'!'), f), i32::from(b'!'));
        assert_eq!(fclose(f), 0);
        assert_eq!(n, 16);
        assert_eq!(unsafe { core::slice::from_raw_parts(p, 17) }, b"hello, world\0\0\0!\0");
        unsafe { crate::malloc::free(p) };
    }

    #[test]
    fn open_memstream_grows_past_its_first_buffer() {
        let mut p: *mut u8 = core::ptr::null_mut();
        let mut n: usize = 0;
        let f = unsafe { open_memstream(&raw mut p, &raw mut n) };
        let chunk = [b'z'; 1000];
        for _ in 0..50 {
            assert_eq!(unsafe { crate::stdio::fwrite(chunk.as_ptr(), 1, chunk.len(), f) }, chunk.len());
        }
        assert_eq!(fclose(f), 0);
        assert_eq!(n, 50_000);
        let all = unsafe { core::slice::from_raw_parts(p, 50_001) };
        assert!(all[..50_000].iter().all(|&b| b == b'z'));
        assert_eq!(all[50_000], 0);
        unsafe { crate::malloc::free(p) };
    }

    #[test]
    fn open_memstream_trims_to_the_position_at_fclose() {
        let mut p: *mut u8 = core::ptr::null_mut();
        let mut n: usize = 0;
        let f = unsafe { open_memstream(&raw mut p, &raw mut n) };
        assert_eq!(unsafe { fputs(s(c"abcdef"), f) }, 1);
        assert_eq!(fseek(f, 2, SEEK_SET), 0);
        assert_eq!(fclose(f), 0);
        assert_eq!(n, 2, "glibc's _IO_mem_finish: trimmed to the position");
        assert_eq!(unsafe { core::slice::from_raw_parts(p, 3) }, b"ab\0");
        unsafe { crate::malloc::free(p) };
    }

    #[test]
    fn open_memstream_refuses_null_places() {
        let mut n = 0usize;
        errno::set_errno(0);
        assert!(unsafe { open_memstream(core::ptr::null_mut(), &raw mut n) }.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn open_wmemstream_counts_wide_characters() {
        let mut p: *mut WcharT = core::ptr::null_mut();
        let mut n: usize = 0;
        let f = unsafe { open_wmemstream(&raw mut p, &raw mut n) };
        assert!(!f.is_null());
        assert_eq!(crate::stdio::fwide(f, 0), 1, "wide from the start");
        for wc in [0xe9, 0x20ac, 0x1f600, i32::from(b'x')] {
            assert_eq!(unsafe { crate::wchar::fputwc(wc, f) }, wc);
        }
        assert_eq!(fflush(f), 0);
        assert_eq!(n, 4, "four characters, whatever their UTF-8 length");
        assert_eq!(unsafe { core::slice::from_raw_parts(p, 5) }, &[0xe9, 0x20ac, 0x1f600, i32::from(b'x'), 0]);
        assert_eq!(ftell(f), 4);
        assert_eq!(fseek(f, 1, SEEK_SET), 0);
        assert_eq!(unsafe { crate::wchar::fputwc(i32::from(b'Y'), f) }, i32::from(b'Y'));
        assert_eq!(fclose(f), 0);
        assert_eq!(n, 2, "trimmed to the position");
        assert_eq!(unsafe { core::slice::from_raw_parts(p, 3) }, &[0xe9, i32::from(b'Y'), 0]);
        unsafe { crate::malloc::free(p.cast()) };
    }

    #[test]
    fn open_wmemstream_refuses_byte_output() {
        let mut p: *mut WcharT = core::ptr::null_mut();
        let mut n: usize = 0;
        let f = unsafe { open_wmemstream(&raw mut p, &raw mut n) };
        assert_eq!(unsafe { fputs(s(c"bytes"), f) }, crate::stdio::EOF, "a wide stream");
        assert_eq!(fclose(f), 0);
        assert_eq!(n, 0);
        unsafe { crate::malloc::free(p.cast()) };
    }
}
