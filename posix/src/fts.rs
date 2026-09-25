// The arithmetic in this file is on path and name lengths, which are capped at
// `u16::MAX` before any sum is formed (an entry's `fts_pathlen` is a `u16`,
// and `grow_path` refuses to reach it), and on allocation sizes built from
// those. None of it can overflow a `usize`; clippy cannot see the caps across
// the functions that establish them.
#![allow(clippy::arithmetic_side_effects)]

//! `<fts.h>` — file tree traversal, with glibc's ABI and BSD's algorithm.
//!
//! `fts` is the cursor-style walker `find(1)`, `rm -r`, `du` and `chmod -R`
//! are built on in the BSD and GNU worlds: `fts_open` names the roots,
//! `fts_read` returns one entry at a time — directories twice, pre-order
//! ([`FTS_D`]) and post-order ([`FTS_DP`]) — and `fts_set` steers.
//!
//! ## The ABI is glibc's
//!
//! [`FtsEnt`] and [`Fts`] have glibc's `FTSENT` and `FTS` layouts field for
//! field (pinned by offset in the tests), and every constant has glibc's
//! value. musl ships no `fts`, so a C program that calls these functions was
//! compiled against glibc's `<fts.h>`, and anything else would be a silent
//! disagreement about memory. Until 2026-09-25 neither held: the structs were
//! this crate's own shape, and `FTS_AGAIN`/`FTS_FOLLOW`/`FTS_NOINSTR`/
//! `FTS_SKIP` were numbered 2/1/4/3 against glibc's 1/2/3/4 — so a program
//! built against glibc asking to skip a subtree was told nothing, and
//! descended into it.
//!
//! ## The algorithm is BSD's
//!
//! This follows the structure of 4.4BSD's `fts.c`, which glibc and every BSD
//! still use: each entry is its own heap node ([`FtsEnt`], name inline, `stat`
//! buffer after it); reading a directory builds its children as a linked list
//! in one pass and closes the stream at once; `fts_read` walks that tree
//! freeing each node as it leaves it; and every entry's `fts_path` points into
//! one shared path buffer, rewritten as the walk moves and grown (with every
//! live node re-pointed) when a name will not fit. What that buys over the
//! implementation it replaced:
//!
//! - **No depth limit and no stream limit.** Nothing is held open between
//!   calls, so depth costs neither `Dir` slots nor descriptors, and streams
//!   are heap objects. (There were 8 levels and 2 streams.)
//! - **Every root is walked**, in `argv` order or sorted by `compar`; children
//!   are sorted by `compar` too. (Only the first root was, and `compar` was
//!   ignored.)
//! - **`fts_children`**, **`FTS_SEEDOT`** and **`FTS_XDEV`** work, and a
//!   directory that is its own ancestor — a symlink loop under
//!   [`FTS_LOGICAL`] — is reported [`FTS_DC`] rather than walked until
//!   something runs out.
//! - `fts_parent`, `fts_link`, `fts_cycle`, `fts_number` and `fts_pointer`
//!   are real, so the BSD `du` idiom of accumulating into
//!   `ent->fts_parent->fts_number` works.
//!
//! ## What differs from glibc, deliberately
//!
//! - **It never changes directory.** Every walk behaves as [`FTS_NOCHDIR`]
//!   (which is reported in `fts_options`): `fts_accpath` equals `fts_path` for
//!   everything below the roots. `chdir`-based walking is an optimisation for
//!   very long paths that also makes `fts` unusable in a threaded program,
//!   and this libc's working directory is process state that other threads
//!   read.
//! - `fts_symfd` is always -1, since there is no directory to return to.
//! - A NULL `ftsp` or `FTSENT *` is refused with `EBADF`/`EFAULT` rather than
//!   dereferenced.
//! - `fts_open` insists on exactly one of [`FTS_LOGICAL`] and
//!   [`FTS_PHYSICAL`], as the manual page says it must.
//!
//! ## Testing
//!
//! The walk reaches the filesystem only through an [`FsOps`] — `stat` and
//! "list this directory" — so the tests run it against an in-memory tree on
//! the host, where the real calls are stubbed out. The implementation it
//! replaced could only be tested at its edges for that reason, and none of
//! its traversal logic had ever run in a test.

use crate::errno;
use crate::fcntl::{S_IFDIR, S_IFLNK, S_IFMT, S_IFREG};
use crate::stat::Stat;

// ---------------------------------------------------------------------------
// fts_open options (glibc values)
// ---------------------------------------------------------------------------

/// Follow a symlink named as a root.
pub const FTS_COMFOLLOW: i32 = 0x0001;
/// Follow every symlink (logical walk).
pub const FTS_LOGICAL: i32 = 0x0002;
/// Do not change directory — always in effect here (see the module docs).
pub const FTS_NOCHDIR: i32 = 0x0004;
/// Do not `stat` entries that `readdir` says are not directories.
pub const FTS_NOSTAT: i32 = 0x0008;
/// Do not follow symlinks (physical walk).
pub const FTS_PHYSICAL: i32 = 0x0010;
/// Return `.` and `..` (as [`FTS_DOT`]).
pub const FTS_SEEDOT: i32 = 0x0020;
/// Do not descend into a directory on another device than its root.
pub const FTS_XDEV: i32 = 0x0040;
/// Return whiteout entries — accepted; this system has none.
pub const FTS_WHITEOUT: i32 = 0x0080;
/// Every option a caller may pass.
pub const FTS_OPTIONMASK: i32 = 0x00ff;
/// Private: the children were built by `fts_children(FTS_NAMEONLY)`.
/// Also the `instr` argument `fts_children` accepts, as in glibc.
pub const FTS_NAMEONLY: i32 = 0x0100;
/// Private: stop — an error the walk cannot recover from.
const FTS_STOP: i32 = 0x0200;

/// `fts_level` of the invisible parent of the roots.
pub const FTS_ROOTPARENTLEVEL: i16 = -1;
/// `fts_level` of the roots.
pub const FTS_ROOTLEVEL: i16 = 0;

// ---------------------------------------------------------------------------
// fts_info values (glibc values)
// ---------------------------------------------------------------------------

/// Directory, pre-order.
pub const FTS_D: u16 = 1;
/// Directory that is its own ancestor (a cycle); `fts_cycle` names it.
pub const FTS_DC: u16 = 2;
/// None of the other types.
pub const FTS_DEFAULT: u16 = 3;
/// Directory that cannot be read; `fts_errno` says why.
pub const FTS_DNR: u16 = 4;
/// `.` or `..` (only with [`FTS_SEEDOT`]).
pub const FTS_DOT: u16 = 5;
/// Directory, post-order.
pub const FTS_DP: u16 = 6;
/// Error; `fts_errno` says which.
pub const FTS_ERR: u16 = 7;
/// Regular file.
pub const FTS_F: u16 = 8;
/// Not yet read (the state before the first `fts_read`).
pub const FTS_INIT: u16 = 9;
/// `stat` failed; `fts_errno` says why.
pub const FTS_NS: u16 = 10;
/// Not `stat`ed, by request ([`FTS_NOSTAT`] or [`FTS_NAMEONLY`]).
pub const FTS_NSOK: u16 = 11;
/// Symbolic link.
pub const FTS_SL: u16 = 12;
/// Symbolic link whose target does not exist.
pub const FTS_SLNONE: u16 = 13;
/// Whiteout (never produced here).
pub const FTS_W: u16 = 14;

// ---------------------------------------------------------------------------
// fts_flags (private in glibc, visible in the struct)
// ---------------------------------------------------------------------------

/// glibc: do not `chdir` back up through this node. Never set here.
pub const FTS_DONTCHDIR: u16 = 0x01;
/// This directory was reached by following a symlink (`FTS_FOLLOW`).
pub const FTS_SYMFOLLOW: u16 = 0x02;

// ---------------------------------------------------------------------------
// fts_set instructions (glibc values)
// ---------------------------------------------------------------------------

/// Return this entry again on the next `fts_read`.
pub const FTS_AGAIN: i32 = 1;
/// Follow this symlink.
pub const FTS_FOLLOW: i32 = 2;
/// No instruction.
pub const FTS_NOINSTR: i32 = 3;
/// Do not descend into this directory.
pub const FTS_SKIP: i32 = 4;

// ---------------------------------------------------------------------------
// The structures (glibc layout)
// ---------------------------------------------------------------------------

/// A file in the hierarchy — glibc's `FTSENT`.
///
/// Allocated with its name inline: `fts_name` is declared with one byte, as
/// in C, and the allocation extends past the struct to hold the whole name,
/// its NUL and — unless [`FTS_NOSTAT`] — the `stat` buffer `fts_statp`
/// points at. Only a pointer to one is ever valid; never copy one by value.
#[repr(C)]
pub struct FtsEnt {
    /// For [`FTS_DC`]: the ancestor this directory repeats.
    pub fts_cycle: *mut FtsEnt,
    /// The directory containing this entry; the invisible root parent for a
    /// root.
    pub fts_parent: *mut FtsEnt,
    /// The next entry in the same directory (in `fts_children`'s list, and
    /// among the roots).
    pub fts_link: *mut FtsEnt,
    /// For the caller's use; 0 initially.
    pub fts_number: i64,
    /// For the caller's use; null initially.
    pub fts_pointer: *mut core::ffi::c_void,
    /// A path the file can be reached by. Equal to `fts_path` here.
    pub fts_accpath: *mut u8,
    /// The path from the root, in the stream's one shared buffer: valid (and
    /// NUL-terminated) only for the entry `fts_read` returned last.
    pub fts_path: *mut u8,
    /// The error for [`FTS_DNR`], [`FTS_ERR`] and [`FTS_NS`] entries.
    pub fts_errno: i32,
    /// glibc's descriptor for following a symlink; always -1 here.
    pub fts_symfd: i32,
    /// Length of `fts_path`.
    pub fts_pathlen: u16,
    /// Length of `fts_name`.
    pub fts_namelen: u16,
    /// Inode number, from the entry's `stat`.
    pub fts_ino: u64,
    /// Device, from the entry's `stat`.
    pub fts_dev: u64,
    /// Link count, from the entry's `stat`.
    pub fts_nlink: u64,
    /// Depth: [`FTS_ROOTPARENTLEVEL`] (-1), then 0 for the roots and one more
    /// per level below.
    pub fts_level: i16,
    /// What the entry is: [`FTS_D`], [`FTS_F`], …
    pub fts_info: u16,
    /// [`FTS_SYMFOLLOW`] if reached by following a symlink.
    pub fts_flags: u16,
    /// The pending [`fts_set`] instruction.
    pub fts_instr: u16,
    /// The entry's `stat`; null under [`FTS_NOSTAT`].
    pub fts_statp: *mut Stat,
    /// The file's name — the first byte of it; the rest follows in the same
    /// allocation.
    pub fts_name: [u8; 1],
}

/// The comparison `fts_open` sorts siblings with, glibc's type.
pub type FtsCompar = unsafe extern "C" fn(*const *const FtsEnt, *const *const FtsEnt) -> i32;

/// An open traversal — glibc's `FTS`.
#[repr(C)]
pub struct Fts {
    /// The entry `fts_read` returned last.
    pub fts_cur: *mut FtsEnt,
    /// Children built by `fts_children`, not yet walked.
    pub fts_child: *mut FtsEnt,
    /// Scratch for sorting.
    pub fts_array: *mut *mut FtsEnt,
    /// Device of the root being walked ([`FTS_XDEV`]).
    pub fts_dev: u64,
    /// The shared path buffer every entry's `fts_path` points into.
    pub fts_path: *mut u8,
    /// glibc's descriptor for the starting directory; always -1 here.
    pub fts_rfd: i32,
    /// Capacity of `fts_path`.
    pub fts_pathlen: i32,
    /// Capacity of `fts_array`.
    pub fts_nitems: i32,
    /// The caller's comparison, if any.
    pub fts_compar: Option<FtsCompar>,
    /// The `fts_open` options, with [`FTS_NOCHDIR`] always set.
    pub fts_options: i32,
}

// ---------------------------------------------------------------------------
// The filesystem, as the walk sees it
// ---------------------------------------------------------------------------

/// `stat` (`follow`) or `lstat` `path` into `out`; the errno on failure.
pub(crate) type StatFn = fn(path: *const u8, follow: bool, out: &mut Stat) -> Result<(), i32>;

/// What a listing hands each entry to: its name and `d_type`. Returning
/// false stops the listing.
pub(crate) type EntrySink<'a> = dyn FnMut(&[u8], u8) -> bool + 'a;

/// Call `each` for every entry of directory `path`, including `.` and `..`
/// where the directory lists them, until `each` returns false. The errno if
/// the directory cannot be read.
pub(crate) type ListFn = fn(path: *const u8, each: &mut EntrySink<'_>) -> Result<(), i32>;

/// The two questions a walk asks of the filesystem. The real ones are
/// [`REAL_OPS`]; the tests answer them from an in-memory tree.
pub(crate) struct FsOps {
    /// See [`StatFn`].
    pub(crate) stat: StatFn,
    /// See [`ListFn`].
    pub(crate) list: ListFn,
}

/// The real filesystem, through this libc's own `stat` and `<dirent.h>`.
pub(crate) static REAL_OPS: FsOps = FsOps {
    stat: real_stat,
    list: real_list,
};

fn real_stat(path: *const u8, follow: bool, out: &mut Stat) -> Result<(), i32> {
    let rc = if follow {
        crate::file::stat(path, out)
    } else {
        crate::file::lstat(path, out)
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(errno::get_errno())
    }
}

fn real_list(path: *const u8, each: &mut EntrySink<'_>) -> Result<(), i32> {
    let dir = crate::dirent::opendir(path);
    if dir.is_null() {
        return Err(errno::get_errno());
    }
    let mut result = Ok(());
    loop {
        errno::set_errno(0);
        let ent = crate::dirent::readdir(dir);
        if ent.is_null() {
            let e = errno::get_errno();
            if e != 0 {
                result = Err(e);
            }
            break;
        }
        // SAFETY: `readdir` returned a live entry, valid until the next call
        // on this stream; its name is NUL-terminated within `d_name`.
        let (name, d_type) = unsafe {
            let name_ptr = core::ptr::addr_of!((*ent).d_name).cast::<u8>();
            let len = crate::string::strlen(name_ptr);
            (core::slice::from_raw_parts(name_ptr, len), (*ent).d_type)
        };
        if !each(name, d_type) {
            break;
        }
    }
    // The listing has been read; a failure to release the stream changes
    // nothing about what was read, and `closedir` has no other failure mode
    // than a bad pointer, which this is not.
    let _ = crate::dirent::closedir(dir);
    result
}

// ---------------------------------------------------------------------------
// The stream
// ---------------------------------------------------------------------------

/// What `fts_open` allocates: the public [`Fts`] first, so the pointer handed
/// out is a pointer to this, plus what the caller has no business seeing.
#[repr(C)]
struct Stream {
    fts: Fts,
    ops: &'static FsOps,
}

/// The stream behind a caller's `FTS *`.
///
/// # Safety
///
/// `ftsp` must be non-null and a live pointer returned by [`fts_open`] or
/// `open_with`.
unsafe fn stream<'a>(ftsp: *mut Fts) -> &'a mut Stream {
    // SAFETY: the caller's contract; `Fts` is `Stream`'s first field in a
    // `repr(C)` struct, so the two pointers are the same address.
    unsafe { &mut *ftsp.cast::<Stream>() }
}

impl Stream {
    fn isset(&self, opt: i32) -> bool {
        self.fts.fts_options & opt != 0
    }

    fn set(&mut self, opt: i32) {
        self.fts.fts_options |= opt;
    }

    fn clear(&mut self, opt: i32) {
        self.fts.fts_options &= !opt;
    }
}

// ---------------------------------------------------------------------------
// Entries
// ---------------------------------------------------------------------------

/// Byte offset of `fts_name` in an entry.
const NAME_OFFSET: usize = core::mem::offset_of!(FtsEnt, fts_name);

/// A pointer to `p`'s inline name.
///
/// # Safety
///
/// `p` must be a live entry.
unsafe fn name_ptr(p: *mut FtsEnt) -> *mut u8 {
    // SAFETY: the name starts at `NAME_OFFSET` inside `p`'s own allocation.
    unsafe { p.cast::<u8>().add(NAME_OFFSET) }
}

/// `p`'s name as a slice.
///
/// # Safety
///
/// `p` must be a live entry; the slice must not outlive it.
unsafe fn name_of<'a>(p: *mut FtsEnt) -> &'a [u8] {
    // SAFETY: `fts_namelen` bytes of name were written at `name_ptr(p)`.
    unsafe { core::slice::from_raw_parts(name_ptr(p), usize::from((*p).fts_namelen)) }
}

/// Allocate an entry named `name`, glibc's `fts_alloc`: the struct, the name
/// and its NUL, and — unless [`FTS_NOSTAT`] — an aligned, zeroed `Stat`, in
/// one block. Null with `errno` set if it cannot be had.
fn alloc_ent(sp: &Stream, name: &[u8]) -> *mut FtsEnt {
    let Ok(namelen) = u16::try_from(name.len()) else {
        errno::set_errno(errno::ENAMETOOLONG);
        return core::ptr::null_mut();
    };
    let nostat = sp.isset(FTS_NOSTAT);
    let name_end = NAME_OFFSET + name.len() + 1;
    let (total, stat_off) = if nostat {
        (name_end.max(core::mem::size_of::<FtsEnt>()), 0)
    } else {
        let align = core::mem::align_of::<Stat>();
        let stat_off = (name_end + align - 1) & !(align - 1);
        (stat_off + core::mem::size_of::<Stat>(), stat_off)
    };
    let block = crate::malloc::malloc(total);
    if block.is_null() {
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    }
    let p = block.cast::<FtsEnt>();
    // SAFETY: `block` is a fresh allocation of `total` bytes, which covers the
    // struct (`total >= size_of::<FtsEnt>()` in both arms, since the name
    // starts inside it and the stat, if any, after it), the name and its NUL
    // from `NAME_OFFSET`, and the stat at `stat_off`, aligned for `Stat` —
    // `malloc`'s 16-byte alignment covers `FtsEnt`'s 8 and `Stat`'s.
    unsafe {
        p.write(FtsEnt {
            fts_cycle: core::ptr::null_mut(),
            fts_parent: core::ptr::null_mut(),
            fts_link: core::ptr::null_mut(),
            fts_number: 0,
            fts_pointer: core::ptr::null_mut(),
            fts_accpath: core::ptr::null_mut(),
            fts_path: sp.fts.fts_path,
            fts_errno: 0,
            fts_symfd: -1,
            fts_pathlen: 0,
            fts_namelen: namelen,
            fts_ino: 0,
            fts_dev: 0,
            fts_nlink: 0,
            fts_level: 0,
            fts_info: 0,
            fts_flags: 0,
            fts_instr: FTS_NOINSTR as u16,
            fts_statp: core::ptr::null_mut(),
            fts_name: [0],
        });
        core::ptr::copy_nonoverlapping(name.as_ptr(), name_ptr(p), name.len());
        name_ptr(p).add(name.len()).write(0);
        if !nostat {
            let statp = block.add(stat_off).cast::<Stat>();
            core::ptr::write_bytes(statp, 0, 1);
            (*p).fts_statp = statp;
        }
    }
    p
}

/// Free an entry.
///
/// # Safety
///
/// `p` must be a live entry nothing will use again.
unsafe fn free_ent(p: *mut FtsEnt) {
    // SAFETY: entries are `malloc` blocks (`alloc_ent`).
    unsafe { crate::malloc::free(p.cast::<u8>()) };
}

/// Free a list linked through `fts_link`.
///
/// # Safety
///
/// Every entry on the list must be live and used by nothing else.
unsafe fn free_list(mut p: *mut FtsEnt) {
    while !p.is_null() {
        // SAFETY: `p` is a live entry of the list (the caller's contract),
        // read before it is freed.
        let next = unsafe { (*p).fts_link };
        // SAFETY: as above.
        unsafe { free_ent(p) };
        p = next;
    }
}

/// `.` or `..`?
fn is_dot(name: &[u8]) -> bool {
    name == b"." || name == b".."
}

// ---------------------------------------------------------------------------
// The path buffer
// ---------------------------------------------------------------------------

/// Make the path buffer hold at least `needed` bytes (glibc's `fts_palloc`,
/// which grows by the request plus 256).
///
/// An entry's `fts_pathlen` is a `u16`, so a path of `u16::MAX` bytes or more
/// cannot be described and is `ENAMETOOLONG`. On failure the buffer is left
/// as it was — glibc frees it, and with it every entry's `fts_path`.
fn grow_path(sp: &mut Stream, needed: usize) -> Result<(), i32> {
    let have = usize::try_from(sp.fts.fts_pathlen).unwrap_or(0);
    if needed <= have {
        return Ok(());
    }
    let want = needed.saturating_add(256);
    if want >= usize::from(u16::MAX) {
        return Err(errno::ENAMETOOLONG);
    }
    // SAFETY: `fts_path` is null or this stream's own `malloc` block.
    let grown = unsafe { crate::malloc::realloc(sp.fts.fts_path, want) };
    if grown.is_null() {
        return Err(errno::ENOMEM);
    }
    sp.fts.fts_path = grown;
    sp.fts.fts_pathlen = want as i32;
    Ok(())
}

/// Where a child's name starts in the path buffer: after its parent's path,
/// less a trailing `/` that the parent's path already ends in (glibc's
/// `NAPPEND`).
///
/// # Safety
///
/// `p` must be a live entry whose path is the one now in the buffer.
unsafe fn napppend(p: *mut FtsEnt) -> usize {
    // SAFETY: the caller's contract; `fts_path` holds `fts_pathlen` bytes.
    unsafe {
        let len = usize::from((*p).fts_pathlen);
        if len > 0 && *(*p).fts_path.add(len - 1) == b'/' {
            len - 1
        } else {
            len
        }
    }
}

/// Re-point every live entry at the path buffer after it moved (glibc's
/// `fts_padjust`): the unwalked children, and — from `head`, a list just
/// built — each entry, then its later siblings, then its parent's, up to the
/// roots. Earlier siblings have already been freed.
///
/// glibc moves each access path by its offset from the old buffer, because
/// under `chdir` an access path can point into the middle of it. Here every
/// access path below the roots *is* the buffer (the walk never changes
/// directory), so each is simply re-pointed — which stays right however many
/// times the buffer moved while one directory was read, where offsets from a
/// single "old" address would not.
///
/// # Safety
///
/// `head` must be null or a list of live entries whose `fts_parent` chains
/// are live up to the root parent.
unsafe fn adjust_paths(sp: &mut Stream, head: *mut FtsEnt) {
    let new = sp.fts.fts_path;
    let fix = |p: *mut FtsEnt| {
        // SAFETY: `p` is live (the caller's contract). An access path at the
        // entry's own name — a root not yet loaded — is left alone.
        unsafe {
            let acc = (*p).fts_accpath;
            if !acc.is_null() && acc != name_ptr(p) {
                (*p).fts_accpath = new;
            }
            (*p).fts_path = new;
        }
    };
    let mut p = sp.fts.fts_child;
    while !p.is_null() {
        fix(p);
        // SAFETY: live list entry.
        p = unsafe { (*p).fts_link };
    }
    let mut p = head;
    // SAFETY: every entry reached is live, and the walk ends at the root
    // parent, whose level is below `FTS_ROOTLEVEL`.
    unsafe {
        while !p.is_null() && (*p).fts_level >= FTS_ROOTLEVEL {
            fix(p);
            p = if (*p).fts_link.is_null() {
                (*p).fts_parent
            } else {
                (*p).fts_link
            };
        }
    }
}

// ---------------------------------------------------------------------------
// stat
// ---------------------------------------------------------------------------

/// All-zero `Stat`.
fn zeroed_stat() -> Stat {
    // SAFETY: `Stat` is a `repr(C)` struct of integers; all-zero is valid.
    unsafe { core::mem::zeroed() }
}

/// `stat` or `lstat` `p` and classify it (glibc's `fts_stat`).
///
/// Follows symlinks under [`FTS_LOGICAL`] or when `follow`. A dangling link
/// being followed is [`FTS_SLNONE`]. A directory also records its device and
/// inode, and is [`FTS_DC`] when an ancestor has the same pair — the cycle
/// check that keeps a logical walk out of a symlink loop.
///
/// # Safety
///
/// `p` must be a live entry whose `fts_accpath` names it and whose
/// `fts_parent` chain is live up to the root parent.
unsafe fn stat_ent(sp: &Stream, p: *mut FtsEnt, follow: bool) -> u16 {
    let mut local = zeroed_stat();
    // SAFETY: `fts_statp`, when non-null, is this entry's own buffer.
    let sbp: &mut Stat = unsafe {
        if (*p).fts_statp.is_null() {
            &mut local
        } else {
            &mut *(*p).fts_statp
        }
    };
    // SAFETY: the caller's contract.
    let path = unsafe { (*p).fts_accpath };
    if sp.isset(FTS_LOGICAL) || follow {
        if let Err(e) = (sp.ops.stat)(path, true, sbp) {
            if (sp.ops.stat)(path, false, sbp).is_ok() {
                errno::set_errno(0);
                return FTS_SLNONE;
            }
            *sbp = zeroed_stat();
            // SAFETY: live entry.
            unsafe { (*p).fts_errno = e };
            return FTS_NS;
        }
    } else if let Err(e) = (sp.ops.stat)(path, false, sbp) {
        *sbp = zeroed_stat();
        // SAFETY: live entry.
        unsafe { (*p).fts_errno = e };
        return FTS_NS;
    }
    // SAFETY: live entry.
    unsafe {
        (*p).fts_dev = sbp.st_dev;
        (*p).fts_ino = sbp.st_ino;
        (*p).fts_nlink = sbp.st_nlink;
    }
    match sbp.st_mode & S_IFMT {
        S_IFDIR => {
            // SAFETY: live entry and live ancestors (the caller's contract).
            unsafe {
                if is_dot(name_of(p)) {
                    return FTS_DOT;
                }
                let mut t = (*p).fts_parent;
                while !t.is_null() && (*t).fts_level >= FTS_ROOTLEVEL {
                    if (*t).fts_ino == sbp.st_ino && (*t).fts_dev == sbp.st_dev {
                        (*p).fts_cycle = t;
                        return FTS_DC;
                    }
                    t = (*t).fts_parent;
                }
            }
            FTS_D
        }
        S_IFLNK => FTS_SL,
        S_IFREG => FTS_F,
        _ => FTS_DEFAULT,
    }
}

// ---------------------------------------------------------------------------
// Reading a directory
// ---------------------------------------------------------------------------

/// Why a directory is being read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Build {
    /// By `fts_read`, to walk it: a failure is reported on the directory.
    Read,
    /// By `fts_children`, to show the caller.
    Child,
    /// By `fts_children(FTS_NAMEONLY)`: names only, nothing `stat`ed.
    Names,
}

/// Read the current directory's entries into a list (glibc's `fts_build`).
///
/// Null when there are none or on failure. For [`Build::Read`], an
/// unreadable directory becomes [`FTS_DNR`] and an empty one [`FTS_DP`] — the
/// post-order visit, since there is nothing to walk. Running out of memory
/// or path length is [`FTS_ERR`] and stops the walk ([`FTS_STOP`]), as in
/// glibc.
///
/// The directory is read in one pass and released before this returns, so a
/// walk never holds a stream open between calls.
fn build(sp: &mut Stream, kind: Build) -> *mut FtsEnt {
    let cur = sp.fts.fts_cur;
    // SAFETY: `fts_cur` is the directory being visited, a live entry whose
    // path is the one in the buffer.
    let (level, base, cur_pathlen) = unsafe {
        (
            (*cur).fts_level.saturating_add(1),
            napppend(cur),
            usize::from((*cur).fts_pathlen),
        )
    };
    // The directory's own path, copied: the buffer may move while it is read.
    let dir_path = crate::malloc::malloc(cur_pathlen + 1);
    if dir_path.is_null() {
        return build_failed(sp, cur, errno::ENOMEM);
    }
    // SAFETY: `fts_accpath` names the directory and is NUL-terminated at
    // `fts_pathlen` (it is the buffer, or a root's own name, which is as
    // long); `dir_path` has room for that and the NUL.
    unsafe {
        core::ptr::copy_nonoverlapping((*cur).fts_accpath, dir_path, cur_pathlen);
        dir_path.add(cur_pathlen).write(0);
    }
    let nostat = sp.isset(FTS_NOSTAT);
    let seedot = sp.isset(FTS_SEEDOT);
    let name_at = base + 1; // after the '/' this directory's children follow
    let mut head: *mut FtsEnt = core::ptr::null_mut();
    let mut tail: *mut FtsEnt = core::ptr::null_mut();
    let mut nitems: usize = 0;
    let mut moved = false;
    let mut fatal: Option<i32> = None;

    let ops = sp.ops;
    let listed = (ops.list)(dir_path, &mut |name, d_type| {
        if !seedot && is_dot(name) {
            return true;
        }
        let p = alloc_ent(sp, name);
        if p.is_null() {
            fatal = Some(errno::get_errno());
            return false;
        }
        let pathlen = name_at + name.len();
        if pathlen >= usize::from(u16::MAX) {
            // SAFETY: `p` is ours and on no list yet.
            unsafe { free_ent(p) };
            fatal = Some(errno::ENAMETOOLONG);
            return false;
        }
        let old = sp.fts.fts_path;
        if let Err(e) = grow_path(sp, pathlen + 1) {
            // SAFETY: as above.
            unsafe { free_ent(p) };
            fatal = Some(e);
            return false;
        }
        if old != sp.fts.fts_path {
            moved = true;
        }
        let buf = sp.fts.fts_path;
        // SAFETY: the buffer holds `pathlen + 1` bytes (just ensured); `p` is
        // a fresh entry.
        unsafe {
            *buf.add(base) = b'/';
            core::ptr::copy_nonoverlapping(name.as_ptr(), buf.add(name_at), name.len());
            *buf.add(pathlen) = 0;
            (*p).fts_level = level;
            (*p).fts_parent = cur;
            (*p).fts_pathlen = pathlen as u16;
            (*p).fts_path = buf;
            (*p).fts_accpath = buf;
            (*p).fts_info = if kind == Build::Names
                || (nostat
                    && d_type != crate::dirent::DT_DIR
                    && d_type != crate::dirent::DT_UNKNOWN)
            {
                FTS_NSOK
            } else {
                stat_ent(sp, p, false)
            };
            if tail.is_null() {
                head = p;
            } else {
                (*tail).fts_link = p;
            }
        }
        tail = p;
        nitems += 1;
        true
    });
    // SAFETY: `dir_path` is ours.
    unsafe { crate::malloc::free(dir_path) };

    // Entries built before the buffer moved point at the old one; so do the
    // directory and everything above it.
    if moved {
        // SAFETY: the list and the chain above it are live.
        unsafe { adjust_paths(sp, head) };
    }
    // Restore the directory's own path in the buffer.
    // SAFETY: the buffer holds at least `cur_pathlen + 1` bytes: it held the
    // directory's path before and has only grown.
    unsafe { *sp.fts.fts_path.add(cur_pathlen) = 0 };

    if let Some(e) = fatal {
        // SAFETY: the partial list is ours alone.
        unsafe { free_list(head) };
        return build_failed(sp, cur, e);
    }
    if let Err(e) = listed {
        // SAFETY: as above.
        unsafe { free_list(head) };
        if kind == Build::Read {
            // SAFETY: `cur` is live.
            unsafe {
                (*cur).fts_info = FTS_DNR;
                (*cur).fts_errno = e;
            }
        }
        errno::set_errno(e);
        return core::ptr::null_mut();
    }
    if nitems == 0 {
        if kind == Build::Read {
            // SAFETY: `cur` is live.
            unsafe { (*cur).fts_info = FTS_DP };
        }
        return core::ptr::null_mut();
    }
    if nitems > 1 {
        if let Some(compar) = sp.fts.fts_compar {
            return sort(sp, head, nitems, compar);
        }
    }
    head
}

/// A read that cannot go on: the directory becomes [`FTS_ERR`], the walk
/// stops, and `errno` says why.
fn build_failed(sp: &mut Stream, cur: *mut FtsEnt, e: i32) -> *mut FtsEnt {
    // SAFETY: `cur` is the live current entry.
    unsafe {
        (*cur).fts_info = FTS_ERR;
        (*cur).fts_errno = e;
    }
    sp.set(FTS_STOP);
    errno::set_errno(e);
    core::ptr::null_mut()
}

/// Sort a list of `nitems` entries with `compar` (glibc's `fts_sort`). If
/// the scratch array cannot be had, the list is returned unsorted, as glibc
/// does.
fn sort(sp: &mut Stream, head: *mut FtsEnt, nitems: usize, compar: FtsCompar) -> *mut FtsEnt {
    let have = usize::try_from(sp.fts.fts_nitems).unwrap_or(0);
    if nitems > have {
        let want = nitems.saturating_add(40);
        let grown = crate::malloc::reallocarray(
            sp.fts.fts_array.cast::<u8>(),
            want,
            core::mem::size_of::<*mut FtsEnt>(),
        );
        if grown.is_null() {
            return head;
        }
        sp.fts.fts_array = grown.cast();
        sp.fts.fts_nitems = i32::try_from(want).unwrap_or(i32::MAX);
    }
    let array = sp.fts.fts_array;
    // SAFETY: `array` holds at least `nitems` pointers, and the list has
    // exactly `nitems` live entries.
    unsafe {
        let mut p = head;
        let mut i = 0usize;
        while !p.is_null() {
            array.add(i).write(p);
            i += 1;
            p = (*p).fts_link;
        }
        // The caller's comparison takes `const FTSENT **`, and `qsort` hands
        // its comparison pointers to elements — `FTSENT **` here. The two
        // function types differ only in pointee types, which is the same ABI.
        let qcompar: unsafe extern "C" fn(*const u8, *const u8) -> i32 = core::mem::transmute::<
            FtsCompar,
            unsafe extern "C" fn(*const u8, *const u8) -> i32,
        >(compar);
        crate::stdlib::qsort(
            array.cast::<u8>(),
            nitems,
            core::mem::size_of::<*mut FtsEnt>(),
            qcompar,
        );
        for j in 0..nitems - 1 {
            (**array.add(j)).fts_link = *array.add(j + 1);
        }
        (**array.add(nitems - 1)).fts_link = core::ptr::null_mut();
        *array
    }
}

// ---------------------------------------------------------------------------
// fts_open
// ---------------------------------------------------------------------------

/// Open a traversal of the NULL-terminated list of paths `argv`.
///
/// `options` must include exactly one of [`FTS_LOGICAL`] and
/// [`FTS_PHYSICAL`], and nothing outside [`FTS_OPTIONMASK`]; otherwise
/// `EINVAL`. An empty path is `ENOENT`, as in glibc. An empty list is a
/// traversal that ends at once. `compar`, if given, orders the roots and the
/// entries of every directory.
///
/// Returns the stream, or NULL with `errno` set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_open(
    argv: *const *const u8,
    options: i32,
    compar: Option<FtsCompar>,
) -> *mut Fts {
    open_with(argv, options, compar, &REAL_OPS)
}

/// [`fts_open`] against a chosen filesystem — the tests' seam.
pub(crate) fn open_with(
    argv: *const *const u8,
    options: i32,
    compar: Option<FtsCompar>,
    ops: &'static FsOps,
) -> *mut Fts {
    if argv.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    if options & !FTS_OPTIONMASK != 0 {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }
    if (options & FTS_LOGICAL != 0) == (options & FTS_PHYSICAL != 0) {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }
    let block = crate::malloc::malloc(core::mem::size_of::<Stream>());
    if block.is_null() {
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    }
    let st = block.cast::<Stream>();
    // SAFETY: a fresh allocation of `size_of::<Stream>()`, 16-byte aligned.
    unsafe {
        st.write(Stream {
            fts: Fts {
                fts_cur: core::ptr::null_mut(),
                fts_child: core::ptr::null_mut(),
                fts_array: core::ptr::null_mut(),
                fts_dev: 0,
                fts_path: core::ptr::null_mut(),
                fts_rfd: -1,
                fts_pathlen: 0,
                fts_nitems: 0,
                fts_compar: compar,
                fts_options: options | FTS_NOCHDIR,
            },
            ops,
        });
    }
    // SAFETY: just initialised; ours alone until returned.
    let sp = unsafe { &mut *st };
    match populate(sp, argv) {
        // The allocation's own pointer, not one derived from `sp`: the caller
        // keeps it for the life of the stream, long after this borrow ends.
        // `Fts` is `Stream`'s first field, so it is the same address.
        Ok(()) => st.cast::<Fts>(),
        Err(e) => {
            // SAFETY: `populate` leaves only `fts_cur`'s chain (possibly
            // empty) and the path buffer to release, which `close_stream`
            // handles.
            unsafe { close_stream(st) };
            errno::set_errno(e);
            core::ptr::null_mut()
        }
    }
}

/// Build the root list under an invisible root parent, and the `FTS_INIT`
/// entry `fts_read` starts from (glibc's `fts_open` body).
fn populate(sp: &mut Stream, argv: *const *const u8) -> Result<(), i32> {
    // The buffer must hold the longest root; `grow_path` adds 256.
    let mut longest = 0usize;
    let mut n = 0usize;
    loop {
        // SAFETY: `argv` is a NULL-terminated array (the caller's contract).
        let a = unsafe { *argv.add(n) };
        if a.is_null() {
            break;
        }
        // SAFETY: each element is a NUL-terminated string.
        longest = longest.max(unsafe { crate::string::strlen(a) });
        n += 1;
    }
    grow_path(sp, longest.max(crate::unistd::PATH_MAX) + 1)?;

    let parent = alloc_ent(sp, b"");
    if parent.is_null() {
        return Err(errno::get_errno());
    }
    // SAFETY: fresh entry.
    unsafe { (*parent).fts_level = FTS_ROOTPARENTLEVEL };

    let mut root: *mut FtsEnt = core::ptr::null_mut();
    let mut tail: *mut FtsEnt = core::ptr::null_mut();
    let mut outcome = Ok(());
    for i in 0..n {
        // SAFETY: `i < n`, counted above.
        let a = unsafe { *argv.add(i) };
        // SAFETY: NUL-terminated.
        let bytes = unsafe { core::slice::from_raw_parts(a, crate::string::strlen(a)) };
        if bytes.is_empty() {
            outcome = Err(errno::ENOENT);
            break;
        }
        let p = alloc_ent(sp, bytes);
        if p.is_null() {
            outcome = Err(errno::get_errno());
            break;
        }
        // SAFETY: fresh entry; its name holds the whole path until
        // `load_root` trims it, so it is the access path meanwhile.
        unsafe {
            (*p).fts_level = FTS_ROOTLEVEL;
            (*p).fts_parent = parent;
            (*p).fts_accpath = name_ptr(p);
            (*p).fts_info = stat_ent(sp, p, sp.isset(FTS_COMFOLLOW));
            // A root named "." or ".." is a real directory to walk.
            if (*p).fts_info == FTS_DOT {
                (*p).fts_info = FTS_D;
            }
            if sp.fts.fts_compar.is_some() {
                (*p).fts_link = root;
                root = p;
            } else {
                if tail.is_null() {
                    root = p;
                } else {
                    (*tail).fts_link = p;
                }
                tail = p;
            }
        }
    }
    if let Err(e) = outcome {
        // SAFETY: the partial root list and the parent are ours alone.
        unsafe {
            free_list(root);
            free_ent(parent);
        }
        return Err(e);
    }
    if n > 1 {
        if let Some(compar) = sp.fts.fts_compar {
            root = sort(sp, root, n, compar);
        }
    }
    let cur = alloc_ent(sp, b"");
    if cur.is_null() {
        let e = errno::get_errno();
        // SAFETY: as above.
        unsafe {
            free_list(root);
            free_ent(parent);
        }
        return Err(e);
    }
    // SAFETY: fresh entry. With no roots, its link is null and the first
    // `fts_read` climbs straight to the root parent and ends; `parent` must
    // still be reachable for that, so it is hung off the dummy.
    unsafe {
        (*cur).fts_link = root;
        (*cur).fts_parent = parent;
        (*cur).fts_info = FTS_INIT;
    }
    sp.fts.fts_cur = cur;
    Ok(())
}

/// Make root `p` the entry being walked (glibc's `fts_load`): its full path
/// goes into the buffer, its name becomes the last component, and the
/// stream's device becomes its device ([`FTS_XDEV`]).
///
/// # Safety
///
/// `p` must be a live root.
unsafe fn load_root(sp: &mut Stream, p: *mut FtsEnt) {
    // SAFETY: the caller's contract; the buffer was sized in `populate` to
    // hold the longest root and its NUL.
    unsafe {
        let len = usize::from((*p).fts_namelen);
        (*p).fts_pathlen = (*p).fts_namelen;
        core::ptr::copy_nonoverlapping(name_ptr(p), sp.fts.fts_path, len + 1);
        let name = name_of(p);
        if let Some(slash) = name.iter().rposition(|&b| b == b'/') {
            // "/" stays "/"; "/usr" becomes "usr"; "a/b/" becomes "".
            if slash != 0 || len > 1 {
                let rest = len - slash - 1;
                core::ptr::copy(name_ptr(p).add(slash + 1), name_ptr(p), rest + 1);
                (*p).fts_namelen = rest as u16;
            }
        }
        (*p).fts_accpath = sp.fts.fts_path;
        (*p).fts_path = sp.fts.fts_path;
        sp.fts.fts_dev = (*p).fts_dev;
    }
}

// ---------------------------------------------------------------------------
// fts_read
// ---------------------------------------------------------------------------

/// Return the next entry, or NULL at the end (with `errno` 0) or on an error
/// that stops the walk (with `errno` set).
///
/// The entry and its `fts_path` are valid until the next call; a directory's
/// entry lives until its post-order visit, so `fts_parent` is always valid.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_read(ftsp: *mut Fts) -> *mut FtsEnt {
    if ftsp.is_null() {
        errno::set_errno(errno::EBADF);
        return core::ptr::null_mut();
    }
    // SAFETY: a stream from `fts_open` (the caller's contract).
    let sp = unsafe { stream(ftsp) };
    if sp.fts.fts_cur.is_null() || sp.isset(FTS_STOP) {
        return core::ptr::null_mut();
    }
    // SAFETY: every entry reached below is live — the walk frees a node only
    // after moving past it — and the path buffer holds the current entry's
    // path.
    unsafe { read_next(sp) }
}

/// The body of [`fts_read`] (glibc's `fts_read`, less the `chdir`s).
///
/// # Safety
///
/// `sp.fts.fts_cur` must be a live entry.
unsafe fn read_next(sp: &mut Stream) -> *mut FtsEnt {
    // SAFETY: throughout — see `fts_read`.
    unsafe {
        let mut p = sp.fts.fts_cur;
        let instr = i32::from((*p).fts_instr);
        (*p).fts_instr = FTS_NOINSTR as u16;

        // Re-visit: re-stat and return the same entry.
        if instr == FTS_AGAIN {
            (*p).fts_info = stat_ent(sp, p, false);
            return p;
        }
        // Follow a symlink the caller asked about: re-stat through it and
        // return it again, now typed by its target.
        if instr == FTS_FOLLOW && ((*p).fts_info == FTS_SL || (*p).fts_info == FTS_SLNONE) {
            (*p).fts_info = stat_ent(sp, p, true);
            if (*p).fts_info == FTS_D {
                (*p).fts_flags |= FTS_SYMFOLLOW;
            }
            return p;
        }

        // A directory in pre-order: descend, unless told to skip it or it is
        // on another device under FTS_XDEV — then its post-order visit is next.
        if (*p).fts_info == FTS_D {
            if instr == FTS_SKIP || (sp.isset(FTS_XDEV) && (*p).fts_dev != sp.fts.fts_dev) {
                free_list(sp.fts.fts_child);
                sp.fts.fts_child = core::ptr::null_mut();
                (*p).fts_info = FTS_DP;
                return p;
            }
            // Children read by `fts_children(FTS_NAMEONLY)` carry no types;
            // walking needs them, so read the directory again.
            if !sp.fts.fts_child.is_null() && sp.isset(FTS_NAMEONLY) {
                sp.clear(FTS_NAMEONLY);
                free_list(sp.fts.fts_child);
                sp.fts.fts_child = core::ptr::null_mut();
            }
            if sp.fts.fts_child.is_null() {
                let head = build(sp, Build::Read);
                if head.is_null() {
                    if sp.isset(FTS_STOP) {
                        return core::ptr::null_mut();
                    }
                    // Unreadable (FTS_DNR) or empty (FTS_DP): `build`
                    // retyped the directory; return it again.
                    return p;
                }
                sp.fts.fts_child = head;
            }
            p = sp.fts.fts_child;
            sp.fts.fts_child = core::ptr::null_mut();
            return name_and_return(sp, p);
        }

        // Move to the next entry at this level, or up.
        loop {
            let done = p;
            let next = (*done).fts_link;
            if next.is_null() {
                break;
            }
            free_ent(done);
            p = next;
            if (*p).fts_level == FTS_ROOTLEVEL {
                load_root(sp, p);
                sp.fts.fts_cur = p;
                return p;
            }
            if i32::from((*p).fts_instr) == FTS_SKIP {
                continue;
            }
            if i32::from((*p).fts_instr) == FTS_FOLLOW {
                (*p).fts_info = stat_ent(sp, p, true);
                if (*p).fts_info == FTS_D {
                    (*p).fts_flags |= FTS_SYMFOLLOW;
                }
                (*p).fts_instr = FTS_NOINSTR as u16;
            }
            return name_and_return(sp, p);
        }

        // No more at this level: the parent's post-order visit.
        let done = p;
        p = (*done).fts_parent;
        free_ent(done);
        if (*p).fts_level == FTS_ROOTPARENTLEVEL {
            free_ent(p);
            sp.fts.fts_cur = core::ptr::null_mut();
            errno::set_errno(0);
            return core::ptr::null_mut();
        }
        *sp.fts.fts_path.add(usize::from((*p).fts_pathlen)) = 0;
        (*p).fts_info = if (*p).fts_errno != 0 { FTS_ERR } else { FTS_DP };
        sp.fts.fts_cur = p;
        p
    }
}

/// Put child `p`'s name into the path after its parent's, and return it.
///
/// # Safety
///
/// `p` must be a live entry below the roots whose parent's path is the one
/// in the buffer; `build` made the buffer long enough.
unsafe fn name_and_return(sp: &mut Stream, p: *mut FtsEnt) -> *mut FtsEnt {
    // SAFETY: the caller's contract.
    unsafe {
        let at = napppend((*p).fts_parent);
        let buf = sp.fts.fts_path;
        *buf.add(at) = b'/';
        let len = usize::from((*p).fts_namelen);
        core::ptr::copy_nonoverlapping(name_ptr(p), buf.add(at + 1), len + 1);
        sp.fts.fts_cur = p;
    }
    p
}

// ---------------------------------------------------------------------------
// fts_children, fts_set, fts_close
// ---------------------------------------------------------------------------

/// The entries of the directory `fts_read` returned last, as a list linked
/// through `fts_link` — or, before the first `fts_read`, the roots.
///
/// NULL with `errno` 0 when there are none: the current entry is not a
/// directory in pre-order, or the directory is empty. NULL with `errno` set
/// on an error. `instr` is 0 or [`FTS_NAMEONLY`], which returns names only
/// (every entry [`FTS_NSOK`]); anything else is `EINVAL`.
///
/// The list is valid until the next `fts_read` or `fts_children`; the
/// following `fts_read` walks it rather than reading the directory again.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_children(ftsp: *mut Fts, instr: i32) -> *mut FtsEnt {
    if ftsp.is_null() {
        errno::set_errno(errno::EBADF);
        return core::ptr::null_mut();
    }
    if instr != 0 && instr != FTS_NAMEONLY {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }
    // SAFETY: a stream from `fts_open`.
    let sp = unsafe { stream(ftsp) };
    let p = sp.fts.fts_cur;
    errno::set_errno(0);
    if p.is_null() || sp.isset(FTS_STOP) {
        return core::ptr::null_mut();
    }
    // SAFETY: `p` is the live current entry.
    unsafe {
        if (*p).fts_info == FTS_INIT {
            return (*p).fts_link;
        }
        if (*p).fts_info != FTS_D {
            return core::ptr::null_mut();
        }
        free_list(sp.fts.fts_child);
    }
    sp.fts.fts_child = core::ptr::null_mut();
    let kind = if instr == FTS_NAMEONLY {
        sp.set(FTS_NAMEONLY);
        Build::Names
    } else {
        Build::Child
    };
    let head = build(sp, kind);
    sp.fts.fts_child = head;
    head
}

/// Leave an instruction on entry `p` for the next `fts_read`: [`FTS_AGAIN`],
/// [`FTS_FOLLOW`], [`FTS_SKIP`] or [`FTS_NOINSTR`] (0 is accepted as no
/// instruction, as in glibc).
///
/// Returns 0, or — glibc's value, not the `-1` its manual page states — 1
/// with `errno` `EINVAL` for any other instruction; `EBADF`/`EFAULT` for a
/// NULL stream or entry.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_set(ftsp: *mut Fts, p: *mut FtsEnt, instr: i32) -> i32 {
    if ftsp.is_null() {
        errno::set_errno(errno::EBADF);
        return 1;
    }
    if p.is_null() {
        errno::set_errno(errno::EFAULT);
        return 1;
    }
    if !matches!(instr, 0 | FTS_AGAIN | FTS_FOLLOW | FTS_NOINSTR | FTS_SKIP) {
        errno::set_errno(errno::EINVAL);
        return 1;
    }
    // SAFETY: a live entry of this stream (the caller's contract). 0 is
    // stored as given, like glibc; `fts_read` treats it as no instruction.
    unsafe { (*p).fts_instr = instr as u16 };
    0
}

/// Close a traversal, freeing every entry it still holds. Returns 0, or -1
/// with `EBADF` for a NULL stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_close(ftsp: *mut Fts) -> i32 {
    if ftsp.is_null() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // SAFETY: a stream from `fts_open`, not used again.
    unsafe { close_stream(ftsp.cast::<Stream>()) };
    0
}

/// Free a stream and everything it holds (glibc's `fts_close`).
///
/// From the current entry, every live entry is reached through `fts_link`
/// (later siblings) and then `fts_parent`, up to the root parent; earlier
/// siblings were freed as the walk passed them.
///
/// # Safety
///
/// `st` must be a stream from `open_with`, not used again.
unsafe fn close_stream(st: *mut Stream) {
    // SAFETY: the caller's contract; see above for why the walk reaches each
    // live entry exactly once.
    unsafe {
        let sp = &mut *st;
        let mut p = sp.fts.fts_cur;
        if !p.is_null() {
            while (*p).fts_level >= FTS_ROOTLEVEL {
                let done = p;
                p = if (*done).fts_link.is_null() {
                    (*done).fts_parent
                } else {
                    (*done).fts_link
                };
                free_ent(done);
            }
            free_ent(p);
        }
        free_list(sp.fts.fts_child);
        crate::malloc::free(sp.fts.fts_array.cast::<u8>());
        crate::malloc::free(sp.fts.fts_path);
        crate::malloc::free(st.cast::<u8>());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::collections::HashMap;
    use std::string::String;
    use std::vec::Vec;

    // -----------------------------------------------------------------------
    // An in-memory filesystem behind the `FsOps` seam
    // -----------------------------------------------------------------------

    /// One node of the in-memory tree.
    #[derive(Clone)]
    enum Node {
        Dir {
            children: Vec<Vec<u8>>,
            ino: u64,
            dev: u64,
            readable: bool,
        },
        File {
            ino: u64,
        },
        Link {
            target: Vec<u8>,
            ino: u64,
        },
    }

    /// What `mk` creates.
    enum Kind<'a> {
        Dir,
        /// A directory on another device.
        DirOnDev(u64),
        /// A directory `opendir` refuses (`EACCES`).
        Unreadable,
        File,
        /// A symlink to the given absolute tree path.
        Link(&'a str),
    }

    std::thread_local! {
        static FS: std::cell::RefCell<HashMap<Vec<u8>, Node>> =
            std::cell::RefCell::new(HashMap::new());
        static NEXT_INO: core::cell::Cell<u64> = const { core::cell::Cell::new(100) };
        /// Report `DT_UNKNOWN` for every entry, as some filesystems do.
        static DTYPE_UNKNOWN: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
        /// Every path `stat` was asked about, for the NOSTAT tests.
        static STATTED: std::cell::RefCell<Vec<Vec<u8>>> = std::cell::RefCell::new(Vec::new());
    }

    /// Canonical key: components joined by '/', no leading or trailing slash;
    /// the root is the empty key.
    fn key(path: &[u8]) -> Vec<u8> {
        let parts: Vec<&[u8]> = path
            .split(|&b| b == b'/')
            .filter(|c| !c.is_empty())
            .collect();
        parts.join(&b'/')
    }

    fn parent_key(k: &[u8]) -> Vec<u8> {
        match k.iter().rposition(|&b| b == b'/') {
            Some(i) => k[..i].to_vec(),
            None => Vec::new(),
        }
    }

    fn base(k: &[u8]) -> Vec<u8> {
        match k.iter().rposition(|&b| b == b'/') {
            Some(i) => k[i + 1..].to_vec(),
            None => k.to_vec(),
        }
    }

    /// A fresh tree holding only the root directory.
    fn reset() {
        FS.with(|fs| {
            let mut fs = fs.borrow_mut();
            fs.clear();
            fs.insert(
                Vec::new(),
                Node::Dir {
                    children: Vec::new(),
                    ino: 2,
                    dev: 1,
                    readable: true,
                },
            );
        });
        DTYPE_UNKNOWN.with(|c| c.set(false));
        STATTED.with(|s| s.borrow_mut().clear());
    }

    /// Create `path` (its parent must exist) as `kind`.
    fn mk(path: &str, kind: Kind<'_>) {
        let k = key(path.as_bytes());
        let parent = parent_key(&k);
        let ino = NEXT_INO.with(|n| {
            let v = n.get();
            n.set(v + 1);
            v
        });
        FS.with(|fs| {
            let mut fs = fs.borrow_mut();
            let parent_dev = match fs.get(&parent) {
                Some(Node::Dir { dev, .. }) => *dev,
                _ => panic!("mk {path}: parent missing"),
            };
            let node = match kind {
                Kind::Dir => Node::Dir {
                    children: Vec::new(),
                    ino,
                    dev: parent_dev,
                    readable: true,
                },
                Kind::DirOnDev(dev) => Node::Dir {
                    children: Vec::new(),
                    ino,
                    dev,
                    readable: true,
                },
                Kind::Unreadable => Node::Dir {
                    children: Vec::new(),
                    ino,
                    dev: parent_dev,
                    readable: false,
                },
                Kind::File => Node::File { ino },
                Kind::Link(t) => Node::Link {
                    target: key(t.as_bytes()),
                    ino,
                },
            };
            fs.insert(k.clone(), node);
            if let Some(Node::Dir { children, .. }) = fs.get_mut(&parent) {
                children.push(base(&k));
            }
        });
    }

    /// Resolve `path` to a key, following symlinks — all of them, or all but
    /// a final one when `!follow_last`. The errno on failure.
    fn resolve(path: &[u8], follow_last: bool, depth: u32) -> Result<Vec<u8>, i32> {
        if depth > 40 {
            return Err(errno::ELOOP);
        }
        let comps: Vec<Vec<u8>> = path
            .split(|&b| b == b'/')
            .filter(|c| !c.is_empty())
            .map(<[u8]>::to_vec)
            .collect();
        let mut cur: Vec<u8> = Vec::new();
        for (i, c) in comps.iter().enumerate() {
            let last = i + 1 == comps.len();
            if c.as_slice() == b"." {
                continue;
            }
            if c.as_slice() == b".." {
                cur = parent_key(&cur);
                continue;
            }
            let next = if cur.is_empty() {
                c.clone()
            } else {
                let mut n = cur.clone();
                n.push(b'/');
                n.extend_from_slice(c);
                n
            };
            let node = FS.with(|fs| fs.borrow().get(&next).cloned());
            match node {
                None => return Err(errno::ENOENT),
                Some(Node::Link { target, .. }) if !last || follow_last => {
                    cur = resolve(&target, true, depth + 1)?;
                }
                Some(Node::File { .. } | Node::Link { .. }) if !last => {
                    return Err(errno::ENOTDIR);
                }
                Some(_) => cur = next,
            }
        }
        Ok(cur)
    }

    fn c_path(path: *const u8) -> Vec<u8> {
        // SAFETY: the walk passes NUL-terminated paths.
        let len = unsafe { crate::string::strlen(path) };
        // SAFETY: as above.
        unsafe { core::slice::from_raw_parts(path, len) }.to_vec()
    }

    fn mem_stat(path: *const u8, follow: bool, out: &mut Stat) -> Result<(), i32> {
        let p = c_path(path);
        STATTED.with(|s| s.borrow_mut().push(p.clone()));
        let k = resolve(&p, follow, 0)?;
        let node = FS
            .with(|fs| fs.borrow().get(&k).cloned())
            .ok_or(errno::ENOENT)?;
        *out = zeroed_stat();
        match node {
            Node::Dir {
                children, ino, dev, ..
            } => {
                let subdirs = FS.with(|fs| {
                    let fs = fs.borrow();
                    children
                        .iter()
                        .filter(|c| {
                            let mut ck = k.clone();
                            if !ck.is_empty() {
                                ck.push(b'/');
                            }
                            ck.extend_from_slice(c);
                            matches!(fs.get(&ck), Some(Node::Dir { .. }))
                        })
                        .count()
                });
                out.st_mode = S_IFDIR | 0o755;
                out.st_ino = ino;
                out.st_dev = dev;
                out.st_nlink = 2 + subdirs as u64;
            }
            Node::File { ino } => {
                out.st_mode = S_IFREG | 0o644;
                out.st_ino = ino;
                out.st_dev = 1;
                out.st_nlink = 1;
            }
            Node::Link { ino, .. } => {
                out.st_mode = S_IFLNK | 0o777;
                out.st_ino = ino;
                out.st_dev = 1;
                out.st_nlink = 1;
            }
        }
        Ok(())
    }

    fn mem_list(path: *const u8, each: &mut EntrySink<'_>) -> Result<(), i32> {
        let k = resolve(&c_path(path), true, 0)?;
        let node = FS
            .with(|fs| fs.borrow().get(&k).cloned())
            .ok_or(errno::ENOENT)?;
        let Node::Dir {
            children, readable, ..
        } = node
        else {
            return Err(errno::ENOTDIR);
        };
        if !readable {
            return Err(errno::EACCES);
        }
        let unknown = DTYPE_UNKNOWN.with(core::cell::Cell::get);
        let dt = |name: &[u8]| -> u8 {
            if unknown {
                return crate::dirent::DT_UNKNOWN;
            }
            let mut ck = k.clone();
            if !ck.is_empty() {
                ck.push(b'/');
            }
            ck.extend_from_slice(name);
            match FS.with(|fs| fs.borrow().get(&ck).cloned()) {
                Some(Node::Dir { .. }) => crate::dirent::DT_DIR,
                Some(Node::File { .. }) => crate::dirent::DT_REG,
                Some(Node::Link { .. }) => crate::dirent::DT_LNK,
                None => crate::dirent::DT_UNKNOWN,
            }
        };
        if !each(b".", crate::dirent::DT_DIR) || !each(b"..", crate::dirent::DT_DIR) {
            return Ok(());
        }
        for c in &children {
            if !each(c, dt(c)) {
                break;
            }
        }
        Ok(())
    }

    static MEM_OPS: FsOps = FsOps {
        stat: mem_stat,
        list: mem_list,
    };

    // -----------------------------------------------------------------------
    // Driving a walk
    // -----------------------------------------------------------------------

    /// One `fts_read` result, copied out.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Seen {
        info: u16,
        level: i16,
        path: String,
        name: String,
    }

    fn seen(info: u16, level: i16, path: &str, name: &str) -> Seen {
        Seen {
            info,
            level,
            path: path.into(),
            name: name.into(),
        }
    }

    fn cstrings(roots: &[&str]) -> (Vec<Vec<u8>>, Vec<*const u8>) {
        let owned: Vec<Vec<u8>> = roots
            .iter()
            .map(|r| {
                let mut v = r.as_bytes().to_vec();
                v.push(0);
                v
            })
            .collect();
        let mut ptrs: Vec<*const u8> = owned.iter().map(|v| v.as_ptr()).collect();
        ptrs.push(core::ptr::null());
        (owned, ptrs)
    }

    fn open(roots: &[&str], options: i32, compar: Option<FtsCompar>) -> *mut Fts {
        let (_owned, ptrs) = cstrings(roots);
        let sp = open_with(ptrs.as_ptr(), options, compar, &MEM_OPS);
        assert!(!sp.is_null(), "open failed: errno {}", errno::get_errno());
        sp
    }

    /// Copy out an entry, checking the invariants every entry must meet.
    fn copy_out(p: *mut FtsEnt) -> Seen {
        // SAFETY: a live entry just returned by `fts_read`.
        unsafe {
            let path = c_path((*p).fts_path);
            assert_eq!(path.len(), usize::from((*p).fts_pathlen), "fts_pathlen");
            let name = name_of(p).to_vec();
            assert_eq!(c_path(name_ptr(p)), name, "fts_name NUL-terminated");
            assert_eq!((*p).fts_accpath, (*p).fts_path, "accpath == path (NOCHDIR)");
            Seen {
                info: (*p).fts_info,
                level: (*p).fts_level,
                path: String::from_utf8(path).unwrap(),
                name: String::from_utf8(name).unwrap(),
            }
        }
    }

    /// Walk to the end, returning every entry; checks `errno` is 0 at the
    /// end, closes the stream, and checks nothing leaked.
    fn walk(roots: &[&str], options: i32, compar: Option<FtsCompar>) -> Vec<Seen> {
        let before = crate::malloc::live_allocations::count();
        let sp = open(roots, options, compar);
        let mut out = Vec::new();
        loop {
            errno::set_errno(12345);
            let p = fts_read(sp);
            if p.is_null() {
                assert_eq!(errno::get_errno(), 0, "end of walk leaves errno 0");
                break;
            }
            out.push(copy_out(p));
        }
        assert_eq!(fts_close(sp), 0);
        assert_eq!(crate::malloc::live_allocations::count(), before, "leaked");
        out
    }

    fn paths(v: &[Seen]) -> Vec<(u16, String)> {
        v.iter().map(|s| (s.info, s.path.clone())).collect()
    }

    fn p(info: u16, path: &str) -> (u16, String) {
        (info, path.into())
    }

    /// Sort by name, for `compar`.
    unsafe extern "C" fn by_name(a: *const *const FtsEnt, b: *const *const FtsEnt) -> i32 {
        // SAFETY: `fts` hands live entries.
        unsafe {
            let na = name_of((*a).cast_mut());
            let nb = name_of((*b).cast_mut());
            na.cmp(nb) as i32
        }
    }

    /// Sort by name, backwards.
    unsafe extern "C" fn by_name_rev(a: *const *const FtsEnt, b: *const *const FtsEnt) -> i32 {
        // SAFETY: as above.
        unsafe { by_name(b, a) }
    }

    // -----------------------------------------------------------------------
    // The ABI is glibc's
    // -----------------------------------------------------------------------

    #[test]
    fn ftsent_has_glibcs_layout() {
        use core::mem::offset_of;
        assert_eq!(offset_of!(FtsEnt, fts_cycle), 0);
        assert_eq!(offset_of!(FtsEnt, fts_parent), 8);
        assert_eq!(offset_of!(FtsEnt, fts_link), 16);
        assert_eq!(offset_of!(FtsEnt, fts_number), 24);
        assert_eq!(offset_of!(FtsEnt, fts_pointer), 32);
        assert_eq!(offset_of!(FtsEnt, fts_accpath), 40);
        assert_eq!(offset_of!(FtsEnt, fts_path), 48);
        assert_eq!(offset_of!(FtsEnt, fts_errno), 56);
        assert_eq!(offset_of!(FtsEnt, fts_symfd), 60);
        assert_eq!(offset_of!(FtsEnt, fts_pathlen), 64);
        assert_eq!(offset_of!(FtsEnt, fts_namelen), 66);
        assert_eq!(offset_of!(FtsEnt, fts_ino), 72);
        assert_eq!(offset_of!(FtsEnt, fts_dev), 80);
        assert_eq!(offset_of!(FtsEnt, fts_nlink), 88);
        assert_eq!(offset_of!(FtsEnt, fts_level), 96);
        assert_eq!(offset_of!(FtsEnt, fts_info), 98);
        assert_eq!(offset_of!(FtsEnt, fts_flags), 100);
        assert_eq!(offset_of!(FtsEnt, fts_instr), 102);
        assert_eq!(offset_of!(FtsEnt, fts_statp), 104);
        assert_eq!(offset_of!(FtsEnt, fts_name), 112);
        assert_eq!(core::mem::size_of::<FtsEnt>(), 120);
    }

    #[test]
    fn fts_has_glibcs_layout() {
        use core::mem::offset_of;
        assert_eq!(offset_of!(Fts, fts_cur), 0);
        assert_eq!(offset_of!(Fts, fts_child), 8);
        assert_eq!(offset_of!(Fts, fts_array), 16);
        assert_eq!(offset_of!(Fts, fts_dev), 24);
        assert_eq!(offset_of!(Fts, fts_path), 32);
        assert_eq!(offset_of!(Fts, fts_rfd), 40);
        assert_eq!(offset_of!(Fts, fts_pathlen), 44);
        assert_eq!(offset_of!(Fts, fts_nitems), 48);
        assert_eq!(offset_of!(Fts, fts_compar), 56);
        assert_eq!(offset_of!(Fts, fts_options), 64);
        assert_eq!(core::mem::size_of::<Fts>(), 72);
    }

    /// glibc's values. The instructions are the ones that were wrong: 2/1/4/3
    /// here against glibc's 1/2/3/4, so `fts_set(FTS_SKIP)` from a program
    /// built against glibc's header meant "no instruction".
    #[test]
    fn constants_have_glibcs_values() {
        assert_eq!(
            [
                FTS_COMFOLLOW,
                FTS_LOGICAL,
                FTS_NOCHDIR,
                FTS_NOSTAT,
                FTS_PHYSICAL
            ],
            [1, 2, 4, 8, 0x10]
        );
        assert_eq!(
            [
                FTS_SEEDOT,
                FTS_XDEV,
                FTS_WHITEOUT,
                FTS_OPTIONMASK,
                FTS_NAMEONLY
            ],
            [0x20, 0x40, 0x80, 0xff, 0x100]
        );
        assert_eq!(
            [
                FTS_D,
                FTS_DC,
                FTS_DEFAULT,
                FTS_DNR,
                FTS_DOT,
                FTS_DP,
                FTS_ERR
            ],
            [1, 2, 3, 4, 5, 6, 7]
        );
        assert_eq!(
            [FTS_F, FTS_INIT, FTS_NS, FTS_NSOK, FTS_SL, FTS_SLNONE, FTS_W],
            [8, 9, 10, 11, 12, 13, 14]
        );
        assert_eq!([FTS_AGAIN, FTS_FOLLOW, FTS_NOINSTR, FTS_SKIP], [1, 2, 3, 4]);
        assert_eq!((FTS_ROOTPARENTLEVEL, FTS_ROOTLEVEL), (-1, 0));
        assert_eq!((FTS_DONTCHDIR, FTS_SYMFOLLOW), (1, 2));
    }

    // -----------------------------------------------------------------------
    // fts_open
    // -----------------------------------------------------------------------

    #[test]
    fn open_refuses_what_it_must() {
        reset();
        errno::set_errno(0);
        assert!(open_with(core::ptr::null(), FTS_PHYSICAL, None, &MEM_OPS).is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);

        let (_o, ptrs) = cstrings(&["x"]);
        for bad in [0, FTS_LOGICAL | FTS_PHYSICAL, FTS_PHYSICAL | 0x400] {
            errno::set_errno(0);
            assert!(
                open_with(ptrs.as_ptr(), bad, None, &MEM_OPS).is_null(),
                "{bad:#x}"
            );
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        let before = crate::malloc::live_allocations::count();
        let (_o, ptrs) = cstrings(&["x", ""]);
        errno::set_errno(0);
        assert!(open_with(ptrs.as_ptr(), FTS_PHYSICAL, None, &MEM_OPS).is_null());
        assert_eq!(errno::get_errno(), errno::ENOENT, "an empty path is ENOENT");
        assert_eq!(
            crate::malloc::live_allocations::count(),
            before,
            "and frees what it built"
        );
    }

    /// An empty list is a walk with nothing in it, as in glibc — it used to
    /// be refused with EINVAL.
    #[test]
    fn an_empty_root_list_walks_nothing() {
        reset();
        assert_eq!(walk(&[], FTS_PHYSICAL, None), []);
    }

    #[test]
    fn nochdir_is_always_reported() {
        reset();
        mk("f", Kind::File);
        let sp = open(&["f"], FTS_PHYSICAL, None);
        // SAFETY: a live stream.
        let opts = unsafe { (*sp).fts_options };
        assert_eq!(opts & FTS_NOCHDIR, FTS_NOCHDIR);
        assert_eq!(opts & FTS_PHYSICAL, FTS_PHYSICAL);
        assert_eq!(fts_close(sp), 0);
    }

    // -----------------------------------------------------------------------
    // The shape of a walk
    // -----------------------------------------------------------------------

    fn small_tree() {
        reset();
        mk("t", Kind::Dir);
        mk("t/a", Kind::Dir);
        mk("t/a/x", Kind::File);
        mk("t/b", Kind::File);
        mk("t/c", Kind::Dir);
    }

    /// Pre-order, children in directory order, post-order after the last
    /// child, then NULL with errno 0.
    #[test]
    fn a_walk_is_pre_and_post_order_in_directory_order() {
        small_tree();
        let got = walk(&["t"], FTS_PHYSICAL, None);
        assert_eq!(
            got,
            [
                seen(FTS_D, 0, "t", "t"),
                seen(FTS_D, 1, "t/a", "a"),
                seen(FTS_F, 2, "t/a/x", "x"),
                seen(FTS_DP, 1, "t/a", "a"),
                seen(FTS_F, 1, "t/b", "b"),
                seen(FTS_D, 1, "t/c", "c"),
                seen(FTS_DP, 1, "t/c", "c"),
                seen(FTS_DP, 0, "t", "t"),
            ]
        );
    }

    #[test]
    fn a_file_root_is_one_entry() {
        reset();
        mk("f", Kind::File);
        assert_eq!(walk(&["f"], FTS_PHYSICAL, None), [seen(FTS_F, 0, "f", "f")]);
    }

    #[test]
    fn a_missing_root_is_fts_ns_with_its_errno() {
        reset();
        let sp = open(&["nope"], FTS_PHYSICAL, None);
        let e = fts_read(sp);
        assert!(!e.is_null());
        // SAFETY: live entry.
        unsafe {
            assert_eq!((*e).fts_info, FTS_NS);
            assert_eq!((*e).fts_errno, errno::ENOENT);
            assert_eq!((*e).fts_level, 0);
        }
        assert!(fts_read(sp).is_null());
        assert_eq!(fts_close(sp), 0);
    }

    /// Every root, in `argv` order — only the first used to be walked.
    #[test]
    fn every_root_is_walked_in_argv_order() {
        small_tree();
        mk("f", Kind::File);
        let got = paths(&walk(&["f", "t/c", "t/a"], FTS_PHYSICAL, None));
        assert_eq!(
            got,
            [
                p(FTS_F, "f"),
                p(FTS_D, "t/c"),
                p(FTS_DP, "t/c"),
                p(FTS_D, "t/a"),
                p(FTS_F, "t/a/x"),
                p(FTS_DP, "t/a"),
            ]
        );
    }

    /// `compar` orders the roots and every directory's entries — it used to
    /// be ignored.
    #[test]
    fn compar_sorts_roots_and_siblings() {
        small_tree();
        mk("t/0", Kind::File);
        let got = paths(&walk(&["t/c", "t/0", "t/a"], FTS_PHYSICAL, Some(by_name)));
        assert_eq!(
            got,
            [
                p(FTS_F, "t/0"),
                p(FTS_D, "t/a"),
                p(FTS_F, "t/a/x"),
                p(FTS_DP, "t/a"),
                p(FTS_D, "t/c"),
                p(FTS_DP, "t/c"),
            ]
        );
        let got = paths(&walk(&["t"], FTS_PHYSICAL, Some(by_name_rev)));
        assert_eq!(
            got.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(),
            ["t", "t/c", "t/c", "t/b", "t/a", "t/a/x", "t/a", "t/0", "t"]
        );
    }

    /// How a root's `fts_name` is cut from its path: glibc's `fts_load`.
    #[test]
    fn a_roots_name_is_its_last_component() {
        reset();
        mk("d", Kind::Dir);
        mk("d/e", Kind::Dir);
        let names: Vec<(String, String)> = walk(&["d/e", "d/", "/d"], FTS_PHYSICAL, None)
            .into_iter()
            .filter(|s| s.level == 0 && s.info != FTS_DP)
            .map(|s| (s.path, s.name))
            .collect();
        assert_eq!(
            names,
            [
                ("d/e".into(), "e".into()),
                ("d/".into(), String::new()),
                ("/d".into(), "d".into()),
            ]
        );
        assert_eq!(walk(&["/"], FTS_PHYSICAL, None)[0].name, "/");
    }

    /// A root with a trailing slash does not get a doubled one in its
    /// children's paths.
    #[test]
    fn a_trailing_slash_is_not_doubled() {
        small_tree();
        let got = paths(&walk(&["t/"], FTS_PHYSICAL, None));
        assert_eq!(got[1], p(FTS_D, "t/a"));
        assert_eq!(got.last().unwrap(), &p(FTS_DP, "t/"));
    }

    #[test]
    fn an_empty_directory_is_d_then_dp() {
        reset();
        mk("e", Kind::Dir);
        assert_eq!(
            paths(&walk(&["e"], FTS_PHYSICAL, None)),
            [p(FTS_D, "e"), p(FTS_DP, "e")]
        );
    }

    /// An unreadable directory comes back a second time as FTS_DNR with the
    /// errno, and gets no post-order visit.
    #[test]
    fn an_unreadable_directory_is_dnr_with_its_errno() {
        reset();
        mk("t", Kind::Dir);
        mk("t/locked", Kind::Unreadable);
        mk("t/after", Kind::File);
        let sp = open(&["t"], FTS_PHYSICAL, None);
        let mut got = Vec::new();
        loop {
            let e = fts_read(sp);
            if e.is_null() {
                break;
            }
            // SAFETY: live entry.
            let errno_now = unsafe { (*e).fts_errno };
            got.push((copy_out(e).info, copy_out(e).path, errno_now));
        }
        assert_eq!(fts_close(sp), 0);
        assert_eq!(
            got,
            [
                (FTS_D, "t".into(), 0),
                (FTS_D, "t/locked".into(), 0),
                (FTS_DNR, "t/locked".into(), errno::EACCES),
                (FTS_F, "t/after".into(), 0),
                (FTS_DP, "t".into(), 0),
            ]
        );
    }

    // -----------------------------------------------------------------------
    // Depth and width
    // -----------------------------------------------------------------------

    /// 600 levels of 12-byte names: 7.8 KB of path, past the buffer's first
    /// allocation, so the buffer moves (repeatedly) while entries point into
    /// it. The walk used to stop at the eighth level.
    #[test]
    fn a_tree_600_levels_deep_is_walked_to_the_bottom() {
        reset();
        let mut path = String::from("r");
        mk(&path, Kind::Dir);
        for i in 0..600 {
            path.push_str(&std::format!("/level_{i:05}"));
            mk(&path, Kind::Dir);
        }
        mk(&std::format!("{path}/leaf"), Kind::File);
        let got = walk(&["r"], FTS_PHYSICAL, None);
        assert_eq!(got.len(), 601 * 2 + 1);
        let leaf = &got[601];
        assert_eq!((leaf.info, leaf.level), (FTS_F, 601));
        assert_eq!(leaf.path, std::format!("{path}/leaf"));
        // Every post-order entry names its own directory, after the moves.
        for (i, s) in got[602..].iter().enumerate() {
            assert_eq!(s.info, FTS_DP);
            assert_eq!(i32::from(s.level), 600 - i as i32);
        }
        assert_eq!(got.last().unwrap().path, "r");
    }

    #[test]
    fn a_directory_of_3000_entries_is_walked_whole() {
        reset();
        mk("w", Kind::Dir);
        for i in 0..3000 {
            mk(&std::format!("w/f{i}"), Kind::File);
        }
        let got = walk(&["w"], FTS_PHYSICAL, None);
        assert_eq!(got.len(), 3002);
        assert_eq!(got[1].path, "w/f0");
        assert_eq!(got[3000].path, "w/f2999");
    }

    /// Two walks at once, and more than the two streams there used to be.
    #[test]
    fn many_streams_can_be_open_at_once() {
        small_tree();
        let streams: Vec<*mut Fts> = (0..10).map(|_| open(&["t"], FTS_PHYSICAL, None)).collect();
        for &sp in &streams {
            let first = fts_read(sp);
            assert!(!first.is_null());
            assert_eq!(copy_out(first).path, "t");
        }
        for sp in streams {
            assert_eq!(fts_close(sp), 0);
        }
    }

    // -----------------------------------------------------------------------
    // fts_set
    // -----------------------------------------------------------------------

    #[test]
    fn skip_turns_a_directory_straight_into_its_post_order_visit() {
        small_tree();
        let sp = open(&["t"], FTS_PHYSICAL, None);
        let mut got = Vec::new();
        loop {
            let e = fts_read(sp);
            if e.is_null() {
                break;
            }
            let s = copy_out(e);
            if s.info == FTS_D && s.name == "a" {
                assert_eq!(fts_set(sp, e, FTS_SKIP), 0);
            }
            got.push((s.info, s.path));
        }
        assert_eq!(fts_close(sp), 0);
        assert!(!got.iter().any(|(_, s)| s == "t/a/x"), "{got:?}");
        assert!(got.contains(&(FTS_DP, "t/a".into())));
    }

    #[test]
    fn again_returns_the_same_entry_re_statted() {
        small_tree();
        let sp = open(&["t"], FTS_PHYSICAL, None);
        let root = fts_read(sp);
        let a = fts_read(sp);
        assert_eq!(copy_out(a).path, "t/a");
        assert_eq!(fts_set(sp, a, FTS_AGAIN), 0);
        let again = fts_read(sp);
        assert_eq!(again, a);
        assert_eq!(copy_out(again).info, FTS_D);
        assert_eq!(
            copy_out(fts_read(sp)).path,
            "t/a/x",
            "then the walk goes on"
        );
        let _ = root;
        assert_eq!(fts_close(sp), 0);
    }

    #[test]
    fn follow_re_stats_a_symlink_through_it_and_walks_the_target() {
        reset();
        mk("t", Kind::Dir);
        mk("real", Kind::Dir);
        mk("real/inside", Kind::File);
        mk("t/link", Kind::Link("/real"));
        let sp = open(&["t"], FTS_PHYSICAL, None);
        let _root = fts_read(sp);
        let l = fts_read(sp);
        assert_eq!(copy_out(l).info, FTS_SL, "physical: a link is a link");
        assert_eq!(fts_set(sp, l, FTS_FOLLOW), 0);
        let again = fts_read(sp);
        assert_eq!(again, l);
        assert_eq!(copy_out(again).info, FTS_D, "followed: the target's type");
        // SAFETY: live entry.
        assert_eq!(unsafe { (*again).fts_flags } & FTS_SYMFOLLOW, FTS_SYMFOLLOW);
        assert_eq!(copy_out(fts_read(sp)).path, "t/link/inside");
        assert_eq!(fts_close(sp), 0);
    }

    #[test]
    fn set_refuses_an_unknown_instruction_with_glibcs_1() {
        small_tree();
        let sp = open(&["t"], FTS_PHYSICAL, None);
        let e = fts_read(sp);
        errno::set_errno(0);
        assert_eq!(fts_set(sp, e, 99), 1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(fts_set(sp, core::ptr::null_mut(), FTS_SKIP), 1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(fts_set(core::ptr::null_mut(), e, FTS_SKIP), 1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(fts_close(sp), 0);
    }

    // -----------------------------------------------------------------------
    // Symlinks, cycles, devices
    // -----------------------------------------------------------------------

    #[test]
    fn physical_reports_links_and_logical_follows_them() {
        reset();
        mk("t", Kind::Dir);
        mk("t/f", Kind::File);
        mk("t/tofile", Kind::Link("/t/f"));
        mk("t/dangling", Kind::Link("/nowhere"));
        let phys = paths(&walk(&["t"], FTS_PHYSICAL, None));
        assert!(phys.contains(&p(FTS_SL, "t/tofile")));
        assert!(phys.contains(&p(FTS_SL, "t/dangling")));
        let logical = paths(&walk(&["t"], FTS_LOGICAL, None));
        assert!(logical.contains(&p(FTS_F, "t/tofile")));
        assert!(logical.contains(&p(FTS_SLNONE, "t/dangling")));
    }

    /// A link back to an ancestor, followed, is FTS_DC with `fts_cycle` set,
    /// and is not descended into — a logical walk of a symlink loop used to
    /// go on until the depth limit.
    #[test]
    fn a_directory_that_is_its_own_ancestor_is_fts_dc() {
        reset();
        mk("t", Kind::Dir);
        mk("t/sub", Kind::Dir);
        mk("t/sub/up", Kind::Link("/t"));
        let sp = open(&["t"], FTS_LOGICAL, None);
        let mut got = Vec::new();
        loop {
            let e = fts_read(sp);
            if e.is_null() {
                break;
            }
            let s = copy_out(e);
            if s.info == FTS_DC {
                // SAFETY: live entries; the cycle is an ancestor, still live.
                unsafe {
                    let cyc = (*e).fts_cycle;
                    assert!(!cyc.is_null());
                    assert_eq!((*cyc).fts_level, 0, "the cycle is the root");
                }
            }
            got.push((s.info, s.path));
        }
        assert_eq!(fts_close(sp), 0);
        assert_eq!(
            got,
            [
                (FTS_D, "t".into()),
                (FTS_D, "t/sub".into()),
                (FTS_DC, "t/sub/up".into()),
                (FTS_DP, "t/sub".into()),
                (FTS_DP, "t".into()),
            ]
        );
    }

    #[test]
    fn comfollow_follows_a_root_link_only() {
        reset();
        mk("real", Kind::Dir);
        mk("real/x", Kind::File);
        mk("real/l", Kind::Link("/real/x"));
        mk("root", Kind::Link("/real"));
        let got = paths(&walk(&["root"], FTS_PHYSICAL | FTS_COMFOLLOW, None));
        assert_eq!(got[0], p(FTS_D, "root"));
        assert!(
            got.contains(&p(FTS_SL, "root/l")),
            "below the root, links are links"
        );
        let got = paths(&walk(&["root"], FTS_PHYSICAL, None));
        assert_eq!(got, [p(FTS_SL, "root")]);
    }

    #[test]
    fn xdev_does_not_descend_onto_another_device() {
        reset();
        mk("t", Kind::Dir);
        mk("t/mnt", Kind::DirOnDev(9));
        mk("t/mnt/far", Kind::File);
        mk("t/near", Kind::Dir);
        mk("t/near/x", Kind::File);
        let got = paths(&walk(&["t"], FTS_PHYSICAL | FTS_XDEV, None));
        assert_eq!(
            got,
            [
                p(FTS_D, "t"),
                p(FTS_D, "t/mnt"),
                p(FTS_DP, "t/mnt"),
                p(FTS_D, "t/near"),
                p(FTS_F, "t/near/x"),
                p(FTS_DP, "t/near"),
                p(FTS_DP, "t"),
            ]
        );
        let all = paths(&walk(&["t"], FTS_PHYSICAL, None));
        assert!(all.contains(&p(FTS_F, "t/mnt/far")), "without XDEV it does");
    }

    // -----------------------------------------------------------------------
    // NOSTAT, SEEDOT
    // -----------------------------------------------------------------------

    /// Only what might be a directory is `stat`ed; the rest is FTS_NSOK with
    /// no `stat` buffer.
    #[test]
    fn nostat_stats_only_what_might_be_a_directory() {
        small_tree();
        STATTED.with(|s| s.borrow_mut().clear());
        let got = paths(&walk(&["t"], FTS_PHYSICAL | FTS_NOSTAT, None));
        assert!(got.contains(&p(FTS_NSOK, "t/b")));
        assert!(got.contains(&p(FTS_D, "t/a")));
        let statted: Vec<String> = STATTED.with(|s| {
            s.borrow()
                .iter()
                .map(|v| String::from_utf8(v.clone()).unwrap())
                .collect()
        });
        assert!(
            !statted.iter().any(|s| s == "t/b" || s == "t/a/x"),
            "{statted:?}"
        );
        // With no d_type to go on, everything must be statted.
        DTYPE_UNKNOWN.with(|c| c.set(true));
        let got = paths(&walk(&["t"], FTS_PHYSICAL | FTS_NOSTAT, None));
        assert!(got.contains(&p(FTS_F, "t/b")));
    }

    #[test]
    fn seedot_returns_dot_entries_without_descending() {
        reset();
        mk("t", Kind::Dir);
        mk("t/f", Kind::File);
        let got = paths(&walk(&["t"], FTS_PHYSICAL | FTS_SEEDOT, None));
        assert_eq!(
            got,
            [
                p(FTS_D, "t"),
                p(FTS_DOT, "t/."),
                p(FTS_DOT, "t/.."),
                p(FTS_F, "t/f"),
                p(FTS_DP, "t"),
            ]
        );
    }

    // -----------------------------------------------------------------------
    // fts_children
    // -----------------------------------------------------------------------

    fn list_names(mut e: *mut FtsEnt) -> Vec<(u16, String)> {
        let mut v = Vec::new();
        while !e.is_null() {
            // SAFETY: live list entries.
            unsafe {
                v.push((
                    (*e).fts_info,
                    String::from_utf8(name_of(e).to_vec()).unwrap(),
                ));
                e = (*e).fts_link;
            }
        }
        v
    }

    #[test]
    fn children_before_the_first_read_are_the_roots() {
        small_tree();
        let sp = open(&["t/a", "t/b"], FTS_PHYSICAL, None);
        let roots = list_names(fts_children(sp, 0));
        assert_eq!(roots, [(FTS_D, "t/a".into()), (FTS_F, "t/b".into())]);
        assert_eq!(fts_close(sp), 0);
    }

    /// `fts_children` on a pre-order directory lists it, and the next
    /// `fts_read` walks that same list.
    #[test]
    fn children_of_a_directory_and_the_walk_that_follows() {
        small_tree();
        let sp = open(&["t"], FTS_PHYSICAL, None);
        let _root = fts_read(sp);
        let kids = list_names(fts_children(sp, 0));
        assert_eq!(
            kids,
            [
                (FTS_D, "a".into()),
                (FTS_F, "b".into()),
                (FTS_D, "c".into())
            ]
        );
        assert_eq!(copy_out(fts_read(sp)).path, "t/a");
        // Names only: nothing is typed.
        let names = list_names(fts_children(sp, FTS_NAMEONLY));
        assert_eq!(names, [(FTS_NSOK, "x".into())]);
        // ...and walking re-reads the directory to type them.
        assert_eq!(copy_out(fts_read(sp)).info, FTS_F);
        // A file has no children, and that is not an error.
        errno::set_errno(7);
        assert!(fts_children(sp, 0).is_null());
        assert_eq!(errno::get_errno(), 0);
        assert!(fts_children(sp, 5).is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(fts_close(sp), 0);
    }

    // -----------------------------------------------------------------------
    // The tree is real
    // -----------------------------------------------------------------------

    /// `fts_parent` is live and `fts_number` persists: BSD `du` sums sizes
    /// into `ent->fts_parent->fts_number` and reads the total at FTS_DP.
    #[test]
    fn numbers_accumulate_up_the_parent_chain() {
        small_tree();
        let sp = open(&["t"], FTS_PHYSICAL, None);
        let mut totals = Vec::new();
        loop {
            let e = fts_read(sp);
            if e.is_null() {
                break;
            }
            // SAFETY: live entry and live parent.
            unsafe {
                match (*e).fts_info {
                    FTS_F => {
                        let parent = (*e).fts_parent;
                        (*parent).fts_number += 1;
                    }
                    FTS_DP => {
                        totals.push((copy_out(e).path, (*e).fts_number));
                        let parent = (*e).fts_parent;
                        if (*parent).fts_level >= FTS_ROOTLEVEL {
                            (*parent).fts_number += (*e).fts_number;
                        }
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(fts_close(sp), 0);
        assert_eq!(
            totals,
            [("t/a".into(), 1), ("t/c".into(), 0), ("t".into(), 2)]
        );
    }

    #[test]
    fn stat_fields_are_filled_in() {
        small_tree();
        let sp = open(&["t"], FTS_PHYSICAL, None);
        let root = fts_read(sp);
        // SAFETY: live entry.
        unsafe {
            assert!(!(*root).fts_statp.is_null());
            assert_eq!((*(*root).fts_statp).st_mode & S_IFMT, S_IFDIR);
            assert_eq!((*root).fts_nlink, 4, "2 + two subdirectories");
            assert_ne!((*root).fts_ino, 0);
            assert_eq!((*root).fts_symfd, -1);
            assert_eq!((*(*root).fts_parent).fts_level, FTS_ROOTPARENTLEVEL);
        }
        assert_eq!(fts_close(sp), 0);
    }

    /// Closing part-way frees everything, wherever the walk had got to.
    #[test]
    fn closing_mid_walk_leaks_nothing() {
        small_tree();
        for stop_after in 0..8 {
            let before = crate::malloc::live_allocations::count();
            let sp = open(&["t"], FTS_PHYSICAL, None);
            for _ in 0..stop_after {
                let _ = fts_read(sp);
            }
            if stop_after == 2 {
                let _ = fts_children(sp, 0);
            }
            assert_eq!(fts_close(sp), 0);
            assert_eq!(
                crate::malloc::live_allocations::count(),
                before,
                "closed after {stop_after} reads"
            );
        }
    }

    #[test]
    fn null_streams_are_refused() {
        errno::set_errno(0);
        assert!(fts_read(core::ptr::null_mut()).is_null());
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(fts_close(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert!(fts_children(core::ptr::null_mut(), 0).is_null());
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    /// The real filesystem path is the one `fts_open` uses; on the host its
    /// calls fail, and that failure must come back as FTS_NS, not a crash.
    #[test]
    fn the_real_ops_fail_cleanly_on_the_host() {
        let (_o, ptrs) = cstrings(&["/definitely/not/here"]);
        let sp = fts_open(ptrs.as_ptr(), FTS_PHYSICAL, None);
        assert!(!sp.is_null());
        let e = fts_read(sp);
        assert!(!e.is_null());
        // SAFETY: live entry.
        assert_eq!(unsafe { (*e).fts_info }, FTS_NS);
        assert_eq!(fts_close(sp), 0);
    }
}
