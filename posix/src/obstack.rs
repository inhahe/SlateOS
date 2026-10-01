//! GNU's object stacks, `<obstack.h>`: the functions its macros call --
//! `_obstack_begin`, `_obstack_begin_1`, `_obstack_newchunk`, `obstack_free`
//! (and `_obstack_free`), `_obstack_memory_used`, `_obstack_allocated_p` --
//! and the failure handler and exit status they use. `obstack_printf` and
//! `obstack_vprintf` are `printf.rs`'s, in members of their own, and reach
//! this one only through `_obstack_newchunk`'s C name, as glibc's do: a
//! program with obstacks of its own (gnulib's) never has two.
//!
//! An obstack keeps objects one after another in chunks from the program's
//! own allocation function. One object at a time grows at the end; finishing
//! it starts the next after it, aligned; freeing an object frees it and
//! everything made after it. Most of the interface is `posix/include`'s
//! `<obstack.h>` macros, which a program expands into its own code -- so
//! `struct obstack` is glibc 2.39's field for field, the macros do what
//! glibc's installed header's do, and these functions what glibc's do, down
//! to the size each new chunk is asked for, which a program's allocation
//! function sees (`posix/tools/oracle/obstack_harness.py`,
//! `obstack_oracle.txt`; design-decisions §1162).
//!
//! glibc 2.39 installs the older form of the interface -- `int` lengths, a
//! `long` chunk size -- and so does this.

use core::ffi::c_void;

/// `struct _obstack_chunk`: the head of each chunk.
#[repr(C)]
pub struct ObstackChunk {
    /// One past the chunk's end.
    pub limit: *mut u8,
    /// The chunk before it, or NULL.
    pub prev: *mut ObstackChunk,
    /// Where its objects begin.
    pub contents: [u8; 4],
}

/// `struct obstack`, glibc's.
#[repr(C)]
pub struct Obstack {
    /// The size new chunks are asked for in, at least.
    pub chunk_size: i64,
    /// The current chunk.
    pub chunk: *mut ObstackChunk,
    /// Where the growing object begins.
    pub object_base: *mut u8,
    /// Where its next byte goes.
    pub next_free: *mut u8,
    /// One past the current chunk's end.
    pub chunk_limit: *mut u8,
    /// The portable macros' scratch: a `ptrdiff_t` or a pointer.
    pub temp: isize,
    /// An object's address is a multiple of this plus one.
    pub alignment_mask: i32,
    /// `void *(*)(long)`, or with `use_extra_arg` `void *(*)(void *,
    /// long)`.
    pub chunkfun: *mut c_void,
    /// `void (*)(void *)`, or with `use_extra_arg` `void (*)(void *, void
    /// *)`.
    pub freefun: *mut c_void,
    /// The first argument of both, with `use_extra_arg`.
    pub extra_arg: *mut c_void,
    /// glibc's three bit-fields, from the low bit: `use_extra_arg`,
    /// `maybe_empty_object`, `alloc_failed`.
    pub bits: u32,
}

const USE_EXTRA_ARG: u32 = 1;
const MAYBE_EMPTY_OBJECT: u32 = 1 << 1;
const ALLOC_FAILED: u32 = 1 << 2;

/// glibc's `struct obstack`: 88 bytes, the bit-fields one `unsigned int` at
/// 80.
const _: () = {
    assert!(size_of::<Obstack>() == 88);
    assert!(core::mem::offset_of!(Obstack, alignment_mask) == 48);
    assert!(core::mem::offset_of!(Obstack, chunkfun) == 56);
    assert!(core::mem::offset_of!(Obstack, bits) == 80);
    assert!(core::mem::offset_of!(ObstackChunk, contents) == 16);
};

/// A chunk's size when the program asks for none: what GNU malloc could
/// fit in a 4096-byte block, its 12-byte head and 4 bytes after rounded up
/// to `long double`'s 16 -- 4064, glibc's.
const DEFAULT_CHUNK_SIZE: i64 = 4064;

/// An object's alignment when the program asks for none: `long double`'s,
/// the strictest a scalar has.
const DEFAULT_ALIGNMENT: i32 = 16;

/// What `obstack_alloc_failed_handler` starts as: glibc's
/// `print_and_abort`, which says so and exits.
unsafe extern "C" fn print_and_abort() {
    // What fails to be written is lost, as glibc's message is: the exit
    // that follows is the handler's whole point.
    // SAFETY: a C string; the process's standard error.
    let _ = unsafe {
        crate::stdio::fputs(
            c"memory exhausted\n".as_ptr().cast(),
            crate::stdio::stderr_stream(),
        )
    };
    // SAFETY: a plain read of a C variable.
    let status = unsafe { (&raw const obstack_exit_failure).read() };
    crate::crt::exit(status);
}

/// Called when a chunk cannot be had. A program may set its own; it must
/// not return.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut obstack_alloc_failed_handler: Option<unsafe extern "C" fn()> = Some(print_and_abort);

/// The status the default handler exits with: `EXIT_FAILURE`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut obstack_exit_failure: i32 = 1;

/// The handler called, as glibc calls it when a chunk cannot be had. It
/// must not return -- it exits or longjmps -- and one that does leaves no
/// chunk to go on with: glibc's code goes on through the null one and
/// faults, and this aborts, before the macro that asked for room can write
/// past the chunk there is.
fn alloc_failed() {
    // SAFETY: a plain read of a C variable.
    let handler = unsafe { (&raw const obstack_alloc_failed_handler).read() };
    if let Some(f) = handler {
        // SAFETY: the program's handler, or the default.
        unsafe { f() };
    }
    // Host tests: no handler can longjmp out of Rust, so the tests' return,
    // and a return stands for the longjmp -- the obstack as it was, which is
    // what a longjmp out of here leaves. `services/ctest-obstack` sees the
    // abort.
    #[cfg(not(test))]
    crate::unistd::abort();
}

/// `p` rounded up to a multiple of `mask + 1`, counted from address 0 --
/// glibc's `__PTR_ALIGN`, a pointer fitting a `ptrdiff_t` here.
fn align(p: *mut u8, mask: i32) -> *mut u8 {
    let m = usize::try_from(mask).unwrap_or(0);
    p.map_addr(|a| a.wrapping_add(m) & !m)
}

/// A chunk of `size` bytes from `h`'s allocation function.
///
/// # Safety
///
/// `h`'s `chunkfun` is the program's, of the shape `use_extra_arg` says.
unsafe fn call_chunkfun(h: &Obstack, size: i64) -> *mut ObstackChunk {
    if h.chunkfun.is_null() {
        return core::ptr::null_mut();
    }
    if h.bits & USE_EXTRA_ARG != 0 {
        // SAFETY: with use_extra_arg, chunkfun is `void *(*)(void *, long)`.
        let f: unsafe extern "C" fn(*mut c_void, i64) -> *mut c_void =
            unsafe { core::mem::transmute(h.chunkfun) };
        // SAFETY: the program's function, given its own argument.
        unsafe { f(h.extra_arg, size) }.cast()
    } else {
        // SAFETY: without it, `void *(*)(long)`.
        let f: unsafe extern "C" fn(i64) -> *mut c_void =
            unsafe { core::mem::transmute(h.chunkfun) };
        // SAFETY: the program's function.
        unsafe { f(size) }.cast()
    }
}

/// `chunk` given back to `h`'s free function.
///
/// # Safety
///
/// As [`call_chunkfun`], for `freefun`; `chunk` came from `chunkfun`.
unsafe fn call_freefun(h: &Obstack, chunk: *mut ObstackChunk) {
    if h.freefun.is_null() {
        return;
    }
    if h.bits & USE_EXTRA_ARG != 0 {
        // SAFETY: with use_extra_arg, freefun is `void (*)(void *, void *)`.
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void) =
            unsafe { core::mem::transmute(h.freefun) };
        // SAFETY: the program's function, given its own argument.
        unsafe { f(h.extra_arg, chunk.cast()) };
    } else {
        // SAFETY: without it, `void (*)(void *)`.
        let f: unsafe extern "C" fn(*mut c_void) = unsafe { core::mem::transmute(h.freefun) };
        // SAFETY: the program's function.
        unsafe { f(chunk.cast()) };
    }
}

/// glibc's `_obstack_begin_worker`: `h` begun with its first chunk.
///
/// # Safety
///
/// `h` is writable; its function fields and `use_extra_arg` are set.
unsafe fn begin(h: *mut Obstack, size: i32, alignment: i32) -> i32 {
    // SAFETY: the caller's obstack.
    let o = unsafe { &mut *h };
    let alignment = if alignment == 0 {
        DEFAULT_ALIGNMENT
    } else {
        alignment
    };
    o.chunk_size = if size == 0 {
        DEFAULT_CHUNK_SIZE
    } else {
        i64::from(size)
    };
    o.alignment_mask = alignment.wrapping_sub(1);
    // SAFETY: the program's functions, per the contract.
    let chunk = unsafe { call_chunkfun(o, o.chunk_size) };
    if chunk.is_null() {
        alloc_failed();
        return 0;
    }
    o.chunk = chunk;
    // SAFETY: a chunk of chunk_size bytes, its head first.
    unsafe {
        let contents = (&raw mut (*chunk).contents).cast::<u8>();
        o.object_base = align(contents, o.alignment_mask);
        o.next_free = o.object_base;
        o.chunk_limit = chunk
            .cast::<u8>()
            .wrapping_add(usize::try_from(o.chunk_size).unwrap_or(0));
        (*chunk).limit = o.chunk_limit;
        (*chunk).prev = core::ptr::null_mut();
    }
    o.bits &= !(MAYBE_EMPTY_OBJECT | ALLOC_FAILED);
    1
}

/// `obstack_init`, `obstack_begin` and `obstack_specify_allocation`: `h`
/// begun, its chunks `size` bytes (4064 for 0) from `chunkfun`, its objects
/// aligned to `alignment` (16 for 0): 1. When the first chunk cannot be had
/// the failure handler is called, which does not come back here
/// (`alloc_failed`); 0 is only the host tests', whose handler returns.
///
/// # Safety
///
/// `h` is writable; the two functions are the program's, as `obstack.h`
/// describes them.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _obstack_begin(
    h: *mut Obstack,
    size: i32,
    alignment: i32,
    chunkfun: Option<unsafe extern "C" fn(i64) -> *mut c_void>,
    freefun: Option<unsafe extern "C" fn(*mut c_void)>,
) -> i32 {
    if h.is_null() {
        return 0;
    }
    // SAFETY: the caller's obstack.
    let o = unsafe { &mut *h };
    o.chunkfun = chunkfun.map_or(core::ptr::null_mut(), |f| f as *mut c_void);
    o.freefun = freefun.map_or(core::ptr::null_mut(), |f| f as *mut c_void);
    o.bits &= !USE_EXTRA_ARG;
    // SAFETY: per the contract.
    unsafe { begin(h, size, alignment) }
}

/// `obstack_specify_allocation_with_arg`: `_obstack_begin` with functions
/// that take `arg` first.
///
/// # Safety
///
/// As [`_obstack_begin`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _obstack_begin_1(
    h: *mut Obstack,
    size: i32,
    alignment: i32,
    chunkfun: Option<unsafe extern "C" fn(*mut c_void, i64) -> *mut c_void>,
    freefun: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    arg: *mut c_void,
) -> i32 {
    if h.is_null() {
        return 0;
    }
    // SAFETY: the caller's obstack.
    let o = unsafe { &mut *h };
    o.chunkfun = chunkfun.map_or(core::ptr::null_mut(), |f| f as *mut c_void);
    o.freefun = freefun.map_or(core::ptr::null_mut(), |f| f as *mut c_void);
    o.extra_arg = arg;
    o.bits |= USE_EXTRA_ARG;
    // SAFETY: per the contract.
    unsafe { begin(h, size, alignment) }
}

/// A new chunk for the growing object and `length` bytes more: the object
/// moved into it, and the chunk it left freed if the object was all it
/// held. The new chunk is the object's size and `length`, an eighth of the
/// object's size again, the alignment's slack and 100 bytes more -- or the
/// obstack's chunk size, if that is more -- as glibc's is.
///
/// # Safety
///
/// `h` is a begun obstack.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _obstack_newchunk(h: *mut Obstack, length: i32) {
    if h.is_null() {
        return;
    }
    // SAFETY: the caller's obstack.
    let o = unsafe { &mut *h };
    let old = o.chunk;
    // SAFETY: both in the current chunk.
    let obj_size = unsafe { o.next_free.offset_from(o.object_base) };
    let obj = i64::try_from(obj_size).unwrap_or(0);
    let mut new_size = obj
        .saturating_add(i64::from(length))
        .saturating_add(obj >> 3)
        .saturating_add(i64::from(o.alignment_mask))
        .saturating_add(100);
    if new_size < o.chunk_size {
        new_size = o.chunk_size;
    }
    // SAFETY: the program's functions.
    let chunk = unsafe { call_chunkfun(o, new_size) };
    if chunk.is_null() {
        alloc_failed();
        return;
    }
    o.chunk = chunk;
    let limit = chunk
        .cast::<u8>()
        .wrapping_add(usize::try_from(new_size).unwrap_or(0));
    // SAFETY: a fresh chunk of new_size bytes, its head first.
    let base = unsafe {
        (*chunk).prev = old;
        (*chunk).limit = limit;
        align((&raw mut (*chunk).contents).cast::<u8>(), o.alignment_mask)
    };
    o.chunk_limit = limit;
    let n = usize::try_from(obj_size).unwrap_or(0);
    // SAFETY: the object's `n` bytes, into the new chunk, which has room:
    // two chunks, so no overlap.
    unsafe { core::ptr::copy_nonoverlapping(o.object_base, base, n) };
    // The chunk left is freed if the object was all it held -- not if it
    // may hold an empty object as well, which freeing it would lose.
    if o.bits & MAYBE_EMPTY_OBJECT == 0 && !old.is_null() {
        // SAFETY: the chunk before, a live one.
        let first = align(
            unsafe { (&raw mut (*old).contents).cast::<u8>() },
            o.alignment_mask,
        );
        if o.object_base == first {
            // SAFETY: the chunk is no longer in the chain once the new one
            // skips it; given back once.
            unsafe {
                (*chunk).prev = (*old).prev;
                call_freefun(o, old);
            }
        }
    }
    o.object_base = base;
    o.next_free = base.wrapping_add(n);
    o.bits &= !MAYBE_EMPTY_OBJECT;
}

/// Whether `obj` is in `chunk` or at its end, as glibc's tests it: after
/// the chunk's head, and not past its limit.
fn holds(chunk: *mut ObstackChunk, obj: *mut c_void) -> bool {
    // SAFETY: a live chunk's limit.
    let limit = unsafe { (*chunk).limit };
    (chunk as usize) < (obj as usize) && (obj as usize) <= (limit as usize)
}

/// `obstack_free` for an object outside the current chunk: the chunks after
/// the one `obj` is in freed, and `obj` and all after it with them -- all of
/// them, for NULL. An `obj` in none of them is a program's error, and
/// `abort`s, as glibc's does.
///
/// # Safety
///
/// `h` is a begun obstack (or one every chunk of which `obstack_free(h,
/// NULL)` has freed).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn obstack_free(h: *mut Obstack, obj: *mut c_void) {
    if h.is_null() {
        return;
    }
    // SAFETY: the caller's obstack.
    let o = unsafe { &mut *h };
    let mut lp = o.chunk;
    while !lp.is_null() && !holds(lp, obj) {
        // SAFETY: a live chunk, given back once, the chain read first.
        let prev = unsafe { (*lp).prev };
        // SAFETY: as above.
        unsafe { call_freefun(o, lp) };
        lp = prev;
        // Past a chunk boundary, whether the chunk now current holds an
        // empty object cannot be told: assume it may.
        o.bits |= MAYBE_EMPTY_OBJECT;
    }
    if lp.is_null() {
        if !obj.is_null() {
            crate::unistd::abort();
        }
        // Every chunk freed: the obstack holds none until it is begun again.
        o.chunk = core::ptr::null_mut();
        o.object_base = core::ptr::null_mut();
        o.next_free = core::ptr::null_mut();
        o.chunk_limit = core::ptr::null_mut();
        return;
    }
    o.object_base = obj.cast();
    o.next_free = obj.cast();
    // SAFETY: the chunk `obj` is in.
    o.chunk_limit = unsafe { (*lp).limit };
    o.chunk = lp;
}

/// glibc's other name for [`obstack_free`].
///
/// # Safety
///
/// As [`obstack_free`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _obstack_free(h: *mut Obstack, obj: *mut c_void) {
    // SAFETY: per the contract.
    unsafe { obstack_free(h, obj) };
}

/// The bytes of every chunk `h` holds, their heads counted.
///
/// # Safety
///
/// `h` is a begun obstack.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _obstack_memory_used(h: *mut Obstack) -> i32 {
    if h.is_null() {
        return 0;
    }
    let mut total: i64 = 0;
    // SAFETY: the caller's obstack.
    let mut lp = unsafe { (*h).chunk };
    while !lp.is_null() {
        // SAFETY: a live chunk.
        let (limit, prev) = unsafe { ((*lp).limit, (*lp).prev) };
        total = total
            .saturating_add(i64::try_from((limit as usize).wrapping_sub(lp as usize)).unwrap_or(0));
        lp = prev;
    }
    i32::try_from(total).unwrap_or(i32::MAX)
}

/// Whether `obj` is in one of `h`'s chunks: 1 or 0. glibc exports it and
/// declares it in no header.
///
/// # Safety
///
/// `h` is a begun obstack.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _obstack_allocated_p(h: *mut Obstack, obj: *mut c_void) -> i32 {
    if h.is_null() {
        return 0;
    }
    // SAFETY: the caller's obstack.
    let mut lp = unsafe { (*h).chunk };
    while !lp.is_null() && !holds(lp, obj) {
        // SAFETY: a live chunk.
        lp = unsafe { (*lp).prev };
    }
    i32::from(!lp.is_null())
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::cell::RefCell;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers (`posix/tools/oracle/obstack_harness.py`, which
    /// says the forms).
    const ORACLE: &str = include_str!("obstack_oracle.txt");

    /// The failure handler is the process's one: the tests that set it take
    /// turns.
    static HANDLER: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The counting allocator's record, the calling thread's.
    #[derive(Default)]
    struct Log {
        allocs: Vec<i64>,
        frees: usize,
        fail_in: u32,
        failed: bool,
    }

    std::thread_local! {
        static LOG: RefCell<Log> = RefCell::new(Log::default());
    }

    /// The harness's allocator: the size asked for recorded, and 4096-aligned
    /// blocks, so that where an object lands does not depend on the heap.
    unsafe extern "C" fn chunk(size: i64) -> *mut c_void {
        let fail = LOG.with(|l| {
            let mut l = l.borrow_mut();
            if l.fail_in > 0 {
                l.fail_in -= 1;
                if l.fail_in == 0 {
                    return true;
                }
            }
            l.allocs.push(size);
            false
        });
        if fail {
            return core::ptr::null_mut();
        }
        let n = usize::try_from(size).unwrap().div_ceil(4096) * 4096;
        crate::malloc::aligned_alloc(4096, n.max(4096)).cast()
    }

    unsafe extern "C" fn unchunk(p: *mut c_void) {
        LOG.with(|l| l.borrow_mut().frees += 1);
        // SAFETY: a block `chunk` gave.
        unsafe { crate::malloc::free(p.cast()) };
    }

    unsafe extern "C" fn chunk1(arg: *mut c_void, size: i64) -> *mut c_void {
        assert_eq!(arg as usize, 0x5eed);
        // SAFETY: as `chunk`.
        unsafe { chunk(size) }
    }

    unsafe extern "C" fn unchunk1(arg: *mut c_void, p: *mut c_void) {
        assert_eq!(arg as usize, 0x5eed);
        // SAFETY: as `unchunk`.
        unsafe { unchunk(p) };
    }

    /// The harness's handler longjmps out; this one says so and returns, and
    /// the operation it interrupted is abandoned, as the harness's is.
    unsafe extern "C" fn note_failure() {
        LOG.with(|l| l.borrow_mut().failed = true);
    }

    fn failed() -> bool {
        LOG.with(|l| std::mem::take(&mut l.borrow_mut().failed))
    }

    // -- the header's macros, as posix/include/obstack.h writes them --------

    fn addr<T>(p: *const T) -> isize {
        p as isize
    }

    fn object_size(h: &Obstack) -> u32 {
        addr(h.next_free).wrapping_sub(addr(h.object_base)) as u32
    }

    fn room(h: &Obstack) -> u32 {
        addr(h.chunk_limit).wrapping_sub(addr(h.next_free)) as u32
    }

    /// `_obstack_newchunk`, and whether a chunk was had.
    fn newchunk(h: &mut Obstack, len: i32) -> bool {
        // SAFETY: a begun obstack.
        unsafe { _obstack_newchunk(h, len) };
        !failed()
    }

    fn grow(h: &mut Obstack, src: &[u8]) -> bool {
        let len = i32::try_from(src.len()).unwrap();
        if addr(h.next_free) + len as isize > addr(h.chunk_limit) && !newchunk(h, len) {
            return false;
        }
        // SAFETY: room for `len` bytes at next_free, made above.
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), h.next_free, src.len());
            h.next_free = h.next_free.add(src.len());
        }
        true
    }

    fn grow0(h: &mut Obstack, src: &[u8]) -> bool {
        let len = i32::try_from(src.len()).unwrap();
        if addr(h.next_free) + len as isize + 1 > addr(h.chunk_limit) && !newchunk(h, len + 1) {
            return false;
        }
        // SAFETY: room for `len + 1` bytes, made above.
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), h.next_free, src.len());
            h.next_free = h.next_free.add(src.len());
            h.next_free.write(0);
            h.next_free = h.next_free.add(1);
        }
        true
    }

    fn grow_bytes(h: &mut Obstack, b: &[u8]) -> bool {
        if addr(h.next_free) + b.len() as isize > addr(h.chunk_limit)
            && !newchunk(h, i32::try_from(b.len()).unwrap())
        {
            return false;
        }
        // SAFETY: room made above.
        unsafe {
            core::ptr::copy_nonoverlapping(b.as_ptr(), h.next_free, b.len());
            h.next_free = h.next_free.add(b.len());
        }
        true
    }

    fn blank(h: &mut Obstack, len: i32) -> bool {
        if addr(h.chunk_limit) - addr(h.next_free) < len as isize && !newchunk(h, len) {
            return false;
        }
        h.next_free = h.next_free.wrapping_offset(len as isize);
        true
    }

    fn make_room(h: &mut Obstack, len: i32) -> bool {
        if addr(h.chunk_limit) - addr(h.next_free) < len as isize {
            return newchunk(h, len);
        }
        true
    }

    fn finish(h: &mut Obstack) -> *mut u8 {
        let value = h.object_base;
        if h.next_free == value {
            h.bits |= MAYBE_EMPTY_OBJECT;
        }
        h.next_free = align(h.next_free, h.alignment_mask);
        let chunk = addr(h.chunk);
        if addr(h.next_free) - chunk > addr(h.chunk_limit) - chunk {
            h.next_free = h.chunk_limit;
        }
        h.object_base = h.next_free;
        value
    }

    fn free(h: &mut Obstack, obj: *mut u8) {
        if addr(obj) > addr(h.chunk) && addr(obj) < addr(h.chunk_limit) {
            h.next_free = obj;
            h.object_base = obj;
        } else {
            // SAFETY: a begun obstack.
            unsafe { obstack_free(h, obj.cast()) };
        }
    }

    fn empty_p(h: &Obstack) -> bool {
        // SAFETY: a begun obstack's current chunk.
        unsafe {
            (*h.chunk).prev.is_null()
                && h.next_free == align((&raw mut (*h.chunk).contents).cast(), h.alignment_mask)
        }
    }

    fn chunks(h: &Obstack) -> usize {
        let mut n = 0;
        let mut c = h.chunk;
        while !c.is_null() {
            n += 1;
            // SAFETY: a live chunk.
            c = unsafe { (*c).prev };
        }
        n
    }

    /// The chunk `p` is in, counted back from the current one, and its
    /// offset there.
    fn place(h: &Obstack, p: *mut u8) -> (i32, isize) {
        let mut k = 0;
        let mut c = h.chunk;
        while !c.is_null() {
            // SAFETY: a live chunk.
            let limit = unsafe { (*c).limit };
            if addr(c) < addr(p) && addr(p) <= addr(limit) {
                return (k, addr(p) - addr(c));
            }
            k += 1;
            // SAFETY: as above.
            c = unsafe { (*c).prev };
        }
        (-1, 0)
    }

    fn state(h: &Obstack, obj: Option<(usize, *mut u8, bool)>) -> String {
        let base = addr(h.chunk);
        let (allocs, frees) = LOG.with(|l| {
            let mut l = l.borrow_mut();
            (std::mem::take(&mut l.allocs), std::mem::take(&mut l.frees))
        });
        let list: Vec<String> = allocs.iter().map(|a| format!("{a}")).collect();
        let mut s = format!(
            "{} {} {} {} {} {} {} {} [{}] {}",
            object_size(h),
            room(h),
            addr(h.object_base) - base,
            addr(h.next_free) - base,
            addr(h.chunk_limit) - base,
            chunks(h),
            // SAFETY: a begun obstack.
            unsafe { _obstack_memory_used((&raw const *h).cast_mut()) },
            i32::from(empty_p(h)),
            list.join(","),
            frees
        );
        if let Some((k, p, good)) = obj {
            let (which, off) = place(h, p);
            s.push_str(&format!(
                " #{k}:{which}:{off}:{}",
                if good { "ok" } else { "bad" }
            ));
        }
        s
    }

    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|k| (k * 7 + 3) as u8).collect()
    }

    /// One operation of a scenario: the state line it ends in, or "failed"
    /// when the failure handler was called.
    fn step(h: &mut Obstack, op: &str, objs: &mut Vec<*mut u8>) -> String {
        let num = |s: &str| -> i64 { s.parse().unwrap() };
        let (name, arg) = op.split_once(':').unwrap_or((op, ""));
        let ok = match name {
            "begin" | "begin1" => {
                let (s, a) = arg.split_once(':').unwrap();
                let (s, a) = (
                    i32::try_from(num(s)).unwrap(),
                    i32::try_from(num(a)).unwrap(),
                );
                // SAFETY: a writable obstack; the harness's functions.
                unsafe {
                    if name == "begin" {
                        _obstack_begin(h, s, a, Some(chunk), Some(unchunk));
                    } else {
                        _obstack_begin_1(
                            h,
                            s,
                            a,
                            Some(chunk1),
                            Some(unchunk1),
                            0x5eed as *mut c_void,
                        );
                    }
                }
                true
            }
            "mask" => {
                h.alignment_mask = i32::try_from(num(arg)).unwrap();
                true
            }
            "grow" => grow(h, &pattern(usize::try_from(num(arg)).unwrap())),
            "grow0" => grow0(h, &pattern(usize::try_from(num(arg)).unwrap())),
            "1grow" => grow_bytes(h, &[u8::try_from(num(arg)).unwrap()]),
            "ptr" => grow_bytes(h, &(num(arg) as usize).to_ne_bytes()),
            "int" => grow_bytes(h, &i32::try_from(num(arg)).unwrap().to_ne_bytes()),
            "blank" => blank(h, i32::try_from(num(arg)).unwrap()),
            "room" => make_room(h, i32::try_from(num(arg)).unwrap()),
            "newchunk" => newchunk(h, i32::try_from(num(arg)).unwrap()),
            "fail" => {
                LOG.with(|l| l.borrow_mut().fail_in = u32::try_from(num(arg)).unwrap());
                true
            }
            "free" => {
                let k = usize::try_from(num(&arg[1..])).unwrap();
                free(h, objs[k - 1]);
                true
            }
            "freeall" => {
                // SAFETY: a begun obstack.
                unsafe { obstack_free(h, core::ptr::null_mut()) };
                let frees = LOG.with(|l| std::mem::take(&mut l.borrow_mut().frees));
                return format!("freed {frees}");
            }
            _ => {
                let mut good = true;
                let p = match name {
                    "finish" => finish(h),
                    "alloc" => {
                        let n = i32::try_from(num(arg)).unwrap();
                        if !blank(h, n) {
                            return String::from("failed");
                        }
                        let p = finish(h);
                        // SAFETY: n bytes of the new object.
                        unsafe { core::ptr::write_bytes(p, 0x5a, usize::try_from(n).unwrap()) };
                        p
                    }
                    "copy" | "copy0" => {
                        let n = usize::try_from(num(arg)).unwrap();
                        let src = pattern(n);
                        let grew = if name == "copy" {
                            grow(h, &src)
                        } else {
                            grow0(h, &src)
                        };
                        if !grew {
                            return String::from("failed");
                        }
                        let p = finish(h);
                        // SAFETY: the new object's n (and n + 1) bytes.
                        unsafe {
                            good = core::slice::from_raw_parts(p, n) == src.as_slice()
                                && (name == "copy" || *p.add(n) == 0);
                        }
                        p
                    }
                    other => panic!("op {other}"),
                };
                objs.push(p);
                return state(h, Some((objs.len(), p, good)));
            }
        };
        if !ok {
            return String::from("failed");
        }
        state(h, None)
    }

    /// Every scenario's every step answered as glibc's: the same sizes asked
    /// of the allocator, every object in the same place, the same room.
    #[test]
    fn every_scenario_is_glibcs() {
        let _g = HANDLER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: a plain write of a C variable, the tests' lock held.
        let saved = unsafe { (&raw const obstack_alloc_failed_handler).read() };
        // SAFETY: as above.
        unsafe { (&raw mut obstack_alloc_failed_handler).write(Some(note_failure)) };
        let mut wrong = Vec::new();
        let mut steps = 0;
        let mut current = String::new();
        let mut ob: Option<std::boxed::Box<Obstack>> = None;
        let mut objs = Vec::new();
        for line in ORACLE.lines().filter(|l| l.starts_with("S ")) {
            let (lhs, want) = line.split_once(" = ").unwrap();
            let f: Vec<&str> = lhs.split(' ').collect();
            if f[1] != current {
                current = String::from(f[1]);
                if let Some(old) = ob.as_mut() {
                    if !old.chunk.is_null() {
                        // SAFETY: the scenario before's obstack, its chunks freed.
                        unsafe { obstack_free(&raw mut **old, core::ptr::null_mut()) };
                    }
                }
                LOG.with(|l| *l.borrow_mut() = Log::default());
                // SAFETY: a zeroed obstack is one `begin` may begin.
                ob = Some(std::boxed::Box::new(unsafe { core::mem::zeroed() }));
                objs.clear();
            }
            let h = ob.as_mut().unwrap();
            steps += 1;
            let got = step(h, f[2], &mut objs);
            if got != want {
                wrong.push(format!("{lhs}: {got}, want {want}"));
            }
        }
        if let Some(old) = ob.as_mut() {
            if !old.chunk.is_null() {
                // SAFETY: the last scenario's obstack.
                unsafe { obstack_free(&raw mut **old, core::ptr::null_mut()) };
            }
        }
        LOG.with(|l| *l.borrow_mut() = Log::default());
        // SAFETY: as above.
        unsafe { (&raw mut obstack_alloc_failed_handler).write(saved) };
        for w in wrong.iter().take(20) {
            std::eprintln!("{w}");
        }
        assert!(
            wrong.is_empty(),
            "{} of {steps} steps are not glibc's",
            wrong.len()
        );
        assert!(steps > 150, "{steps}");
    }

    /// obstack_init's defaults, as glibc gives them.
    #[test]
    fn the_defaults_are_glibcs() {
        let _g = HANDLER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let line = ORACLE.lines().find(|l| l.starts_with("D ")).unwrap();
        let mut ob: Obstack = unsafe { core::mem::zeroed() };
        // SAFETY: a writable obstack; the harness's functions.
        assert_eq!(
            unsafe { _obstack_begin(&raw mut ob, 0, 0, Some(chunk), Some(unchunk)) },
            1
        );
        // SAFETY: plain reads of C variables.
        let (status, handler) = unsafe {
            (
                (&raw const obstack_exit_failure).read(),
                (&raw const obstack_alloc_failed_handler).read(),
            )
        };
        let got = format!(
            "{} {} {} {}",
            ob.chunk_size,
            ob.alignment_mask,
            status,
            i32::from(handler.is_some())
        );
        assert!(line.ends_with(&got), "{line} vs {got}");
        // SAFETY: begun above.
        unsafe { obstack_free(&raw mut ob, core::ptr::null_mut()) };
        LOG.with(|l| *l.borrow_mut() = Log::default());
    }

    /// `obstack_printf` and `obstack_vprintf` append to the growing object,
    /// with no NUL, and answer how much, as glibc's do -- and the fortified
    /// `__obstack_vprintf_chk` as `obstack_vprintf`.
    #[test]
    fn obstack_printf_is_glibcs() {
        let cases = ORACLE.lines().filter(|l| l.starts_with("P "));
        for (line, chk) in cases.flat_map(|l| [(l, false), (l, true)]) {
            let (lhs, want) = line.split_once(" = ").unwrap();
            let f: Vec<&str> = lhs.splitn(3, ' ').collect();
            let fmt = if f[1] == "\\-" {
                String::new()
            } else {
                String::from(f[1])
            };
            let arg = f[2];
            let mut ob: Obstack = unsafe { core::mem::zeroed() };
            // SAFETY: a writable obstack; the harness's functions.
            unsafe { _obstack_begin(&raw mut ob, 0, 0, Some(chunk), Some(unchunk)) };
            assert!(grow(&mut ob, b"<"));
            let mut cfmt = fmt.clone().into_bytes();
            cfmt.push(0);
            let a = b"a\0";
            let b = b"b\0";
            let mut sarg = String::from(arg).into_bytes();
            sarg.push(0);
            let ints: Vec<u64> = match fmt.as_str() {
                "%d" | "%5000d" => std::vec![arg.parse::<i64>().unwrap() as u64],
                "%c%c" => std::vec![u64::from(arg.as_bytes()[0]), u64::from(arg.as_bytes()[1])],
                "%s-%s" => std::vec![a.as_ptr() as u64, b.as_ptr() as u64],
                _ => std::vec![sarg.as_ptr() as u64],
            };
            let h = &raw mut ob;
            let r = crate::printf::tests::with_valist(&ints, &[], |ap| {
                // SAFETY: a begun obstack; a format and its arguments.
                unsafe {
                    if chk {
                        crate::fortify_printf::__obstack_vprintf_chk(h, 1, cfmt.as_ptr(), ap)
                    } else {
                        crate::printf::obstack_vprintf(h, cfmt.as_ptr(), ap)
                    }
                }
            });
            let size = object_size(&ob) as usize;
            let p = finish(&mut ob);
            // SAFETY: the object's `size` bytes.
            let bytes = unsafe { core::slice::from_raw_parts(p, size) };
            let mut text = format!("{r} {size} ");
            for &c in bytes.iter().take(64) {
                text.push_str(&format!("{c:02x}"));
            }
            if size > 64 {
                let sum = bytes
                    .iter()
                    .fold(0u64, |s, &c| s.wrapping_mul(31).wrapping_add(u64::from(c)));
                text.push_str(&format!("...{sum:x}"));
            }
            assert_eq!(text, want, "{lhs}, the _chk form: {chk}");
            // SAFETY: begun above.
            unsafe { obstack_free(&raw mut ob, core::ptr::null_mut()) };
            LOG.with(|l| *l.borrow_mut() = Log::default());
        }
    }

    /// `obstack_printf` past its chunk when no chunk can be had -- here the
    /// handler's return, standing for its longjmp (`alloc_failed`); a
    /// program's own `_obstack_newchunk` might return without room too --
    /// fails with -1, having written nothing past the chunk it had; the
    /// obstack is whole after, and grows again once chunks can be had.
    #[test]
    fn obstack_printf_fails_when_no_chunk_can_be_had() {
        let _g = HANDLER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: plain reads and writes of a C variable; the tests that set
        // it take turns.
        let saved = unsafe { (&raw const obstack_alloc_failed_handler).read() };
        // SAFETY: as above.
        unsafe { (&raw mut obstack_alloc_failed_handler).write(Some(note_failure)) };
        let mut ob: Obstack = unsafe { core::mem::zeroed() };
        // SAFETY: a writable obstack; the harness's functions.
        unsafe { _obstack_begin(&raw mut ob, 0, 0, Some(chunk), Some(unchunk)) };
        assert!(grow(&mut ob, b"<"));
        LOG.with(|l| l.borrow_mut().fail_in = 1);
        let r = crate::printf::tests::with_valist(&[7], &[], |ap| {
            // SAFETY: a begun obstack; a format and its argument.
            unsafe { crate::printf::obstack_vprintf(&raw mut ob, b"%9000d\0".as_ptr(), ap) }
        });
        assert_eq!(r, -1);
        assert!(LOG.with(|l| l.borrow().failed));
        assert!(ob.next_free <= ob.chunk_limit, "written past the chunk");
        let size = object_size(&ob) as usize;
        // SAFETY: the object's `size` bytes.
        let bytes = unsafe { core::slice::from_raw_parts(ob.object_base, size) };
        assert_eq!(bytes[0], b'<');
        assert!(
            bytes[1..].iter().all(|&c| c == b' '),
            "not a prefix of the output"
        );
        // Chunks to be had again: the next call adds the whole of its output.
        let r = crate::printf::tests::with_valist(&[7], &[], |ap| {
            // SAFETY: as above.
            unsafe { crate::printf::obstack_vprintf(&raw mut ob, b"%5000d\0".as_ptr(), ap) }
        });
        assert_eq!(r, 5000);
        assert_eq!(object_size(&ob) as usize, size + 5000);
        // SAFETY: begun above.
        unsafe { obstack_free(&raw mut ob, core::ptr::null_mut()) };
        // SAFETY: as above.
        unsafe { (&raw mut obstack_alloc_failed_handler).write(saved) };
        LOG.with(|l| *l.borrow_mut() = Log::default());
    }

    /// `_obstack_allocated_p` answers for each chunk's objects, and for
    /// nothing outside them.
    #[test]
    fn allocated_p_knows_each_chunks_objects() {
        let mut ob: Obstack = unsafe { core::mem::zeroed() };
        // SAFETY: a writable obstack; the harness's functions.
        unsafe { _obstack_begin(&raw mut ob, 0, 0, Some(chunk), Some(unchunk)) };
        assert!(blank(&mut ob, 100));
        let first = finish(&mut ob);
        assert!(blank(&mut ob, 10_000));
        let second = finish(&mut ob);
        let outside = [0u8; 4];
        let h = &raw mut ob;
        // SAFETY: a begun obstack; addresses only compared.
        unsafe {
            assert_eq!(_obstack_allocated_p(h, first.cast()), 1);
            assert_eq!(_obstack_allocated_p(h, second.cast()), 1);
            assert_eq!(
                _obstack_allocated_p(h, outside.as_ptr().cast_mut().cast()),
                0
            );
            obstack_free(h, core::ptr::null_mut());
        }
        LOG.with(|l| *l.borrow_mut() = Log::default());
    }
}
