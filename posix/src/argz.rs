//! argz and envz vectors (GNU `<argz.h>`, `<envz.h>`): a list of strings
//! kept as one block of memory, each string followed by its NUL -- `"a\0bc\0"`
//! is `a` and `bc` -- with the block's length beside it; and an envz vector,
//! an argz one whose entries are `name=value` or a bare `name`, like an
//! environment.
//!
//! Written from glibc's manual; glibc 2.39 is run only for its answers
//! (`posix/tools/oracle/argz_harness.py`, `argz_oracle.txt`), which the tests
//! replay, and they are glibc's but for one thing: `argz_replace` adds to
//! `*replace_count` the replacements it made, as the manual says ("the number
//! of replacements performed"), where glibc's adds one for each entry it
//! changed, however many it replaced in it.
//!
//! The vectors are the caller's, allocated by this library's `malloc`; a
//! vector left with no entries is freed and NULL (but by `envz_strip`, which
//! glibc's leaves allocated, as here). Every change that inserts or rebuilds
//! copies what it is given first, so an argument that points into the
//! vector being changed is read before the vector moves.
//!
//! gnulib's `argz` module defines the `argz_*` names where the C library
//! lacks them, as musl does, so they are an archive member of their own
//! (`mod gnu_argz`), as are the `envz_*` ones; what both call is here, under
//! names no program defines.

// Offsets and lengths here are positions inside a vector found by scanning
// that same vector, so every index is in bounds and every sum at most its
// length; where lengths from different places combine (a new vector's), the
// adds are checked.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use core::ptr::null_mut;

use crate::errno::{EINVAL, ENOMEM};

/// `error_t`: an `errno` value, or 0.
pub type ErrorT = i32;

/// The bytes of the vector at `argz`, `len` long.
///
/// # Safety
///
/// `argz` is NULL (with any `len`, read as 0) or has `len` readable bytes,
/// which outlive the slice.
unsafe fn bytes<'a>(argz: *const u8, len: usize) -> &'a [u8] {
    if argz.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller's contract.
        unsafe { core::slice::from_raw_parts(argz, len) }
    }
}

/// The entries of a vector's bytes, each without its NUL. Bytes after the
/// last NUL, which a well-formed vector has none of, are an entry too.
fn entries(v: &[u8]) -> impl Iterator<Item = &[u8]> + Clone {
    let body = v.strip_suffix(&[0]).unwrap_or(v);
    let empty = v.is_empty();
    body.split(|&b| b == 0).filter(move |_| !empty)
}

/// A NUL-terminated string's bytes, without the NUL.
///
/// # Safety
///
/// `s` is non-NULL and NUL-terminated.
unsafe fn cstr<'a>(s: *const u8) -> &'a [u8] {
    // SAFETY: the caller's contract.
    unsafe { core::ffi::CStr::from_ptr(s.cast()) }.to_bytes()
}

/// A block of `n` bytes from `malloc`; NULL for none (or for `n` = 0).
fn alloc(n: usize) -> *mut u8 {
    if n == 0 {
        null_mut()
    } else {
        crate::malloc::malloc(n)
    }
}

/// Give back a block `alloc` or the caller's `malloc` made.
///
/// # Safety
///
/// `p` is NULL or a live block of this library's `malloc`.
unsafe fn release(p: *mut u8) {
    // SAFETY: the caller's contract.
    unsafe { crate::malloc::free(p) };
}

/// A new vector of `parts`, each followed by a NUL: its block and length.
/// `Err(ENOMEM)` if the block cannot be had; `(NULL, 0)` for no parts.
fn build<'a>(parts: impl Iterator<Item = &'a [u8]> + Clone) -> Result<(*mut u8, usize), ErrorT> {
    let len = parts
        .clone()
        .try_fold(0usize, |n, p| n.checked_add(p.len())?.checked_add(1))
        .ok_or(ENOMEM)?;
    if len == 0 {
        return Ok((null_mut(), 0));
    }
    let block = alloc(len);
    if block.is_null() {
        return Err(ENOMEM);
    }
    let mut at = 0usize;
    for p in parts {
        // SAFETY: the parts' lengths and NULs add up to `len`.
        unsafe {
            core::ptr::copy_nonoverlapping(p.as_ptr(), block.add(at), p.len());
            block.add(at + p.len()).write(0);
        }
        at += p.len() + 1;
    }
    Ok((block, len))
}

/// Put `(block, len)` in the caller's place, freeing the vector it replaces.
///
/// # Safety
///
/// `argz` and `len` are the caller's; the old vector is theirs to free.
unsafe fn replace_with(argz: *mut *mut u8, len: *mut usize, new: (*mut u8, usize)) {
    // SAFETY: the caller's contract.
    unsafe {
        release(*argz);
        *argz = new.0;
        *len = new.1;
    }
}

/// Append `extra` to the caller's vector, growing it in place when the
/// allocator can. `extra` may point into the vector: it is found again in
/// the new block by its offset.
///
/// # Safety
///
/// `argz` and `len` are the caller's valid vector; `extra` readable.
unsafe fn append(argz: *mut *mut u8, len: *mut usize, extra: &[u8]) -> ErrorT {
    if extra.is_empty() {
        return 0;
    }
    // SAFETY: the caller's vector.
    let (old, old_len) = unsafe { (*argz, *len) };
    let Some(new_len) = old_len.checked_add(extra.len()) else {
        return ENOMEM;
    };
    let base = old as usize;
    let inside = !old.is_null() && (base..base + old_len).contains(&(extra.as_ptr() as usize));
    let offset = (extra.as_ptr() as usize).wrapping_sub(base);
    // SAFETY: `old` is NULL or the caller's block from this malloc.
    let grown = unsafe { crate::malloc::realloc(old, new_len) };
    if grown.is_null() {
        return ENOMEM;
    }
    // SAFETY: `grown` has `new_len` bytes; the source is the caller's bytes,
    // or where they now are inside `grown` (which a realloc keeps).
    unsafe {
        let src = if inside {
            grown.add(offset)
        } else {
            extra.as_ptr()
        };
        core::ptr::copy(src, grown.add(old_len), extra.len());
        *argz = grown;
        *len = new_len;
    }
    0
}

/// Split `string` at every `sep` into a vector's bytes, as glibc splits it:
/// a separator ends the entry before it, but one at the start or after
/// another is dropped -- no empty entry but the last, which a separator at
/// the very end leaves.
fn split_sep(string: &[u8], sep: u8) -> Result<(*mut u8, usize), ErrorT> {
    if string.is_empty() {
        return Ok((null_mut(), 0));
    }
    let block = alloc(string.len() + 1);
    if block.is_null() {
        return Err(ENOMEM);
    }
    let mut at = 0usize;
    for &b in string {
        let byte = if b == sep {
            // SAFETY: `at - 1` is a written byte of the block.
            if at == 0 || unsafe { block.add(at - 1).read() } == 0 {
                continue;
            }
            0
        } else {
            b
        };
        // SAFETY: at most `string.len()` bytes are written before the NUL.
        unsafe { block.add(at).write(byte) };
        at += 1;
    }
    // SAFETY: as above.
    unsafe { block.add(at).write(0) };
    Ok((block, at + 1))
}

/// The name part of an envz entry: up to its `=`, or all of it.
fn name_of(entry: &[u8]) -> &[u8] {
    entry.split(|&b| b == b'=').next().unwrap_or(entry)
}

/// Where in `v` the entry named as `name` is -- its offset -- if any.
fn find_entry(v: &[u8], name: &[u8]) -> Option<usize> {
    let name = name_of(name);
    let mut at = 0usize;
    for e in entries(v) {
        if name_of(e) == name {
            return Some(at);
        }
        at += e.len() + 1;
    }
    None
}

/// Delete the entry at `offset` in the caller's vector: `argz_delete`'s
/// work, and `envz_remove`'s. Freed and NULL once nothing is left.
///
/// # Safety
///
/// `argz` and `len` are the caller's valid vector; `offset` an entry's.
unsafe fn delete_at(argz: *mut *mut u8, len: *mut usize, offset: usize) {
    // SAFETY: the caller's vector.
    let (v, n) = unsafe { (*argz, *len) };
    // SAFETY: as above.
    let all = unsafe { bytes(v, n) };
    let Some(rest) = all.get(offset..) else {
        return;
    };
    let entry_len = rest
        .iter()
        .position(|&b| b == 0)
        .map_or(rest.len(), |i| i + 1);
    let after = offset + entry_len;
    // SAFETY: both ranges are inside the vector.
    unsafe {
        core::ptr::copy(v.add(after), v.add(offset), n - after);
        *len = n - entry_len;
        if *len == 0 {
            release(v);
            *argz = null_mut();
        }
    }
}

/// Own archive member -- gnulib's `argz` module defines these where the C
/// library lacks them. (glibc's `__argz_*` aliases, which only its headers'
/// inline versions call, are not here: this library's headers have none.)
mod gnu_argz {
    use super::*;

    /// `argz_create`: a vector of `argv`'s strings, into `*argz` and
    /// `*len`; 0, or `ENOMEM`. No strings is `(NULL, 0)`.
    ///
    /// # Safety
    ///
    /// `argv` a NULL-terminated array of strings; `argz` and `len` writable.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_create(
        argv: *const *const u8,
        argz: *mut *mut u8,
        len: *mut usize,
    ) -> ErrorT {
        let mut count = 0usize;
        if !argv.is_null() {
            // SAFETY: a NULL-terminated array, by the contract.
            while !unsafe { argv.add(count).read() }.is_null() {
                count += 1;
            }
        }
        // SAFETY: each a NUL-terminated string.
        let parts = (0..count).map(|i| unsafe { cstr(argv.add(i).read()) });
        match build(parts) {
            Ok((v, n)) => {
                // SAFETY: the caller's writable places.
                unsafe {
                    *argz = v;
                    *len = n;
                }
                0
            }
            Err(e) => e,
        }
    }

    /// `argz_create_sep`: `string` split at every `sep` -- see
    /// [`split_sep`] -- into `*argz` and `*len`; 0, or `ENOMEM`.
    ///
    /// # Safety
    ///
    /// `string` NUL-terminated; `argz` and `len` writable.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_create_sep(
        string: *const u8,
        sep: i32,
        argz: *mut *mut u8,
        len: *mut usize,
    ) -> ErrorT {
        // SAFETY: the caller's string.
        match split_sep(unsafe { cstr(string) }, sep as u8) {
            Ok((v, n)) => {
                // SAFETY: the caller's writable places.
                unsafe {
                    *argz = v;
                    *len = n;
                }
                0
            }
            Err(e) => e,
        }
    }

    /// `argz_count`: how many entries.
    ///
    /// # Safety
    ///
    /// `argz` NULL or `len` readable bytes.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_count(argz: *const u8, len: usize) -> usize {
        // SAFETY: the caller's vector.
        entries(unsafe { bytes(argz, len) }).count()
    }

    /// `argz_extract`: a pointer to each entry into `argv`, and NULL after
    /// the last.
    ///
    /// # Safety
    ///
    /// `argz` NULL or `len` readable bytes; `argv` room for
    /// `argz_count + 1` pointers.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_extract(argz: *const u8, len: usize, argv: *mut *mut u8) {
        // SAFETY: the caller's vector.
        let v = unsafe { bytes(argz, len) };
        let mut i = 0usize;
        let mut at = 0usize;
        for e in entries(v) {
            // SAFETY: room for every entry and the NULL, by the contract.
            unsafe { argv.add(i).write(argz.add(at).cast_mut()) };
            i += 1;
            at += e.len() + 1;
        }
        // SAFETY: as above.
        unsafe { argv.add(i).write(null_mut()) };
    }

    /// `argz_stringify`: every NUL but the last made `sep`, so that the
    /// vector reads as one string.
    ///
    /// # Safety
    ///
    /// `argz` NULL or `len` writable bytes.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_stringify(argz: *mut u8, len: usize, sep: i32) {
        if argz.is_null() || len < 2 {
            return;
        }
        // SAFETY: the caller's `len` bytes.
        let v = unsafe { core::slice::from_raw_parts_mut(argz, len) };
        let last = len - 1;
        for b in v.iter_mut().take(last) {
            if *b == 0 {
                *b = sep as u8;
            }
        }
    }

    /// `argz_append`: `buf`'s `buf_len` bytes -- entries of their own, each
    /// with its NUL -- after the vector's; 0, or `ENOMEM`.
    ///
    /// # Safety
    ///
    /// `argz` and `len` the caller's vector; `buf` `buf_len` readable bytes.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_append(
        argz: *mut *mut u8,
        len: *mut usize,
        buf: *const u8,
        buf_len: usize,
    ) -> ErrorT {
        // SAFETY: the caller's contract.
        unsafe { append(argz, len, bytes(buf, buf_len)) }
    }

    /// `argz_add`: `str` as an entry at the end; 0, or `ENOMEM`.
    ///
    /// # Safety
    ///
    /// `argz` and `len` the caller's vector; `str` NUL-terminated.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_add(
        argz: *mut *mut u8,
        len: *mut usize,
        str: *const u8,
    ) -> ErrorT {
        // SAFETY: the caller's string, with its NUL.
        let entry = unsafe { core::ffi::CStr::from_ptr(str.cast()) }.to_bytes_with_nul();
        // SAFETY: the caller's vector.
        unsafe { append(argz, len, entry) }
    }

    /// `argz_add_sep`: `string` split at every `delim`, as
    /// [`argz_create_sep`] splits it, at the end; 0, or `ENOMEM`.
    ///
    /// # Safety
    ///
    /// As [`argz_add`].
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_add_sep(
        argz: *mut *mut u8,
        len: *mut usize,
        string: *const u8,
        delim: i32,
    ) -> ErrorT {
        // SAFETY: the caller's string.
        let (split, n) = match split_sep(unsafe { cstr(string) }, delim as u8) {
            Ok(v) => v,
            Err(e) => return e,
        };
        // SAFETY: the caller's vector, and our own block.
        let r = unsafe { append(argz, len, bytes(split, n)) };
        // SAFETY: our own block.
        unsafe { release(split) };
        r
    }

    /// `argz_delete`: the entry `entry` points at out of the vector; the
    /// vector freed and NULL once it is empty. A NULL `entry` does nothing.
    ///
    /// # Safety
    ///
    /// `argz` and `len` the caller's vector; `entry` NULL or an entry's start.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_delete(argz: *mut *mut u8, len: *mut usize, entry: *mut u8) {
        if entry.is_null() {
            return;
        }
        // SAFETY: the caller's vector.
        let (v, n) = unsafe { (*argz, *len) };
        let offset = (entry as usize).wrapping_sub(v as usize);
        if v.is_null() || offset >= n {
            return;
        }
        // SAFETY: an entry of the caller's vector.
        unsafe { delete_at(argz, len, offset) };
    }

    /// `argz_insert`: `entry` as an entry before the one `before` points into
    /// -- from its start, as glibc does when it points into the middle -- or
    /// at the end for a NULL `before`. 0, `ENOMEM`, or `EINVAL` for a
    /// `before` outside the vector.
    ///
    /// # Safety
    ///
    /// `argz` and `len` the caller's vector; `entry` NUL-terminated.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_insert(
        argz: *mut *mut u8,
        len: *mut usize,
        before: *mut u8,
        entry: *const u8,
    ) -> ErrorT {
        if before.is_null() {
            // SAFETY: forwarded.
            return unsafe { argz_add(argz, len, entry) };
        }
        // SAFETY: the caller's vector.
        let (v, n) = unsafe { (*argz, *len) };
        let mut offset = (before as usize).wrapping_sub(v as usize);
        if v.is_null() || offset >= n {
            return EINVAL;
        }
        // SAFETY: the caller's vector and string.
        let (all, new) = unsafe { (bytes(v, n), cstr(entry)) };
        while offset > 0 && all.get(offset - 1).is_some_and(|&b| b != 0) {
            offset -= 1;
        }
        let (head, tail) = all.split_at(offset);
        let head = head.strip_suffix(&[0]).map(|h| (h, true));
        let tail = tail.strip_suffix(&[0]).unwrap_or(tail);
        // The entries before, the new one, those after -- built afresh, so
        // that an `entry` inside the vector is copied before it is freed.
        let parts = head
            .into_iter()
            .flat_map(|(h, _)| h.split(|&b| b == 0))
            .chain(core::iter::once(new))
            .chain(tail.split(|&b| b == 0));
        match build(parts) {
            Ok(built) => {
                // SAFETY: the caller's places, and their old vector to free.
                unsafe { replace_with(argz, len, built) };
                0
            }
            Err(e) => e,
        }
    }

    /// `argz_replace`: every occurrence of `str` in the entries replaced by
    /// `with`, left to right and not overlapping; `*replace_count`, unless it
    /// is NULL, increased by the number of replacements -- as glibc's manual
    /// says, where glibc's adds one an entry changed. An empty or NULL `str`
    /// replaces nothing. 0, or `ENOMEM`.
    ///
    /// # Safety
    ///
    /// `argz` and `len` the caller's vector; `str` and `with` NULL or
    /// NUL-terminated; `replace_count` NULL or writable.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_replace(
        argz: *mut *mut u8,
        len: *mut usize,
        str: *const u8,
        with: *const u8,
        replace_count: *mut u32,
    ) -> ErrorT {
        if str.is_null() {
            return 0;
        }
        // SAFETY: the caller's strings and vector.
        let (pat, with, v) = unsafe {
            (
                cstr(str),
                if with.is_null() { &[][..] } else { cstr(with) },
                bytes(*argz, *len),
            )
        };
        if pat.is_empty() {
            return 0;
        }
        let hits = |e: &[u8]| {
            let mut n = 0usize;
            let mut at = 0usize;
            while let Some(i) = e
                .get(at..)
                .and_then(|r| r.windows(pat.len()).position(|w| w == pat))
            {
                n += 1;
                at += i + pat.len();
            }
            n
        };
        let total: usize = entries(v).map(hits).sum();
        if total == 0 {
            return 0;
        }
        // Each entry's new length, then the new vector, written in place.
        let new_len = entries(v)
            .map(|e| {
                let k = hits(e);
                e.len() - k * pat.len() + k * with.len() + 1
            })
            .try_fold(0usize, usize::checked_add);
        let Some(new_len) = new_len else {
            return ENOMEM;
        };
        let block = alloc(new_len);
        if block.is_null() {
            return ENOMEM;
        }
        let mut out = 0usize;
        let mut put = |b: &[u8]| {
            // SAFETY: the lengths above add up to `new_len`.
            unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), block.add(out), b.len()) };
            out += b.len();
        };
        for e in entries(v) {
            let mut at = 0usize;
            while let Some(i) = e
                .get(at..)
                .and_then(|r| r.windows(pat.len()).position(|w| w == pat))
            {
                put(&e[at..at + i]);
                put(with);
                at += i + pat.len();
            }
            put(&e[at..]);
            put(&[0]);
        }
        // SAFETY: the caller's places and old vector; and their counter.
        unsafe {
            replace_with(argz, len, (block, new_len));
            if !replace_count.is_null() {
                let n = u32::try_from(total).unwrap_or(u32::MAX);
                *replace_count = (*replace_count).wrapping_add(n);
            }
        }
        0
    }

    /// `argz_next`: the entry after `entry` -- or the first, for a NULL
    /// `entry` -- or NULL after the last.
    ///
    /// # Safety
    ///
    /// `argz` NULL or `len` readable bytes; `entry` NULL or an entry's start.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn argz_next(argz: *const u8, len: usize, entry: *const u8) -> *mut u8 {
        if argz.is_null() || len == 0 {
            return null_mut();
        }
        if entry.is_null() {
            return argz.cast_mut();
        }
        // SAFETY: the caller's vector.
        let v = unsafe { bytes(argz, len) };
        let offset = (entry as usize).wrapping_sub(argz as usize);
        let Some(rest) = v.get(offset..) else {
            return null_mut();
        };
        let next = offset
            + rest
                .iter()
                .position(|&b| b == 0)
                .map_or(rest.len(), |i| i + 1);
        if next < len {
            // SAFETY: inside the vector.
            unsafe { argz.add(next).cast_mut() }
        } else {
            null_mut()
        }
    }
}
pub use gnu_argz::{
    argz_add, argz_add_sep, argz_append, argz_count, argz_create, argz_create_sep, argz_delete,
    argz_extract, argz_insert, argz_next, argz_replace, argz_stringify,
};

/// Own archive member: the envz functions, over what `gnu_argz`'s are over,
/// not over those.
mod envz_forms {
    use super::*;

    /// `envz_entry`: the entry named `name` -- `name=...`, or a bare
    /// `name` -- or NULL. A `name` with an `=` in it is named by what comes
    /// before it.
    ///
    /// # Safety
    ///
    /// `envz` NULL or `len` readable bytes; `name` NUL-terminated.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn envz_entry(envz: *const u8, len: usize, name: *const u8) -> *mut u8 {
        // SAFETY: the caller's vector and string.
        let (v, name) = unsafe { (bytes(envz, len), cstr(name)) };
        match find_entry(v, name) {
            // SAFETY: inside the vector.
            Some(at) => unsafe { envz.add(at).cast_mut() },
            None => null_mut(),
        }
    }

    /// `envz_get`: the value of the entry named `name` -- after its `=` --
    /// or NULL for none, or for a bare `name`.
    ///
    /// # Safety
    ///
    /// As [`envz_entry`].
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn envz_get(envz: *const u8, len: usize, name: *const u8) -> *mut u8 {
        // SAFETY: the caller's vector and string.
        let (v, key) = unsafe { (bytes(envz, len), cstr(name)) };
        let Some(at) = find_entry(v, key) else {
            return null_mut();
        };
        let entry = v.get(at..).unwrap_or(&[]);
        let entry = entry.split(|&b| b == 0).next().unwrap_or(entry);
        match entry.iter().position(|&b| b == b'=') {
            // SAFETY: inside the vector.
            Some(eq) => unsafe { envz.add(at + eq + 1).cast_mut() },
            None => null_mut(),
        }
    }

    /// `envz_add`: `name=value` -- or a bare `name` for a NULL `value` --
    /// at the end, any entry of that name removed first. 0, or `ENOMEM`.
    ///
    /// # Safety
    ///
    /// `envz` and `len` the caller's vector; `name` NUL-terminated, `value`
    /// NULL or NUL-terminated.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn envz_add(
        envz: *mut *mut u8,
        len: *mut usize,
        name: *const u8,
        value: *const u8,
    ) -> ErrorT {
        // The new entry first, in a block of its own: `name` and `value`
        // may be inside the vector the removal moves.
        // SAFETY: the caller's strings.
        let (key, val) = unsafe {
            (
                cstr(name),
                if value.is_null() {
                    None
                } else {
                    Some(cstr(value))
                },
            )
        };
        let parts: [&[u8]; 3] = match val {
            Some(v) => [key, b"=", v],
            None => [key, &[], &[]],
        };
        let Some(n) = parts.iter().try_fold(1usize, |n, p| n.checked_add(p.len())) else {
            return ENOMEM;
        };
        let entry = alloc(n);
        if entry.is_null() {
            return ENOMEM;
        }
        let mut at = 0usize;
        for p in parts {
            // SAFETY: the parts add up to `n - 1`.
            unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), entry.add(at), p.len()) };
            at += p.len();
        }
        // SAFETY: the NUL, at `n - 1`.
        unsafe { entry.add(at).write(0) };
        // SAFETY: the caller's vector; our own block.
        unsafe {
            if let Some(off) = find_entry(bytes(*envz, *len), bytes(entry, at)) {
                delete_at(envz, len, off);
            }
            let r = append(envz, len, bytes(entry, n));
            release(entry);
            r
        }
    }

    /// `envz_merge`: each entry of `envz2` added -- at the end -- unless one
    /// of its name is there already; with `override` nonzero, one of its name
    /// is removed and it is added all the same. 0, or `ENOMEM`.
    ///
    /// # Safety
    ///
    /// `envz` and `len` the caller's vector; `envz2` NULL or `len2` readable.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn envz_merge(
        envz: *mut *mut u8,
        len: *mut usize,
        envz2: *const u8,
        len2: usize,
        override_: i32,
    ) -> ErrorT {
        // A copy of the second, which may be the first.
        // SAFETY: the caller's vector.
        let (copy, n2) = match build(entries(unsafe { bytes(envz2, len2) })) {
            Ok(c) => c,
            Err(e) => return e,
        };
        // SAFETY: our own block.
        let second = unsafe { bytes(copy, n2) };
        let mut result = 0;
        for e in entries(second) {
            // SAFETY: the caller's vector.
            let found = find_entry(unsafe { bytes(*envz, *len) }, e);
            if let Some(off) = found {
                if override_ == 0 {
                    continue;
                }
                // SAFETY: an entry of the caller's vector.
                unsafe { delete_at(envz, len, off) };
            }
            // SAFETY: the caller's vector; the entry with its NUL, in `copy`.
            let with_nul = unsafe { core::slice::from_raw_parts(e.as_ptr(), e.len() + 1) };
            // SAFETY: as above.
            let r = unsafe { append(envz, len, with_nul) };
            if r != 0 {
                result = r;
                break;
            }
        }
        // SAFETY: our own block.
        unsafe { release(copy) };
        result
    }

    /// `envz_remove`: the entry named `name`, if there is one, out; the
    /// vector freed and NULL once it is empty.
    ///
    /// # Safety
    ///
    /// `envz` and `len` the caller's vector; `name` NUL-terminated.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn envz_remove(envz: *mut *mut u8, len: *mut usize, name: *const u8) {
        // SAFETY: the caller's vector and string.
        if let Some(off) = find_entry(unsafe { bytes(*envz, *len) }, unsafe { cstr(name) }) {
            // SAFETY: an entry of the caller's vector.
            unsafe { delete_at(envz, len, off) };
        }
    }

    /// `envz_strip`: every bare `name` entry out, in place. The vector is
    /// kept, as glibc's keeps it, even when nothing is left in it.
    ///
    /// # Safety
    ///
    /// `envz` and `len` the caller's vector.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn envz_strip(envz: *mut *mut u8, len: *mut usize) {
        // SAFETY: the caller's vector.
        let (v, n) = unsafe { (*envz, *len) };
        if v.is_null() || n == 0 {
            return;
        }
        // SAFETY: the caller's `n` bytes.
        let all = unsafe { core::slice::from_raw_parts_mut(v, n) };
        let mut read = 0usize;
        let mut write = 0usize;
        while read < n {
            let end = all[read..]
                .iter()
                .position(|&b| b == 0)
                .map_or(n, |i| read + i + 1);
            if all[read..end].contains(&b'=') {
                all.copy_within(read..end, write);
                write += end - read;
            }
            read = end;
        }
        // SAFETY: the caller's length.
        unsafe { *len = write };
    }
}
pub use envz_forms::{envz_add, envz_entry, envz_get, envz_merge, envz_remove, envz_strip};

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::null;

    /// glibc 2.39's answers (`posix/tools/oracle/argz_harness.py`).
    const ORACLE: &str = include_str!("argz_oracle.txt");

    fn oracle(name: &str) -> &'static str {
        ORACLE
            .lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(" ="))
            .unwrap_or_else(|| panic!("no `{name}` in the oracle"))
    }

    /// A vector as the harness writes it.
    fn vec(v: *const u8, len: usize) -> String {
        if v.is_null() {
            return format!(" NULL {len}");
        }
        let mut s = format!(" {len} ");
        for &b in unsafe { bytes(v, len) } {
            match b {
                0 => s.push('|'),
                0x20..=0x7e => s.push(b as char),
                _ => s.push_str(&format!("\\x{b:02x}")),
            }
        }
        s
    }

    fn err(e: ErrorT) -> String {
        match e {
            0 => " 0".into(),
            ENOMEM => " ENOMEM".into(),
            EINVAL => " EINVAL".into(),
            _ => " other".into(),
        }
    }

    /// A vector of `items`, by `argz_add`.
    fn make(items: &[&str]) -> (*mut u8, usize) {
        let (mut v, mut len) = (null_mut(), 0);
        for it in items {
            let c = std::ffi::CString::new(*it).unwrap();
            assert_eq!(
                unsafe { argz_add(&raw mut v, &raw mut len, c.as_ptr().cast()) },
                0
            );
        }
        (v, len)
    }

    fn free(v: *mut u8) {
        unsafe { release(v) };
    }

    fn c(s: &str) -> std::ffi::CString {
        std::ffi::CString::new(s).unwrap()
    }

    #[test]
    fn create_and_split_are_glibcs() {
        let (mut v, mut len) = (null_mut(), 0);
        let items = [c("a"), c("bc"), c(""), c("def")];
        let argv: Vec<*const u8> = items
            .iter()
            .map(|s| s.as_ptr().cast())
            .chain([null()])
            .collect();
        let e = unsafe { argz_create(argv.as_ptr(), &raw mut v, &raw mut len) };
        assert_eq!(err(e) + &vec(v, len), oracle("create"));
        free(v);
        let none = [null::<u8>()];
        let e = unsafe { argz_create(none.as_ptr(), &raw mut v, &raw mut len) };
        assert_eq!(err(e) + &vec(v, len), oracle("create-empty"));
        let one_empty = [c("")];
        let argv = [one_empty[0].as_ptr().cast::<u8>(), null()];
        let e = unsafe { argz_create(argv.as_ptr(), &raw mut v, &raw mut len) };
        assert_eq!(err(e) + &vec(v, len), oracle("create-one-empty"));
        free(v);
        for (i, s) in ["a:bc::def:", ":a", "", ":::", "abc", "a::", "::b"]
            .iter()
            .enumerate()
        {
            let s = c(s);
            let e = unsafe {
                argz_create_sep(s.as_ptr().cast(), i32::from(b':'), &raw mut v, &raw mut len)
            };
            let got = err(e) + &vec(v, len) + &format!(" count={}", unsafe { argz_count(v, len) });
            assert_eq!(got, oracle(&format!("create_sep {i}")), "{s:?}");
            free(v);
        }
    }

    #[test]
    fn walking_is_glibcs() {
        let (v, len) = make(&["one", "", "three"]);
        assert_eq!(
            format!(" {}", unsafe { argz_count(v, len) }),
            oracle("count")
        );
        let mut argv = [null_mut::<u8>(); 8];
        unsafe { argz_extract(v, len, argv.as_mut_ptr()) };
        let mut got = String::new();
        for p in argv.iter().take_while(|p| !p.is_null()) {
            got += &format!(
                " [{}]",
                unsafe { core::ffi::CStr::from_ptr(p.cast()) }
                    .to_str()
                    .unwrap()
            );
        }
        got += &format!(" {}", i32::from(argv[3].is_null()));
        assert_eq!(got, oracle("extract"));
        let mut got = String::new();
        let mut e = unsafe { argz_next(v, len, null()) };
        while !e.is_null() {
            got += &format!(
                " [{}]",
                unsafe { core::ffi::CStr::from_ptr(e.cast()) }
                    .to_str()
                    .unwrap()
            );
            e = unsafe { argz_next(v, len, e) };
        }
        got += &format!(
            " {}",
            i32::from(unsafe { argz_next(null(), 0, null()) }.is_null())
        );
        assert_eq!(got, oracle("next"));
        unsafe { argz_stringify(v, len, i32::from(b',')) };
        assert_eq!(vec(v, len), oracle("stringify"));
        free(v);
        unsafe { argz_stringify(null_mut(), 0, i32::from(b',')) };
    }

    #[test]
    fn adding_is_glibcs() {
        let (mut v, mut len) = (null_mut(), 0);
        let mut got = String::new();
        for s in ["x", "", "yz"] {
            got += &err(unsafe { argz_add(&raw mut v, &raw mut len, c(s).as_ptr().cast()) });
        }
        assert_eq!(got + &vec(v, len), oracle("add"));
        let mut got = err(unsafe { argz_append(&raw mut v, &raw mut len, b"p\0q\0".as_ptr(), 4) });
        got += &err(unsafe { argz_append(&raw mut v, &raw mut len, b"".as_ptr(), 0) });
        assert_eq!(got + &vec(v, len), oracle("append"));
        let mut got =
            err(unsafe { argz_add_sep(&raw mut v, &raw mut len, c("m,,n,").as_ptr().cast(), 44) });
        got += &err(unsafe { argz_add_sep(&raw mut v, &raw mut len, c("").as_ptr().cast(), 44) });
        assert_eq!(got + &vec(v, len), oracle("add_sep"));
        // Appending a vector's own bytes: found again after it moves.
        let before = len;
        assert_eq!(unsafe { argz_append(&raw mut v, &raw mut len, v, 2) }, 0);
        assert_eq!(len, before + 2);
        assert_eq!(unsafe { bytes(v, len) }[before..], *b"x\0");
        free(v);
    }

    #[test]
    fn deleting_is_glibcs() {
        for (k, which) in ["first", "middle", "last"].iter().enumerate() {
            let (mut v, mut len) = make(&["aa", "bb", "cc"]);
            let mut e = v;
            for _ in 0..k {
                e = unsafe { argz_next(v, len, e) };
            }
            unsafe { argz_delete(&raw mut v, &raw mut len, e) };
            assert_eq!(vec(v, len), oracle(&format!("delete-{which}")));
            free(v);
        }
        let (mut v, mut len) = make(&["solo"]);
        unsafe { argz_delete(&raw mut v, &raw mut len, v) };
        assert_eq!(vec(v, len), oracle("delete-only"));
        let (mut v, mut len) = make(&["aa", "bb", "cc"]);
        unsafe { argz_delete(&raw mut v, &raw mut len, null_mut()) };
        assert_eq!(vec(v, len), oracle("delete-null"));
        free(v);
    }

    #[test]
    fn inserting_is_glibcs() {
        let new = c("new");
        let ins = |at: Option<usize>, name: &str| {
            let (mut v, mut len) = make(&["aa", "bb", "cc"]);
            let before = at.map_or(null_mut(), |o| v.wrapping_add(o));
            let e = unsafe { argz_insert(&raw mut v, &raw mut len, before, new.as_ptr().cast()) };
            assert_eq!(err(e) + &vec(v, len), oracle(name), "{name}");
            free(v);
        };
        ins(Some(0), "insert-first");
        ins(Some(3), "insert-middle");
        ins(None, "insert-null");
        ins(Some(4), "insert-inside");
        ins(Some(9 + 5), "insert-outside");
        let (mut v, mut len) = (null_mut(), 0);
        let e = unsafe { argz_insert(&raw mut v, &raw mut len, null_mut(), new.as_ptr().cast()) };
        assert_eq!(err(e) + &vec(v, len), oracle("insert-empty"));
        free(v);
        // An entry that is one of the vector's own.
        let (mut v, mut len) = make(&["aa", "bb"]);
        let own = unsafe { argz_next(v, len, v) };
        assert_eq!(unsafe { argz_insert(&raw mut v, &raw mut len, v, own) }, 0);
        assert_eq!(vec(v, len), " 9 bb|aa|bb|");
        free(v);
    }

    /// Every replacement glibc's makes, and the count the manual gives.
    #[test]
    fn replacing_is_glibcs_but_the_count() {
        let cases: [(&[&str], &str, &str); 10] = [
            (&["abcab", "xab", "zz"], "ab", "X"),
            (&["abcab", "xab", "zz"], "ab", "LONGER"),
            (&["abcab", "xab", "zz"], "ab", ""),
            (&["aaa"], "aa", "b"),
            (&["abc"], "", "X"),
            (&["abc"], "zz", "X"),
            (&["abc"], "abc", ""),
            (&["ababab"], "ab", "X"),
            (&["ab", "ab", "ab"], "ab", "X"),
            (&["xabyabz", "q"], "ab", "--"),
        ];
        // What the manual's count adds, case by case: every occurrence.
        let occurrences = [3, 3, 3, 1, 0, 0, 1, 3, 3, 2];
        for (i, ((items, s, w), want_added)) in cases.iter().zip(occurrences).enumerate() {
            let (mut v, mut len) = make(items);
            let mut count = 7u32;
            let e = unsafe {
                argz_replace(
                    &raw mut v,
                    &raw mut len,
                    c(s).as_ptr().cast(),
                    c(w).as_ptr().cast(),
                    &raw mut count,
                )
            };
            let line = oracle(&format!("replace {i}"));
            let (vector, glibc_count) = line.rsplit_once(" count=").unwrap();
            assert_eq!(err(e) + &vec(v, len), vector, "case {i}");
            assert_eq!(
                count,
                7 + want_added,
                "case {i}: glibc's gives {glibc_count}"
            );
            free(v);
        }
        let (mut v, mut len) = make(&["abcab", "xab", "zz"]);
        let e = unsafe {
            argz_replace(
                &raw mut v,
                &raw mut len,
                c("ab").as_ptr().cast(),
                c("Y").as_ptr().cast(),
                null_mut(),
            )
        };
        assert_eq!(err(e) + &vec(v, len), oracle("replace-null-count"));
        free(v);
    }

    #[test]
    fn envz_is_glibcs() {
        let (mut v, mut len) = make(&["A=1", "B", "C=", "PATH=/bin:/usr/bin", "AB=2"]);
        for name in ["A", "B", "C", "PATH", "AB", "D", "A=1", ""] {
            let n = c(name);
            let e = unsafe { envz_entry(v, len, n.as_ptr().cast()) };
            let g = unsafe { envz_get(v, len, n.as_ptr().cast()) };
            let s = |p: *mut u8| {
                unsafe { core::ffi::CStr::from_ptr(p.cast()) }
                    .to_str()
                    .unwrap()
                    .to_string()
            };
            let got = format!(
                " {} {}",
                if e.is_null() { "NULL".into() } else { s(e) },
                if g.is_null() {
                    "NULL".into()
                } else {
                    format!("[{}]", s(g))
                }
            );
            assert_eq!(got, oracle(&format!("envz {name}")), "{name}");
        }
        let mut got = String::new();
        for (n, val) in [("A", Some("9")), ("E", Some("5")), ("B", None), ("C", None)] {
            let n = c(n);
            let val = val.map(c);
            let vp = val.as_ref().map_or(null(), |x| x.as_ptr().cast());
            got += &err(unsafe { envz_add(&raw mut v, &raw mut len, n.as_ptr().cast(), vp) });
        }
        assert_eq!(got + &vec(v, len), oracle("envz_add"));
        unsafe {
            envz_remove(&raw mut v, &raw mut len, c("PATH").as_ptr().cast());
            envz_remove(&raw mut v, &raw mut len, c("NOPE").as_ptr().cast());
        }
        assert_eq!(vec(v, len), oracle("envz_remove"));
        unsafe { envz_strip(&raw mut v, &raw mut len) };
        assert_eq!(vec(v, len), oracle("envz_strip"));
        free(v);
        for over in 0..2 {
            let (mut a, mut alen) = make(&["X=1", "Y=2", "Z"]);
            let (b, blen) = make(&["Y=20", "W=3", "Z=30", "X"]);
            let e = unsafe { envz_merge(&raw mut a, &raw mut alen, b, blen, over) };
            assert_eq!(
                err(e) + &vec(a, alen),
                oracle(&format!("envz_merge {over}"))
            );
            free(a);
            free(b);
        }
        let (mut v, mut len) = make(&["P", "Q"]);
        unsafe { envz_strip(&raw mut v, &raw mut len) };
        assert_eq!(vec(v, len), oracle("envz_strip-all"));
        free(v);
        let (mut v, mut len) = make(&["P", "Q"]);
        unsafe {
            envz_remove(&raw mut v, &raw mut len, c("P").as_ptr().cast());
            envz_remove(&raw mut v, &raw mut len, c("Q").as_ptr().cast());
        }
        assert_eq!(vec(v, len), oracle("envz_remove-all"));
        // Adding a name and value that are the vector's own.
        let (mut v, mut len) = make(&["K=old"]);
        let name = v;
        unsafe { *v.add(1) = 0 };
        let value = v.wrapping_add(2);
        assert_eq!(
            unsafe { envz_add(&raw mut v, &raw mut len, name, value) },
            0
        );
        // "K" removed, leaving "old"; then K=old, built before the removal.
        assert_eq!(vec(v, len), " 10 old|K=old|");
        free(v);
    }
}
