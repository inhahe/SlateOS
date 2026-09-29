//! C standard I/O: `FILE` streams.
//!
//! # Design
//!
//! musl's `FILE` model, with glibc's behaviour wherever a program can tell
//! the two apart (`design-decisions.md` §1121).
//!
//! A [`File`] is a buffer and four operations -- `read`, `write`, `seek`,
//! `close` -- that say what is at the far end: a file descriptor, a
//! program's own callbacks (`fopencookie`), or memory (`fmemopen`,
//! `open_memstream`, `open_wmemstream`, in `stdio_mem.rs`).  Everything
//! between the program and those four -- buffering, `ungetc`, line
//! buffering, positions, locking -- is written once, here, for all of them.
//!
//! The buffer is a *window* in musl's sense: while reading, `rpos..rend` is
//! what is buffered and not yet returned; while writing, `wbase..wpos` is
//! what is buffered and not yet written, and `wend` its limit.  A null window
//! is not active.  `getc` and `putc` are a comparison and a copy while the
//! window lasts; only its edges ([`uflow`], [`overflow`]) reach the far end.
//! [`UNGET`] bytes below the buffer give `ungetc` room to push back into.
//!
//! # What is glibc's
//!
//! - **Mode strings.** `fopen` reads glibc's letters -- `r`, `w`, `a`, then
//!   up to six of `+`, `x` (`O_EXCL`), `e` (`O_CLOEXEC`), `b`, `m`, `c`,
//!   stopping at the end or a `,` -- and `fdopen` and `fopencookie` read
//!   theirs, which are fewer (§1121 has the table).
//! - **Buffering.** A stream is line buffered if its descriptor is a
//!   terminal, decided at its first read or write unless the program chose
//!   with `setvbuf`; otherwise fully buffered, in 4096 bytes -- glibc's size
//!   for a file or a pipe.  `stderr` is unbuffered.  Refilling a line
//!   buffered or unbuffered input stream flushes `stdout` first, if `stdout`
//!   is line buffered: glibc's rule for making a prompt appear.
//! - **A stream opened `a`** starts at the end of the file, so `ftell` says
//!   where the next write lands.
//! - **Reading after writing** on an update stream moves the descriptor back
//!   over read-ahead first, so the write lands where the program's position
//!   is -- glibc's behaviour, not musl's.
//! - **End of file is sticky** until `clearerr`, a seek or `ungetc`.
//! - **`fflush` on an input stream** moves the descriptor back to the
//!   stream's position and discards the read-ahead; on a pipe, which cannot
//!   move, it keeps it.  `fclose` does not; `exit` does, for every stream the
//!   program used.
//! - **Orientation.** A stream is byte or wide from its first byte or wide
//!   call (`fwide`); `fputs`, `puts`, `fwrite`, `printf` and every read fail
//!   on a wide stream, and the wide calls on a byte stream.
//! - **Return values:** `fputs` 1, `puts` the length plus one, `setvbuf` of
//!   an unknown mode `EOF` with `errno` untouched.
//!
//! # Locking
//!
//! Each stream has a recursive lock (glibc's `_IO_lock_t`: a futex word, the
//! owning thread, a depth), taken by every call and by `flockfile`; the
//! `_unlocked` forms skip it, and `__fsetlocking(FSETLOCKING_BYCALLER)`
//! turns it off.  Until 2026-09-27 there was none at all -- `flockfile` was
//! a no-op on the premise that "our stdio is single-threaded" -- so two
//! threads writing one stream raced on its buffer index.
//!
//! # Where the streams live
//!
//! `stdin`, `stdout` and `stderr` are statics with static buffers.  Every
//! other stream is one `malloc` -- the `File`, the pushback room and the
//! buffer -- on a doubly linked list (`fflush(NULL)`, `exit`, `popen`'s
//! children).  Until 2026-09-27 they were 16 slots of a static pool, so a
//! program's 17th open stream failed with `EMFILE`.

use core::sync::atomic::{AtomicI32, Ordering};

use crate::errno;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// End of file, or an error.
pub const EOF: i32 = -1;

/// `fseek` whence: from the start.
pub const SEEK_SET: i32 = 0;
/// `fseek` whence: from the current position.
pub const SEEK_CUR: i32 = 1;
/// `fseek` whence: from the end.
pub const SEEK_END: i32 = 2;

/// `setvbuf`: fully buffered.
pub const _IOFBF: i32 = 0;
/// `setvbuf`: line buffered.
pub const _IOLBF: i32 = 1;
/// `setvbuf`: unbuffered.
pub const _IONBF: i32 = 2;

/// `<stdio.h>`'s `BUFSIZ` as a C program compiled against musl's headers
/// sees it (§1119): the size of the buffer a program hands `setbuf`.  Not
/// the size of the buffers this library allocates, which is [`BUF_SIZE`].
pub const BUFSIZ: usize = 1024;

/// Bytes of pushback room below every buffer, for `ungetc` and `ungetwc` --
/// musl's `UNGET`, enough for one character of any encoding.
const UNGET: usize = 8;

/// The buffer a stream gets unless the program installs its own: glibc's
/// size for a file or a pipe (their `st_blksize`), four times musl's.
const BUF_SIZE: usize = 4096;

/// The standard descriptors.
const STDIN_FD: i32 = 0;
const STDOUT_FD: i32 = 1;
const STDERR_FD: i32 = 2;

/// `__fsetlocking`'s arguments and answers (`<stdio_ext.h>`).
pub const FSETLOCKING_QUERY: i32 = 0;
/// The library locks the stream on every call (the default).
pub const FSETLOCKING_INTERNAL: i32 = 1;
/// The caller does, with `flockfile`.
pub const FSETLOCKING_BYCALLER: i32 = 2;

// Stream flags.

/// One of the three standard streams: static, never freed.
const F_PERM: u32 = 1;
/// Not open for reading.
const F_NORD: u32 = 1 << 1;
/// Not open for writing.
const F_NOWR: u32 = 1 << 2;
/// End of file seen (sticky until cleared).
const F_EOF: u32 = 1 << 3;
/// An error seen.
const F_ERR: u32 = 1 << 4;
/// The program chose the buffering (`setvbuf` and its kin): no terminal
/// check at first use.
const F_SVB: u32 = 1 << 5;
/// Opened for append.
const F_APP: u32 = 1 << 6;
/// First-use setup done.
const F_INIT: u32 = 1 << 7;
/// `__fsetlocking(FSETLOCKING_BYCALLER)`: the calls do not lock.
const F_USERLOCK: u32 = 1 << 8;
/// Opened with `e`: `freopen` keeps close-on-exec on the descriptor it
/// reuses.
const F_CLOEXEC: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// The stream
// ---------------------------------------------------------------------------

/// What is at a stream's far end.  Called with the stream locked.
struct Ops {
    /// Called with the read window empty: store up to `len` bytes at `dst`,
    /// optionally refilling the buffer with more, and return how many went
    /// to `dst` -- 0 at end of file or on an error, having set `F_EOF` or
    /// `F_ERR`.
    read: unsafe fn(*mut File, *mut u8, usize) -> usize,
    /// Write out the pending window, then `len` bytes from `src`; return how
    /// many of `src`'s were written.  After a success the write window is the
    /// whole buffer again; after a failure it is inactive and `F_ERR` set,
    /// and what was pending is gone, as in glibc.
    write: unsafe fn(*mut File, *const u8, usize) -> usize,
    /// `lseek`'s contract.
    seek: unsafe fn(*mut File, i64, i32) -> i64,
    /// Release the far end; 0 or `EOF`.
    close: unsafe fn(*mut File) -> i32,
}

/// A stream's lock: glibc's `_IO_lock_t`, a futex word (`lowlevellock`)
/// with the owning thread's id and a depth, so the owner can take it again.
struct FileLock {
    word: AtomicI32,
    owner: AtomicI32,
    count: u32,
}

impl FileLock {
    /// Free.
    const fn new() -> Self {
        Self {
            word: AtomicI32::new(0),
            owner: AtomicI32::new(0),
            count: 0,
        }
    }
}

/// A C `FILE`.  Opaque to C, which only ever holds a pointer to one.
pub struct File {
    flags: u32,
    rpos: *mut u8,
    rend: *mut u8,
    wbase: *mut u8,
    wpos: *mut u8,
    wend: *mut u8,
    /// Where the data goes: [`UNGET`] bytes of pushback room lie below it.
    buf: *mut u8,
    /// 0 for an unbuffered stream.
    buf_size: usize,
    /// The stream's own buffer and size, for when a program's `setvbuf`
    /// buffer is taken away again (`freopen`).
    own_buf: *mut u8,
    own_size: usize,
    /// `'\n'` if line buffered, `EOF` if not.
    lbf: i32,
    ops: &'static Ops,
    /// The descriptor, or -1 (glibc says -2 for a cookie stream internally,
    /// but `fileno` answers -1 and `EBADF` for both).
    fd: i32,
    /// A cookie or memory stream's state.
    cookie: *mut core::ffi::c_void,
    prev: *mut File,
    next: *mut File,
    lock: FileLock,
    /// Orientation (`fwide`): 0 not yet, negative byte, positive wide.
    mode: i32,
    /// `popen`'s child, or 0.
    pipe_pid: i32,
    /// `fgetln`'s line.
    getln_buf: *mut u8,
    getln_size: usize,
}

impl File {
    /// A stream at `fd` over `buf_size` bytes at `buf`, with `flags`.
    const fn new(
        fd: i32,
        buf: *mut u8,
        buf_size: usize,
        flags: u32,
        lbf: i32,
        ops: &'static Ops,
    ) -> Self {
        Self {
            flags,
            rpos: core::ptr::null_mut(),
            rend: core::ptr::null_mut(),
            wbase: core::ptr::null_mut(),
            wpos: core::ptr::null_mut(),
            wend: core::ptr::null_mut(),
            buf,
            buf_size,
            own_buf: buf,
            own_size: buf_size,
            lbf,
            ops,
            fd,
            cookie: core::ptr::null_mut(),
            prev: core::ptr::null_mut(),
            next: core::ptr::null_mut(),
            lock: FileLock::new(),
            mode: 0,
            pipe_pid: 0,
            getln_buf: core::ptr::null_mut(),
            getln_size: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// The standard streams
// ---------------------------------------------------------------------------

static mut STDIN_BUF: [u8; UNGET + BUF_SIZE] = [0; UNGET + BUF_SIZE];
static mut STDOUT_BUF: [u8; UNGET + BUF_SIZE] = [0; UNGET + BUF_SIZE];
/// `stderr` is unbuffered: pushback room only.
static mut STDERR_BUF: [u8; UNGET] = [0; UNGET];

/// `stdin`: read-only; line buffered if a terminal, decided at first use.
static mut STDIN_FILE: File = File::new(
    STDIN_FD,
    // SAFETY: `UNGET` is within `STDIN_BUF`.
    unsafe { (&raw mut STDIN_BUF).cast::<u8>().add(UNGET) },
    BUF_SIZE,
    F_PERM | F_NOWR,
    EOF,
    &FD_OPS,
);

/// `stdout`: write-only; line buffered if a terminal, decided at first use.
static mut STDOUT_FILE: File = File::new(
    STDOUT_FD,
    // SAFETY: `UNGET` is within `STDOUT_BUF`.
    unsafe { (&raw mut STDOUT_BUF).cast::<u8>().add(UNGET) },
    BUF_SIZE,
    F_PERM | F_NORD,
    EOF,
    &FD_OPS,
);

/// `stderr`: write-only and unbuffered, whatever it is.
static mut STDERR_FILE: File = File::new(
    STDERR_FD,
    // SAFETY: `UNGET` is `STDERR_BUF`'s length: the one-past-the-end pointer.
    unsafe { (&raw mut STDERR_BUF).cast::<u8>().add(UNGET) },
    0,
    F_PERM | F_NORD | F_INIT,
    EOF,
    &FD_OPS,
);

/// `stdin` as a `FILE *`.
pub(crate) fn stdin_stream() -> *mut u8 {
    (&raw mut STDIN_FILE).cast()
}

/// `stdout` as a `FILE *`.
pub(crate) fn stdout_stream() -> *mut u8 {
    (&raw mut STDOUT_FILE).cast()
}

/// `stderr` as a `FILE *`.
pub(crate) fn stderr_stream() -> *mut u8 {
    (&raw mut STDERR_FILE).cast()
}

/// A C `FILE *` as a Rust static: `stdin`, `stdout` and `stderr` are data
/// symbols C reads, and they hold the addresses of the three streams.
#[repr(transparent)]
pub struct StdStream(*mut File);

// SAFETY: the pointer is written once, at compile time, and never through
// this type; sharing it between threads shares an address, and access to the
// stream behind it is serialised by the stream's own lock.
unsafe impl Sync for StdStream {}

/// C's `stdin`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static stdin: StdStream = StdStream(&raw mut STDIN_FILE);
/// C's `stdout`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static stdout: StdStream = StdStream(&raw mut STDOUT_FILE);
/// C's `stderr`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static stderr: StdStream = StdStream(&raw mut STDERR_FILE);
/// glibc's names for the same three, which glibc-built objects reference.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static _IO_stdin_: StdStream = StdStream(&raw mut STDIN_FILE);
/// As [`_IO_stdin_`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static _IO_stdout_: StdStream = StdStream(&raw mut STDOUT_FILE);
/// As [`_IO_stdin_`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static _IO_stderr_: StdStream = StdStream(&raw mut STDERR_FILE);

/// The `File` behind a C `FILE *`: its address.  NULL is no stream: `None`
/// with `errno` `EBADF` (§1120), and each entry point fails as on a stream
/// error, where glibc would fault reading through it.
fn stream_to_file(stream: *mut u8) -> Option<*mut File> {
    if stream.is_null() {
        errno::set_errno(errno::EBADF);
        return None;
    }
    Some(stream.cast::<File>())
}

// ---------------------------------------------------------------------------
// Locking
// ---------------------------------------------------------------------------

/// Take `f`'s lock, recursively.  `false` if the stream is not locked by
/// the library (`FSETLOCKING_BYCALLER`), so nothing is to be released.
///
/// # Safety
///
/// `f` is a live stream.
unsafe fn lock_file(f: *mut File) -> bool {
    // SAFETY: the caller's contract.  `flags` is read without the lock, as
    // glibc reads `_IO_USER_LOCK`: only `__fsetlocking` writes the bit.
    if unsafe { (*f).flags } & F_USERLOCK != 0 {
        return false;
    }
    // SAFETY: as above.
    unsafe { lock_file_always(f) };
    true
}

/// Take `f`'s lock, recursively, whatever `__fsetlocking` said: `flockfile`,
/// which the program asked for by name.
///
/// # Safety
///
/// `f` is a live stream.
unsafe fn lock_file_always(f: *mut File) {
    let tid = crate::pthread::current_tid();
    // SAFETY: the caller's contract.  Only the two atomics are borrowed:
    // `count` beside them is written through the raw pointer, by the owner.
    let (word, owner) = unsafe { (&(*f).lock.word, &(*f).lock.owner) };
    if owner.load(Ordering::Relaxed) == tid {
        // Ours already.
        // SAFETY: as above.
        unsafe { (*f).lock.count = (*f).lock.count.saturating_add(1) };
        return;
    }
    crate::lowlevellock::lll_lock(word);
    owner.store(tid, Ordering::Relaxed);
    // SAFETY: held now.
    unsafe { (*f).lock.count = 1 };
}

/// Take `f`'s lock if it is free or already ours; `false` if another thread
/// holds it.
///
/// # Safety
///
/// As [`lock_file`].
unsafe fn trylock_file(f: *mut File) -> bool {
    let tid = crate::pthread::current_tid();
    // SAFETY: as in `lock_file_always`.
    let (word, owner) = unsafe { (&(*f).lock.word, &(*f).lock.owner) };
    if owner.load(Ordering::Relaxed) == tid {
        // SAFETY: owned by this thread.
        unsafe { (*f).lock.count = (*f).lock.count.saturating_add(1) };
        return true;
    }
    if !crate::lowlevellock::lll_trylock(word) {
        return false;
    }
    owner.store(tid, Ordering::Relaxed);
    // SAFETY: held now.
    unsafe { (*f).lock.count = 1 };
    true
}

/// Release one level of `f`'s lock.
///
/// # Safety
///
/// `f` is a live stream whose lock this thread holds.
unsafe fn unlock_file(f: *mut File) {
    // SAFETY: the caller's contract: this thread is the owner.
    unsafe {
        let count = (*f).lock.count.saturating_sub(1);
        (*f).lock.count = count;
        if count == 0 {
            (*f).lock.owner.store(0, Ordering::Relaxed);
            crate::lowlevellock::lll_unlock(&(*f).lock.word);
        }
    }
}

/// A held stream lock, released when dropped.
struct Locked(*mut File, bool);

impl Drop for Locked {
    fn drop(&mut self) {
        if self.1 {
            // SAFETY: `locked` took it, on this thread.
            unsafe { unlock_file(self.0) };
        }
    }
}

/// Lock `f` for the rest of the scope.
///
/// # Safety
///
/// `f` is a live stream, and outlives the guard.
unsafe fn locked(f: *mut File) -> Locked {
    // SAFETY: the caller's contract.
    let took = unsafe { lock_file(f) };
    Locked(f, took)
}

/// A stream locked for a caller outside this module -- `printf`, which
/// holds it for the whole of one call so that two threads' output cannot
/// interleave inside it, as glibc's `vfprintf` does.  `None` for NULL.
pub(crate) struct StreamGuard {
    _lock: Locked,
}

/// Lock `stream` for the guard's life, and claim byte orientation for it:
/// `None` (with `errno` untouched) if the stream is wide, as glibc's
/// `vfprintf` answers -1 on one.  NULL is `None` with `EBADF`.
pub(crate) fn lock_byte_stream(stream: *mut u8) -> Option<StreamGuard> {
    let f = stream_to_file(stream)?;
    // SAFETY: a non-null `FILE *` is a live stream by C's contract.
    let guard = unsafe { locked(f) };
    // SAFETY: locked.
    if unsafe { orient(f, -1) } > 0 {
        return None;
    }
    Some(StreamGuard { _lock: guard })
}

// ---------------------------------------------------------------------------
// The open-file list
// ---------------------------------------------------------------------------

/// Every stream but the three standard ones, newest first, and the lock
/// that guards the links.
struct OpenList {
    head: *mut File,
    lock: AtomicI32,
}

crate::perprocess::process_global! {
    /// This process's list (one per test thread on the host, so that one
    /// test's `fflush(NULL)` does not reach another's streams).
    fn open_list() -> OpenList = OpenList {
        head: core::ptr::null_mut(),
        lock: AtomicI32::new(0),
    };
}

/// Take the list's lock and return it.
fn list_lock() -> *mut OpenList {
    let list = open_list();
    // SAFETY: `open_list` is never null; the lock word is shared by design.
    crate::lowlevellock::lll_lock(unsafe { &(*list).lock });
    list
}

/// Release the list's lock.
fn list_unlock(list: *mut OpenList) {
    // SAFETY: as `list_lock`.
    crate::lowlevellock::lll_unlock(unsafe { &(*list).lock });
}

/// Put `f` at the head of the list.
///
/// # Safety
///
/// `f` is a new stream on no list.
unsafe fn list_add(f: *mut File) {
    let list = list_lock();
    // SAFETY: the list is locked; `f` is the caller's.
    unsafe {
        (*f).next = (*list).head;
        (*f).prev = core::ptr::null_mut();
        if !(*list).head.is_null() {
            (*(*list).head).prev = f;
        }
        (*list).head = f;
    }
    list_unlock(list);
}

/// Take `f` off the list.
///
/// # Safety
///
/// `f` is on this process's list.
unsafe fn list_remove(f: *mut File) {
    let list = list_lock();
    // SAFETY: the list is locked, and `f` and its neighbours are on it.
    unsafe {
        if !(*f).prev.is_null() {
            (*(*f).prev).next = (*f).next;
        }
        if !(*f).next.is_null() {
            (*(*f).next).prev = (*f).prev;
        }
        if core::ptr::eq((*list).head, f) {
            (*list).head = (*f).next;
        }
    }
    list_unlock(list);
}

/// Call `each` on every stream: the three standard ones, then the list's,
/// with the list locked throughout (so no stream on it can be freed under
/// `each`, which `fclose` does only after taking it off).
fn for_each_stream(mut each: impl FnMut(*mut File)) {
    each(&raw mut STDIN_FILE);
    each(&raw mut STDOUT_FILE);
    each(&raw mut STDERR_FILE);
    let list = list_lock();
    // SAFETY: the list is locked, so every stream on it is live.
    let mut f = unsafe { (*list).head };
    while !f.is_null() {
        each(f);
        // SAFETY: as above.
        f = unsafe { (*f).next };
    }
    list_unlock(list);
}

// ---------------------------------------------------------------------------
// fork
// ---------------------------------------------------------------------------

/// The forking thread's id, from [`lock_for_fork`] to the child's reset.
static FORKING_TID: AtomicI32 = AtomicI32::new(0);

/// Before `fork`: hold the list's lock across it, so the child's list is
/// not caught half-linked -- glibc's `_IO_list_lock`.
pub(crate) fn lock_for_fork() {
    FORKING_TID.store(crate::pthread::current_tid(), Ordering::Relaxed);
    let list = open_list();
    // SAFETY: never null.
    crate::lowlevellock::lll_lock(unsafe { &(*list).lock });
}

/// After `fork`, in the parent.
pub(crate) fn unlock_after_fork_parent() {
    list_unlock(open_list());
}

/// After `fork`, in the child: the list is the child's alone, and every
/// stream lock another thread held is released, since that thread is not in
/// the child -- glibc's `fresetlockfiles`.  A lock the forking thread held
/// stays held, re-owned by its new id, which glibc does not do: glibc resets
/// those too, and the forking thread's later `funlockfile` then releases a
/// lock it no longer has.
pub(crate) fn unlock_after_fork_child() {
    let old = FORKING_TID.load(Ordering::Relaxed);
    let list = open_list();
    // SAFETY: never null; the child is single-threaded.
    unsafe { (*list).lock.store(0, Ordering::Relaxed) };
    let new = crate::pthread::current_tid();
    // SAFETY: live streams, and the child has one thread.
    for_each_stream(|f| unsafe { reset_lock_after_fork(f, old, new) });
}

/// One stream's lock in a fork child: re-owned by `new` if `old` (the
/// forking thread) held it, released if anyone else did.
///
/// # Safety
///
/// `f` is live and no other thread can reach it.
unsafe fn reset_lock_after_fork(f: *mut File, old: i32, new: i32) {
    // SAFETY: the caller's contract.
    unsafe {
        if (*f).lock.owner.load(Ordering::Relaxed) == old && (*f).lock.count > 0 {
            (*f).lock.owner.store(new, Ordering::Relaxed);
        } else {
            (*f).lock.word.store(0, Ordering::Relaxed);
            (*f).lock.owner.store(0, Ordering::Relaxed);
            (*f).lock.count = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// The descriptor end
// ---------------------------------------------------------------------------

static FD_OPS: Ops = Ops {
    read: fd_read,
    write: fd_write,
    seek: fd_seek,
    close: fd_close,
};

/// Read through `f`'s descriptor.  A request as large as the buffer, or any
/// request on an unbuffered stream, goes straight to the caller; anything
/// smaller refills the buffer with one `read` and takes its share.
///
/// Refilling a line buffered or unbuffered stream flushes `stdout` first if
/// `stdout` is line buffered: glibc's `_IO_new_file_underflow`, and the reason
/// `printf("name: "); fgets(…, stdin)` shows its prompt on a terminal.
unsafe fn fd_read(f: *mut File, dst: *mut u8, len: usize) -> usize {
    // SAFETY: the caller holds `f`'s lock.
    let file = unsafe { &mut *f };
    let direct = file.buf_size == 0 || len >= file.buf_size;
    if file.lbf == i32::from(b'\n') || file.buf_size == 0 {
        flush_stdout_for_input(f);
    }
    // SAFETY: as above -- re-borrowed after the flush, which took only
    // `stdout`'s lock and wrote only `stdout`.
    let file = unsafe { &mut *f };
    let (to, want) = if direct {
        (dst, len)
    } else {
        (file.buf, file.buf_size)
    };
    let n = crate::file::read(file.fd, to, want);
    let Ok(got) = usize::try_from(n) else {
        file.flags |= F_ERR;
        return 0;
    };
    if got == 0 {
        file.flags |= F_EOF;
        return 0;
    }
    if direct {
        return got;
    }
    let take = got.min(len);
    // SAFETY: `got <= buf_size` bytes were read into the buffer and `dst`
    // holds `len >= take`; the two do not overlap.
    unsafe {
        core::ptr::copy_nonoverlapping(file.buf, dst, take);
        file.rpos = file.buf.add(take);
        file.rend = file.buf.add(got);
    }
    take
}

/// Flush `stdout` if it is line buffered and has output waiting, for an input
/// stream about to block on its descriptor.  Not when `f` is `stdout`.
fn flush_stdout_for_input(f: *mut File) {
    let out = &raw mut STDOUT_FILE;
    if core::ptr::eq(f, out) {
        return;
    }
    // SAFETY: `stdout` is static; locked for the check and the flush.
    unsafe {
        let _g = locked(out);
        if (*out).lbf == i32::from(b'\n')
            && (*out).flags & F_NOWR == 0
            && (*out).wpos != (*out).wbase
        {
            ((*out).ops.write)(out, core::ptr::null(), 0);
        }
    }
}

/// Write all of `len` bytes at `src` to `fd`, retrying short writes: `false`
/// on an error, or on a write that took nothing.
fn write_all(fd: i32, mut src: *const u8, mut len: usize) -> bool {
    while len > 0 {
        let n = crate::file::write(fd, src, len);
        let Ok(done) = usize::try_from(n) else {
            return false;
        };
        if done == 0 {
            return false;
        }
        let done = done.min(len);
        // SAFETY: `done <= len` bytes of `src` were readable.
        src = unsafe { src.add(done) };
        len = len.wrapping_sub(done);
    }
    true
}

/// Write `f`'s pending window, then `len` bytes at `src`, to its descriptor.
unsafe fn fd_write(f: *mut File, src: *const u8, len: usize) -> usize {
    // SAFETY: the caller holds `f`'s lock.
    let file = unsafe { &mut *f };
    // SAFETY: `wbase..wpos` is inside the buffer when active, and both are
    // null (length 0) when not.
    let pending = unsafe { pending_bytes(file) };
    if pending > 0 && !write_all(file.fd, file.wbase, pending) {
        write_failed(file);
        return 0;
    }
    if len > 0 && !write_all(file.fd, src, len) {
        write_failed(file);
        return 0;
    }
    reset_write_window(file);
    len
}

/// Bytes in `f`'s write window.
///
/// # Safety
///
/// `wbase..wpos` is `f`'s window (or both null).
unsafe fn pending_bytes(file: &File) -> usize {
    if file.wbase.is_null() {
        return 0;
    }
    // SAFETY: the caller's contract: same allocation, `wbase <= wpos`.
    usize::try_from(unsafe { file.wpos.offset_from(file.wbase) }).unwrap_or(0)
}

/// After a write failed: the window goes, with what it held, and the stream
/// is in error -- glibc's `new_do_write` empties the buffer either way.
fn write_failed(file: &mut File) {
    file.wbase = core::ptr::null_mut();
    file.wpos = core::ptr::null_mut();
    file.wend = core::ptr::null_mut();
    file.flags |= F_ERR;
}

/// After a write succeeded: the whole buffer is the window again.
fn reset_write_window(file: &mut File) {
    file.wbase = file.buf;
    file.wpos = file.buf;
    // SAFETY: `buf_size` bytes from `buf` are the buffer.
    file.wend = unsafe { file.buf.add(file.buf_size) };
}

unsafe fn fd_seek(f: *mut File, off: i64, whence: i32) -> i64 {
    // SAFETY: the caller holds `f`'s lock.
    crate::file::lseek(unsafe { (*f).fd }, off, whence)
}

unsafe fn fd_close(f: *mut File) -> i32 {
    // SAFETY: as above.
    if crate::file::close(unsafe { (*f).fd }) < 0 {
        EOF
    } else {
        0
    }
}

/// Whether `fd` is a terminal, without disturbing `errno` -- glibc's
/// `local_isatty`, which `isatty`'s `ENOTTY` would otherwise leak into a
/// program's first `printf`.
fn fd_is_tty(fd: i32) -> bool {
    let saved = errno::get_errno();
    let tty = crate::ioctl::isatty(fd) == 1;
    errno::set_errno(saved);
    tty
}

// ---------------------------------------------------------------------------
// Orientation and first use
// ---------------------------------------------------------------------------

/// `fwide`'s core: claim `want`'s orientation for `f` if it has none, and
/// return the orientation it has.
///
/// # Safety
///
/// `f` is locked (or its caller's).
pub(crate) unsafe fn orient(f: *mut File, want: i32) -> i32 {
    // SAFETY: the caller's contract.
    let file = unsafe { &mut *f };
    if file.mode == 0 && want != 0 {
        file.mode = want.signum();
    }
    file.mode
}

/// The setup glibc does when a stream first gets its buffer: a descriptor
/// stream whose buffering the program did not choose is line buffered if it
/// is a terminal.
fn first_use(file: &mut File) {
    if file.flags & F_INIT != 0 {
        return;
    }
    file.flags |= F_INIT;
    if file.flags & F_SVB == 0 && core::ptr::eq(file.ops, &FD_OPS) && fd_is_tty(file.fd) {
        file.lbf = i32::from(b'\n');
    }
}

// ---------------------------------------------------------------------------
// Switching direction
// ---------------------------------------------------------------------------

/// Make `f` ready to read: flush what it was writing, and open an empty read
/// window at the top of the buffer, leaving the whole buffer below it for
/// `ungetc`.  `EOF` if it cannot be read (`F_ERR`, `EBADF`), or if it has
/// seen end of file, which is sticky; 0 otherwise.
///
/// # Safety
///
/// `f` is locked.
unsafe fn toread(f: *mut File) -> i32 {
    // SAFETY: the caller's contract.
    let file = unsafe { &mut *f };
    first_use(file);
    if file.wpos != file.wbase {
        // SAFETY: `f` is locked; a flush writes only the pending window.
        unsafe { (file.ops.write)(f, core::ptr::null(), 0) };
    }
    // SAFETY: re-borrowed after the call through `ops`.
    let file = unsafe { &mut *f };
    file.wbase = core::ptr::null_mut();
    file.wpos = core::ptr::null_mut();
    file.wend = core::ptr::null_mut();
    if file.flags & F_NORD != 0 {
        file.flags |= F_ERR;
        errno::set_errno(errno::EBADF);
        return EOF;
    }
    // SAFETY: `buf_size` bytes from `buf` are the buffer.
    let top = unsafe { file.buf.add(file.buf_size) };
    file.rpos = top;
    file.rend = top;
    if file.flags & F_EOF != 0 { EOF } else { 0 }
}

/// Make `f` ready to write: `EOF` if it cannot be written (`F_ERR`,
/// `EBADF`).  A read window with read-ahead in it is given back to the far
/// end first, so the write lands at the stream's position and not after the
/// read-ahead -- glibc's behaviour on an update stream.
///
/// # Safety
///
/// `f` is locked.
unsafe fn towrite(f: *mut File) -> i32 {
    // SAFETY: the caller's contract.
    let file = unsafe { &mut *f };
    first_use(file);
    if file.mode == 0 {
        file.mode = -1;
    }
    if file.flags & F_NOWR != 0 {
        file.flags |= F_ERR;
        errno::set_errno(errno::EBADF);
        return EOF;
    }
    if file.rpos != file.rend {
        // SAFETY: `rpos..rend` is the read window, one allocation.
        let unread = unsafe { file.rend.offset_from(file.rpos) };
        let saved = errno::get_errno();
        // SAFETY: `f` is locked.  A stream that cannot seek (a pipe) keeps
        // its old place, and the failure is not the program's to see.
        if unsafe { (file.ops.seek)(f, unread.wrapping_neg() as i64, SEEK_CUR) } < 0 {
            errno::set_errno(saved);
        }
    }
    // SAFETY: re-borrowed after the call through `ops`.
    let file = unsafe { &mut *f };
    file.rpos = core::ptr::null_mut();
    file.rend = core::ptr::null_mut();
    reset_write_window(file);
    0
}

// ---------------------------------------------------------------------------
// The window's edges
// ---------------------------------------------------------------------------

/// `getc`'s slow path: the read window is empty.  Claims byte orientation,
/// and fails on a wide stream (glibc's `__uflow`).
///
/// # Safety
///
/// `f` is locked.
unsafe fn uflow(f: *mut File) -> i32 {
    // SAFETY: the caller's contract.
    if unsafe { orient(f, -1) } > 0 {
        return EOF;
    }
    // SAFETY: as above.
    unsafe { uflow_bytes(f) }
}

/// [`uflow`] without the orientation check: the wide functions' byte
/// source, whose stream is wide.
///
/// # Safety
///
/// `f` is locked.
unsafe fn uflow_bytes(f: *mut File) -> i32 {
    // SAFETY: the caller's contract.
    if unsafe { toread(f) } != 0 {
        return EOF;
    }
    let mut c = 0u8;
    // SAFETY: `f` is locked and `c` holds one byte.
    if unsafe { ((*f).ops.read)(f, &raw mut c, 1) } == 1 {
        i32::from(c)
    } else {
        EOF
    }
}

/// `putc`'s slow path: the write window is full, inactive, or `c` ends a
/// line on a line buffered stream.
///
/// # Safety
///
/// `f` is locked.
unsafe fn overflow(f: *mut File, c: u8) -> i32 {
    // SAFETY: the caller's contract.
    if unsafe { (*f).wend.is_null() } && unsafe { towrite(f) } != 0 {
        return EOF;
    }
    // SAFETY: as above.
    let file = unsafe { &mut *f };
    if file.wpos != file.wend && i32::from(c) != file.lbf {
        // SAFETY: `wpos < wend`, inside the buffer.
        unsafe {
            *file.wpos = c;
            file.wpos = file.wpos.add(1);
        }
        return i32::from(c);
    }
    // SAFETY: `f` is locked; `c` is one byte.
    if unsafe { (file.ops.write)(f, &raw const c, 1) } != 1 {
        return EOF;
    }
    i32::from(c)
}

/// `getc` without the lock.
///
/// # Safety
///
/// `f` is locked, or the caller has taken responsibility (`_unlocked`).
#[inline]
pub(crate) unsafe fn getc_raw(f: *mut File) -> i32 {
    // SAFETY: the caller's contract.
    let file = unsafe { &mut *f };
    if file.rpos != file.rend {
        // SAFETY: `rpos < rend`, inside the buffer.
        unsafe {
            let c = *file.rpos;
            file.rpos = file.rpos.add(1);
            return i32::from(c);
        }
    }
    // SAFETY: as above.
    unsafe { uflow(f) }
}

/// `putc` without the lock.
///
/// # Safety
///
/// As [`getc_raw`].
#[inline]
pub(crate) unsafe fn putc_raw(c: i32, f: *mut File) -> i32 {
    // C passes an int and writes it as an unsigned char.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = c as u8;
    // SAFETY: the caller's contract.
    let file = unsafe { &mut *f };
    if i32::from(byte) != file.lbf && file.wpos != file.wend {
        // SAFETY: `wpos < wend`, inside the buffer.
        unsafe {
            *file.wpos = byte;
            file.wpos = file.wpos.add(1);
        }
        return i32::from(byte);
    }
    // SAFETY: as above.
    unsafe { overflow(f, byte) }
}

/// Write `len` bytes through `f`'s buffer: musl's `__fwritex`.  Returns how
/// many were accepted.  A line buffered stream writes out through the last
/// newline at once and buffers the rest; a request larger than the room left
/// goes straight to the far end, after what was pending.
///
/// # Safety
///
/// `f` is locked; `src` holds `len` bytes.
unsafe fn fwritex(src: *const u8, len: usize, f: *mut File) -> usize {
    // SAFETY: the caller's contract.
    if unsafe { (*f).wend.is_null() } && unsafe { towrite(f) } != 0 {
        return 0;
    }
    // SAFETY: as above.
    let file = unsafe { &mut *f };
    // SAFETY: `wpos..wend` is the window, one allocation.
    let room = usize::try_from(unsafe { file.wend.offset_from(file.wpos) }).unwrap_or(0);
    if len > room {
        // SAFETY: `f` is locked; `src` holds `len`.
        return unsafe { (file.ops.write)(f, src, len) };
    }
    let mut head = 0;
    if file.lbf >= 0 {
        // SAFETY: `src` holds `len` bytes.
        let bytes = unsafe { core::slice::from_raw_parts(src, len) };
        if let Some(nl) = bytes.iter().rposition(|&b| i32::from(b) == file.lbf) {
            head = nl.wrapping_add(1);
            // SAFETY: `f` is locked; `head <= len`.
            let n = unsafe { (file.ops.write)(f, src, head) };
            if n < head {
                return n;
            }
        }
    }
    // SAFETY: re-borrowed after the call through `ops`.
    let file = unsafe { &mut *f };
    let tail = len.wrapping_sub(head);
    // SAFETY: `tail <= len <= room`, so it fits at `wpos` (the window was
    // reset to the whole buffer if the head was written); `src + head` holds
    // `tail` bytes.
    unsafe {
        core::ptr::copy_nonoverlapping(src.add(head), file.wpos, tail);
        file.wpos = file.wpos.add(tail);
    }
    len
}

/// Read up to `len` bytes into `dst`: the buffered ones first, then from the
/// far end until satisfied or it says end of file or error -- so a `fread`
/// from a pipe waits for all it asked for, as glibc's does.  Returns how many
/// were read.
///
/// # Safety
///
/// `f` is locked; `dst` holds `len` bytes.
unsafe fn fread_raw(dst: *mut u8, len: usize, f: *mut File) -> usize {
    // SAFETY: the caller's contract.
    if unsafe { orient(f, -1) } > 0 {
        return 0;
    }
    // SAFETY: as above.
    let file = unsafe { &mut *f };
    let mut done = 0usize;
    if file.rpos != file.rend {
        // SAFETY: `rpos..rend` is the window.
        let buffered = usize::try_from(unsafe { file.rend.offset_from(file.rpos) }).unwrap_or(0);
        let k = buffered.min(len);
        // SAFETY: `k` bytes are buffered and `dst` holds `len >= k`.
        unsafe {
            core::ptr::copy_nonoverlapping(file.rpos, dst, k);
            file.rpos = file.rpos.add(k);
        }
        done = k;
    }
    while done < len {
        // SAFETY: `f` is locked.
        if unsafe { toread(f) } != 0 {
            break;
        }
        // SAFETY: `dst + done` holds `len - done` bytes.
        let k = unsafe { ((*f).ops.read)(f, dst.add(done), len.wrapping_sub(done)) };
        if k == 0 {
            break;
        }
        done = done.wrapping_add(k);
    }
    done
}

// ---------------------------------------------------------------------------
// Making and unmaking streams
// ---------------------------------------------------------------------------

/// Where a stream's buffer starts in its allocation: after the `File` and
/// the pushback room.
const BUF_OFFSET: usize = core::mem::size_of::<File>() + UNGET;

/// Bytes to allocate for a stream with its own buffer: the `File`, the
/// pushback room, and [`BUF_SIZE`].
const STREAM_ALLOC: usize = BUF_OFFSET + BUF_SIZE;

/// A new stream with its own buffer, on the open-file list: `None` (with
/// `ENOMEM`) if the allocation fails.
///
/// # Safety
///
/// `cookie` is what `ops` expects.
unsafe fn new_stream(
    fd: i32,
    flags: u32,
    ops: &'static Ops,
    cookie: *mut core::ffi::c_void,
) -> Option<*mut File> {
    let mem = crate::malloc::malloc(STREAM_ALLOC);
    if mem.is_null() {
        errno::set_errno(errno::ENOMEM);
        return None;
    }
    let f = mem.cast::<File>();
    // SAFETY: `mem` holds `STREAM_ALLOC` bytes, suitably aligned for `File`
    // (`malloc`'s alignment is `max_align_t`'s); the buffer follows the
    // `File` and its pushback room.
    unsafe {
        let buf = mem.add(BUF_OFFSET);
        f.write(File::new(fd, buf, BUF_SIZE, flags, EOF, ops));
        (*f).cookie = cookie;
        list_add(f);
    }
    Some(f)
}

/// Free a stream [`new_stream`] made, once it is off the list.
///
/// # Safety
///
/// `f` came from `new_stream` and nothing else refers to it.
unsafe fn free_stream(f: *mut File) {
    // SAFETY: the caller's contract; `getln_buf` is `fgetln`'s allocation or
    // null.
    unsafe {
        crate::malloc::free((*f).getln_buf);
        crate::malloc::free(f.cast());
    }
}

/// What a mode string asks for.
struct Mode {
    /// `open` flags.
    oflags: i32,
    /// `F_NORD`/`F_NOWR`/`F_APP`/`F_CLOEXEC`.
    flags: u32,
}

/// The first letter of a mode: `(access flags, stream flags)`, or `None`
/// for anything but `r`, `w` and `a`.
fn mode_letter(c: u8) -> Option<(i32, u32)> {
    use crate::fcntl::{O_APPEND, O_CREAT, O_RDONLY, O_TRUNC, O_WRONLY};
    match c {
        b'r' => Some((O_RDONLY, F_NOWR)),
        b'w' => Some((O_WRONLY | O_CREAT | O_TRUNC, F_NORD)),
        b'a' => Some((O_WRONLY | O_CREAT | O_APPEND, F_NORD | F_APP)),
        _ => None,
    }
}

/// `+` on a mode: read and write.
fn plus(m: &mut Mode) {
    m.oflags = (m.oflags & !crate::fcntl::O_ACCMODE) | crate::fcntl::O_RDWR;
    m.flags &= !(F_NORD | F_NOWR);
}

/// `fopen`'s mode, as glibc's `_IO_new_file_fopen` reads it: `r`, `w` or
/// `a`, then at most six more letters, stopping at the end or at `,` (where
/// glibc's `,ccs=` would follow): `+` read and write, `x` `O_EXCL`, `e`
/// `O_CLOEXEC`, and `b`, `m`, `c` and anything else ignored.
///
/// # Safety
///
/// `mode` is a C string.
unsafe fn fopen_mode(mode: *const u8) -> Option<Mode> {
    // SAFETY: the caller's contract.
    let (oflags, flags) = mode_letter(unsafe { *mode })?;
    let mut m = Mode { oflags, flags };
    for i in 1..7usize {
        // SAFETY: the loop stops at the NUL, so every byte read is in the
        // string.
        match unsafe { *mode.add(i) } {
            0 | b',' => break,
            b'+' => plus(&mut m),
            b'x' => m.oflags |= crate::fcntl::O_EXCL,
            b'e' => {
                m.oflags |= crate::fcntl::O_CLOEXEC;
                m.flags |= F_CLOEXEC;
            }
            _ => {}
        }
    }
    Some(m)
}

/// Open `path` as a stream.
///
/// NULL for either argument is `EFAULT`, the substitute for the fault glibc
/// would take (§1115, §303): for the path the kernel's own answer, for the
/// mode at the place glibc reads it, first.  A bad first letter is
/// `EINVAL`.  A stream opened `a` (not `a+`) starts at the end of the file,
/// as glibc's does, so `ftell` answers where the next write will land.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fopen(path: *const u8, mode: *const u8) -> *mut u8 {
    if mode.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: a non-null mode is a C string by C's contract.
    let Some(m) = (unsafe { fopen_mode(mode) }) else {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    };
    let fd = crate::file::open(path, m.oflags, 0o666);
    if fd < 0 {
        return core::ptr::null_mut();
    }
    // SAFETY: descriptor streams keep no cookie.
    let Some(f) = (unsafe { new_stream(fd, m.flags, &FD_OPS, core::ptr::null_mut()) }) else {
        crate::file::close(fd);
        return core::ptr::null_mut();
    };
    if m.flags & (F_APP | F_NORD) == F_APP | F_NORD && !seek_to_end(fd) {
        // SAFETY: `f` is ours alone.
        unsafe {
            list_remove(f);
            free_stream(f);
        }
        let e = errno::get_errno();
        crate::file::close(fd);
        errno::set_errno(e);
        return core::ptr::null_mut();
    }
    f.cast()
}

/// Move `fd` to its end: `false` on an error other than `ESPIPE` (a pipe
/// has no end to move to, and glibc does not count that).
fn seek_to_end(fd: i32) -> bool {
    let saved = errno::get_errno();
    if crate::file::lseek(fd, 0, SEEK_END) >= 0 {
        return true;
    }
    if errno::get_errno() == errno::ESPIPE {
        errno::set_errno(saved);
        return true;
    }
    false
}

/// `fopen64`: `fopen`, off_t being 64 bits already.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fopen64(path: *const u8, mode: *const u8) -> *mut u8 {
    // SAFETY: the caller's contract, forwarded.
    unsafe { fopen(path, mode) }
}

/// A stream on an open descriptor.
///
/// glibc's `_IO_new_fdopen`: the mode is `r`, `w` or `a` and at most four
/// more letters, of which only `+` counts (and ends the reading) -- `e`, `x`
/// and the rest are ignored, as glibc ignores them here.  The descriptor must
/// be open (`EBADF`, from `fcntl`) with an access mode that allows the
/// stream's (`EINVAL`).  `a` puts `O_APPEND` on the descriptor if it lacks
/// it, and then, for a write-only stream, moves to the end.
///
/// A new stream, whatever the descriptor: `fdopen(1, "w")` is a second
/// stream on standard output with its own buffer, not `stdout`, as in glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fdopen(fd: i32, mode: *const u8) -> *mut u8 {
    use crate::fcntl::{O_ACCMODE, O_APPEND, O_RDONLY, O_WRONLY};
    use crate::fcntl_ops::{F_GETFL, F_SETFL};
    if mode.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: a C string by C's contract.
    let Some((_, mut flags)) = mode_letter(unsafe { *mode }) else {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    };
    for i in 1..5usize {
        // SAFETY: stops at the NUL.
        match unsafe { *mode.add(i) } {
            0 => break,
            b'+' => {
                flags &= !(F_NORD | F_NOWR);
                break;
            }
            _ => {}
        }
    }
    let fl = crate::fcntl_ops::fcntl(fd, F_GETFL, 0);
    if fl == -1 {
        return core::ptr::null_mut();
    }
    let acc = fl & O_ACCMODE;
    if (acc == O_RDONLY && flags & F_NOWR == 0) || (acc == O_WRONLY && flags & F_NORD == 0) {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }
    let mut do_seek = false;
    if flags & F_APP != 0 && fl & O_APPEND == 0 {
        do_seek = true;
        if crate::fcntl_ops::fcntl(fd, F_SETFL, i64::from(fl | O_APPEND)) == -1 {
            return core::ptr::null_mut();
        }
    }
    // SAFETY: descriptor streams keep no cookie.
    let Some(f) = (unsafe { new_stream(fd, flags, &FD_OPS, core::ptr::null_mut()) }) else {
        return core::ptr::null_mut();
    };
    if do_seek && flags & (F_APP | F_NORD) == F_APP | F_NORD && !seek_to_end(fd) {
        // SAFETY: `f` is ours alone.
        unsafe {
            list_remove(f);
            free_stream(f);
        }
        return core::ptr::null_mut();
    }
    f.cast()
}

/// Flush `f`'s output and give back its read-ahead: glibc's `_IO_new_file_sync`.
/// `EOF` if the output could not be written, or the descriptor could not be
/// moved back for a reason other than being a pipe -- in which case the
/// read-ahead is kept, since it cannot be read again.
///
/// # Safety
///
/// `f` is locked.
unsafe fn sync(f: *mut File) -> i32 {
    // SAFETY: the caller's contract.
    let file = unsafe { &mut *f };
    if file.wpos != file.wbase {
        // SAFETY: `f` is locked.
        unsafe { (file.ops.write)(f, core::ptr::null(), 0) };
        // SAFETY: re-borrowed.
        if unsafe { (*f).wpos.is_null() } {
            return EOF;
        }
    }
    // SAFETY: re-borrowed.
    let file = unsafe { &mut *f };
    if file.rpos != file.rend {
        // SAFETY: the read window, one allocation.
        let unread = unsafe { file.rend.offset_from(file.rpos) };
        let saved = errno::get_errno();
        // SAFETY: `f` is locked.
        if unsafe { (file.ops.seek)(f, unread.wrapping_neg() as i64, SEEK_CUR) } < 0 {
            if errno::get_errno() == errno::ESPIPE {
                errno::set_errno(saved);
                return 0;
            }
            return EOF;
        }
    }
    // SAFETY: re-borrowed.
    let file = unsafe { &mut *f };
    file.wbase = core::ptr::null_mut();
    file.wpos = core::ptr::null_mut();
    file.wend = core::ptr::null_mut();
    file.rpos = core::ptr::null_mut();
    file.rend = core::ptr::null_mut();
    0
}

/// Write out `f`'s pending output, if any: `EOF` if that failed.
///
/// # Safety
///
/// `f` is locked.
unsafe fn flush_output(f: *mut File) -> i32 {
    // SAFETY: the caller's contract.
    unsafe {
        if (*f).wpos != (*f).wbase {
            ((*f).ops.write)(f, core::ptr::null(), 0);
            if (*f).wpos.is_null() {
                return EOF;
            }
        }
    }
    0
}

/// Flush a stream; NULL flushes every stream's output.
///
/// On an input stream, glibc's `fflush` moves the descriptor back to the
/// stream's position and discards what was read ahead (see [`sync`]).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fflush(stream: *mut u8) -> i32 {
    if stream.is_null() {
        return flush_all();
    }
    let f = stream.cast::<File>();
    // SAFETY: a non-null `FILE *` is a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { sync(f) }
}

/// `fflush` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fflush_unlocked(stream: *mut u8) -> i32 {
    if stream.is_null() {
        return flush_all();
    }
    // SAFETY: the caller holds the stream, by the `_unlocked` contract.
    unsafe { sync(stream.cast()) }
}

/// `fflush(NULL)`: every stream's pending output (glibc's `_IO_flush_all`,
/// which leaves input streams alone).
fn flush_all() -> i32 {
    let mut r = 0;
    for_each_stream(|f| {
        // SAFETY: a live stream, locked for the flush.
        unsafe {
            let _g = locked(f);
            if flush_output(f) != 0 {
                r = EOF;
            }
        }
    });
    r
}

/// What `exit` does to the streams, as glibc's `_IO_cleanup` does: every
/// stream's output flushed, and every stream the program used synced -- an
/// input stream's descriptor moved back to where the program stopped
/// reading, so a process that inherits it continues from there.
pub(crate) fn exit_cleanup() {
    for_each_stream(|f| {
        // SAFETY: a live stream, locked for the flush.
        unsafe {
            let _g = locked(f);
            let _ = flush_output(f); // exit has no one to tell
            if (*f).mode != 0 || (*f).rpos != (*f).rend {
                let _ = sync(f); // as above
            }
        }
    });
}

/// Close a stream: flush its output, release its far end, and free it (the
/// three standard streams are static and stay, closed).  Pending input is
/// not given back to the descriptor, as in glibc.  The close's answer if it
/// is not 0 (`EOF`, or a `popen` child's wait status), else `EOF` if the
/// flush failed; the stream is gone either way.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fclose(stream: *mut u8) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return EOF;
    };
    let r;
    {
        // SAFETY: a live stream.
        let _g = unsafe { locked(f) };
        // SAFETY: locked.
        unsafe {
            let wrote = flush_output(f);
            // glibc's `_IO_new_file_close_it`: the close's answer if it is
            // not 0 -- for a `popen` stream, the child's wait status -- else
            // the flush's.
            let closed = ((*f).ops.close)(f);
            r = if closed != 0 { closed } else { wrote };
            let file = &mut *f;
            file.rpos = core::ptr::null_mut();
            file.rend = core::ptr::null_mut();
            file.wbase = core::ptr::null_mut();
            file.wpos = core::ptr::null_mut();
            file.wend = core::ptr::null_mut();
            file.fd = -1;
        }
    }
    // SAFETY: the flags are ours to read; the stream is closed.
    if unsafe { (*f).flags } & F_PERM != 0 {
        return r;
    }
    // SAFETY: a heap stream on the list; off it before it is freed, so no
    // `fflush(NULL)` can reach it after.
    unsafe {
        list_remove(f);
        free_stream(f);
    }
    r
}

/// Close every stream but the three standard ones (glibc's `fcloseall`,
/// which flushes those three instead).  0, or `EOF` if any close failed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fcloseall() -> i32 {
    let mut r = if flush_all() == 0 { 0 } else { EOF };
    loop {
        let list = list_lock();
        // SAFETY: locked.
        let head = unsafe { (*list).head };
        list_unlock(list);
        if head.is_null() {
            return r;
        }
        if fclose(head.cast()) != 0 {
            r = EOF;
        }
    }
}

/// Reopen `stream` on `path` with `mode`, keeping its descriptor number, or
/// change its mode if `path` is NULL (glibc's `freopen`).
///
/// glibc opens the new file first, then moves it onto the stream's old
/// descriptor with `dup3`; if the open fails the stream is closed and NULL
/// returned.  With NULL `path` it does the same with `/proc/self/fd/N`: a new
/// open of the same file, so the position starts again (at the end for
/// `a`), `w` truncates, and the access mode can change.  Where there is
/// nothing to reopen by name -- a pipe or a socket, whose `/proc` link names
/// no file on this system (`ENOENT`, `ENXIO`) -- the descriptor's flags are
/// changed in place instead, as musl does, so the common
/// `freopen(NULL, "wb", stdout)` still works on a pipe; that way cannot
/// change the access mode, and a mode that needs it is refused (`EBADF`).
///
/// A cookie or memory stream is not reopened: NULL, the stream untouched,
/// as glibc's `_IO_IS_FILEBUF` check leaves it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn freopen(path: *const u8, mode: *const u8, stream: *mut u8) -> *mut u8 {
    let Some(f) = stream_to_file(stream) else {
        return core::ptr::null_mut();
    };
    // SAFETY: a live stream.
    let g = unsafe { locked(f) };
    // SAFETY: locked; glibc ignores a failed flush here.
    let _ = unsafe { sync(f) };
    // SAFETY: locked.
    if !core::ptr::eq(unsafe { (*f).ops }, &FD_OPS) {
        return core::ptr::null_mut();
    }
    if mode.is_null() {
        errno::set_errno(errno::EFAULT);
        drop(g);
        fclose(stream);
        return core::ptr::null_mut();
    }
    // SAFETY: a C string by C's contract.
    let Some(m) = (unsafe { fopen_mode(mode) }) else {
        errno::set_errno(errno::EINVAL);
        drop(g);
        fclose(stream);
        return core::ptr::null_mut();
    };
    // SAFETY: locked.
    let fd = unsafe { (*f).fd };
    let reopened = if path.is_null() {
        let saved = errno::get_errno();
        match reopen_fd_path(fd, &m) {
            Ok(newfd) => Some(newfd),
            Err(e) if e == errno::ENOENT || e == errno::ENXIO => {
                if !reflag(fd, &m) {
                    drop(g);
                    fclose(stream);
                    return core::ptr::null_mut();
                }
                errno::set_errno(saved);
                None
            }
            Err(e) => {
                drop(g);
                fclose(stream);
                errno::set_errno(e);
                return core::ptr::null_mut();
            }
        }
    } else {
        let newfd = crate::file::open(path, m.oflags, 0o666);
        if newfd < 0 {
            let e = errno::get_errno();
            drop(g);
            fclose(stream);
            errno::set_errno(e);
            return core::ptr::null_mut();
        }
        Some(newfd)
    };
    if let Some(newfd) = reopened {
        if fd < 0 {
            // Closed already (`fclose(stdout)` leaves the static stream with
            // no descriptor): the new one is simply the stream's, as glibc
            // skips its `dup3` for a `-1`.
            // SAFETY: locked.
            unsafe { (*f).fd = newfd };
        } else if newfd != fd {
            let cloexec = if m.flags & F_CLOEXEC != 0 {
                crate::fcntl::O_CLOEXEC
            } else {
                0
            };
            if crate::file::dup3(newfd, fd, cloexec) < 0 {
                let e = errno::get_errno();
                crate::file::close(newfd);
                drop(g);
                fclose(stream);
                errno::set_errno(e);
                return core::ptr::null_mut();
            }
            crate::file::close(newfd);
        }
    }
    // SAFETY: locked.
    let fd = unsafe { (*f).fd };
    // SAFETY: locked.
    unsafe {
        let file = &mut *f;
        file.flags = (file.flags & (F_PERM | F_USERLOCK)) | m.flags;
        file.buf = file.own_buf;
        file.buf_size = file.own_size;
        file.lbf = EOF;
        file.mode = 0;
        file.rpos = core::ptr::null_mut();
        file.rend = core::ptr::null_mut();
        file.wbase = core::ptr::null_mut();
        file.wpos = core::ptr::null_mut();
        file.wend = core::ptr::null_mut();
    }
    if m.flags & (F_APP | F_NORD) == F_APP | F_NORD && !seek_to_end(fd) {
        drop(g);
        fclose(stream);
        return core::ptr::null_mut();
    }
    stream
}

/// `freopen(NULL, mode, f)` as glibc does it: open `/proc/self/fd/<fd>`
/// with the mode's flags.  The new descriptor, or the open's `errno`.
fn reopen_fd_path(fd: i32, m: &Mode) -> Result<i32, i32> {
    let Ok(mut n) = u32::try_from(fd) else {
        return Err(errno::EBADF);
    };
    let mut path = [0u8; 32];
    let prefix = b"/proc/self/fd/";
    let mut digits = [0u8; 10];
    let mut k = 0usize;
    loop {
        if let Some(slot) = digits.get_mut(k) {
            #[allow(clippy::cast_possible_truncation)]
            {
                *slot = b'0'.wrapping_add((n % 10) as u8);
            }
        }
        k = k.wrapping_add(1);
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let mut at = 0usize;
    for &b in prefix
        .iter()
        .chain(digits.get(..k).unwrap_or(&[]).iter().rev())
    {
        if let Some(slot) = path.get_mut(at) {
            *slot = b;
        }
        at = at.wrapping_add(1);
    }
    // `path` is zeroed past `at`: the terminator is there.
    let newfd = crate::file::open(path.as_ptr(), m.oflags, 0o666);
    if newfd < 0 {
        Err(errno::get_errno())
    } else {
        Ok(newfd)
    }
}

/// The in-place fallback of `freopen(NULL, mode, f)`: the descriptor's
/// flags changed with `F_SETFL`, as musl does.  `false` (with `errno`) if
/// that cannot give the mode.
fn reflag(fd: i32, m: &Mode) -> bool {
    use crate::fcntl::{O_ACCMODE, O_CREAT, O_EXCL, O_TRUNC};
    use crate::fcntl_ops::{F_GETFL, F_SETFD, F_SETFL};
    let fl = crate::fcntl_ops::fcntl(fd, F_GETFL, 0);
    if fl == -1 {
        return false;
    }
    let want = m.oflags & O_ACCMODE;
    let have = fl & O_ACCMODE;
    if want != have && have != crate::fcntl::O_RDWR {
        errno::set_errno(errno::EBADF);
        return false;
    }
    if m.flags & F_CLOEXEC != 0
        && crate::fcntl_ops::fcntl(fd, F_SETFD, i64::from(crate::fdtable::FD_CLOEXEC)) == -1
    {
        return false;
    }
    let status = m.oflags & !(O_ACCMODE | O_CREAT | O_EXCL | O_TRUNC | crate::fcntl::O_CLOEXEC);
    crate::fcntl_ops::fcntl(fd, F_SETFL, i64::from(status)) != -1
}

/// `freopen64`: `freopen`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn freopen64(path: *const u8, mode: *const u8, stream: *mut u8) -> *mut u8 {
    // SAFETY: forwarded.
    unsafe { freopen(path, mode, stream) }
}

// ---------------------------------------------------------------------------
// Positions
// ---------------------------------------------------------------------------

/// `fseeko` without the lock: glibc's order -- a bad `whence` is `EINVAL`
/// at once; pending output is written (and its failure reported); a
/// `SEEK_CUR` offset counts from the stream's position, not the read-ahead's.
/// A successful seek discards the read-ahead and the pushback and clears end
/// of file.
///
/// # Safety
///
/// `f` is locked.
unsafe fn seek_raw(f: *mut File, off: i64, whence: i32) -> i32 {
    if whence != SEEK_SET && whence != SEEK_CUR && whence != SEEK_END {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: the caller's contract.
    let file = unsafe { &mut *f };
    let mut off = off;
    if whence == SEEK_CUR && !file.rend.is_null() {
        // SAFETY: the read window.
        let unread = unsafe { file.rend.offset_from(file.rpos) } as i64;
        off = off.wrapping_sub(unread);
    }
    // SAFETY: `f` is locked.
    if unsafe { flush_output(f) } != 0 {
        return -1;
    }
    // SAFETY: re-borrowed.
    let file = unsafe { &mut *f };
    file.wbase = core::ptr::null_mut();
    file.wpos = core::ptr::null_mut();
    file.wend = core::ptr::null_mut();
    // SAFETY: `f` is locked.
    if unsafe { (file.ops.seek)(f, off, whence) } < 0 {
        return -1;
    }
    // SAFETY: re-borrowed.
    let file = unsafe { &mut *f };
    file.rpos = core::ptr::null_mut();
    file.rend = core::ptr::null_mut();
    file.flags &= !F_EOF;
    0
}

/// `ftello` without the lock: the far end's position, less the read-ahead
/// or plus the pending output.  In append mode with output pending, from the
/// end of the file, where that output will go.
///
/// # Safety
///
/// `f` is locked.
unsafe fn tell_raw(f: *mut File) -> i64 {
    // SAFETY: the caller's contract.
    let file = unsafe { &mut *f };
    // SAFETY: the write window, when active.
    let pending = unsafe { pending_bytes(file) } as i64;
    let whence = if file.flags & F_APP != 0 && pending > 0 {
        SEEK_END
    } else {
        SEEK_CUR
    };
    // SAFETY: `f` is locked.
    let pos = unsafe { (file.ops.seek)(f, 0, whence) };
    if pos < 0 {
        return pos;
    }
    // SAFETY: re-borrowed.
    let file = unsafe { &*f };
    if !file.rend.is_null() {
        // SAFETY: the read window.
        pos.wrapping_add(unsafe { file.rpos.offset_from(file.rend) } as i64)
    } else {
        pos.wrapping_add(pending)
    }
}

/// Seek within a stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fseek(stream: *mut u8, offset: i64, whence: i32) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return -1;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { seek_raw(f, offset, whence) }
}

/// The stream's position.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftell(stream: *mut u8) -> i64 {
    let Some(f) = stream_to_file(stream) else {
        return -1;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { tell_raw(f) }
}

/// Own archive member — gnulib replaces `fseeko` (§339).
mod gnu_fseeko {
    /// `fseek` with an `off_t`, the same width here.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn fseeko(stream: *mut u8, offset: crate::types::OffT, whence: i32) -> i32 {
        super::fseek(stream, offset, whence)
    }
}
pub use gnu_fseeko::fseeko;

/// Own archive member — gnulib replaces `ftello` (§339).
mod gnu_ftello {
    /// `ftell` as an `off_t`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn ftello(stream: *mut u8) -> crate::types::OffT {
        super::ftell(stream)
    }
}
pub use gnu_ftello::ftello;

/// `fseeko64`: `fseek`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fseeko64(stream: *mut u8, offset: crate::types::OffT, whence: i32) -> i32 {
    fseek(stream, offset, whence)
}

/// `ftello64`: `ftell`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftello64(stream: *mut u8) -> crate::types::OffT {
    ftell(stream)
}

/// Back to the start, with the error indicator cleared too.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rewind(stream: *mut u8) {
    let Some(f) = stream_to_file(stream) else {
        return;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe {
        let _ = seek_raw(f, 0, SEEK_SET); // rewind reports nothing
        (*f).flags &= !F_ERR;
    }
}

/// `fpos_t`'s first eight bytes: musl's `fpos_t` is a 16-byte union whose
/// `long long` member holds the offset, and that is all this library puts in
/// it (the multibyte state glibc keeps beside it is always initial here).
pub type FposT = i64;

/// Store the stream's position.  A NULL `pos` is `EFAULT` (§1115).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgetpos(stream: *mut u8, pos: *mut FposT) -> i32 {
    let off = ftell(stream);
    if off < 0 {
        return -1;
    }
    if pos.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-null, and the caller's `fpos_t`.
    unsafe { pos.write_unaligned(off) };
    0
}

/// Return to a position `fgetpos` stored.  A NULL `pos` is `EFAULT`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsetpos(stream: *mut u8, pos: *const FposT) -> i32 {
    if pos.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-null, the caller's `fpos_t`.
    fseek(stream, unsafe { pos.read_unaligned() }, SEEK_SET)
}

/// `fgetpos64`: `fgetpos`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgetpos64(stream: *mut u8, pos: *mut FposT) -> i32 {
    fgetpos(stream, pos)
}

/// `fsetpos64`: `fsetpos`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsetpos64(stream: *mut u8, pos: *const FposT) -> i32 {
    fsetpos(stream, pos)
}

// ---------------------------------------------------------------------------
// Bytes in and out
// ---------------------------------------------------------------------------

/// Read a byte.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgetc(stream: *mut u8) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return EOF;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { getc_raw(f) }
}

/// `fgetc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getc(stream: *mut u8) -> i32 {
    fgetc(stream)
}

/// `fgetc(stdin)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getchar() -> i32 {
    fgetc(stdin_stream())
}

/// `fgetc` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgetc_unlocked(stream: *mut u8) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return EOF;
    };
    // SAFETY: the caller holds the stream (`_unlocked`).
    unsafe { getc_raw(f) }
}

/// `getc` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getc_unlocked(stream: *mut u8) -> i32 {
    fgetc_unlocked(stream)
}

/// `getchar` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getchar_unlocked() -> i32 {
    fgetc_unlocked(stdin_stream())
}

/// Write a byte.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fputc(c: i32, stream: *mut u8) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return EOF;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { putc_raw(c, f) }
}

/// `fputc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putc(c: i32, stream: *mut u8) -> i32 {
    fputc(c, stream)
}

/// `fputc(c, stdout)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putchar(c: i32) -> i32 {
    fputc(c, stdout_stream())
}

/// `fputc` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fputc_unlocked(c: i32, stream: *mut u8) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return EOF;
    };
    // SAFETY: the caller holds the stream.
    unsafe { putc_raw(c, f) }
}

/// `putc` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putc_unlocked(c: i32, stream: *mut u8) -> i32 {
    fputc_unlocked(c, stream)
}

/// `putchar` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putchar_unlocked(c: i32) -> i32 {
    fputc_unlocked(c, stdout_stream())
}

/// `fread`'s body, with the stream's lock already dealt with.  A NULL `ptr`
/// for a non-empty request is `EFAULT` and a stream error (§1115: glibc
/// would fault at the copy).
///
/// # Safety
///
/// `f` is locked or held; `ptr` holds `size * nmemb` bytes.
unsafe fn fread_items(ptr: *mut u8, size: usize, nmemb: usize, f: *mut File) -> usize {
    // glibc multiplies without a check; a product that wraps asks for less,
    // never more, than the caller's buffer.
    let len = size.wrapping_mul(nmemb);
    if len == 0 {
        return 0;
    }
    if ptr.is_null() {
        errno::set_errno(errno::EFAULT);
        // SAFETY: the caller's contract.
        unsafe { (*f).flags |= F_ERR };
        return 0;
    }
    // SAFETY: the caller's contract.
    let got = unsafe { fread_raw(ptr, len, f) };
    if got == len {
        nmemb
    } else {
        got.checked_div(size).unwrap_or(0)
    }
}

/// Read `nmemb` items of `size` bytes: all of them unless end of file or an
/// error comes first, however many `read`s that takes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fread(ptr: *mut u8, size: usize, nmemb: usize, stream: *mut u8) -> usize {
    let Some(f) = stream_to_file(stream) else {
        return 0;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked; `ptr` is the caller's.
    unsafe { fread_items(ptr, size, nmemb, f) }
}

/// `fread` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fread_unlocked(
    ptr: *mut u8,
    size: usize,
    nmemb: usize,
    stream: *mut u8,
) -> usize {
    let Some(f) = stream_to_file(stream) else {
        return 0;
    };
    // SAFETY: held by the caller.
    unsafe { fread_items(ptr, size, nmemb, f) }
}

/// `fwrite`'s body.  Byte orientation is claimed, and a wide stream refused
/// (glibc's `_IO_fwrite`).  A NULL `ptr` is `EFAULT` and a stream error.
///
/// # Safety
///
/// As [`fread_items`].
unsafe fn fwrite_items(ptr: *const u8, size: usize, nmemb: usize, f: *mut File) -> usize {
    let len = size.wrapping_mul(nmemb);
    if len == 0 {
        return 0;
    }
    // SAFETY: the caller's contract.
    if unsafe { orient(f, -1) } > 0 {
        return 0;
    }
    if ptr.is_null() {
        errno::set_errno(errno::EFAULT);
        // SAFETY: as above.
        unsafe { (*f).flags |= F_ERR };
        return 0;
    }
    // SAFETY: as above.
    let put = unsafe { fwritex(ptr, len, f) };
    if put == len {
        nmemb
    } else {
        put.checked_div(size).unwrap_or(0)
    }
}

/// Write `nmemb` items of `size` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fwrite(
    ptr: *const u8,
    size: usize,
    nmemb: usize,
    stream: *mut u8,
) -> usize {
    let Some(f) = stream_to_file(stream) else {
        return 0;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { fwrite_items(ptr, size, nmemb, f) }
}

/// `fwrite` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fwrite_unlocked(
    ptr: *const u8,
    size: usize,
    nmemb: usize,
    stream: *mut u8,
) -> usize {
    let Some(f) = stream_to_file(stream) else {
        return 0;
    };
    // SAFETY: held by the caller.
    unsafe { fwrite_items(ptr, size, nmemb, f) }
}

/// `fgets`'s body: glibc's `_IO_fgets`.  At most `n - 1` bytes, through the
/// first newline.  NULL if nothing was read, or if an error *new to this
/// call* stopped it (other than `EAGAIN`, where glibc returns the part read,
/// for a non-blocking descriptor); an error already on the stream does not
/// count, and stays.
///
/// # Safety
///
/// `f` is locked or held; `s` holds `n` bytes.
unsafe fn fgets_raw(s: *mut u8, n: i32, f: *mut File) -> *mut u8 {
    if n <= 0 {
        return core::ptr::null_mut();
    }
    if s.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    if n == 1 {
        // SAFETY: `s` holds one byte.
        unsafe { *s = 0 };
        return s;
    }
    // SAFETY: the caller's contract.
    let old_error = unsafe { (*f).flags } & F_ERR;
    // SAFETY: as above.
    unsafe { (*f).flags &= !F_ERR };
    let mut room = usize::try_from(n).unwrap_or(0).wrapping_sub(1);
    let mut p = s;
    while room > 0 {
        // SAFETY: as above.
        let file = unsafe { &mut *f };
        if file.rpos != file.rend {
            // SAFETY: the read window.
            let avail = unsafe {
                core::slice::from_raw_parts(
                    file.rpos,
                    usize::try_from(file.rend.offset_from(file.rpos)).unwrap_or(0),
                )
            };
            let (line, nl) = match avail.iter().position(|&b| b == b'\n') {
                Some(i) => (i.wrapping_add(1), true),
                None => (avail.len(), false),
            };
            let take = line.min(room);
            // SAFETY: `take` bytes are buffered and fit at `p`.
            unsafe {
                core::ptr::copy_nonoverlapping(file.rpos, p, take);
                file.rpos = file.rpos.add(take);
                p = p.add(take);
            }
            room = room.wrapping_sub(take);
            if (nl && take == line) || room == 0 {
                break;
            }
        }
        // SAFETY: locked.
        let c = unsafe { getc_raw(f) };
        if c == EOF {
            break;
        }
        // SAFETY: `room > 0` bytes remain at `p`.
        unsafe {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                *p = c as u8;
            }
            p = p.add(1);
        }
        room = room.wrapping_sub(1);
        if c == i32::from(b'\n') {
            break;
        }
    }
    // SAFETY: as above.
    let file = unsafe { &mut *f };
    let new_error = file.flags & F_ERR != 0;
    file.flags |= old_error;
    if p == s || (new_error && errno::get_errno() != errno::EAGAIN) {
        return core::ptr::null_mut();
    }
    // SAFETY: at most `n - 1` bytes were stored, so the terminator fits.
    unsafe { *p = 0 };
    s
}

/// Read a line of at most `n - 1` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgets(s: *mut u8, n: i32, stream: *mut u8) -> *mut u8 {
    let Some(f) = stream_to_file(stream) else {
        return core::ptr::null_mut();
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked; `s` is the caller's.
    unsafe { fgets_raw(s, n, f) }
}

/// `fgets` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgets_unlocked(s: *mut u8, n: i32, stream: *mut u8) -> *mut u8 {
    let Some(f) = stream_to_file(stream) else {
        return core::ptr::null_mut();
    };
    // SAFETY: held by the caller.
    unsafe { fgets_raw(s, n, f) }
}

/// `fputs`'s body: glibc's `_IO_fputs` -- 1 on success, `EOF` on an error
/// or a wide stream.  A NULL `s` is `EFAULT`.
///
/// # Safety
///
/// `f` is locked or held; `s` is a C string or NULL.
unsafe fn fputs_raw(s: *const u8, f: *mut File) -> i32 {
    if s.is_null() {
        errno::set_errno(errno::EFAULT);
        return EOF;
    }
    // SAFETY: a C string.
    let len = unsafe { crate::string::strlen(s) };
    // SAFETY: the caller's contract.
    if unsafe { orient(f, -1) } > 0 {
        return EOF;
    }
    // SAFETY: as above; `s` holds `len` bytes.
    if unsafe { fwritex(s, len, f) } == len {
        1
    } else {
        EOF
    }
}

/// Write a string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fputs(s: *const u8, stream: *mut u8) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return EOF;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { fputs_raw(s, f) }
}

/// `fputs` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fputs_unlocked(s: *const u8, stream: *mut u8) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return EOF;
    };
    // SAFETY: held by the caller.
    unsafe { fputs_raw(s, f) }
}

/// Write a string and a newline to `stdout`: the length plus one, capped at
/// `INT_MAX`, as glibc's `_IO_puts` answers, or `EOF`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn puts(s: *const u8) -> i32 {
    if s.is_null() {
        errno::set_errno(errno::EFAULT);
        return EOF;
    }
    // SAFETY: a C string.
    let len = unsafe { crate::string::strlen(s) };
    let f = &raw mut STDOUT_FILE;
    // SAFETY: static.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe {
        if orient(f, -1) > 0 || fwritex(s, len, f) != len || putc_raw(i32::from(b'\n'), f) == EOF {
            return EOF;
        }
    }
    i32::try_from(len.saturating_add(1)).unwrap_or(i32::MAX)
}

/// Push `c` back: it is the next byte read.  Up to [`UNGET`] bytes fit below
/// the buffer however much is buffered, and more while the buffer has room
/// below the read position.  Clears end of file.  `EOF` for `EOF`, for a
/// stream not open for reading, and when there is no room.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ungetc(c: i32, stream: *mut u8) -> i32 {
    if c == EOF {
        return EOF;
    }
    let Some(f) = stream_to_file(stream) else {
        return EOF;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { unget_raw(c, f) }
}

/// `ungetc`'s body.
///
/// # Safety
///
/// `f` is locked or held.
pub(crate) unsafe fn unget_raw(c: i32, f: *mut File) -> i32 {
    // SAFETY: the caller's contract.
    unsafe {
        if (*f).rpos.is_null() {
            let _ = toread(f); // sets up the window, or fails for a write-only stream
        }
        let file = &mut *f;
        if file.rpos.is_null() || file.rpos <= file.buf.sub(UNGET) {
            return EOF;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = c as u8;
        file.rpos = file.rpos.sub(1);
        *file.rpos = byte;
        file.flags &= !F_EOF;
        i32::from(byte)
    }
}

/// `getw`: an `int`, as `fread` reads it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getw(stream: *mut u8) -> i32 {
    let mut w = 0i32;
    // SAFETY: `w` holds four bytes.
    if unsafe { fread((&raw mut w).cast(), 4, 1, stream) } != 1 {
        return EOF;
    }
    w
}

/// `putw`: an `int`, as `fwrite` writes it; 0, or `EOF`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putw(w: i32, stream: *mut u8) -> i32 {
    // SAFETY: `w` is four bytes.
    if unsafe { fwrite((&raw const w).cast(), 4, 1, stream) } < 1 {
        EOF
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// getdelim, getline, fgetln
// ---------------------------------------------------------------------------

/// Fill `f`'s read window without taking from it: the next byte, or `EOF`
/// (glibc's `__underflow`).
///
/// # Safety
///
/// `f` is locked or held.
unsafe fn underflow(f: *mut File) -> i32 {
    // SAFETY: the caller's contract.
    unsafe {
        if (*f).rpos != (*f).rend {
            return i32::from(*(*f).rpos);
        }
        let c = uflow(f);
        if c != EOF {
            // The byte `uflow` took is still in the buffer below `rpos`, or
            // (unbuffered) is put back into the pushback room.
            let _ = unget_raw(c, f);
        }
        c
    }
}

/// `getdelim`'s body: glibc's `__getdelim`, which [`gnu_getdelim`] exports.
///
/// # Safety
///
/// `f` is locked or held; `lineptr` and `n` are NULL or the caller's.
pub(crate) unsafe fn getdelim_raw(
    lineptr: *mut *mut u8,
    n: *mut usize,
    delim: i32,
    f: *mut File,
) -> isize {
    // SAFETY: the caller's contract, throughout.
    unsafe {
        if (*f).flags & F_ERR != 0 {
            return -1;
        }
        if lineptr.is_null() || n.is_null() {
            errno::set_errno(errno::EINVAL);
            (*f).flags |= F_ERR;
            return -1;
        }
        if (*lineptr).is_null() || *n == 0 {
            let p = crate::malloc::malloc(120);
            if p.is_null() {
                (*f).flags |= F_ERR;
                return -1;
            }
            *lineptr = p;
            *n = 120;
        }
        if underflow(f) == EOF {
            return -1;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let d = delim as u8;
        let mut cur: usize = 0;
        loop {
            let file = &mut *f;
            let window = core::slice::from_raw_parts(
                file.rpos,
                usize::try_from(file.rend.offset_from(file.rpos)).unwrap_or(0),
            );
            let hit = window.iter().position(|&b| b == d);
            let len = hit.map_or(window.len(), |i| i.wrapping_add(1));
            let Some(needed) = cur.checked_add(len).and_then(|x| x.checked_add(1)) else {
                errno::set_errno(errno::EOVERFLOW);
                file.flags |= F_ERR;
                return -1;
            };
            if isize::try_from(needed).is_err() {
                errno::set_errno(errno::EOVERFLOW);
                file.flags |= F_ERR;
                return -1;
            }
            if needed > *n {
                let grow = needed.max((*n).saturating_mul(2));
                let p = crate::malloc::realloc(*lineptr, grow);
                if p.is_null() {
                    file.flags |= F_ERR;
                    return -1;
                }
                *lineptr = p;
                *n = grow;
            }
            core::ptr::copy_nonoverlapping(file.rpos, (*lineptr).add(cur), len);
            file.rpos = file.rpos.add(len);
            cur = cur.wrapping_add(len);
            if hit.is_some() || underflow(f) == EOF {
                break;
            }
        }
        *(*lineptr).add(cur) = 0;
        isize::try_from(cur).unwrap_or(isize::MAX)
    }
}

/// Own archive member — gnulib replaces `getdelim` (§339).
mod gnu_getdelim {
    use super::*;

    /// Read through `delim` into `*lineptr`, growing it with `realloc`: the
    /// bytes read, not counting the terminator, or -1 at end of file or on an
    /// error.  glibc's: a stream already in error reads nothing; NULL
    /// `lineptr` or `n` is `EINVAL` and a stream error; a last line without
    /// `delim` is returned as it is.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn getdelim(
        lineptr: *mut *mut u8,
        n: *mut usize,
        delim: i32,
        stream: *mut u8,
    ) -> isize {
        let Some(f) = stream_to_file(stream) else {
            return -1;
        };
        // SAFETY: a live stream.
        let _g = unsafe { locked(f) };
        // SAFETY: locked; the pointers are the caller's.
        unsafe { getdelim_raw(lineptr, n, delim, f) }
    }
}
pub use gnu_getdelim::getdelim;

/// Own archive member — gnulib replaces `getline` (§339).
mod gnu_getline {
    /// `getdelim` through a newline.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn getline(
        lineptr: *mut *mut u8,
        n: *mut usize,
        stream: *mut u8,
    ) -> isize {
        // SAFETY: forwarded.
        unsafe { super::getdelim(lineptr, n, i32::from(b'\n'), stream) }
    }
}
pub use gnu_getline::getline;

/// One line of `stream`, through its newline, into `*lineptr` (grown with
/// `realloc`, as `getline`'s): for the account-file readers
/// (`nss_files::fget_entry`), which glibc builds on `fgets`. So, unlike
/// `getline`, a stream already in error is still read -- the error stays
/// set, and only a new one is an error, as glibc's `fgets` has it.
/// `Ok(0)` at end of file.
///
/// # Safety
///
/// `stream` is an open stream; `lineptr` and `n` are a `getline` pair.
pub(crate) unsafe fn read_line_as_fgets(
    lineptr: *mut *mut u8,
    n: *mut usize,
    stream: *mut u8,
) -> Result<usize, i32> {
    let Some(f) = stream_to_file(stream) else {
        return Err(errno::EBADF);
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked; the pointers are the caller's.
    unsafe {
        let old_error = (*f).flags & F_ERR;
        (*f).flags &= !F_ERR;
        let got = getdelim_raw(lineptr, n, i32::from(b'\n'), f);
        let new_error = (*f).flags & F_ERR != 0;
        (*f).flags |= old_error;
        match usize::try_from(got) {
            Ok(len) => Ok(len),
            Err(_) if new_error => Err(match errno::get_errno() {
                0 => errno::EIO,
                e => e,
            }),
            Err(_) => Ok(0),
        }
    }
}

/// Mark `stream` in error, as glibc's `fseterr_unlocked`: an account-file
/// reader that could not seek back to re-read a line.
pub(crate) fn set_stream_error(stream: *mut u8) {
    let Some(f) = stream_to_file(stream) else {
        return;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { (*f).flags |= F_ERR };
}

/// `__getdelim`: glibc's internal name for `getdelim`, which glibc-built
/// objects call.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __getdelim(
    lineptr: *mut *mut u8,
    n: *mut usize,
    delim: i32,
    stream: *mut u8,
) -> isize {
    // SAFETY: forwarded.
    unsafe { getdelim(lineptr, n, delim, stream) }
}

/// `fgetln` (BSD, and musl's): the next line, through its newline, where it
/// lies -- in the buffer if it is all there, else in a line the stream keeps
/// -- with its length in `*len`; NULL at end of file or on an error.  The
/// pointer is good until the next call on the stream.  glibc has no
/// `fgetln`; this is musl's, which programs built against musl's headers
/// can call.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetln(stream: *mut u8, len: *mut usize) -> *mut u8 {
    let Some(f) = stream_to_file(stream) else {
        return core::ptr::null_mut();
    };
    if len.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe {
        let _ = underflow(f); // an empty window stays empty
        let file = &mut *f;
        if file.rpos != file.rend {
            let window = core::slice::from_raw_parts(
                file.rpos,
                usize::try_from(file.rend.offset_from(file.rpos)).unwrap_or(0),
            );
            if let Some(i) = window.iter().position(|&b| b == b'\n') {
                let line = file.rpos;
                let k = i.wrapping_add(1);
                file.rpos = file.rpos.add(k);
                *len = k;
                return line;
            }
        }
        let mut size = file.getln_size;
        let n = getdelim_raw(&raw mut file.getln_buf, &raw mut size, i32::from(b'\n'), f);
        (*f).getln_size = size;
        match usize::try_from(n) {
            Ok(k) if k > 0 => {
                *len = k;
                (*f).getln_buf
            }
            _ => core::ptr::null_mut(),
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// The descriptor under a stream; -1 with `EBADF` for a cookie or memory
/// stream, or a closed one (glibc's `fileno`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fileno(stream: *mut u8) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return -1;
    };
    // SAFETY: a live stream; `fd` is written only by open, `freopen` and
    // close, and glibc reads it unlocked too.
    let fd = unsafe { (*f).fd };
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    fd
}

/// `fileno`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fileno_unlocked(stream: *mut u8) -> i32 {
    fileno(stream)
}

/// Whether end of file has been seen.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn feof(stream: *mut u8) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return 0;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    i32::from(unsafe { (*f).flags } & F_EOF != 0)
}

/// Whether an error has been seen.  NULL counts as one (§1120).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ferror(stream: *mut u8) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return 1;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    i32::from(unsafe { (*f).flags } & F_ERR != 0)
}

/// Clear end of file and the error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clearerr(stream: *mut u8) {
    let Some(f) = stream_to_file(stream) else {
        return;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { (*f).flags &= !(F_EOF | F_ERR) };
}

/// `feof` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn feof_unlocked(stream: *mut u8) -> i32 {
    // SAFETY: a live stream, held by the caller.
    stream_to_file(stream).map_or(0, |f| i32::from(unsafe { (*f).flags } & F_EOF != 0))
}

/// `ferror` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ferror_unlocked(stream: *mut u8) -> i32 {
    // SAFETY: as above.
    stream_to_file(stream).map_or(1, |f| i32::from(unsafe { (*f).flags } & F_ERR != 0))
}

/// `clearerr` without the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clearerr_unlocked(stream: *mut u8) {
    if let Some(f) = stream_to_file(stream) {
        // SAFETY: as above.
        unsafe { (*f).flags &= !(F_EOF | F_ERR) };
    }
}

/// A stream's orientation, claiming `mode`'s if it has none: negative byte,
/// positive wide, 0 none yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fwide(stream: *mut u8, mode: i32) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return 0;
    };
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe { orient(f, mode) }
}

// ---------------------------------------------------------------------------
// Buffering
// ---------------------------------------------------------------------------

/// Choose a stream's buffering, at any time (glibc syncs the stream first,
/// musl does not care): `_IOFBF` full, `_IOLBF` by line, `_IONBF` none.
/// With `buf`, the stream uses the program's `size` bytes -- the first
/// [`UNGET`] of them as pushback room -- until it is closed or reopened.
/// Any other `mode` is `EOF`, with `errno` untouched, as in glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setvbuf(stream: *mut u8, buf: *mut u8, mode: i32, size: usize) -> i32 {
    let Some(f) = stream_to_file(stream) else {
        return EOF;
    };
    if mode != _IOFBF && mode != _IOLBF && mode != _IONBF {
        return EOF;
    }
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe {
        let takes_buffer = mode == _IONBF || (!buf.is_null() && size > UNGET);
        if takes_buffer && sync(f) != 0 {
            return EOF;
        }
        let file = &mut *f;
        file.flags |= F_SVB | F_INIT;
        match mode {
            _IONBF => {
                file.buf = file.own_buf;
                file.buf_size = 0;
                file.lbf = EOF;
            }
            _ => {
                if takes_buffer {
                    file.buf = buf.add(UNGET);
                    file.buf_size = size.wrapping_sub(UNGET);
                } else if file.buf_size == 0 {
                    file.buf = file.own_buf;
                    file.buf_size = file.own_size;
                }
                file.lbf = if mode == _IOLBF {
                    i32::from(b'\n')
                } else {
                    EOF
                };
            }
        }
    }
    0
}

/// `setvbuf(stream, buf, buf ? _IOFBF : _IONBF, BUFSIZ)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setbuf(stream: *mut u8, buf: *mut u8) {
    let mode = if buf.is_null() { _IONBF } else { _IOFBF };
    let _ = setvbuf(stream, buf, mode, BUFSIZ); // setbuf reports nothing
}

/// `setvbuf` with the size given (BSD).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setbuffer(stream: *mut u8, buf: *mut u8, size: usize) {
    let mode = if buf.is_null() { _IONBF } else { _IOFBF };
    let _ = setvbuf(stream, buf, mode, size); // as setbuf
}

/// Line buffering (BSD).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setlinebuf(stream: *mut u8) {
    let _ = setvbuf(stream, core::ptr::null_mut(), _IOLBF, 0); // as setbuf
}

// ---------------------------------------------------------------------------
// Locks
// ---------------------------------------------------------------------------

/// Hold a stream across several calls.  Taken whatever `__fsetlocking`
/// said, as glibc's is: the program asked.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn flockfile(stream: *mut core::ffi::c_void) {
    let Some(f) = stream_to_file(stream.cast()) else {
        return;
    };
    // SAFETY: a live stream.
    unsafe { lock_file_always(f) };
}

/// `flockfile` if the stream is free or already ours: 0, or nonzero if
/// another thread holds it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftrylockfile(stream: *mut core::ffi::c_void) -> i32 {
    let Some(f) = stream_to_file(stream.cast()) else {
        return -1;
    };
    // SAFETY: a live stream.
    if unsafe { trylock_file(f) } { 0 } else { -1 }
}

/// Release one `flockfile`.  From a thread that does not hold the stream it
/// does nothing -- undefined in POSIX, and in glibc a release of another
/// thread's lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn funlockfile(stream: *mut core::ffi::c_void) {
    let Some(f) = stream_to_file(stream.cast()) else {
        return;
    };
    // SAFETY: a live stream; only its owner reaches the release.
    unsafe {
        if (*f).lock.owner.load(Ordering::Relaxed) != crate::pthread::current_tid() {
            return;
        }
        unlock_file(f);
    }
}

// ---------------------------------------------------------------------------
// Crate-internal access for printf
// ---------------------------------------------------------------------------

/// Write `len` bytes to `stream` through its buffer, for `printf` (which
/// holds the stream with [`lock_byte_stream`] around the whole call; the lock
/// is recursive).  The count written, or -1.
pub(crate) fn write_stream(stream: *mut u8, data: *const u8, len: usize) -> i64 {
    let Some(f) = stream_to_file(stream) else {
        return -1;
    };
    if len == 0 {
        return 0;
    }
    // SAFETY: a live stream.
    let _g = unsafe { locked(f) };
    // SAFETY: locked; `data` holds `len`.
    let n = unsafe { fwritex(data, len, f) };
    if n == len {
        i64::try_from(n).unwrap_or(i64::MAX)
    } else {
        -1
    }
}

/// A stream held for one wide-character call (`fputwc`, `fgetwc`,
/// `wprintf` and the rest, in `wchar.rs` and `printf.rs`), with wide
/// orientation claimed.  The wide functions read and write the stream's
/// bytes -- the UTF-8 of their characters -- through it, past the byte
/// functions' refusal of a wide stream.
pub(crate) struct WideStream {
    f: *mut File,
    _lock: Locked,
}

/// Lock `stream` and claim wide orientation for it: `None` if it is a byte
/// stream, as glibc's wide calls fail on one (`errno` untouched), or NULL
/// (`EBADF`).
pub(crate) fn lock_wide_stream(stream: *mut u8) -> Option<WideStream> {
    let f = stream_to_file(stream)?;
    // SAFETY: a non-null `FILE *` is a live stream.
    let lock = unsafe { locked(f) };
    // SAFETY: locked.
    if unsafe { orient(f, 1) } < 0 {
        return None;
    }
    Some(WideStream { f, _lock: lock })
}

impl WideStream {
    /// The next byte, or `EOF`.
    pub(crate) fn getc(&self) -> i32 {
        // SAFETY: held.
        unsafe {
            let file = &mut *self.f;
            if file.rpos != file.rend {
                let c = *file.rpos;
                file.rpos = file.rpos.add(1);
                return i32::from(c);
            }
            uflow_bytes(self.f)
        }
    }

    /// Write one byte: `false` on an error.
    pub(crate) fn putc(&self, b: u8) -> bool {
        // SAFETY: held; the orientation is wide, so `towrite` keeps it.
        unsafe { putc_raw(i32::from(b), self.f) != EOF }
    }

    /// Push one byte back: `false` if there is no room.
    pub(crate) fn unget(&self, b: u8) -> bool {
        // SAFETY: held.
        unsafe { unget_raw(i32::from(b), self.f) != EOF }
    }

    /// Mark the stream in error: glibc's answer to a byte sequence that is
    /// no character.
    pub(crate) fn set_error(&self) {
        // SAFETY: held.
        unsafe { (*self.f).flags |= F_ERR };
    }

    /// Whether the stream is in error.
    pub(crate) fn error(&self) -> bool {
        // SAFETY: held.
        unsafe { (*self.f).flags & F_ERR != 0 }
    }

    /// Clear the error, and only the error (end of file stays sticky).
    pub(crate) fn clear_error(&self) {
        // SAFETY: held.
        unsafe { (*self.f).flags &= !F_ERR };
    }

    /// The stream, for `write_stream`.
    pub(crate) fn stream(&self) -> *mut u8 {
        self.f.cast()
    }
}

/// Lock `stream` for a `wscanf` call and claim wide orientation, as glibc's
/// `vfwscanf` does: `None` (the call answers `EOF`) for a byte stream or NULL
/// (`EBADF`).  A stream not open for reading is `EBADF` -- after the
/// orientation is claimed, and with no stream error, as glibc's `ORIENT`
/// then `ARGCHECK` have it -- the same as [`lock_scan_stream`].
pub(crate) fn lock_wscan_stream(stream: *mut u8) -> Option<WideStream> {
    let f = stream_to_file(stream)?;
    // SAFETY: a non-null `FILE *` is a live stream.
    let lock = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe {
        if orient(f, 1) < 0 {
            return None;
        }
        if (*f).flags & F_NORD != 0 {
            errno::set_errno(errno::EBADF);
            return None;
        }
    }
    Some(WideStream { f, _lock: lock })
}

/// A stream held for one `scanf` call, byte-oriented: the engine in
/// `scanf.rs` reads it with glibc's `inchar` and gives back the one
/// character it looked at too far with `ungetc`.
pub(crate) struct ScanStream {
    f: *mut File,
    _lock: Locked,
}

/// Lock `stream` for a `scanf` call and claim byte orientation, as glibc's
/// `vfscanf` does: `None` (the call answers `EOF`) for a wide stream or NULL
/// (`EBADF`).  A stream not open for reading is `EBADF`, after the
/// orientation is claimed: glibc's `ARGCHECK` sets `errno` and not the
/// stream's error (`vfscanf-internal.c`; `ferror` answers 0 after it).
pub(crate) fn lock_scan_stream(stream: *mut u8) -> Option<ScanStream> {
    let f = stream_to_file(stream)?;
    // SAFETY: a non-null `FILE *` is a live stream.
    let lock = unsafe { locked(f) };
    // SAFETY: locked.
    unsafe {
        if orient(f, -1) > 0 {
            return None;
        }
        if (*f).flags & F_NORD != 0 {
            errno::set_errno(errno::EBADF);
            return None;
        }
    }
    Some(ScanStream { f, _lock: lock })
}

impl ScanStream {
    /// The next byte, or `EOF`.
    pub(crate) fn getc(&self) -> i32 {
        // SAFETY: held.
        unsafe { getc_raw(self.f) }
    }

    /// Give back `c`, the byte `getc` last returned.  There is always room
    /// for it: it came out of the window just now, or the pushback room is
    /// empty.
    pub(crate) fn unget(&self, c: u8) {
        // SAFETY: held.
        let _ = unsafe { unget_raw(i32::from(c), self.f) }; // see the doc
    }
}

// ---------------------------------------------------------------------------
// stdio_ext.h
// ---------------------------------------------------------------------------
//
// gnulib reaches into a platform's FILE for these when the platform lacks
// them, so a program may bring its own copy of any of them: each is its own
// archive member (the `mod gnu_*` idiom, §339/§340), as
// `scripts/check-libc-shape.py`'s REPLACEABLE list requires.

/// Own archive member — gnulib's `fpending` module defines `__fpending`.
mod gnu_fpending {
    use super::*;

    /// Bytes of output buffered and not yet written.  gnulib's
    /// `close-stream` asks, to tell a lost write from a clean close.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __fpending(stream: *mut u8) -> usize {
        // SAFETY: a live stream; a read of two pointers.
        stream_to_file(stream).map_or(0, |f| unsafe { pending_bytes(&*f) })
    }
}
pub use gnu_fpending::__fpending;

/// Own archive member — gnulib's `freadahead` module defines `__freadahead`.
mod gnu_freadahead {
    use super::*;

    /// Bytes readable without reaching the far end, pushback included
    /// (musl's).
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __freadahead(stream: *mut u8) -> usize {
        let Some(f) = stream_to_file(stream) else {
            return 0;
        };
        // SAFETY: a live stream; the read window, when active.
        unsafe {
            let file = &*f;
            if file.rend.is_null() {
                return 0;
            }
            usize::try_from(file.rend.offset_from(file.rpos)).unwrap_or(0)
        }
    }
}
pub use gnu_freadahead::__freadahead;

/// Own archive member — gnulib's `freadptr` module defines `__freadptr`.
mod gnu_freadptr {
    use super::*;

    /// The buffered input, where it lies, with its length in `*sizep`; NULL
    /// if none.  Pushback is part of it: it lies just below the buffered
    /// bytes, in one run with them.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn __freadptr(stream: *mut u8, sizep: *mut usize) -> *const u8 {
        let Some(f) = stream_to_file(stream) else {
            return core::ptr::null();
        };
        // SAFETY: a live stream.
        let file = unsafe { &*f };
        if file.rpos == file.rend || sizep.is_null() {
            return core::ptr::null();
        }
        // SAFETY: the read window; `sizep` is the caller's.
        unsafe { *sizep = usize::try_from(file.rend.offset_from(file.rpos)).unwrap_or(0) };
        file.rpos
    }
}
pub use gnu_freadptr::__freadptr;

/// Own archive member — gnulib's `freadptrinc` module defines `__freadptrinc`.
mod gnu_freadptrinc {
    use super::*;

    /// Consume `inc` of the bytes [`__freadptr`] showed -- no more than are
    /// there, so a caller that overshoots cannot move past the window.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __freadptrinc(stream: *mut u8, inc: usize) {
        let Some(f) = stream_to_file(stream) else {
            return;
        };
        // SAFETY: a live stream; the read window.
        unsafe {
            let file = &mut *f;
            if file.rend.is_null() {
                return;
            }
            let avail = usize::try_from(file.rend.offset_from(file.rpos)).unwrap_or(0);
            file.rpos = file.rpos.add(inc.min(avail));
        }
    }
}
pub use gnu_freadptrinc::__freadptrinc;

/// Own archive member — gnulib's `fseterr` module defines `__fseterr`.
mod gnu_fseterr {
    use super::*;

    /// Set the stream's error indicator.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __fseterr(stream: *mut u8) {
        if let Some(f) = stream_to_file(stream) {
            // SAFETY: a live stream.
            unsafe { (*f).flags |= F_ERR };
        }
    }
}
pub use gnu_fseterr::__fseterr;

/// Own archive member — gnulib's `fpurge` module defines `__fpurge`.
mod gnu_fpurge {
    use super::*;

    /// Discard buffered input and output, pushback included, without
    /// writing anything (glibc's `__fpurge`).  bash calls it in forked
    /// children so they do not write out the parent's buffered output again.
    ///
    /// Returns 0: musl's `<stdio_ext.h>` declares it returning `int`, glibc's
    /// `void`, so a caller of either is answered. (It returned nothing until
    /// 2026-09-29, and a caller through musl's header read an unset
    /// register.)
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __fpurge(stream: *mut u8) -> i32 {
        let Some(f) = stream_to_file(stream) else {
            return 0;
        };
        // SAFETY: a live stream.
        let _g = unsafe { locked(f) };
        // SAFETY: locked.
        unsafe {
            let file = &mut *f;
            file.rpos = core::ptr::null_mut();
            file.rend = core::ptr::null_mut();
            if !file.wbase.is_null() {
                file.wpos = file.wbase;
            }
        }
        0
    }
}
pub use gnu_fpurge::__fpurge;

/// Own archive member — gnulib's `fpurge` module defines `fpurge` when the
/// platform has none, and musl has one (BSD's name, returning 0).
mod gnu_fpurge_bsd {
    /// `__fpurge`, returning 0 (musl's `fpurge`).
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn fpurge(stream: *mut u8) -> i32 {
        super::__fpurge(stream)
    }
}
pub use gnu_fpurge_bsd::fpurge;

/// Own archive member — gnulib's `freading` module defines `__freading`.
mod gnu_freading {
    use super::*;

    /// Whether the stream is read-only, or was last read (glibc's: not
    /// writing now, and a read window exists).
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __freading(stream: *mut u8) -> i32 {
        // SAFETY: a live stream.
        stream_to_file(stream).map_or(0, |f| unsafe {
            let file = &*f;
            i32::from(
                file.flags & F_NOWR != 0
                    || (file.flags & F_NORD == 0 && file.wbase.is_null() && !file.rend.is_null()),
            )
        })
    }
}
pub use gnu_freading::__freading;

/// Own archive member — gnulib's `fwriting` module defines `__fwriting`.
mod gnu_fwriting {
    use super::*;

    /// Whether the stream is write-only, or is writing now.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __fwriting(stream: *mut u8) -> i32 {
        // SAFETY: a live stream.
        stream_to_file(stream).map_or(0, |f| unsafe {
            i32::from((*f).flags & F_NORD != 0 || !(*f).wend.is_null())
        })
    }
}
pub use gnu_fwriting::__fwriting;

/// Own archive member — gnulib may define `__freadable`.
mod gnu_freadable {
    use super::*;

    /// Whether the stream is open for reading.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __freadable(stream: *mut u8) -> i32 {
        // SAFETY: a live stream.
        stream_to_file(stream).map_or(0, |f| unsafe { i32::from((*f).flags & F_NORD == 0) })
    }
}
pub use gnu_freadable::__freadable;

/// Own archive member — gnulib may define `__fwritable`.
mod gnu_fwritable {
    use super::*;

    /// Whether the stream is open for writing.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __fwritable(stream: *mut u8) -> i32 {
        // SAFETY: a live stream.
        stream_to_file(stream).map_or(0, |f| unsafe { i32::from((*f).flags & F_NOWR == 0) })
    }
}
pub use gnu_fwritable::__fwritable;

/// Own archive member — gnulib may define `__flbf`.
mod gnu_flbf {
    use super::*;

    /// Whether the stream is line buffered.  A terminal is found to be one
    /// at its first read or write, as in glibc, so before that this is 0.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __flbf(stream: *mut u8) -> i32 {
        // SAFETY: a live stream.
        stream_to_file(stream).map_or(0, |f| unsafe { i32::from((*f).lbf == i32::from(b'\n')) })
    }
}
pub use gnu_flbf::__flbf;

/// Own archive member — gnulib may define `__fbufsize`.
mod gnu_fbufsize {
    use super::*;

    /// The buffer's size: 0 for an unbuffered stream.  (glibc says 1 there,
    /// the byte it keeps for one, and 0 for a stream not yet used, whose
    /// buffer it allocates late; this library's buffers exist from the
    /// start.)
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __fbufsize(stream: *mut u8) -> usize {
        // SAFETY: a live stream.
        stream_to_file(stream).map_or(0, |f| unsafe { (*f).buf_size })
    }
}
pub use gnu_fbufsize::__fbufsize;

/// Own archive member — gnulib may define `__fsetlocking`.
mod gnu_fsetlocking {
    use super::*;

    /// Query or set who locks the stream: `FSETLOCKING_INTERNAL` (every call
    /// does) or `FSETLOCKING_BYCALLER` (the program does, with `flockfile`).
    /// Returns what it was.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn __fsetlocking(stream: *mut u8, kind: i32) -> i32 {
        let Some(f) = stream_to_file(stream) else {
            return FSETLOCKING_INTERNAL;
        };
        // SAFETY: a live stream; the program owns it while it changes this,
        // as glibc requires.
        unsafe {
            let was = if (*f).flags & F_USERLOCK != 0 {
                FSETLOCKING_BYCALLER
            } else {
                FSETLOCKING_INTERNAL
            };
            if kind != FSETLOCKING_QUERY {
                (*f).flags &= !F_USERLOCK;
                if kind == FSETLOCKING_BYCALLER {
                    (*f).flags |= F_USERLOCK;
                }
            }
            was
        }
    }
}
pub use gnu_fsetlocking::__fsetlocking;

/// Flush every line buffered stream (glibc's `_flushlbf`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _flushlbf() {
    for_each_stream(|f| {
        // SAFETY: a live stream, locked for the flush.
        unsafe {
            let _g = locked(f);
            if (*f).lbf == i32::from(b'\n') {
                let _ = flush_output(f); // _flushlbf reports nothing
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Cookie streams
// ---------------------------------------------------------------------------

/// `cookie_io_functions_t`: a program's four callbacks, any of them NULL.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CookieIoFunctions {
    /// `ssize_t read(void *cookie, char *buf, size_t size)`.
    pub read: Option<unsafe extern "C" fn(*mut core::ffi::c_void, *mut u8, usize) -> isize>,
    /// `ssize_t write(void *cookie, const char *buf, size_t size)`.
    pub write: Option<unsafe extern "C" fn(*mut core::ffi::c_void, *const u8, usize) -> isize>,
    /// `int seek(void *cookie, off64_t *offset, int whence)`.
    pub seek: Option<unsafe extern "C" fn(*mut core::ffi::c_void, *mut i64, i32) -> i32>,
    /// `int close(void *cookie)`.
    pub close: Option<unsafe extern "C" fn(*mut core::ffi::c_void) -> i32>,
}

/// A cookie stream's state, allocated with the stream.
struct Cookie {
    cookie: *mut core::ffi::c_void,
    io: CookieIoFunctions,
}

static COOKIE_OPS: Ops = Ops {
    read: cookie_read,
    write: cookie_write,
    seek: cookie_seek,
    close: cookie_close,
};

/// The `Cookie` of a cookie stream.
///
/// # Safety
///
/// `f` is a cookie stream.
unsafe fn cookie_of(f: *mut File) -> *mut Cookie {
    // SAFETY: the caller's contract: `fopencookie` put it there.
    unsafe { (*f).cookie.cast() }
}

/// Read through the program's `read`: straight into the caller's buffer for
/// a request as large as ours, else a refill of ours (glibc calls `read`
/// with its buffer's size).  No `read` is an error, as glibc's -1 is.
unsafe fn cookie_read(f: *mut File, dst: *mut u8, len: usize) -> usize {
    // SAFETY: the caller holds `f`'s lock; `f` is a cookie stream.
    let (file, c) = unsafe { (&mut *f, &*cookie_of(f)) };
    let Some(read) = c.io.read else {
        file.flags |= F_ERR;
        return 0;
    };
    let direct = file.buf_size == 0 || len >= file.buf_size;
    let (to, want) = if direct {
        (dst, len)
    } else {
        (file.buf, file.buf_size)
    };
    // SAFETY: the program's callback, with a buffer of `want` bytes.
    let n = unsafe { read(c.cookie, to, want) };
    let Ok(got) = usize::try_from(n) else {
        file.flags |= F_ERR;
        return 0;
    };
    if got == 0 {
        file.flags |= F_EOF;
        return 0;
    }
    let got = got.min(want);
    if direct {
        return got;
    }
    let take = got.min(len);
    // SAFETY: as in `fd_read`.
    unsafe {
        core::ptr::copy_nonoverlapping(file.buf, dst, take);
        file.rpos = file.buf.add(take);
        file.rend = file.buf.add(got);
    }
    take
}

/// Write through the program's `write`, retrying what it did not take; a
/// short or failed write is a stream error, as glibc's `_IO_cookie_write`
/// makes it.  No `write` is an error that discards the output (glibc's).
unsafe fn cookie_write(f: *mut File, src: *const u8, len: usize) -> usize {
    // SAFETY: the caller holds `f`'s lock; `f` is a cookie stream.
    let (file, c) = unsafe { (&mut *f, &*cookie_of(f)) };
    let Some(write) = c.io.write else {
        write_failed(file);
        return 0;
    };
    let put = |mut p: *const u8, mut n: usize| -> bool {
        while n > 0 {
            // SAFETY: the program's callback, with `n` bytes at `p`.
            let r = unsafe { write(c.cookie, p, n) };
            let Ok(done) = usize::try_from(r) else {
                return false;
            };
            if done == 0 {
                return false;
            }
            let done = done.min(n);
            // SAFETY: `done <= n`.
            p = unsafe { p.add(done) };
            n = n.wrapping_sub(done);
        }
        true
    };
    // SAFETY: the window, when active.
    let pending = unsafe { pending_bytes(file) };
    if pending > 0 && !put(file.wbase, pending) {
        write_failed(file);
        return 0;
    }
    if len > 0 && !put(src, len) {
        write_failed(file);
        return 0;
    }
    reset_write_window(file);
    len
}

/// Seek through the program's `seek`: the new position, or -1 (`ESPIPE`
/// when there is no `seek`, as a pipe answers).
unsafe fn cookie_seek(f: *mut File, off: i64, whence: i32) -> i64 {
    // SAFETY: the caller holds `f`'s lock; `f` is a cookie stream.
    let c = unsafe { &*cookie_of(f) };
    let Some(seek) = c.io.seek else {
        errno::set_errno(errno::ESPIPE);
        return -1;
    };
    let mut pos = off;
    // SAFETY: the program's callback.
    if unsafe { seek(c.cookie, &raw mut pos, whence) } == -1 || pos == -1 {
        return -1;
    }
    pos
}

/// The program's `close`, if it gave one.
unsafe fn cookie_close(f: *mut File) -> i32 {
    // SAFETY: the caller holds `f`'s lock; `f` is a cookie stream.
    let c = unsafe { &*cookie_of(f) };
    match c.io.close {
        // SAFETY: the program's callback.
        Some(close) => unsafe { close(c.cookie) },
        None => 0,
    }
}

/// Bytes to allocate for a cookie stream: the stream, then its `Cookie`.
const COOKIE_ALLOC: usize = STREAM_ALLOC + core::mem::size_of::<Cookie>();

/// A stream whose far end is `io`, called with `cookie` (glibc's
/// `fopencookie`).  The mode is `r`, `w` or `a`, and `+` right after it (or
/// after a `b`) makes it read and write -- glibc reads nothing else here.
/// `fileno` on it is -1 with `EBADF`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fopencookie(
    cookie: *mut core::ffi::c_void,
    mode: *const u8,
    io: CookieIoFunctions,
) -> *mut u8 {
    if mode.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: a C string by C's contract.
    let Some((_, mut flags)) = mode_letter(unsafe { *mode }) else {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    };
    // SAFETY: as above; each byte is read only if the one before was not NUL.
    let plus = unsafe { *mode.add(1) == b'+' || (*mode.add(1) == b'b' && *mode.add(2) == b'+') };
    if plus {
        flags &= !(F_NORD | F_NOWR);
    }
    // SAFETY: the allocation holds a stream and then a `Cookie`.
    unsafe { cookie_stream(flags, cookie, io) }.map_or(core::ptr::null_mut(), |f| f.cast())
}

/// A cookie stream with `flags`, for `fopencookie` and `fmemopen`.
///
/// # Safety
///
/// `io`'s callbacks accept `cookie`.
pub(crate) unsafe fn cookie_stream(
    flags: u32,
    cookie: *mut core::ffi::c_void,
    io: CookieIoFunctions,
) -> Option<*mut File> {
    let mem = crate::malloc::malloc(COOKIE_ALLOC);
    if mem.is_null() {
        errno::set_errno(errno::ENOMEM);
        return None;
    }
    // SAFETY: `mem` holds the stream, its pushback room and buffer, then a
    // `Cookie` at `STREAM_ALLOC` -- a multiple of 8, `Cookie`'s alignment.
    unsafe {
        let f = mem.cast::<File>();
        let c = mem.add(STREAM_ALLOC).cast::<Cookie>();
        c.write(Cookie { cookie, io });
        let buf = mem.add(BUF_OFFSET);
        f.write(File::new(
            -1,
            buf,
            BUF_SIZE,
            flags | F_INIT,
            EOF,
            &COOKIE_OPS,
        ));
        (*f).cookie = c.cast();
        list_add(f);
        Some(f)
    }
}

// ---------------------------------------------------------------------------
// popen
// ---------------------------------------------------------------------------

static PIPE_OPS: Ops = Ops {
    read: fd_read,
    write: fd_write,
    seek: fd_seek,
    close: pipe_close,
};

/// Close a `popen` stream: its descriptor, then wait for the child, retrying
/// an interrupted wait -- glibc's `_IO_new_proc_close`.  The child's wait
/// status, or -1.
unsafe fn pipe_close(f: *mut File) -> i32 {
    // SAFETY: the caller holds `f`'s lock.
    let (fd, pid) = unsafe { ((*f).fd, (*f).pipe_pid) };
    // SAFETY: as above.
    unsafe { (*f).pipe_pid = 0 };
    if crate::file::close(fd) < 0 {
        return -1;
    }
    let mut status = 0i32;
    loop {
        let r = crate::process::waitpid(pid, &raw mut status, 0);
        if r >= 0 {
            return status;
        }
        if errno::get_errno() != errno::EINTR {
            return -1;
        }
    }
}

/// Run `command` with `/bin/sh -c -- command` and return a stream on a pipe
/// to its standard input (`w`) or from its standard output (`r`) -- glibc's
/// `popen`.  The mode is `r` or `w` plus an optional `e` (close-on-exec for
/// the stream's descriptor), in any order; anything else, or both or
/// neither of `r` and `w`, is `EINVAL`.
///
/// The child does not inherit the pipes of earlier `popen` streams still
/// open (POSIX), nor the parent's end of its own; the list is held across
/// the spawn so a concurrent `popen` cannot slip a descriptor past it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn popen(command: *const u8, mode: *const u8) -> *mut u8 {
    use crate::spawn::{
        PosixSpawnFileActionsT, posix_spawn, posix_spawn_file_actions_addclose,
        posix_spawn_file_actions_adddup2, posix_spawn_file_actions_destroy,
        posix_spawn_file_actions_init,
    };
    // glibc reads the mode first and faults on a NULL one; a NULL command
    // it passes to the shell as a missing argument, which the child reports.
    if mode.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    let (mut rd, mut wr, mut cloexec) = (false, false, false);
    let mut i = 0usize;
    loop {
        // SAFETY: a C string; stops at its NUL.
        match unsafe { *mode.add(i) } {
            0 => break,
            b'r' => rd = true,
            b'w' => wr = true,
            b'e' => cloexec = true,
            _ => {
                errno::set_errno(errno::EINVAL);
                return core::ptr::null_mut();
            }
        }
        i = i.wrapping_add(1);
    }
    if rd == wr {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }
    let mut fds = [0i32; 2];
    if crate::pipe::pipe2(fds.as_mut_ptr(), crate::fcntl::O_CLOEXEC) < 0 {
        return core::ptr::null_mut();
    }
    let (parent, mut child, child_std) = if rd {
        (fds[0], fds[1], 1)
    } else {
        (fds[1], fds[0], 0)
    };
    let fail = |a: i32, b: i32| {
        let e = errno::get_errno();
        crate::file::close(a);
        crate::file::close(b);
        errno::set_errno(e);
        core::ptr::null_mut()
    };
    // The child's end already has the number it must end up with: move it,
    // or `dup2` onto itself could not clear its close-on-exec (glibc).
    if child == child_std {
        let moved = crate::fcntl_ops::fcntl(child, crate::fcntl_ops::F_DUPFD_CLOEXEC, 0);
        if moved < 0 {
            return fail(parent, child);
        }
        crate::file::close(child);
        child = moved;
    }
    let flags = if rd { F_NOWR } else { F_NORD };
    // SAFETY: a pipe stream keeps no cookie.
    let Some(f) = (unsafe { new_stream(parent, flags, &PIPE_OPS, core::ptr::null_mut()) }) else {
        return fail(parent, child);
    };
    // SAFETY: zeroes are a valid starting state for `init` to overwrite.
    let mut fa: PosixSpawnFileActionsT = unsafe { core::mem::zeroed() };
    posix_spawn_file_actions_init(&raw mut fa);
    let mut err = posix_spawn_file_actions_adddup2(&raw mut fa, child, child_std);
    let list = list_lock();
    if err == 0 {
        // SAFETY: the list is locked, so every stream on it is live.
        let mut p = unsafe { (*list).head };
        while !p.is_null() && err == 0 {
            // SAFETY: as above.
            let (pid, pfd) = unsafe { ((*p).pipe_pid, (*p).fd) };
            if pid != 0 && pfd != child_std && pfd >= 0 {
                err = posix_spawn_file_actions_addclose(&raw mut fa, pfd);
            }
            // SAFETY: as above.
            p = unsafe { (*p).next };
        }
    }
    let mut pid: crate::types::PidT = 0;
    if err == 0 {
        let argv: [*const u8; 5] = [
            c"sh".as_ptr().cast(),
            c"-c".as_ptr().cast(),
            c"--".as_ptr().cast(),
            command,
            core::ptr::null(),
        ];
        err = posix_spawn(
            &raw mut pid,
            c"/bin/sh".as_ptr().cast(),
            &raw const fa,
            core::ptr::null(),
            argv.as_ptr(),
            crate::environ::current_environ(),
        );
        if err == 0 {
            // SAFETY: `f` is live and ours; the list is locked.
            unsafe { (*f).pipe_pid = pid };
        }
    }
    list_unlock(list);
    posix_spawn_file_actions_destroy(&raw mut fa);
    crate::file::close(child);
    if err != 0 {
        // SAFETY: `f` is ours alone.
        unsafe {
            list_remove(f);
            free_stream(f);
        }
        crate::file::close(parent);
        errno::set_errno(err);
        return core::ptr::null_mut();
    }
    if !cloexec {
        // The pipe was made close-on-exec so no other spawn could inherit
        // it; the stream's end keeps that only if the program asked (`e`).
        let _ = crate::fcntl_ops::fcntl(parent, crate::fcntl_ops::F_SETFD, 0); // the stream works either way
    }
    f.cast()
}

/// Close a `popen` stream and return the child's wait status: `fclose`,
/// whose close is the wait (glibc's `pclose`).  Any other stream is simply
/// closed, as glibc's is.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pclose(stream: *mut u8) -> i32 {
    fclose(stream)
}

// ---------------------------------------------------------------------------
// FORTIFY_SOURCE
// ---------------------------------------------------------------------------

/// `__fgets_chk(buf, size, n, stream)`: `fgets` into an object of `size`
/// bytes.  glibc aborts when `n` exceeds it; this clamps (§1105: a smaller
/// read is still a correct `fgets`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __fgets_chk(buf: *mut u8, size: usize, n: i32, stream: *mut u8) -> *mut u8 {
    let n = if usize::try_from(n).is_ok_and(|v| v > size) {
        i32::try_from(size).unwrap_or(i32::MAX)
    } else {
        n
    };
    fgets(buf, n, stream)
}

/// `__fread_chk(ptr, ptrlen, size, nmemb, stream)`: `fread` into an object
/// of `ptrlen` bytes, clamped to it (§1105).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __fread_chk(
    ptr: *mut u8,
    ptrlen: usize,
    size: usize,
    nmemb: usize,
    stream: *mut u8,
) -> usize {
    let fit = ptrlen.checked_div(size).unwrap_or(0);
    // SAFETY: at most `ptrlen` bytes are written.
    unsafe { fread(ptr, size, nmemb.min(fit), stream) }
}

// ---------------------------------------------------------------------------
// perror, remove, temporary names
// ---------------------------------------------------------------------------

/// `s: message\n` on `stderr`, or `message\n` for a NULL or empty `s`,
/// through `stderr`'s stream and without changing its orientation (glibc).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn perror(s: *const u8) {
    let err = errno::get_errno();
    let msg = crate::string::strerror(err);
    let f = &raw mut STDERR_FILE;
    // SAFETY: static.
    let _g = unsafe { locked(f) };
    // SAFETY: locked; `s` and `msg` are C strings.  Nothing here reports
    // failure: perror returns nothing.
    unsafe {
        if !s.is_null() && *s != 0 {
            let _ = fwritex(s, crate::string::strlen(s), f);
            let _ = fwritex(b": ".as_ptr(), 2, f);
        }
        if !msg.is_null() {
            let _ = fwritex(msg, crate::string::strlen(msg), f);
        }
        let _ = fwritex(b"\n".as_ptr(), 1, f);
    }
    errno::set_errno(err);
}

/// Remove a file, or an empty directory (glibc tries `unlink`, then
/// `rmdir` if the name is a directory).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn remove(path: *const u8) -> i32 {
    if crate::file::unlink(path) == 0 {
        return 0;
    }
    if errno::get_errno() == errno::EISDIR && crate::file::rmdir(path) == 0 {
        return 0;
    }
    -1
}

/// `L_tmpnam`: room for [`tmpnam`]'s names.
pub const L_TMPNAM: usize = 20;
/// `TMP_MAX`: how many distinct names [`tmpnam`] promises -- musl's header's
/// 10000, the number a C program compiled here reads (design-decisions.md
/// section 1119).  `tmpnam` itself tries glibc's [`TMPNAM_ATTEMPTS`].
pub const TMP_MAX: u32 = 10_000;

/// How many names [`tmpnam`] tries before it gives up: glibc's
/// `ATTEMPTS_MIN`, 62 cubed, which is also glibc's `TMP_MAX`.
const TMPNAM_ATTEMPTS: u32 = 62 * 62 * 62;

/// Whether `path` names a directory.
fn dir_exists(path: *const u8) -> bool {
    let saved = errno::get_errno();
    // SAFETY: an all-zero `Stat` is a valid output buffer.
    let mut st: crate::stat::Stat = unsafe { core::mem::zeroed() };
    let ok = crate::file::stat(path, &raw mut st) == 0 && st.st_mode & 0o170_000 == 0o040_000;
    errno::set_errno(saved);
    ok
}

/// glibc's `__path_search`: `dir/pfxXXXXXX` into `buf`, with `dir` the first
/// that is a directory of `$TMPDIR` (if `try_tmpdir`), `dir`, and `/tmp`,
/// and `pfx` at most five bytes of it (`"file"` if NULL or empty); a
/// trailing `/` on `dir` is dropped.  The length before the `X`s, or `None`
/// with `ENOENT` (no directory) or `EINVAL` (does not fit).
///
/// # Safety
///
/// `dir` and `pfx` are C strings or NULL.
unsafe fn path_search(
    buf: &mut [u8],
    dir: *const u8,
    pfx: *const u8,
    try_tmpdir: bool,
) -> Option<usize> {
    let mut d: *const u8 = core::ptr::null();
    if try_tmpdir {
        // SAFETY: a C string.
        let env = unsafe { crate::environ::secure_getenv(c"TMPDIR".as_ptr().cast()) };
        if !env.is_null() && dir_exists(env) {
            d = env;
        }
    }
    if d.is_null() && !dir.is_null() && dir_exists(dir) {
        d = dir;
    }
    if d.is_null() {
        if !dir_exists(c"/tmp".as_ptr().cast()) {
            errno::set_errno(errno::ENOENT);
            return None;
        }
        d = c"/tmp".as_ptr().cast();
    }
    // SAFETY: C strings.
    let dir_bytes = unsafe { core::slice::from_raw_parts(d, crate::string::strlen(d)) };
    let mut dlen = dir_bytes.len();
    while dlen > 1 && dir_bytes.get(dlen.wrapping_sub(1)) == Some(&b'/') {
        dlen = dlen.wrapping_sub(1);
    }
    // SAFETY: as above.
    let pfx_bytes: &[u8] = if pfx.is_null() || unsafe { *pfx } == 0 {
        b"file"
    } else {
        // SAFETY: as above.
        unsafe { core::slice::from_raw_parts(pfx, crate::string::strlen(pfx)) }
    };
    let plen = pfx_bytes.len().min(5);
    let stem = dlen.saturating_add(1).saturating_add(plen);
    if stem.saturating_add(7) > buf.len() {
        errno::set_errno(errno::EINVAL);
        return None;
    }
    let parts: [&[u8]; 4] = [
        dir_bytes.get(..dlen).unwrap_or(&[]),
        b"/",
        pfx_bytes.get(..plen).unwrap_or(&[]),
        b"XXXXXX\0",
    ];
    let mut at = 0usize;
    for part in parts {
        for &b in part {
            if let Some(slot) = buf.get_mut(at) {
                *slot = b;
            }
            at = at.wrapping_add(1);
        }
    }
    Some(stem)
}

/// glibc's `__gen_tempname(…, __GT_NOCREATE)`: fill the six `X`s at `at`
/// with random letters and digits until the name does not exist -- `lstat`
/// says `ENOENT` -- trying [`TMPNAM_ATTEMPTS`] times.  `false` with `EEXIST` if every
/// name was taken, or with `lstat`'s error if it failed otherwise.
fn gen_tempname_nocreate(buf: &mut [u8], at: usize) -> bool {
    const LETTERS: &[u8; 62] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let saved = errno::get_errno();
    for _ in 0..TMPNAM_ATTEMPTS {
        let mut r =
            u64::from(crate::random::arc4random()) << 32 | u64::from(crate::random::arc4random());
        for i in 0..6usize {
            let pick = usize::try_from(r % 62).unwrap_or(0);
            r /= 62;
            if let Some(slot) = buf.get_mut(at.wrapping_add(i)) {
                *slot = LETTERS.get(pick).copied().unwrap_or(b'x');
            }
        }
        // SAFETY: an all-zero `Stat` is a valid output buffer; `buf` is a C
        // string (its NUL follows the `X`s).
        let mut st: crate::stat::Stat = unsafe { core::mem::zeroed() };
        if crate::file::lstat(buf.as_ptr(), &raw mut st) == 0 {
            continue;
        }
        if errno::get_errno() == errno::ENOENT {
            errno::set_errno(saved);
            return true;
        }
        return false;
    }
    errno::set_errno(errno::EEXIST);
    false
}

/// A name for a temporary file, `/tmp/fileXXXXXX` with the `X`s chosen so no
/// file has it yet (glibc's).  Into `s` (at least `L_tmpnam` bytes) if not
/// NULL, else into a static buffer the next call overwrites.  NULL if no
/// name could be made.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tmpnam(s: *mut u8) -> *mut u8 {
    static mut NAME: [u8; L_TMPNAM] = [0; L_TMPNAM];
    let mut tmp = [0u8; L_TMPNAM];
    // SAFETY: NULLs for the directory and prefix.
    let Some(at) = (unsafe { path_search(&mut tmp, core::ptr::null(), core::ptr::null(), false) })
    else {
        return core::ptr::null_mut();
    };
    if !gen_tempname_nocreate(&mut tmp, at) {
        return core::ptr::null_mut();
    }
    let out = if s.is_null() {
        (&raw mut NAME).cast::<u8>()
    } else {
        s
    };
    // SAFETY: `out` holds `L_tmpnam` bytes -- the static, or the caller's
    // by `tmpnam`'s contract.
    unsafe { core::ptr::copy_nonoverlapping(tmp.as_ptr(), out, L_TMPNAM) };
    out
}

/// A name for a temporary file in `dir` with prefix `pfx`, as glibc's
/// `tempnam` chooses it (see [`path_search`]): a `malloc`'d string, or NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn tempnam(dir: *const u8, pfx: *const u8) -> *mut u8 {
    let mut buf = [0u8; 4096];
    // SAFETY: the caller's C strings.
    let Some(at) = (unsafe { path_search(&mut buf, dir, pfx, true) }) else {
        return core::ptr::null_mut();
    };
    if !gen_tempname_nocreate(&mut buf, at) {
        return core::ptr::null_mut();
    }
    // SAFETY: `buf` is a C string.
    unsafe { crate::string::strdup(buf.as_ptr()) }
}

// ---------------------------------------------------------------------------
// Test support: the standard streams
// ---------------------------------------------------------------------------

/// Cross-test lock for the three standard streams, which are process-global
/// on the host too (they are C data symbols).  A test that touches one holds
/// [`lock_std_streams_for_test`]'s guard.
///
/// On the host a write to a console descriptor always fails (the syscalls
/// are stubs), so a test must not leave bytes buffered on `stdout`: the
/// guard discards them when it is dropped as well as when it is taken.
#[cfg(test)]
pub static STD_STREAM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Guard returned by [`lock_std_streams_for_test`].
#[cfg(test)]
pub struct StdStreamTestGuard {
    _inner: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for StdStreamTestGuard {
    fn drop(&mut self) {
        // SAFETY: the lock is still held.
        unsafe { purge_std_streams() };
    }
}

/// Empty the standard streams' windows and clear their indicators.
///
/// # Safety
///
/// The caller holds [`STD_STREAM_TEST_LOCK`].
#[cfg(test)]
unsafe fn purge_std_streams() {
    for f in [
        &raw mut STDIN_FILE,
        &raw mut STDOUT_FILE,
        &raw mut STDERR_FILE,
    ] {
        // SAFETY: the caller's contract; static streams.
        unsafe {
            let file = &mut *f;
            file.rpos = core::ptr::null_mut();
            file.rend = core::ptr::null_mut();
            file.wbase = core::ptr::null_mut();
            file.wpos = core::ptr::null_mut();
            file.wend = core::ptr::null_mut();
            file.flags &= !(F_ERR | F_EOF);
            file.mode = 0;
        }
    }
}

/// Take [`STD_STREAM_TEST_LOCK`] (recovering from poison) and start from
/// empty standard streams.  Bind the guard to `_g` for the whole test.
#[cfg(test)]
#[must_use = "the returned guard serialises standard-stream tests; bind it to `_g`"]
pub fn lock_std_streams_for_test() -> StdStreamTestGuard {
    let inner = STD_STREAM_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: the lock is held.
    unsafe { purge_std_streams() };
    StdStreamTestGuard { _inner: inner }
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;
    use core::ffi::c_void;

    // -----------------------------------------------------------------------
    // A far end in memory
    // -----------------------------------------------------------------------

    /// What a cookie stream in a test reads from and writes to, and a record
    /// of how it was called.
    #[derive(Default)]
    struct Mem {
        data: Vec<u8>,
        pos: usize,
        /// Most bytes one `read` returns: 0 means no limit.
        chunk: usize,
        reads: usize,
        /// Each `write` call's bytes.
        writes: Vec<Vec<u8>>,
        /// Every seek, as `(offset, whence)`.
        seeks: Vec<(i64, i32)>,
        fail_writes: bool,
        fail_reads: bool,
        closed: bool,
    }

    unsafe extern "C" fn mem_read(c: *mut c_void, buf: *mut u8, n: usize) -> isize {
        let m = unsafe { &mut *c.cast::<Mem>() };
        m.reads += 1;
        if m.fail_reads {
            errno::set_errno(errno::EIO);
            return -1;
        }
        let left = m.data.len().saturating_sub(m.pos);
        let mut k = left.min(n);
        if m.chunk > 0 {
            k = k.min(m.chunk);
        }
        unsafe { core::ptr::copy_nonoverlapping(m.data.as_ptr().add(m.pos), buf, k) };
        m.pos += k;
        k as isize
    }

    unsafe extern "C" fn mem_write(c: *mut c_void, buf: *const u8, n: usize) -> isize {
        let m = unsafe { &mut *c.cast::<Mem>() };
        if m.fail_writes {
            errno::set_errno(errno::ENOSPC);
            return -1;
        }
        let bytes = unsafe { core::slice::from_raw_parts(buf, n) }.to_vec();
        if m.pos + n > m.data.len() {
            m.data.resize(m.pos + n, 0);
        }
        m.data[m.pos..m.pos + n].copy_from_slice(&bytes);
        m.pos += n;
        m.writes.push(bytes);
        n as isize
    }

    unsafe extern "C" fn mem_seek(c: *mut c_void, off: *mut i64, whence: i32) -> i32 {
        let m = unsafe { &mut *c.cast::<Mem>() };
        let o = unsafe { *off };
        m.seeks.push((o, whence));
        let base = match whence {
            SEEK_SET => 0,
            SEEK_CUR => m.pos as i64,
            SEEK_END => m.data.len() as i64,
            _ => return -1,
        };
        let new = base + o;
        if new < 0 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        m.pos = new as usize;
        unsafe { *off = new };
        0
    }

    unsafe extern "C" fn mem_close(c: *mut c_void) -> i32 {
        unsafe { (*c.cast::<Mem>()).closed = true };
        0
    }

    const MEM_IO: CookieIoFunctions = CookieIoFunctions {
        read: Some(mem_read),
        write: Some(mem_write),
        seek: Some(mem_seek),
        close: Some(mem_close),
    };

    /// A cookie stream over `m`.
    fn open_mem(m: &mut Mem, mode: &core::ffi::CStr) -> *mut u8 {
        let s = unsafe { fopencookie((m as *mut Mem).cast(), mode.as_ptr().cast(), MEM_IO) };
        assert!(!s.is_null(), "fopencookie({mode:?})");
        s
    }

    fn with_data(data: &[u8]) -> Mem {
        Mem {
            data: data.to_vec(),
            ..Mem::default()
        }
    }

    fn flat(writes: &[Vec<u8>]) -> Vec<u8> {
        writes.concat()
    }

    // -----------------------------------------------------------------------
    // Mode strings
    // -----------------------------------------------------------------------

    fn mode(s: &core::ffi::CStr) -> Option<(i32, u32)> {
        unsafe { fopen_mode(s.as_ptr().cast()) }.map(|m| (m.oflags, m.flags))
    }

    #[test]
    fn fopen_modes_are_glibcs() {
        use crate::fcntl::{
            O_APPEND, O_CLOEXEC, O_CREAT, O_EXCL, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY,
        };
        assert_eq!(mode(c"r"), Some((O_RDONLY, F_NOWR)));
        assert_eq!(mode(c"w"), Some((O_WRONLY | O_CREAT | O_TRUNC, F_NORD)));
        assert_eq!(
            mode(c"a"),
            Some((O_WRONLY | O_CREAT | O_APPEND, F_NORD | F_APP))
        );
        assert_eq!(mode(c"r+"), Some((O_RDWR, 0)));
        assert_eq!(mode(c"w+"), Some((O_RDWR | O_CREAT | O_TRUNC, 0)));
        assert_eq!(mode(c"a+"), Some((O_RDWR | O_CREAT | O_APPEND, F_APP)));
        // `+` anywhere in the six letters, not only second or third.
        assert_eq!(mode(c"rbe+"), Some((O_RDWR | O_CLOEXEC, F_CLOEXEC)));
        // `x` is O_EXCL: "wx" must not truncate an existing file.
        assert_eq!(
            mode(c"wx"),
            Some((O_WRONLY | O_CREAT | O_TRUNC | O_EXCL, F_NORD))
        );
        assert_eq!(
            mode(c"re"),
            Some((O_RDONLY | O_CLOEXEC, F_NOWR | F_CLOEXEC))
        );
        // Unknown letters are ignored; `,` ends the letters (`,ccs=`).
        assert_eq!(mode(c"rtq"), Some((O_RDONLY, F_NOWR)));
        assert_eq!(mode(c"r,ccs=UTF-8+"), Some((O_RDONLY, F_NOWR)));
        // Only six letters after the first are read.
        assert_eq!(mode(c"rbbbbbb+"), Some((O_RDONLY, F_NOWR)));
        assert_eq!(mode(c"rbbbbb+"), Some((O_RDWR, 0)));
        // A bad first letter.
        assert_eq!(mode(c"x"), None);
        assert_eq!(mode(c"+r"), None);
        assert_eq!(mode(c""), None);
    }

    #[test]
    fn fopen_refuses_what_glibc_would_fault_on_and_a_bad_mode() {
        errno::set_errno(0);
        assert!(unsafe { fopen(c"/x".as_ptr().cast(), core::ptr::null()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
        errno::set_errno(0);
        assert!(unsafe { fopen(core::ptr::null(), c"r".as_ptr().cast()) }.is_null());
        assert_eq!(
            errno::get_errno(),
            errno::EFAULT,
            "open's own answer for a NULL path"
        );
        errno::set_errno(0);
        assert!(unsafe { fopen(c"/x".as_ptr().cast(), c"q".as_ptr().cast()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn fdopen_checks_its_mode_and_descriptor() {
        errno::set_errno(0);
        assert!(unsafe { fdopen(3, c"z".as_ptr().cast()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        assert!(unsafe { fdopen(-1, c"r".as_ptr().cast()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EBADF, "fcntl's answer");
        errno::set_errno(0);
        assert!(unsafe { fdopen(3, core::ptr::null()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn fopencookie_reads_only_a_plus_that_follows_the_letter() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"rb+");
        let f = s.cast::<File>();
        assert_eq!(unsafe { (*f).flags } & (F_NORD | F_NOWR), 0);
        assert_eq!(fclose(s), 0);
        let s = open_mem(&mut m, c"rx+");
        assert_ne!(
            unsafe { (*s.cast::<File>()).flags } & F_NOWR,
            0,
            "glibc does not see this +"
        );
        assert_eq!(fclose(s), 0);
        errno::set_errno(0);
        assert!(
            unsafe { fopencookie((&raw mut m).cast(), c"q".as_ptr().cast(), MEM_IO) }.is_null()
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // Writing and buffering
    // -----------------------------------------------------------------------

    #[test]
    fn a_fully_buffered_stream_writes_when_full_or_flushed() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        assert_eq!(
            unsafe { fputs(c"hello\n".as_ptr().cast(), s) },
            1,
            "glibc's fputs answers 1"
        );
        assert_eq!(fputc(i32::from(b'x'), s), i32::from(b'x'));
        assert!(m.writes.is_empty(), "buffered");
        assert_eq!(gnu_fpending::__fpending(s), 7);
        assert_eq!(fflush(s), 0);
        assert_eq!(flat(&m.writes), b"hello\nx");
        assert_eq!(gnu_fpending::__fpending(s), 0);
        // A write larger than the room left goes straight through, after
        // what was pending.
        let big = vec![b'z'; BUF_SIZE + 10];
        assert_eq!(fputc(i32::from(b'a'), s), i32::from(b'a'));
        assert_eq!(unsafe { fwrite(big.as_ptr(), 1, big.len(), s) }, big.len());
        let mut want = b"hello\nxa".to_vec();
        want.extend_from_slice(&big);
        assert_eq!(flat(&m.writes), want);
        assert_eq!(fclose(s), 0);
        assert!(m.closed);
    }

    #[test]
    fn a_line_buffered_stream_writes_through_the_last_newline() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        assert_eq!(setvbuf(s, core::ptr::null_mut(), _IOLBF, 0), 0);
        assert_eq!(unsafe { fputs(c"ab\ncd\nef".as_ptr().cast(), s) }, 1);
        assert_eq!(
            flat(&m.writes),
            b"ab\ncd\n",
            "through the last newline at once"
        );
        assert_eq!(gnu_fpending::__fpending(s), 2);
        assert_eq!(fputc(i32::from(b'\n'), s), i32::from(b'\n'));
        assert_eq!(flat(&m.writes), b"ab\ncd\nef\n");
        assert_eq!(gnu_flbf::__flbf(s), 1);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn an_unbuffered_stream_writes_every_call() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        assert_eq!(setvbuf(s, core::ptr::null_mut(), _IONBF, 0), 0);
        assert_eq!(gnu_fbufsize::__fbufsize(s), 0);
        assert_eq!(fputc(i32::from(b'q'), s), i32::from(b'q'));
        assert_eq!(unsafe { fputs(c"rs".as_ptr().cast(), s) }, 1);
        assert_eq!(m.writes, vec![b"q".to_vec(), b"rs".to_vec()]);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn setvbuf_installs_the_programs_buffer_and_refuses_an_unknown_mode() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        let mut mine = [0u8; 64];
        assert_eq!(setvbuf(s, mine.as_mut_ptr(), _IOFBF, mine.len()), 0);
        assert_eq!(gnu_fbufsize::__fbufsize(s), 64 - UNGET);
        assert_eq!(unsafe { fputs(c"kept".as_ptr().cast(), s) }, 1);
        assert_eq!(&mine[UNGET..UNGET + 4], b"kept", "in the program's buffer");
        errno::set_errno(0);
        assert_eq!(setvbuf(s, core::ptr::null_mut(), 7, 0), EOF);
        assert_eq!(errno::get_errno(), 0, "glibc leaves errno alone");
        assert_eq!(fclose(s), 0);
        assert_eq!(flat(&m.writes), b"kept");
    }

    #[test]
    fn a_failed_write_is_an_error_and_loses_what_was_pending() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        assert_eq!(unsafe { fputs(c"lost".as_ptr().cast(), s) }, 1);
        m.fail_writes = true;
        assert_eq!(fflush(s), EOF);
        assert_eq!(ferror(s), 1);
        assert_eq!(
            gnu_fpending::__fpending(s),
            0,
            "glibc empties the buffer either way"
        );
        m.fail_writes = false;
        clearerr(s);
        assert_eq!(fflush(s), 0);
        assert!(m.writes.is_empty(), "nothing was kept to be written again");
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn writing_a_read_only_stream_is_ebadf() {
        let mut m = with_data(b"abc");
        let s = open_mem(&mut m, c"r");
        errno::set_errno(0);
        assert_eq!(fputc(i32::from(b'x'), s), EOF);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(ferror(s), 1);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn reading_a_write_only_stream_is_ebadf() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        errno::set_errno(0);
        assert_eq!(fgetc(s), EOF);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(ferror(s), 1);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn puts_answers_the_length_plus_one() {
        let _g = lock_std_streams_for_test();
        // Fully buffered for the call, so nothing reaches the host's
        // descriptor (whose writes fail); restored after.
        let out = &raw mut STDOUT_FILE;
        let (flags, lbf) = unsafe { ((*out).flags, (*out).lbf) };
        unsafe {
            (*out).flags |= F_INIT;
            (*out).lbf = EOF;
        }
        let r = unsafe { puts(c"four".as_ptr().cast()) };
        unsafe {
            (*out).flags = flags;
            (*out).lbf = lbf;
        }
        assert_eq!(r, 5);
        errno::set_errno(0);
        assert_eq!(unsafe { puts(core::ptr::null()) }, EOF);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -----------------------------------------------------------------------
    // Reading
    // -----------------------------------------------------------------------

    #[test]
    fn fread_waits_for_all_it_asked_for() {
        let data: Vec<u8> = (0..100u8).collect();
        let mut m = with_data(&data);
        m.chunk = 7; // a pipe that hands over a few bytes at a time
        let s = open_mem(&mut m, c"r");
        let mut out = [0u8; 100];
        assert_eq!(unsafe { fread(out.as_mut_ptr(), 1, 100, s) }, 100);
        assert_eq!(out.as_slice(), data.as_slice());
        assert_eq!(feof(s), 0);
        assert_eq!(unsafe { fread(out.as_mut_ptr(), 1, 1, s) }, 0);
        assert_eq!(feof(s), 1);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn fread_counts_whole_items() {
        let mut m = with_data(b"abcdefg");
        let s = open_mem(&mut m, c"r");
        let mut out = [0u8; 9];
        assert_eq!(
            unsafe { fread(out.as_mut_ptr(), 3, 3, s) },
            2,
            "7 bytes are two 3-byte items"
        );
        assert_eq!(&out[..7], b"abcdefg");
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn end_of_file_is_sticky_until_cleared() {
        let mut m = with_data(b"a");
        let s = open_mem(&mut m, c"r");
        assert_eq!(fgetc(s), i32::from(b'a'));
        assert_eq!(fgetc(s), EOF);
        m.data.extend_from_slice(b"b"); // the file grows
        assert_eq!(fgetc(s), EOF, "sticky, as in glibc since 2.28");
        clearerr(s);
        assert_eq!(fgetc(s), i32::from(b'b'));
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn a_read_error_is_an_error() {
        let mut m = with_data(b"abc");
        m.fail_reads = true;
        let s = open_mem(&mut m, c"r");
        assert_eq!(fgetc(s), EOF);
        assert_eq!(ferror(s), 1);
        assert_eq!(feof(s), 0);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn fgets_reads_lines_across_refills() {
        let mut m = with_data(b"first line\nsecond\nlast");
        m.chunk = 4;
        let s = open_mem(&mut m, c"r");
        let mut buf = [0u8; 64];
        let line = |b: &[u8]| {
            let n = b.iter().position(|&c| c == 0).unwrap();
            b[..n].to_vec()
        };
        assert!(!fgets(buf.as_mut_ptr(), 64, s).is_null());
        assert_eq!(line(&buf), b"first line\n");
        assert!(!fgets(buf.as_mut_ptr(), 64, s).is_null());
        assert_eq!(line(&buf), b"second\n");
        assert!(!fgets(buf.as_mut_ptr(), 64, s).is_null());
        assert_eq!(line(&buf), b"last", "a last line without a newline");
        assert!(fgets(buf.as_mut_ptr(), 64, s).is_null());
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn fgets_stops_at_n_minus_one() {
        let mut m = with_data(b"abcdef\n");
        let s = open_mem(&mut m, c"r");
        let mut buf = [0xffu8; 8];
        assert!(!fgets(buf.as_mut_ptr(), 4, s).is_null());
        assert_eq!(&buf[..4], b"abc\0");
        assert!(
            !fgets(buf.as_mut_ptr(), 1, s).is_null(),
            "n == 1 is the empty string"
        );
        assert_eq!(buf[0], 0);
        assert!(fgets(buf.as_mut_ptr(), 0, s).is_null());
        assert!(fgets(buf.as_mut_ptr(), -3, s).is_null());
        errno::set_errno(0);
        assert!(fgets(core::ptr::null_mut(), 8, s).is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn fgets_returns_null_only_for_an_error_new_to_the_call() {
        let mut m = with_data(b"ab");
        let s = open_mem(&mut m, c"r");
        __fseterr_for_test(s);
        let mut buf = [0u8; 8];
        assert!(
            !fgets(buf.as_mut_ptr(), 8, s).is_null(),
            "the old error does not count"
        );
        assert_eq!(&buf[..3], b"ab\0");
        assert_eq!(ferror(s), 1, "and is kept");
        assert_eq!(fclose(s), 0);
    }

    fn __fseterr_for_test(s: *mut u8) {
        gnu_fseterr::__fseterr(s);
    }

    #[test]
    fn getline_reads_lines_of_any_length() {
        let long = vec![b'L'; 3 * BUF_SIZE];
        let mut data = b"short\n".to_vec();
        data.extend_from_slice(&long);
        data.extend_from_slice(b"\nend");
        let mut m = with_data(&data);
        m.chunk = 1000;
        let s = open_mem(&mut m, c"r");
        let mut line: *mut u8 = core::ptr::null_mut();
        let mut n: usize = 0;
        let got = |line: *mut u8, k: isize| {
            unsafe { core::slice::from_raw_parts(line, k as usize) }.to_vec()
        };
        let k = unsafe { getline(&raw mut line, &raw mut n, s) };
        assert_eq!(got(line, k), b"short\n");
        assert!(n >= 7);
        let k = unsafe { getline(&raw mut line, &raw mut n, s) };
        assert_eq!(k as usize, long.len() + 1);
        assert_eq!(got(line, k)[..long.len()], long[..]);
        let k = unsafe { getline(&raw mut line, &raw mut n, s) };
        assert_eq!(got(line, k), b"end");
        assert_eq!(unsafe { *line.add(3) }, 0, "terminated");
        assert_eq!(unsafe { getline(&raw mut line, &raw mut n, s) }, -1);
        unsafe { crate::malloc::free(line) };
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn getdelim_is_glibcs_about_errors() {
        let mut m = with_data(b"a,b");
        let s = open_mem(&mut m, c"r");
        errno::set_errno(0);
        let mut n = 0usize;
        assert_eq!(
            unsafe { getdelim(core::ptr::null_mut(), &raw mut n, i32::from(b','), s) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(ferror(s), 1, "glibc marks the stream too");
        // A stream in error reads nothing.
        let mut line: *mut u8 = core::ptr::null_mut();
        assert_eq!(
            unsafe { getdelim(&raw mut line, &raw mut n, i32::from(b','), s) },
            -1
        );
        assert!(line.is_null());
        clearerr(s);
        assert_eq!(
            unsafe { getdelim(&raw mut line, &raw mut n, i32::from(b','), s) },
            2
        );
        assert_eq!(n, 120, "glibc's first allocation");
        assert_eq!(unsafe { core::slice::from_raw_parts(line, 3) }, b"a,\0");
        unsafe { crate::malloc::free(line) };
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn fgetln_returns_the_line_where_it_lies() {
        let mut m = with_data(b"one\ntwo");
        let s = open_mem(&mut m, c"r");
        let mut len = 0usize;
        let p = unsafe { fgetln(s, &raw mut len) };
        assert_eq!(unsafe { core::slice::from_raw_parts(p, len) }, b"one\n");
        let p = unsafe { fgetln(s, &raw mut len) };
        assert_eq!(unsafe { core::slice::from_raw_parts(p, len) }, b"two");
        assert!(unsafe { fgetln(s, &raw mut len) }.is_null());
        assert_eq!(fclose(s), 0);
    }

    // -----------------------------------------------------------------------
    // ungetc
    // -----------------------------------------------------------------------

    #[test]
    fn ungetc_pushes_back_and_clears_end_of_file() {
        let mut m = with_data(b"xy");
        let s = open_mem(&mut m, c"r");
        assert_eq!(fgetc(s), i32::from(b'x'));
        assert_eq!(ungetc(i32::from(b'Q'), s), i32::from(b'Q'));
        assert_eq!(fgetc(s), i32::from(b'Q'));
        assert_eq!(fgetc(s), i32::from(b'y'));
        assert_eq!(fgetc(s), EOF);
        assert_eq!(feof(s), 1);
        assert_eq!(ungetc(0x1ff, s), 0xff, "an unsigned char");
        assert_eq!(feof(s), 0);
        assert_eq!(fgetc(s), 0xff);
        assert_eq!(ungetc(EOF, s), EOF);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn ungetc_has_room_for_unget_bytes_at_least() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"r");
        assert_eq!(fgetc(s), EOF);
        // At end of file the window sits at the top of the buffer, so the
        // whole buffer and the pushback room below it are free.
        for i in 0..(BUF_SIZE + UNGET) {
            assert_eq!(
                ungetc(i32::from(b'a' + (i % 26) as u8), s),
                i32::from(b'a' + (i % 26) as u8),
                "push {i}"
            );
        }
        assert_eq!(ungetc(i32::from(b'!'), s), EOF, "no room left");
        assert_eq!(gnu_freadahead::__freadahead(s), BUF_SIZE + UNGET);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn ungetc_on_a_write_only_stream_fails() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        assert_eq!(ungetc(i32::from(b'a'), s), EOF);
        assert_eq!(fclose(s), 0);
    }

    // -----------------------------------------------------------------------
    // Positions
    // -----------------------------------------------------------------------

    #[test]
    fn ftell_counts_read_ahead_and_pending_output() {
        let mut m = with_data(b"0123456789");
        let s = open_mem(&mut m, c"r+");
        assert_eq!(fgetc(s), i32::from(b'0'));
        assert_eq!(ftell(s), 1, "the far end is at 10; 9 are read ahead");
        assert_eq!(fseek(s, 3, SEEK_CUR), 0);
        assert_eq!(fgetc(s), i32::from(b'4'));
        assert_eq!(fseek(s, 0, SEEK_END), 0);
        assert_eq!(unsafe { fputs(c"AB".as_ptr().cast(), s) }, 1);
        assert_eq!(ftell(s), 12, "10 at the far end and 2 pending");
        assert_eq!(fclose(s), 0);
        assert_eq!(m.data, b"0123456789AB");
    }

    #[test]
    fn a_write_after_a_read_lands_at_the_streams_position() {
        let mut m = with_data(b"abcdef");
        let s = open_mem(&mut m, c"r+");
        assert_eq!(fgetc(s), i32::from(b'a'));
        // C wants a seek between the two; glibc copes without one, and so
        // does this: the far end is moved back over the read-ahead.
        assert_eq!(fputc(i32::from(b'X'), s), i32::from(b'X'));
        assert_eq!(fclose(s), 0);
        assert_eq!(m.data, b"aXcdef");
    }

    #[test]
    fn fseek_checks_whence_first_and_clears_end_of_file() {
        let mut m = with_data(b"ab");
        let s = open_mem(&mut m, c"r");
        errno::set_errno(0);
        assert_eq!(fseek(s, 0, 9), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert!(m.seeks.is_empty(), "before any seek");
        while fgetc(s) != EOF {}
        assert_eq!(feof(s), 1);
        assert_eq!(fseek(s, 0, SEEK_SET), 0);
        assert_eq!(feof(s), 0);
        assert_eq!(fgetc(s), i32::from(b'a'));
        __fseterr_for_test(s);
        rewind(s);
        assert_eq!(ferror(s), 0, "rewind clears the error too");
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn fgetpos_and_fsetpos_round_trip() {
        let mut m = with_data(b"abcdef");
        let s = open_mem(&mut m, c"r");
        assert_eq!(fgetc(s), i32::from(b'a'));
        assert_eq!(fgetc(s), i32::from(b'b'));
        let mut pos: FposT = -1;
        assert_eq!(fgetpos(s, &raw mut pos), 0);
        assert_eq!(pos, 2);
        assert_eq!(fgetc(s), i32::from(b'c'));
        assert_eq!(fsetpos(s, &raw const pos), 0);
        assert_eq!(fgetc(s), i32::from(b'c'));
        errno::set_errno(0);
        assert_eq!(fgetpos(s, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        errno::set_errno(0);
        assert_eq!(fsetpos(s, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn a_stream_with_no_seek_cannot_seek() {
        let mut m = with_data(b"ab");
        let io = CookieIoFunctions {
            seek: None,
            ..MEM_IO
        };
        let s = unsafe { fopencookie((&raw mut m).cast(), c"r".as_ptr().cast(), io) };
        errno::set_errno(0);
        assert_eq!(fseek(s, 0, SEEK_SET), -1);
        assert_eq!(errno::get_errno(), errno::ESPIPE);
        assert_eq!(ftell(s), -1);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn fflush_on_an_input_stream_gives_back_the_read_ahead() {
        let mut m = with_data(b"abcdef");
        let s = open_mem(&mut m, c"r");
        assert_eq!(fgetc(s), i32::from(b'a'));
        assert_eq!(m.pos, 6, "all read ahead");
        assert_eq!(fflush(s), 0);
        assert_eq!(m.pos, 1, "moved back to the stream's position");
        assert_eq!(gnu_freadahead::__freadahead(s), 0);
        assert_eq!(fgetc(s), i32::from(b'b'));
        assert_eq!(fclose(s), 0);
    }

    // -----------------------------------------------------------------------
    // Orientation
    // -----------------------------------------------------------------------

    #[test]
    fn a_wide_stream_refuses_byte_output_and_reads() {
        let mut m = with_data(b"abc");
        let s = open_mem(&mut m, c"r+");
        assert_eq!(fwide(s, 5), 1);
        assert_eq!(fwide(s, -1), 1, "orientation is set once");
        assert_eq!(unsafe { fputs(c"x".as_ptr().cast(), s) }, EOF);
        assert_eq!(unsafe { fwrite(b"x".as_ptr(), 1, 1, s) }, 0);
        assert_eq!(fgetc(s), EOF);
        assert_eq!(fclose(s), 0);
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        assert_eq!(fwide(s, 0), 0, "none yet");
        assert_eq!(fputc(i32::from(b'a'), s), i32::from(b'a'));
        assert_eq!(fwide(s, 1), -1, "a byte write made it a byte stream");
        assert_eq!(fclose(s), 0);
    }

    // -----------------------------------------------------------------------
    // Streams, the list and closing
    // -----------------------------------------------------------------------

    #[test]
    fn there_is_no_limit_of_sixteen_streams() {
        let _g = lock_std_streams_for_test();
        let mut ms: Vec<Box<Mem>> = (0..100).map(|_| Box::default()).collect();
        let streams: Vec<*mut u8> = ms.iter_mut().map(|m| open_mem(m, c"w")).collect();
        for (i, &s) in streams.iter().enumerate() {
            assert!(!s.is_null(), "stream {i}");
            assert_eq!(
                fputc(i32::from(b'0' + (i % 10) as u8), s),
                i32::from(b'0' + (i % 10) as u8)
            );
        }
        assert_eq!(fflush(core::ptr::null_mut()), 0);
        for (i, m) in ms.iter().enumerate() {
            assert_eq!(
                flat(&m.writes),
                vec![b'0' + (i % 10) as u8],
                "stream {i} flushed by fflush(NULL)"
            );
        }
        for s in streams {
            assert_eq!(fclose(s), 0);
        }
    }

    #[test]
    fn fcloseall_closes_every_stream_it_owns() {
        let mut a = Mem::default();
        let mut b = Mem::default();
        let sa = open_mem(&mut a, c"w");
        let sb = open_mem(&mut b, c"w");
        assert_eq!(unsafe { fputs(c"A".as_ptr().cast(), sa) }, 1);
        assert_eq!(unsafe { fputs(c"B".as_ptr().cast(), sb) }, 1);
        let _g = lock_std_streams_for_test();
        assert_eq!(fcloseall(), 0);
        assert!(a.closed && b.closed);
        assert_eq!(flat(&a.writes), b"A");
        assert_eq!(flat(&b.writes), b"B");
    }

    #[test]
    fn fclose_reports_the_close_callbacks_answer() {
        unsafe extern "C" fn bad_close(_: *mut c_void) -> i32 {
            -1
        }
        let mut m = Mem::default();
        let io = CookieIoFunctions {
            close: Some(bad_close),
            ..MEM_IO
        };
        let s = unsafe { fopencookie((&raw mut m).cast(), c"w".as_ptr().cast(), io) };
        assert_eq!(fclose(s), -1);
    }

    #[test]
    fn fileno_of_a_cookie_stream_is_ebadf() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        errno::set_errno(0);
        assert_eq!(fileno(s), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn freopen_leaves_a_cookie_stream_alone() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        assert!(unsafe { freopen(c"/x".as_ptr().cast(), c"r".as_ptr().cast(), s) }.is_null());
        assert!(!m.closed, "not reopened, not closed");
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn getw_and_putw_round_trip() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w+");
        assert_eq!(putw(-123_456, s), 0);
        assert_eq!(putw(7, s), 0);
        rewind(s);
        assert_eq!(getw(s), -123_456);
        assert_eq!(getw(s), 7);
        assert_eq!(getw(s), EOF);
        assert_eq!(feof(s), 1);
        assert_eq!(fclose(s), 0);
        assert_eq!(m.data.len(), 8);
    }

    // -----------------------------------------------------------------------
    // Locking
    // -----------------------------------------------------------------------

    #[test]
    fn two_threads_writing_one_stream_never_split_a_line() {
        struct Shared(*mut u8);
        unsafe impl Send for Shared {}
        unsafe impl Sync for Shared {}
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        let shared = std::sync::Arc::new(Shared(s));
        let workers: Vec<_> = [
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
        ]
        .into_iter()
        .map(|line| {
            let sh = std::sync::Arc::clone(&shared);
            std::thread::spawn(move || {
                let s = sh.0;
                for _ in 0..2000 {
                    assert_eq!(
                        unsafe { fwrite(line.as_ptr(), 1, line.len(), s) },
                        line.len()
                    );
                }
            })
        })
        .collect();
        for w in workers {
            w.join().unwrap();
        }
        assert_eq!(fclose(s), 0);
        let all = flat(&m.writes);
        assert_eq!(all.len(), 2 * 2000 * 31);
        for line in all.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
            assert!(
                line.iter().all(|&b| b == line[0]) && line.len() == 30,
                "a torn line: {:?}",
                String::from_utf8_lossy(line)
            );
        }
    }

    #[test]
    fn flockfile_is_recursive_and_excludes_other_threads() {
        struct Shared(*mut u8);
        unsafe impl Send for Shared {}
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        flockfile(s.cast());
        flockfile(s.cast());
        assert_eq!(ftrylockfile(s.cast()), 0, "ours: one more level");
        let sh = Shared(s);
        let other = std::thread::spawn(move || {
            let sh = sh;
            ftrylockfile(sh.0.cast())
        })
        .join()
        .unwrap();
        assert_ne!(other, 0, "another thread cannot take it");
        funlockfile(s.cast());
        funlockfile(s.cast());
        funlockfile(s.cast());
        let sh = Shared(s);
        let free = std::thread::spawn(move || {
            let sh = sh;
            let r = ftrylockfile(sh.0.cast());
            if r == 0 {
                funlockfile(sh.0.cast());
            }
            r
        })
        .join()
        .unwrap();
        assert_eq!(free, 0, "released after three unlocks");
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn funlockfile_from_a_stranger_does_nothing() {
        struct Shared(*mut u8);
        unsafe impl Send for Shared {}
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        flockfile(s.cast());
        let sh = Shared(s);
        std::thread::spawn(move || {
            let sh = sh;
            funlockfile(sh.0.cast());
        })
        .join()
        .unwrap();
        let f = s.cast::<File>();
        assert_eq!(unsafe { (*f).lock.count }, 1, "still ours");
        funlockfile(s.cast());
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn fsetlocking_says_and_sets_who_locks() {
        let mut m = Mem::default();
        let s = open_mem(&mut m, c"w");
        assert_eq!(
            gnu_fsetlocking::__fsetlocking(s, FSETLOCKING_QUERY),
            FSETLOCKING_INTERNAL
        );
        assert_eq!(
            gnu_fsetlocking::__fsetlocking(s, FSETLOCKING_BYCALLER),
            FSETLOCKING_INTERNAL
        );
        assert_eq!(
            gnu_fsetlocking::__fsetlocking(s, FSETLOCKING_QUERY),
            FSETLOCKING_BYCALLER
        );
        assert_eq!(fputc(i32::from(b'a'), s), i32::from(b'a'));
        assert_eq!(
            unsafe { (*s.cast::<File>()).lock.count },
            0,
            "the call took no lock"
        );
        assert_eq!(
            gnu_fsetlocking::__fsetlocking(s, FSETLOCKING_INTERNAL),
            FSETLOCKING_BYCALLER
        );
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn the_fork_child_keeps_the_forking_threads_locks_and_drops_the_rest() {
        let mut a = Mem::default();
        let mut b = Mem::default();
        let sa = open_mem(&mut a, c"w");
        let sb = open_mem(&mut b, c"w");
        flockfile(sa.cast());
        // `sb` as another thread would leave it: held, by someone else.
        let fb = sb.cast::<File>();
        unsafe {
            assert!(crate::lowlevellock::lll_trylock(&(*fb).lock.word));
            (*fb).lock.owner.store(-7, Ordering::Relaxed);
            (*fb).lock.count = 1;
        }
        let me = crate::pthread::current_tid();
        let fa = sa.cast::<File>();
        unsafe {
            reset_lock_after_fork(fa, me, me);
            reset_lock_after_fork(fb, me, me);
        }
        unsafe {
            assert_eq!((*fa).lock.count, 1, "the forking thread's lock stays held");
            assert_eq!(
                (*fa).lock.owner.load(Ordering::Relaxed),
                crate::pthread::current_tid()
            );
            assert_eq!((*fb).lock.count, 0, "another thread's is released");
            assert_eq!((*fb).lock.word.load(Ordering::Relaxed), 0);
        }
        funlockfile(sa.cast());
        assert_eq!(fclose(sa), 0);
        assert_eq!(fclose(sb), 0);
    }

    // -----------------------------------------------------------------------
    // stdio_ext
    // -----------------------------------------------------------------------

    #[test]
    fn stdio_ext_reports_what_the_stream_is_doing() {
        let mut m = with_data(b"hello");
        let s = open_mem(&mut m, c"r+");
        assert_eq!(gnu_freadable::__freadable(s), 1);
        assert_eq!(gnu_fwritable::__fwritable(s), 1);
        assert_eq!(gnu_freading::__freading(s), 0, "nothing done yet");
        assert_eq!(gnu_fwriting::__fwriting(s), 0);
        assert_eq!(fgetc(s), i32::from(b'h'));
        assert_eq!(gnu_freading::__freading(s), 1);
        assert_eq!(gnu_freadahead::__freadahead(s), 4);
        let mut n = 0usize;
        let p = unsafe { gnu_freadptr::__freadptr(s, &raw mut n) };
        assert_eq!(unsafe { core::slice::from_raw_parts(p, n) }, b"ello");
        gnu_freadptrinc::__freadptrinc(s, 2);
        assert_eq!(fgetc(s), i32::from(b'l'));
        gnu_freadptrinc::__freadptrinc(s, 99);
        assert_eq!(gnu_freadahead::__freadahead(s), 0, "clamped to the window");
        assert_eq!(fputc(i32::from(b'!'), s), i32::from(b'!'));
        assert_eq!(gnu_fwriting::__fwriting(s), 1);
        assert_eq!(gnu_freading::__freading(s), 0);
        assert_eq!(gnu_fbufsize::__fbufsize(s), BUF_SIZE);
        assert_eq!(gnu_flbf::__flbf(s), 0);
        assert_eq!(fclose(s), 0);
        let mut w = Mem::default();
        let s = open_mem(&mut w, c"w");
        assert_eq!(gnu_freadable::__freadable(s), 0);
        assert_eq!(gnu_fwriting::__fwriting(s), 1, "write-only is writing");
        assert_eq!(fclose(s), 0);
    }

    #[test]
    fn fpurge_throws_away_both_directions() {
        let mut m = with_data(b"abc");
        let s = open_mem(&mut m, c"r+");
        assert_eq!(fgetc(s), i32::from(b'a'));
        assert_eq!(ungetc(i32::from(b'Z'), s), i32::from(b'Z'));
        gnu_fpurge::__fpurge(s);
        assert_eq!(gnu_freadahead::__freadahead(s), 0);
        assert_eq!(fgetc(s), EOF, "the far end was all read");
        assert_eq!(fseek(s, 0, SEEK_END), 0);
        assert_eq!(unsafe { fputs(c"xyz".as_ptr().cast(), s) }, 1);
        assert_eq!(gnu_fpurge_bsd::fpurge(s), 0);
        assert_eq!(gnu_fpending::__fpending(s), 0);
        assert_eq!(fclose(s), 0);
        assert_eq!(m.data, b"abc", "the purged output was never written");
    }

    // -----------------------------------------------------------------------
    // popen
    // -----------------------------------------------------------------------

    #[test]
    fn popen_reads_glibcs_mode_letters() {
        for bad in [c"", c"rw", c"x", c"re+", c"ee"] {
            errno::set_errno(0);
            assert!(
                unsafe { popen(c"true".as_ptr().cast(), bad.as_ptr().cast()) }.is_null(),
                "{bad:?}"
            );
            assert_eq!(errno::get_errno(), errno::EINVAL, "{bad:?}");
        }
        errno::set_errno(0);
        assert!(unsafe { popen(c"true".as_ptr().cast(), core::ptr::null()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -----------------------------------------------------------------------
    // The standard streams, and NULL
    // -----------------------------------------------------------------------

    #[test]
    fn the_standard_streams_are_three_real_pointers() {
        let (i, o, e) = (
            stdin_stream(),
            stdout_stream(),
            (&raw mut STDERR_FILE).cast::<u8>(),
        );
        assert!(!i.is_null() && !o.is_null() && !e.is_null());
        assert!(i != o && o != e && i != e);
        assert_eq!(stdin.0.cast::<u8>(), i);
        assert_eq!(stdout.0.cast::<u8>(), o);
        assert_eq!(stderr.0.cast::<u8>(), e);
        assert_eq!(_IO_stdin_.0.cast::<u8>(), i);
        assert_eq!(fileno(i), 0);
        assert_eq!(fileno(o), 1);
        assert_eq!(fileno(e), 2);
        let _g = lock_std_streams_for_test();
        assert_eq!(gnu_fbufsize::__fbufsize(e), 0, "stderr is unbuffered");
        assert_eq!(gnu_freadable::__freadable(o), 0);
        assert_eq!(gnu_fwritable::__fwritable(i), 0);
    }

    #[test]
    fn a_null_stream_is_ebadf_everywhere() {
        let _g = lock_std_streams_for_test();
        let n = core::ptr::null_mut();
        let check = |what: &str, bad: bool| {
            assert!(bad, "{what}");
            assert_eq!(errno::get_errno(), errno::EBADF, "{what}");
            errno::set_errno(0);
        };
        errno::set_errno(0);
        check("fgetc", fgetc(n) == EOF);
        check("fputc", fputc(1, n) == EOF);
        check("fgets", fgets([0u8; 4].as_mut_ptr(), 4, n).is_null());
        check("fputs", unsafe { fputs(c"x".as_ptr().cast(), n) } == EOF);
        check(
            "fread",
            unsafe { fread([0u8; 4].as_mut_ptr(), 1, 4, n) } == 0,
        );
        check("fwrite", unsafe { fwrite(b"x".as_ptr(), 1, 1, n) } == 0);
        check("fseek", fseek(n, 0, SEEK_SET) == -1);
        check("ftell", ftell(n) == -1);
        check("fclose", fclose(n) == EOF);
        check("ungetc", ungetc(1, n) == EOF);
        check("fileno", fileno(n) == -1);
        check("ferror", ferror(n) == 1);
        check(
            "setvbuf",
            setvbuf(n, core::ptr::null_mut(), _IONBF, 0) == EOF,
        );
        assert_eq!(
            fflush(n),
            0,
            "fflush(NULL) is every stream, which C defines"
        );
    }

    // -----------------------------------------------------------------------
    // FORTIFY_SOURCE
    // -----------------------------------------------------------------------

    #[test]
    fn the_fortified_reads_clamp_to_the_object() {
        let mut m = with_data(b"0123456789\n");
        let s = open_mem(&mut m, c"r");
        let mut small = [0u8; 4];
        assert!(!__fgets_chk(small.as_mut_ptr(), small.len(), 100, s).is_null());
        assert_eq!(&small, b"012\0");
        assert_eq!(unsafe { __fread_chk(small.as_mut_ptr(), 4, 1, 100, s) }, 4);
        assert_eq!(&small, b"3456");
        assert_eq!(unsafe { __fread_chk(small.as_mut_ptr(), 4, 0, 100, s) }, 0);
        assert_eq!(fclose(s), 0);
    }

    // -----------------------------------------------------------------------
    // Temporary names
    // -----------------------------------------------------------------------

    #[test]
    fn path_search_builds_glibcs_template() {
        // No directory exists on the host, so glibc's answer is ENOENT.
        let mut buf = [0u8; 64];
        errno::set_errno(0);
        assert_eq!(
            unsafe { path_search(&mut buf, core::ptr::null(), core::ptr::null(), false) },
            None
        );
        assert_eq!(errno::get_errno(), errno::ENOENT);
        assert!(tmpnam(core::ptr::null_mut()).is_null());
    }

    #[test]
    fn remove_of_nothing_fails() {
        assert_eq!(remove(core::ptr::null()), -1);
    }
}
