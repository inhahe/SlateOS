// Indexing and arithmetic in this file operate on:
//
//  - A fixed-size pool of `Fts` instances (`POOL`) indexed by
//    `inst.idx`, which is range-checked on every entry to the helper.
//  - A `MAX_FTS_DEPTH`-deep stack of in-progress directory frames
//    indexed by `inst.depth.wrapping_sub(1)` after `depth > 0` checks.
//  - A `FTS_NAME_MAX`-sized name buffer filled from a `readdir` entry
//    whose length was measured before the copy.
//  - A `PATH_MAX`-sized path buffer mutated in place; every write is
//    preceded by an explicit length budget check.
//
// In each case the bound is established locally but clippy cannot see
// across the check.  These lints would only be meaningful here if we
// accepted attacker-controlled integer indices into the pool/stack,
// which we do not.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

//! `<fts.h>` — file tree traversal (FreeBSD-style).
//!
//! `fts` is the richer cousin of `ftw`/`nftw`: instead of invoking a
//! user callback for every entry, it exposes a cursor-style API where
//! the caller drives the traversal with repeated `fts_read()` calls.
//! This is what `find(1)`, `rm -rf`, `du`, `chmod -R`, and friends are
//! built on top of.
//!
//! ## Implementation
//!
//! All state is in a static pool of [`Fts`] instances (no heap).
//! Each instance owns:
//!
//! - A traversal stack of up to [`MAX_FTS_DEPTH`] in-progress directory
//!   frames.  **Each frame holds its directory's [`crate::dirent::Dir`]
//!   stream open for as long as the frame is on the stack**, and reads
//!   the next child from it on demand.
//!
//!   It did not always.  Frames used to copy a fixed 64 children into
//!   the frame and close the stream immediately, which was forced on
//!   us: there were once only 8 `Dir` slots system-wide, so one stream
//!   per level would have exhausted the pool at exactly the depth this
//!   module allows.  A directory with a 65th entry therefore lost it,
//!   silently — and since `fts` is what `find`, `rm -rf`, `du` and
//!   `chmod -R` are built on, "silently" meant a recursive delete that
//!   reported success while leaving files behind.
//!
//!   Two things had to become true before holding a stream per level
//!   was correct, and both now are.  `dirent::MAX_OPEN_DIRS` is 64,
//!   comfortably above [`MAX_FTS_DEPTH`], since a slot stopped carrying
//!   a 68 KiB inline buffer.  And a `Dir` *is itself a snapshot* — it
//!   slurps the complete listing to the heap at `opendir` time and
//!   never re-reads — so a held stream is exactly as independent of
//!   later mutation as a copied array was.  The frame gave up nothing
//!   by dropping its copy, and stopped being able to lose a file.
//! - The currently-displayed [`FtsEnt`], whose `fts_path` is a
//!   mutated-in-place buffer that grows as we descend and shrinks as
//!   we ascend.  `fts_name` points into `fts_path` at the basename
//!   offset.
//! - A scratch [`crate::stat::Stat`] buffer that [`FtsEnt::fts_statp`]
//!   points to.  Always lives inside the instance; never freed.
//!
//! Because `fts_path` and `fts_statp` point into the instance, the
//! returned [`FtsEnt`] is invalidated by the next `fts_read()` call.
//! POSIX permits this — callers that need to retain data must copy
//! it out.
//!
//! ## Supported options
//!
//! - [`FTS_PHYSICAL`] — do not follow symlinks (default and only
//!   robust mode without cycle detection).
//! - [`FTS_LOGICAL`] — follow symlinks (best-effort; no cycle
//!   detection, so a symlink loop walks until it hits
//!   [`MAX_FTS_DEPTH`] and stops there with [`FTS_DNR`]).
//! - [`FTS_NOSTAT`] — skip [`crate::file::stat`] calls; `fts_statp`
//!   is left zeroed and entries are typed by readdir's `d_type`.
//! - [`FTS_NOCHDIR`] — accepted (and effectively always set — we
//!   never chdir during traversal).
//! - [`FTS_COMFOLLOW`] — accepted but only follows the *root* link
//!   (matches BSD semantics).
//! - [`FTS_SEEDOT`] — not supported; `.` and `..` are always skipped.
//! - [`FTS_XDEV`] — not supported; we don't track device IDs.
//!
//! ## Supported instructions (via `fts_set`)
//!
//! - [`FTS_SKIP`] — do not descend into this directory (only valid on
//!   a pre-order [`FTS_D`] entry).
//! - [`FTS_AGAIN`] — re-yield the current entry on the next
//!   `fts_read` call.
//! - [`FTS_FOLLOW`] — accepted on [`FTS_SL`]/[`FTS_SLNONE`] entries
//!   but no-op (we don't transparently re-stat for FTS_LOGICAL).
//!
//! ## Limits
//!
//! - [`MAX_FTS_INSTANCES`] = 2 concurrent open FTS streams.
//! - [`MAX_FTS_DEPTH`] = 8 levels deep.  A directory at that depth is
//!   re-yielded as [`FTS_DNR`] with `fts_errno` = `ENOMEM` rather than
//!   being descended into.  It used to be skipped in silence — the walk
//!   simply moved on to the next sibling, and the ninth level, plus the
//!   post-order [`FTS_DP`] for the directory itself, never appeared.  A
//!   `rm -r` driven by that reported success on a tree it had not
//!   emptied.  The limit is still a limit; it is no longer a lie.
//! - **No limit on entries per directory.**  There was one — 64 — and
//!   it was the last place in the traversal that could lose a file
//!   without saying so.  See the implementation note above for what
//!   made it removable.
//! - [`FTS_NAME_MAX`] = 256 bytes per component, which is
//!   [`crate::dirent::Dirent`]'s own `d_name` capacity, so no name a
//!   `readdir` can produce is too long for a frame to carry.  A longer
//!   one cannot arrive; if the two constants ever diverge, the
//!   assertion beside `FTS_NAME_MAX` fails the build rather than
//!   letting names go missing again.
//! - Path length capped at [`crate::unistd::PATH_MAX`] (4096).

use crate::errno;
use crate::fcntl::{S_IFDIR, S_IFLNK, S_IFMT, S_IFREG};
use crate::perprocess::process_global;
use crate::stat::Stat;
use crate::unistd::PATH_MAX;

// ---------------------------------------------------------------------------
// fts_open options
// ---------------------------------------------------------------------------

/// Follow symbolic link given as a root pathname.
pub const FTS_COMFOLLOW: i32 = 0x0001;

/// Logical traversal: follow all symlinks.
pub const FTS_LOGICAL: i32 = 0x0002;

/// Do not chdir during traversal.
pub const FTS_NOCHDIR: i32 = 0x0004;

/// Do not stat files; `fts_statp` is undefined.
pub const FTS_NOSTAT: i32 = 0x0008;

/// Physical traversal: do not follow symlinks.
pub const FTS_PHYSICAL: i32 = 0x0010;

/// Return dot-files (`.` and `..`).
pub const FTS_SEEDOT: i32 = 0x0020;

/// Do not cross mount points.
pub const FTS_XDEV: i32 = 0x0040;

// ---------------------------------------------------------------------------
// fts_info values (FtsEnt::fts_info)
// ---------------------------------------------------------------------------

/// Preorder directory.
pub const FTS_D: i32 = 1;

/// Directory that causes a cycle.
pub const FTS_DC: i32 = 2;

/// Default (unknown file type).
pub const FTS_DEFAULT: i32 = 3;

/// Unreadable directory.
pub const FTS_DNR: i32 = 4;

/// Dot file (`.` or `..`).
pub const FTS_DOT: i32 = 5;

/// Post-order directory.
pub const FTS_DP: i32 = 6;

/// Error (errno set).
pub const FTS_ERR: i32 = 7;

/// Regular file.
pub const FTS_F: i32 = 8;

/// Initialization (root entry not yet read).
pub const FTS_INIT: i32 = 9;

/// No stat info requested ([`FTS_NOSTAT`]).
pub const FTS_NS: i32 = 10;

/// No stat info available (couldn't stat, but proceeding).
pub const FTS_NSOK: i32 = 11;

/// Symbolic link.
pub const FTS_SL: i32 = 12;

/// Symbolic link pointing to a nonexistent target.
pub const FTS_SLNONE: i32 = 13;

/// Whiteout (BSD-specific; never produced here).
pub const FTS_W: i32 = 14;

// ---------------------------------------------------------------------------
// fts_set instructions
// ---------------------------------------------------------------------------

/// Follow this symbolic link.
pub const FTS_FOLLOW: i32 = 1;

/// Read this entry again on the next `fts_read` call.
pub const FTS_AGAIN: i32 = 2;

/// Skip this entry (only meaningful on a pre-order [`FTS_D`]).
pub const FTS_SKIP: i32 = 3;

/// No instruction (the default when an entry is first returned).
pub const FTS_NOINSTR: i32 = 4;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum concurrent open FTS streams.
const MAX_FTS_INSTANCES: usize = 2;

/// Maximum depth of the traversal stack.
///
/// Also the number of [`crate::dirent::Dir`] streams a single traversal
/// holds open at once, one per level, which is why the assertion below
/// exists.
const MAX_FTS_DEPTH: usize = 8;

// A frame holds its directory's stream open for its whole life, so a
// traversal at full depth occupies `MAX_FTS_DEPTH` slots of the `Dir`
// pool -- and two concurrent FTS streams occupy twice that.  If either
// constant is ever raised past the pool, the failure is not a build
// error but a `descend_into_current` that starts returning `FTS_DNR`
// with `EMFILE` partway down a tree, which reads exactly like a
// permissions problem.  Fail here instead, where the cause is legible.
const _: () = assert!(
    MAX_FTS_INSTANCES * MAX_FTS_DEPTH <= crate::dirent::MAX_OPEN_DIRS,
    "FTS holds one Dir stream per level per stream; the Dir pool must cover them all"
);

/// Maximum length of a single path component, including NUL.
///
/// Equal to [`crate::dirent::Dirent`]'s `d_name` capacity by the
/// assertion below: a component that `readdir` can return must fit a
/// frame, because the alternative is skipping it, and a traversal that
/// skips a file it was asked to visit is the bug this module spent its
/// first year having.
pub const FTS_NAME_MAX: usize = 256;

const _: () = assert!(
    FTS_NAME_MAX >= crate::dirent::DIRENT_NAME_MAX,
    "a name readdir can produce must fit an FTS frame"
);

// ---------------------------------------------------------------------------
// Public structures
// ---------------------------------------------------------------------------

/// File tree entry returned by `fts_read`.
#[repr(C)]
pub struct FtsEnt {
    /// Info flags (FTS_D, FTS_F, etc.).
    pub fts_info: i32,

    /// Depth in the tree (root = 0).
    pub fts_level: i32,

    /// Length of `fts_path` (excluding terminating NUL).
    pub fts_pathlen: usize,

    /// Length of `fts_name` (excluding terminating NUL).
    pub fts_namelen: usize,

    /// Numeric link count (copied from stat).
    pub fts_nlink: u64,

    /// Error number (if FTS_ERR or FTS_DNR).
    pub fts_errno: i32,

    /// Instruction set by `fts_set` (FTS_FOLLOW, FTS_SKIP, etc.).
    /// `fts_read` consults this on the next call.
    pub fts_instr: i32,

    /// Stat buffer pointer.  Points into the parent [`Fts`] instance.
    pub fts_statp: *const Stat,

    /// File name (component).  Points into [`Self::fts_path`] at the
    /// basename offset.
    pub fts_name: *const u8,

    /// Full path (NUL-terminated).  Points into the parent [`Fts`].
    pub fts_path: *const u8,

    /// Pointer to parent entry (always null in our impl — we reuse a
    /// single entry slot per stream).
    pub fts_parent: *mut FtsEnt,

    /// Linked list of children (null in our impl — `fts_children` is
    /// not supported beyond returning the current entry's siblings as
    /// a single-node list).
    pub fts_link: *mut FtsEnt,

    /// User-settable number (for sorting).
    pub fts_number: i64,

    /// User-settable pointer.
    pub fts_pointer: *mut u8,
}

/// Opaque handle for an open FTS stream.  Caller treats as opaque;
/// internally indexes into the static instance pool.
#[repr(C)]
pub struct Fts {
    /// Index into `FTS_INSTANCES` + 1 (0 reserved for the empty
    /// sentinel — callers compare against null).
    handle: u32,
    /// Options passed at open time (mirrored here for the public ABI
    /// and to keep `fts_options` accessible without instance lookup).
    pub fts_options: i32,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// One child entry, copied out of a `readdir` result.
///
/// Not stored — this is what [`next_child`] hands to [`yield_child`].
/// The `Dirent` it was copied from is only valid until the next
/// `readdir` on the same stream, and the copy is what makes the
/// distance between those two functions safe to have.
#[derive(Clone, Copy)]
struct Child {
    name: [u8; FTS_NAME_MAX],
    name_len: usize,
    d_type: u8,
}

/// One in-progress directory on the traversal stack.
#[derive(Clone, Copy)]
struct DirFrame {
    /// This directory's open stream, or null for an unused frame.
    ///
    /// Owned by the frame: whoever removes the frame from the stack —
    /// [`pop_and_yield_dp`] on the normal path, [`close_open_frames`]
    /// on `fts_close` — closes it.  A raw pointer rather than a guard
    /// type because `DirFrame` is `Copy` and lives in a `static` pool,
    /// so there is no drop to hang the release on; the two call sites
    /// above are the whole of the ownership discipline.
    dir: *mut crate::dirent::Dir,
    /// Length of `Instance::path` *before* this directory's basename
    /// was appended.  Used to truncate `path` back when popping.
    parent_path_len: u16,
    /// True if we've already yielded `FTS_D` for this directory and
    /// owe the user a matching `FTS_DP`.  (Always `true` after
    /// `descend_into_current` succeeds; kept for clarity in debugging
    /// and possible future cycle-detection logic.)
    yielded_pre: bool,
    /// Stat saved at descent time (so the eventual `FTS_DP` carries
    /// the same nlink etc. as the matching `FTS_D` did).
    saved_stat: Stat,
}

const DIR_FRAME_INIT: DirFrame = DirFrame {
    dir: core::ptr::null_mut(),
    parent_path_len: 0,
    yielded_pre: false,
    saved_stat: zeroed_stat(),
};

/// One FTS stream's state.
struct Instance {
    /// Slot is occupied iff true.  Only ever read/written through the
    /// helpers below, which operate on this process's own table.
    in_use: bool,
    options: i32,
    /// Root path provided at `fts_open`, NUL-terminated.
    root: [u8; PATH_MAX],
    root_len: usize,
    /// True once we've yielded the root itself (in PRE for dirs, F/SL
    /// for non-dirs).
    root_yielded: bool,
    /// True once we've also drained the root and yielded its DP (or
    /// when the root was not a directory).  When this flips to true,
    /// the next `fts_read` returns null and clears errno.
    finished: bool,
    /// Working path buffer, mutated as we descend/ascend.
    path: [u8; PATH_MAX],
    path_len: usize,
    /// Traversal stack — `depth` is the number of valid frames.
    /// Frame `i` represents the directory at depth `i`, whose
    /// `parent_path_len` records where in `path` its basename starts.
    stack: [DirFrame; MAX_FTS_DEPTH],
    depth: usize,
    /// The entry returned to the user.  All `fts_*` pointers in here
    /// point into this instance.
    current: FtsEnt,
    /// Stat scratch for `current.fts_statp`.
    statbuf: Stat,
    /// If `fts_set(FTS_AGAIN)` was applied, the next `fts_read` re-
    /// yields `current` without advancing.
    pending_again: bool,
}

// `large_const_arrays` wants this to be a `static`, and it cannot be. This is a
// *template*, not a datum: `process_global!` below needs it in a const
// initializer, and resetting a slot with `*inst = INSTANCE_INIT` moves out of
// it. Neither is legal against a static unless `Instance` is made `Copy` — and
// making an ~8 KiB struct of two `PATH_MAX` buffers implicitly copyable to
// satisfy a lint would trade a real footgun for a cosmetic win.
#[allow(clippy::large_const_arrays)]
const INSTANCE_INIT: Instance = Instance {
    in_use: false,
    options: 0,
    root: [0; PATH_MAX],
    root_len: 0,
    root_yielded: false,
    finished: false,
    path: [0; PATH_MAX],
    path_len: 0,
    stack: [DIR_FRAME_INIT; MAX_FTS_DEPTH],
    depth: 0,
    current: FtsEnt {
        fts_info: FTS_INIT,
        fts_level: 0,
        fts_pathlen: 0,
        fts_namelen: 0,
        fts_nlink: 0,
        fts_errno: 0,
        fts_instr: FTS_NOINSTR,
        fts_statp: core::ptr::null(),
        fts_name: core::ptr::null(),
        fts_path: core::ptr::null(),
        fts_parent: core::ptr::null_mut(),
        fts_link: core::ptr::null_mut(),
        fts_number: 0,
        fts_pointer: core::ptr::null_mut(),
    },
    statbuf: zeroed_stat(),
    pending_again: false,
};

process_global! {
    /// This process's FTS stream table.
    ///
    /// `process_global!` rather than a bare `static mut`: an `FTS *` handed to
    /// the caller is an index into this table, so the table must have exactly
    /// the same scope as the fd table it parallels.  As a bare `static mut`
    /// this was the one slot pool in the crate that host test threads *shared*,
    /// which is how two `fts_open` calls on different test threads came to
    /// claim the same slot — see
    /// `B-FTS-INSTANCE-POOL-CLAIM-IS-A-DATA-RACE` in `known-issues.md`.
    fn instances_ptr() -> [Instance; MAX_FTS_INSTANCES] = [INSTANCE_INIT, INSTANCE_INIT];

    /// The `Fts` handle bodies — one per instance — that we hand out to the
    /// caller.  Must be per-process for the same reason as the table above:
    /// the handle is only meaningful against its own process's slots.
    fn handles_ptr() -> [Fts; MAX_FTS_INSTANCES] = [
        Fts { handle: 1, fts_options: 0 },
        Fts { handle: 2, fts_options: 0 },
    ];
}

// ---------------------------------------------------------------------------
// Const helper — we need an all-zero `Stat` in `const` initializers,
// but `zeroed_stat()` is not `const`.  Wrap `core::mem::zeroed`.
// ---------------------------------------------------------------------------

/// All-zero [`Stat`] usable in `const` context.
///
/// # Safety
///
/// `Stat` is `#[repr(C)]` of integer fields; the all-zero bit pattern
/// is a valid value for every field.
const fn zeroed_stat() -> Stat {
    // SAFETY: see above.
    unsafe { core::mem::zeroed() }
}

// ---------------------------------------------------------------------------
// Instance allocation
// ---------------------------------------------------------------------------

fn allocate_instance() -> Option<usize> {
    // SAFETY: `instances_ptr()` yields this process's own table (see
    // `perprocess.rs`), so nothing else holds a reference to it here.
    let table = unsafe { &mut *instances_ptr() };
    for (i, inst) in table.iter_mut().enumerate() {
        if !inst.in_use {
            *inst = INSTANCE_INIT;
            inst.in_use = true;
            return Some(i);
        }
    }
    None
}

fn release_instance(idx: usize) {
    // SAFETY: see `allocate_instance`.
    let table = unsafe { &mut *instances_ptr() };
    if let Some(inst) = table.get_mut(idx) {
        // Before the slot is overwritten, not after: `INSTANCE_INIT`
        // nulls every frame's `dir`, and a stream whose only pointer
        // has just been overwritten cannot be closed by anyone.
        close_open_frames(inst);
        *inst = INSTANCE_INIT;
    }
}

fn with_instance<R>(idx: usize, f: impl FnOnce(&mut Instance) -> R) -> Option<R> {
    // SAFETY: see `allocate_instance`.
    let table = unsafe { &mut *instances_ptr() };
    table.get_mut(idx).filter(|i| i.in_use).map(f)
}

/// Convert a `*mut Fts` from the caller to an instance index, or
/// `None` if the handle is null/invalid.
fn handle_to_idx(ftsp: *const Fts) -> Option<usize> {
    if ftsp.is_null() {
        return None;
    }
    // SAFETY: caller passed a value previously returned by `fts_open`.
    let h = unsafe { (*ftsp).handle };
    if h == 0 || (h as usize) > MAX_FTS_INSTANCES {
        return None;
    }
    Some((h as usize).wrapping_sub(1))
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Copy a NUL-terminated path from `src` into `dst`, returning the
/// length (excluding NUL).  Returns `None` if the path doesn't fit.
fn copy_cstr_into(src: *const u8, dst: &mut [u8]) -> Option<usize> {
    if src.is_null() {
        return None;
    }
    let mut i: usize = 0;
    loop {
        if i >= dst.len() {
            return None;
        }
        // SAFETY: src is a valid C string per caller contract.
        let b = unsafe { *src.add(i) };
        if b == 0 {
            dst[i] = 0;
            return Some(i);
        }
        dst[i] = b;
        i = i.wrapping_add(1);
    }
}

/// Append "/name" to `path` of length `len`, NUL-terminating.
/// Returns the new length, or `None` on overflow.  `name` must be a
/// NUL-free byte slice (already validated by readdir).
fn append_component(path: &mut [u8; PATH_MAX], len: usize, name: &[u8]) -> Option<usize> {
    if len >= PATH_MAX {
        return None;
    }
    let needs_sep = len > 0 && path[len.wrapping_sub(1)] != b'/';
    let sep: usize = usize::from(needs_sep);
    let new_len = len.checked_add(sep)?.checked_add(name.len())?;
    if new_len.checked_add(1)? > PATH_MAX {
        return None;
    }
    let mut i = len;
    if needs_sep {
        path[i] = b'/';
        i = i.wrapping_add(1);
    }
    for &b in name {
        path[i] = b;
        i = i.wrapping_add(1);
    }
    path[i] = 0;
    Some(i)
}

/// Find the basename offset in `path[..len]`.  Returns 0 for paths
/// with no '/' (or just the leading one).
fn basename_offset(path: &[u8], len: usize) -> usize {
    let mut i = len;
    while i > 0 {
        i = i.wrapping_sub(1);
        if path[i] == b'/' {
            return i.wrapping_add(1);
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Reading a directory into a frame
// ---------------------------------------------------------------------------

/// Open `path` and attach its stream to `frame`.  Returns:
///
/// - `Ok(())` on success, with `frame.dir` owning an open stream.
/// - `Err(errno_value)` if opendir failed, with `frame.dir` left null.
///
/// Reads nothing.  The children are read one at a time by
/// [`next_child`] as the traversal asks for them, which is what removed
/// the 64-entry cap: there is no longer a buffer to overflow.
fn open_frame(path: *const u8, frame: &mut DirFrame) -> Result<(), i32> {
    let dir = crate::dirent::opendir(path);
    if dir.is_null() {
        // opendir set errno for us.  EMFILE is possible and is a real
        // answer: the process is out of directory streams, and saying
        // so beats descending into a directory we cannot read.
        return Err(errno::get_errno());
    }
    frame.dir = dir;
    Ok(())
}

/// Close a frame's stream if it has one, and null the field.
///
/// Idempotent, because the two owners of a frame's stream — the pop
/// path and the close path — can both plausibly run for the same frame
/// during teardown, and a double `closedir` would free a pool slot
/// another traversal had since taken.
fn close_frame(frame: &mut DirFrame) {
    if !frame.dir.is_null() {
        crate::dirent::closedir(frame.dir);
        frame.dir = core::ptr::null_mut();
    }
}

/// Close every stream still held by an instance's frame stack.
///
/// `fts_close` releasing an instance without this leaks one `Dir` pool
/// slot per level the traversal had reached — and the pool is a fixed
/// 64, so a program that opens and abandons traversals in a loop
/// eventually gets `EMFILE` from an unrelated `opendir`.
fn close_open_frames(inst: &mut Instance) {
    for frame in &mut inst.stack {
        close_frame(frame);
    }
    inst.depth = 0;
}

/// Read the next real child of `frame`'s directory, skipping `.` and
/// `..`.  `None` means the listing is exhausted.
///
/// The name is *copied* out of the `Dirent`: the pointer `readdir`
/// returns is only valid until the next call on the same stream, and
/// the caller goes on to stat and yield before asking for another.
fn next_child(frame: &DirFrame) -> Option<Child> {
    if frame.dir.is_null() {
        return None;
    }
    loop {
        let entry = crate::dirent::readdir(frame.dir);
        if entry.is_null() {
            return None;
        }
        // SAFETY: readdir returned a valid entry pointer.
        let name_ptr = unsafe { core::ptr::addr_of!((*entry).d_name).cast::<u8>() };
        // SAFETY: dirent name is NUL-terminated within DIRENT_NAME_MAX.
        let nlen = unsafe { crate::string::strlen(name_ptr) };
        // Skip "." and ".." regardless of FTS_SEEDOT (not supported).
        if is_dot_or_dotdot(name_ptr, nlen) {
            continue;
        }
        // Unreachable while the compile-time assertion beside
        // `FTS_NAME_MAX` holds; kept because the alternative to a
        // bounds check here is a buffer overrun, and a `continue` that
        // can never run costs nothing.
        if nlen >= FTS_NAME_MAX {
            continue;
        }
        let mut child = Child {
            name: [0; FTS_NAME_MAX],
            name_len: nlen,
            // SAFETY: as above.
            d_type: unsafe { (*entry).d_type },
        };
        for i in 0..nlen {
            // SAFETY: i < nlen and the name is NUL-terminated.
            child.name[i] = unsafe { *name_ptr.add(i) };
        }
        return Some(child);
    }
}

fn is_dot_or_dotdot(name: *const u8, len: usize) -> bool {
    if len == 1 {
        // SAFETY: len validates we can read one byte.
        return unsafe { *name } == b'.';
    }
    if len == 2 {
        // SAFETY: len validates we can read two bytes.
        return unsafe { *name } == b'.' && unsafe { *name.add(1) } == b'.';
    }
    false
}

// ---------------------------------------------------------------------------
// d_type → fts_info classification
// ---------------------------------------------------------------------------

fn dtype_to_info(d_type: u8) -> i32 {
    match d_type {
        crate::dirent::DT_REG => FTS_F,
        crate::dirent::DT_DIR => FTS_D,
        crate::dirent::DT_LNK => FTS_SL,
        crate::dirent::DT_UNKNOWN => FTS_DEFAULT,
        _ => FTS_DEFAULT,
    }
}

fn stat_to_info(sb: &Stat) -> i32 {
    let m = sb.st_mode & S_IFMT;
    if m == S_IFDIR {
        FTS_D
    } else if m == S_IFREG {
        FTS_F
    } else if m == S_IFLNK {
        FTS_SL
    } else {
        FTS_DEFAULT
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Open a file hierarchy for traversal.
///
/// `path_argv` is a NULL-terminated array of NUL-terminated path
/// strings.  Only the first path is honored (multi-root traversal is
/// not implemented — callers needing it should open multiple streams).
///
/// `options` must include exactly one of [`FTS_PHYSICAL`] or
/// [`FTS_LOGICAL`].
///
/// `_compar` (sort comparator) is currently ignored — entries are
/// returned in the order [`crate::dirent::readdir`] yields them
/// (typically directory order, not sorted).
///
/// Returns a non-null `*mut Fts` on success, or null on error with
/// errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_open(
    path_argv: *const *const u8,
    options: i32,
    _compar: Option<unsafe extern "C" fn(*const *const FtsEnt, *const *const FtsEnt) -> i32>,
) -> *mut Fts {
    if path_argv.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // Exactly one of FTS_LOGICAL / FTS_PHYSICAL is required.
    let logical = options & FTS_LOGICAL != 0;
    let physical = options & FTS_PHYSICAL != 0;
    if logical == physical {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }
    // SAFETY: caller passed a NULL-terminated array of C strings.
    let first = unsafe { *path_argv };
    if first.is_null() {
        // Empty path list — POSIX says EINVAL.
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }

    let Some(idx) = allocate_instance() else {
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    };

    let mut ok = true;
    with_instance(idx, |inst| {
        inst.options = options;
        let Some(rlen) = copy_cstr_into(first, &mut inst.root) else {
            ok = false;
            return;
        };
        inst.root_len = rlen;
        // Seed `path` with the root.
        inst.path[..rlen].copy_from_slice(&inst.root[..rlen]);
        inst.path[rlen] = 0;
        inst.path_len = rlen;
    });
    if !ok {
        release_instance(idx);
        errno::set_errno(errno::ENAMETOOLONG);
        return core::ptr::null_mut();
    }

    // SAFETY: indexing into this process's handle pool with a bounded
    // index from `allocate_instance`.
    unsafe {
        let table = handles_ptr();
        (*table)[idx].fts_options = options;
        core::ptr::addr_of_mut!((*table)[idx])
    }
}

/// Read the next entry from an FTS stream.
///
/// Returns a pointer to an internal [`FtsEnt`] which is **invalidated
/// by the next call** (path buffer is reused).  Returns null on
/// end-of-traversal (errno = 0) or error (errno set).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_read(ftsp: *mut Fts) -> *mut FtsEnt {
    let Some(idx) = handle_to_idx(ftsp) else {
        errno::set_errno(errno::EBADF);
        return core::ptr::null_mut();
    };
    let mut out: *mut FtsEnt = core::ptr::null_mut();
    let _ = with_instance(idx, |inst| {
        out = fts_read_inst(inst);
    });
    out
}

/// Inner driver — operates on the borrowed instance.  Walks the state
/// machine until it produces the next entry to yield (or finishes).
fn fts_read_inst(inst: &mut Instance) -> *mut FtsEnt {
    if inst.finished {
        errno::set_errno(0);
        return core::ptr::null_mut();
    }

    // Handle pending FTS_AGAIN — re-yield current without advancing.
    if inst.pending_again {
        inst.pending_again = false;
        inst.current.fts_instr = FTS_NOINSTR;
        return core::ptr::addr_of_mut!(inst.current);
    }

    // If the previous entry was an FTS_D, decide whether to descend.
    let prev_info = inst.current.fts_info;
    let prev_instr = inst.current.fts_instr;
    inst.current.fts_instr = FTS_NOINSTR;
    if prev_info == FTS_D && prev_instr != FTS_SKIP {
        // We are about to descend into the directory just yielded.  Both
        // ways that can fail re-yield *the same entry*, re-typed FTS_DNR
        // with an errno — which is what glibc's `fts_read` does when
        // `fts_build` returns NULL, and the only shape that makes the
        // failure visible at all: `current` is the one entry record, so
        // marking it and then stepping on would overwrite the mark before
        // the caller ever saw it.  A caller that ignores FTS_DNR gets
        // BSD's old behaviour; one that checks it (as `find` does) can
        // print the "Permission denied" it should always have printed.
        if let Some(e) = descent_refusal(inst.depth) {
            inst.current.fts_info = FTS_DNR;
            inst.current.fts_errno = e;
            return core::ptr::addr_of_mut!(inst.current);
        }
        if let Err(e) = descend_into_current(inst) {
            inst.current.fts_info = FTS_DNR;
            inst.current.fts_errno = e;
            return core::ptr::addr_of_mut!(inst.current);
        }
        // A frame was pushed.  Fall through to the main step, which
        // yields the first child (or pops straight back to DP).
    } else if prev_info == FTS_D && prev_instr == FTS_SKIP {
        // User said skip — yield FTS_DP immediately and don't push.
        return emit_post_for_current(inst);
    }

    // Drive the state machine.
    step(inst)
}

/// Whether a descent from `depth` must be refused, and with what `errno`.
///
/// A named function rather than an `if` at the one call site because the
/// choice of `errno` *is* the decision, and it is the part a reader will
/// want to argue with: running out of traversal stack says nothing about
/// the directory, so reporting `EACCES` would send someone to check
/// permissions that are fine.  `ENOMEM` is what BSD's `fts_build` reports
/// when it cannot get the memory it needs, which is the same shape of
/// answer.
fn descent_refusal(depth: usize) -> Option<i32> {
    if depth >= MAX_FTS_DEPTH {
        Some(errno::ENOMEM)
    } else {
        None
    }
}

/// Push a frame for the directory described by `current`, opening the
/// stream the frame will read its children from.  Nothing is read here:
/// the frame holds the stream for as long as it is on the stack, and
/// `next_child` pulls one entry at a time.
///
/// `Err(errno)` means nothing was pushed and the traversal is still at
/// the parent's level; the caller decides what the user sees.  It does
/// *not* touch `current` itself — the failure has to be reported by
/// returning `current`, not by marking it and stepping past it, and only
/// the caller is in a position to return.
fn descend_into_current(inst: &mut Instance) -> Result<(), i32> {
    let frame_idx = inst.depth;
    let mut frame = DIR_FRAME_INIT;
    frame.parent_path_len = inst.path_len as u16;
    frame.yielded_pre = true;
    frame.saved_stat = inst.statbuf;

    // SAFETY: inst.path is NUL-terminated at `path_len`.
    let path_ptr = inst.path.as_ptr();
    open_frame(path_ptr, &mut frame)?;
    inst.stack[frame_idx] = frame;
    inst.depth = inst.depth.wrapping_add(1);
    Ok(())
}

/// Yield the FTS_DP that pairs with the most recently popped FTS_D.
/// Caller invokes when prev was FTS_D + SKIP (we never pushed) or
/// when stepping naturally hits end-of-children.
fn emit_post_for_current(inst: &mut Instance) -> *mut FtsEnt {
    inst.current.fts_info = FTS_DP;
    inst.current.fts_errno = 0;
    inst.current.fts_instr = FTS_NOINSTR;
    core::ptr::addr_of_mut!(inst.current)
}

/// Main state-machine step.  Returns the next entry to yield, or null
/// at end-of-traversal.
fn step(inst: &mut Instance) -> *mut FtsEnt {
    // First-call special-case: yield the root.
    if !inst.root_yielded {
        inst.root_yielded = true;
        return yield_root(inst);
    }

    // If we still have an active directory, walk to its next child.
    // Each iteration either returns a child or pops the frame (also
    // returning), so this is effectively a single-step decision —
    // expressed as `if` rather than `while`.
    if inst.depth > 0 {
        let frame_idx = inst.depth.wrapping_sub(1);
        if let Some(child) = next_child(&inst.stack[frame_idx]) {
            return yield_child(inst, frame_idx, &child);
        }
        // Listing exhausted — pop and yield FTS_DP.
        return pop_and_yield_dp(inst);
    }

    // Stack is empty and we already yielded the root + its drain.
    inst.finished = true;
    errno::set_errno(0);
    core::ptr::null_mut()
}

/// Yield the root entry (first call to `fts_read`).  The root is
/// always stat'd (FTS_NOSTAT only affects descendants).
fn yield_root(inst: &mut Instance) -> *mut FtsEnt {
    // Restore path to just the root.
    let rlen = inst.root_len;
    inst.path[..rlen].copy_from_slice(&inst.root[..rlen]);
    inst.path[rlen] = 0;
    inst.path_len = rlen;

    let stat_rc = if inst.options & FTS_PHYSICAL != 0 && inst.options & FTS_COMFOLLOW == 0 {
        crate::file::lstat(inst.path.as_ptr(), &raw mut inst.statbuf)
    } else {
        crate::file::stat(inst.path.as_ptr(), &raw mut inst.statbuf)
    };

    let base_off = basename_offset(&inst.path, inst.path_len);
    inst.current.fts_path = inst.path.as_ptr();
    inst.current.fts_pathlen = inst.path_len;
    // SAFETY: base_off < path.len() per `basename_offset` contract.
    inst.current.fts_name = unsafe { inst.path.as_ptr().add(base_off) };
    inst.current.fts_namelen = inst.path_len.wrapping_sub(base_off);
    inst.current.fts_statp = core::ptr::addr_of!(inst.statbuf);
    inst.current.fts_level = 0;
    inst.current.fts_instr = FTS_NOINSTR;

    if stat_rc < 0 {
        inst.current.fts_info = FTS_NS;
        inst.current.fts_errno = errno::get_errno();
        inst.current.fts_nlink = 0;
        // Mark instance as already in-its-final-state for non-dir
        // roots: after the caller reads this NS entry, the next step
        // will see stack empty and finish.
        return core::ptr::addr_of_mut!(inst.current);
    }
    inst.current.fts_errno = 0;
    inst.current.fts_nlink = inst.statbuf.st_nlink;
    inst.current.fts_info = stat_to_info(&inst.statbuf);
    core::ptr::addr_of_mut!(inst.current)
}

/// Yield `child`, just read from the directory at stack depth
/// `frame_idx`.  Builds `path = parent_path + "/" + child_name`,
/// stats it (unless FTS_NOSTAT), and populates `current`.
fn yield_child(inst: &mut Instance, frame_idx: usize, child: &Child) -> *mut FtsEnt {
    // Restore path to the parent's prefix before appending.
    let parent_len = inst.stack[frame_idx].parent_path_len as usize;
    inst.path_len = parent_len;
    inst.path[parent_len] = 0;

    let name_len = child.name_len;
    let d_type = child.d_type;

    let Some(new_len) = append_component(&mut inst.path, parent_len, &child.name[..name_len])
    else {
        // Path overflow — yield ERR.
        inst.current.fts_path = inst.path.as_ptr();
        inst.current.fts_pathlen = parent_len;
        inst.current.fts_name = unsafe { inst.path.as_ptr().add(parent_len) };
        inst.current.fts_namelen = 0;
        inst.current.fts_statp = core::ptr::addr_of!(inst.statbuf);
        inst.current.fts_level = frame_idx.wrapping_add(1) as i32;
        inst.current.fts_info = FTS_ERR;
        inst.current.fts_errno = errno::ENAMETOOLONG;
        inst.current.fts_instr = FTS_NOINSTR;
        return core::ptr::addr_of_mut!(inst.current);
    };
    inst.path_len = new_len;

    let base_off = basename_offset(&inst.path, new_len);

    inst.current.fts_path = inst.path.as_ptr();
    inst.current.fts_pathlen = new_len;
    inst.current.fts_name = unsafe { inst.path.as_ptr().add(base_off) };
    inst.current.fts_namelen = new_len.wrapping_sub(base_off);
    inst.current.fts_level = frame_idx.wrapping_add(1) as i32;
    inst.current.fts_statp = core::ptr::addr_of!(inst.statbuf);
    inst.current.fts_instr = FTS_NOINSTR;
    inst.current.fts_errno = 0;

    if inst.options & FTS_NOSTAT != 0 {
        // No stat — classify from d_type.
        inst.statbuf = zeroed_stat();
        inst.current.fts_nlink = 0;
        let info = dtype_to_info(d_type);
        // Even with NOSTAT we still need to know if it's a directory
        // (so we can descend).  d_type is authoritative for that on
        // our filesystem; if it's UNKNOWN we treat as FTS_NSOK and
        // skip descent.
        inst.current.fts_info = if info == FTS_D {
            FTS_D
        } else if info == FTS_DEFAULT {
            FTS_NSOK
        } else {
            info
        };
        return core::ptr::addr_of_mut!(inst.current);
    }

    // Stat the child.  For FTS_PHYSICAL use lstat; for FTS_LOGICAL
    // use stat (follows symlinks).
    let stat_rc = if inst.options & FTS_PHYSICAL != 0 {
        crate::file::lstat(inst.path.as_ptr(), &raw mut inst.statbuf)
    } else {
        crate::file::stat(inst.path.as_ptr(), &raw mut inst.statbuf)
    };
    if stat_rc < 0 {
        let saved = errno::get_errno();
        inst.statbuf = zeroed_stat();
        inst.current.fts_nlink = 0;
        inst.current.fts_errno = saved;
        // Distinguish dangling symlink (FTS_SLNONE) from generic NS.
        if d_type == crate::dirent::DT_LNK {
            inst.current.fts_info = FTS_SLNONE;
        } else {
            inst.current.fts_info = FTS_NS;
        }
        return core::ptr::addr_of_mut!(inst.current);
    }

    inst.current.fts_nlink = inst.statbuf.st_nlink;
    inst.current.fts_info = stat_to_info(&inst.statbuf);
    core::ptr::addr_of_mut!(inst.current)
}

/// Pop the top frame and yield FTS_DP for it.
fn pop_and_yield_dp(inst: &mut Instance) -> *mut FtsEnt {
    let frame_idx = inst.depth.wrapping_sub(1);
    let parent_len = inst.stack[frame_idx].parent_path_len as usize;
    let saved = inst.stack[frame_idx].saved_stat;
    // The frame owns its stream; popping is where that ownership ends.
    // Not doing this here is not merely a leak of one pool slot -- it is
    // a slot leaked per directory *visited*, so a `rm -rf` of a wide
    // tree would run the pool dry partway through.
    close_frame(&mut inst.stack[frame_idx]);
    inst.depth = frame_idx;

    // Restore statbuf to the snapshot taken at descent.
    inst.statbuf = saved;
    inst.current.fts_nlink = saved.st_nlink;
    inst.current.fts_statp = core::ptr::addr_of!(inst.statbuf);

    // `path` currently still has the last child appended; rewind it
    // to just the directory itself.  But the directory's *full* path
    // is `path[..something]` — specifically, parent_len was the path
    // length BEFORE the directory's basename was appended (set in
    // descend_into_current).  We need the path length AT the time
    // we yielded the FTS_D, which is the directory's own length.
    //
    // To recover that, find the byte after the parent's slash where
    // the directory's name lives.  Since we always have just-pushed
    // = "last yielded FTS_D" + appended children, restoring to the
    // directory path means truncating path back to where it was when
    // we descended (i.e., the parent length + "/" + dir_basename).
    //
    // Simpler: when we descended, `inst.path_len` was the directory's
    // own length, and we saved it implicitly because frame.parent_path
    // _len was the *parent's* length.  We didn't store the directory's
    // own length explicitly.  Reconstruct: dir_path = path[..first
    // slash before end > parent_len].  But we may have descended
    // deeper; need to walk back to depth-`frame_idx`'s path.
    //
    // Easier: when we pop, we know the directory we're DP'ing had
    // path = `path[..end_of_dir_component]`.  Since the next-deeper
    // frame would have parent_path_len = dir's length, but we just
    // popped so there's no deeper frame.  So: scan path forward from
    // parent_len to the next '/' or end.
    let mut dir_end = parent_len;
    // Skip leading slash if present (when parent_len points at '/').
    if dir_end < inst.path_len && inst.path[dir_end] == b'/' {
        dir_end = dir_end.wrapping_add(1);
    }
    while dir_end < inst.path_len && inst.path[dir_end] != b'/' {
        dir_end = dir_end.wrapping_add(1);
    }
    inst.path_len = dir_end;
    inst.path[dir_end] = 0;

    let base_off = basename_offset(&inst.path, inst.path_len);
    inst.current.fts_path = inst.path.as_ptr();
    inst.current.fts_pathlen = inst.path_len;
    inst.current.fts_name = unsafe { inst.path.as_ptr().add(base_off) };
    inst.current.fts_namelen = inst.path_len.wrapping_sub(base_off);
    inst.current.fts_level = frame_idx as i32;
    inst.current.fts_info = FTS_DP;
    inst.current.fts_errno = 0;
    inst.current.fts_instr = FTS_NOINSTR;
    core::ptr::addr_of_mut!(inst.current)
}

/// Return the children of the current directory as a singly-linked
/// list.  Our implementation returns a non-functional stub: null with
/// errno ENOSYS.
///
/// Real `fts_children` hands back every sibling at once, which needs an
/// `FtsEnt` per child alive simultaneously — and this module allocates
/// nothing: an entry's `fts_path` points into the one path buffer the
/// instance mutates as it walks, so two live entries would name the
/// same bytes.  Supporting it means a per-child pool, and the callers
/// that matter here (`find`, `rm -rf`, `du`, `chmod -R`) drive
/// `fts_read` instead.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_children(_ftsp: *mut Fts, _instr: i32) -> *mut FtsEnt {
    errno::set_errno(errno::ENOSYS);
    core::ptr::null_mut()
}

/// Set an instruction for the entry `f`.  Supported instructions:
/// [`FTS_SKIP`], [`FTS_AGAIN`], [`FTS_FOLLOW`].
///
/// Returns 0 on success, -1 with errno=EINVAL on bad instruction.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_set(ftsp: *mut Fts, f: *mut FtsEnt, instr: i32) -> i32 {
    let Some(idx) = handle_to_idx(ftsp) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if f.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !matches!(instr, FTS_SKIP | FTS_AGAIN | FTS_FOLLOW | FTS_NOINSTR) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let mut rc = 0;
    let _ = with_instance(idx, |inst| {
        // f should be `&inst.current` — we don't enforce this strictly,
        // since the user is expected to have just received it from
        // fts_read.  Apply the instruction to the canonical current.
        match instr {
            FTS_SKIP => {
                // Will be consulted on the next fts_read call.
                inst.current.fts_instr = FTS_SKIP;
            }
            FTS_AGAIN => {
                inst.pending_again = true;
                inst.current.fts_instr = FTS_AGAIN;
            }
            FTS_FOLLOW => {
                // No-op in our impl (we don't transparently re-stat).
                inst.current.fts_instr = FTS_FOLLOW;
            }
            _ => {
                // FTS_NOINSTR or any unknown instruction — clear back
                // to NOINSTR.
                inst.current.fts_instr = FTS_NOINSTR;
            }
        }
        rc = 0;
    });
    rc
}

/// Close an FTS stream and release its resources.
///
/// Returns 0 on success, -1 with errno=EBADF on an invalid handle.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fts_close(ftsp: *mut Fts) -> i32 {
    let Some(idx) = handle_to_idx(ftsp) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    let mut found = false;
    let _ = with_instance(idx, |_| {
        found = true;
    });
    if !found {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    release_instance(idx);
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Option constants -- ------------------------------------------------

    #[test]
    fn test_option_constants() {
        assert_eq!(FTS_COMFOLLOW, 0x0001);
        assert_eq!(FTS_LOGICAL, 0x0002);
        assert_eq!(FTS_NOCHDIR, 0x0004);
        assert_eq!(FTS_NOSTAT, 0x0008);
        assert_eq!(FTS_PHYSICAL, 0x0010);
        assert_eq!(FTS_SEEDOT, 0x0020);
        assert_eq!(FTS_XDEV, 0x0040);
    }

    #[test]
    fn test_options_powers_of_two() {
        let opts = [
            FTS_COMFOLLOW,
            FTS_LOGICAL,
            FTS_NOCHDIR,
            FTS_NOSTAT,
            FTS_PHYSICAL,
            FTS_SEEDOT,
            FTS_XDEV,
        ];
        for &o in &opts {
            assert!(o > 0);
            assert_eq!(o & (o - 1), 0);
        }
    }

    // -- Info constants -- --------------------------------------------------

    #[test]
    fn test_info_constants() {
        assert_eq!(FTS_D, 1);
        assert_eq!(FTS_DC, 2);
        assert_eq!(FTS_DEFAULT, 3);
        assert_eq!(FTS_DNR, 4);
        assert_eq!(FTS_DOT, 5);
        assert_eq!(FTS_DP, 6);
        assert_eq!(FTS_ERR, 7);
        assert_eq!(FTS_F, 8);
        assert_eq!(FTS_INIT, 9);
        assert_eq!(FTS_NS, 10);
        assert_eq!(FTS_NSOK, 11);
        assert_eq!(FTS_SL, 12);
        assert_eq!(FTS_SLNONE, 13);
        assert_eq!(FTS_W, 14);
    }

    #[test]
    fn test_info_constants_distinct() {
        let infos = [
            FTS_D,
            FTS_DC,
            FTS_DEFAULT,
            FTS_DNR,
            FTS_DOT,
            FTS_DP,
            FTS_ERR,
            FTS_F,
            FTS_INIT,
            FTS_NS,
            FTS_NSOK,
            FTS_SL,
            FTS_SLNONE,
            FTS_W,
        ];
        for i in 0..infos.len() {
            for j in (i + 1)..infos.len() {
                assert_ne!(infos[i], infos[j]);
            }
        }
    }

    // -- Instruction constants -- -------------------------------------------

    #[test]
    fn test_instruction_constants() {
        assert_eq!(FTS_FOLLOW, 1);
        assert_eq!(FTS_AGAIN, 2);
        assert_eq!(FTS_SKIP, 3);
        assert_eq!(FTS_NOINSTR, 4);
    }

    // -- Struct shapes -- ---------------------------------------------------

    #[test]
    fn test_fts_struct_nonzero_size() {
        assert!(core::mem::size_of::<Fts>() > 0);
    }

    #[test]
    fn test_ftsent_struct_nonzero_size() {
        assert!(core::mem::size_of::<FtsEnt>() > 0);
    }

    // -- fts_open argument validation -- ------------------------------------

    #[test]
    fn test_fts_open_null_argv_efault() {
        errno::set_errno(0);
        let r = fts_open(core::ptr::null(), FTS_PHYSICAL, None);
        assert!(r.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_fts_open_no_traversal_mode_einval() {
        // Must specify exactly one of PHYSICAL / LOGICAL.
        let path = b"/\0".as_ptr();
        let argv: [*const u8; 2] = [path, core::ptr::null()];
        errno::set_errno(0);
        let r = fts_open(argv.as_ptr(), 0, None);
        assert!(r.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_fts_open_both_traversal_modes_einval() {
        let path = b"/\0".as_ptr();
        let argv: [*const u8; 2] = [path, core::ptr::null()];
        errno::set_errno(0);
        let r = fts_open(argv.as_ptr(), FTS_PHYSICAL | FTS_LOGICAL, None);
        assert!(r.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_fts_open_empty_argv_einval() {
        let argv: [*const u8; 1] = [core::ptr::null()];
        errno::set_errno(0);
        let r = fts_open(argv.as_ptr(), FTS_PHYSICAL, None);
        assert!(r.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- Open + close round-trip -- -----------------------------------------

    // The instance table is a `process_global!`, so it is this thread's alone:
    // a concurrently-running test cannot consume a slot out from under us and
    // the open must therefore succeed outright.  This used to tolerate a null
    // return "because parallel tests may have taken the slots", which masked
    // the shared-table bug it should have caught
    // (`B-FTS-INSTANCE-POOL-CLAIM-IS-A-DATA-RACE`).
    #[test]
    fn test_fts_open_close_roundtrip() {
        let path = b"/\0".as_ptr();
        let argv: [*const u8; 2] = [path, core::ptr::null()];
        let r = fts_open(argv.as_ptr(), FTS_PHYSICAL, None);
        assert!(!r.is_null(), "this thread's instance pool must be free");
        assert_eq!(fts_close(r), 0);
        // The slot must go back to the pool, so the same open can be repeated
        // indefinitely — a release that failed to clear `in_use` would wedge
        // the pool after `MAX_FTS_INSTANCES` opens.
        for _ in 0..MAX_FTS_INSTANCES * 3 {
            let again = fts_open(argv.as_ptr(), FTS_PHYSICAL, None);
            assert!(!again.is_null(), "closed slot was not returned to the pool");
            assert_eq!(fts_close(again), 0);
        }
    }

    #[test]
    fn test_fts_close_null_returns_ebadf() {
        errno::set_errno(0);
        assert_eq!(fts_close(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- fts_read on closed/null handle -- ----------------------------------

    #[test]
    fn test_fts_read_null_returns_ebadf() {
        errno::set_errno(0);
        let r = fts_read(core::ptr::null_mut());
        assert!(r.is_null());
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- fts_set argument checks -- -----------------------------------------

    #[test]
    fn test_fts_set_null_handle_ebadf() {
        errno::set_errno(0);
        let r = fts_set(core::ptr::null_mut(), core::ptr::null_mut(), FTS_SKIP);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fts_set_null_ent_efault() {
        let path = b"/\0".as_ptr();
        let argv: [*const u8; 2] = [path, core::ptr::null()];
        let r = fts_open(argv.as_ptr(), FTS_PHYSICAL, None);
        if r.is_null() {
            return;
        }
        errno::set_errno(0);
        assert_eq!(fts_set(r, core::ptr::null_mut(), FTS_SKIP), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        fts_close(r);
    }

    #[test]
    fn test_fts_set_bad_instr_einval() {
        let path = b"/\0".as_ptr();
        let argv: [*const u8; 2] = [path, core::ptr::null()];
        let r = fts_open(argv.as_ptr(), FTS_PHYSICAL, None);
        if r.is_null() {
            return;
        }
        // We don't have a real FtsEnt yet (haven't called fts_read);
        // forge a dummy pointer.  fts_set's null check is what we're
        // exercising; with a non-null pointer + bad instr we get
        // EINVAL.
        let mut dummy = FtsEnt {
            fts_info: 0,
            fts_level: 0,
            fts_pathlen: 0,
            fts_namelen: 0,
            fts_nlink: 0,
            fts_errno: 0,
            fts_instr: 0,
            fts_statp: core::ptr::null(),
            fts_name: core::ptr::null(),
            fts_path: core::ptr::null(),
            fts_parent: core::ptr::null_mut(),
            fts_link: core::ptr::null_mut(),
            fts_number: 0,
            fts_pointer: core::ptr::null_mut(),
        };
        errno::set_errno(0);
        assert_eq!(fts_set(r, &raw mut dummy, 9999), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        fts_close(r);
    }

    // -- fts_children always ENOSYS -- --------------------------------------

    #[test]
    fn test_fts_children_enosys() {
        errno::set_errno(0);
        let r = fts_children(core::ptr::null_mut(), 0);
        assert!(r.is_null());
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- Instance pool exhaustion -- ----------------------------------------

    // Exactly `MAX_FTS_INSTANCES` opens must succeed and the next must fail
    // with ENOMEM.  The pool is per-thread (`process_global!`), so this is
    // deterministic rather than "either outcome is fine".
    #[test]
    fn test_fts_open_exhausts_pool_returns_enomem() {
        let path = b"/\0".as_ptr();
        let argv: [*const u8; 2] = [path, core::ptr::null()];
        let mut handles: [*mut Fts; MAX_FTS_INSTANCES] = [core::ptr::null_mut(); MAX_FTS_INSTANCES];
        for (i, slot) in handles.iter_mut().enumerate() {
            *slot = fts_open(argv.as_ptr(), FTS_PHYSICAL, None);
            assert!(!slot.is_null(), "open {i} of a free pool must succeed");
        }
        // Distinct streams must get distinct handles — the whole point of the
        // claim being exclusive.
        assert_ne!(handles.first(), handles.last());

        errno::set_errno(0);
        let over = fts_open(argv.as_ptr(), FTS_PHYSICAL, None);
        assert!(over.is_null(), "pool is full; open must fail");
        assert_eq!(errno::get_errno(), errno::ENOMEM);

        for h in handles.iter() {
            assert_eq!(fts_close(*h), 0);
        }
    }

    // -- handle_to_idx helper -- --------------------------------------------

    #[test]
    fn test_handle_to_idx_null() {
        assert!(handle_to_idx(core::ptr::null()).is_none());
    }

    // -- basename_offset helper -- ------------------------------------------

    #[test]
    fn test_basename_offset_root() {
        let p = b"/\0";
        // For "/" the basename starts after the '/' at offset 1.
        assert_eq!(basename_offset(&pad(p), 1), 1);
    }

    #[test]
    fn test_basename_offset_simple() {
        let p = b"/etc/passwd\0";
        // Last '/' is at index 4; basename starts at 5.
        assert_eq!(basename_offset(&pad(p), 11), 5);
    }

    #[test]
    fn test_basename_offset_no_slash() {
        let p = b"file.txt\0";
        assert_eq!(basename_offset(&pad(p), 8), 0);
    }

    fn pad(s: &[u8]) -> [u8; PATH_MAX] {
        let mut out = [0u8; PATH_MAX];
        out[..s.len()].copy_from_slice(s);
        out
    }

    // -- append_component helper -- -----------------------------------------

    #[test]
    fn test_append_component_no_trailing_slash() {
        let mut buf = [0u8; PATH_MAX];
        let parent = b"/etc";
        buf[..parent.len()].copy_from_slice(parent);
        let new = append_component(&mut buf, parent.len(), b"passwd").unwrap();
        assert_eq!(new, b"/etc/passwd".len());
        assert_eq!(&buf[..new], b"/etc/passwd");
        assert_eq!(buf[new], 0);
    }

    #[test]
    fn test_append_component_trailing_slash() {
        let mut buf = [0u8; PATH_MAX];
        let parent = b"/etc/";
        buf[..parent.len()].copy_from_slice(parent);
        let new = append_component(&mut buf, parent.len(), b"passwd").unwrap();
        assert_eq!(&buf[..new], b"/etc/passwd");
    }

    #[test]
    fn test_append_component_empty_parent() {
        let mut buf = [0u8; PATH_MAX];
        let new = append_component(&mut buf, 0, b"foo").unwrap();
        assert_eq!(&buf[..new], b"foo");
    }

    #[test]
    fn test_append_component_overflow() {
        let mut buf = [0u8; PATH_MAX];
        let parent_len = PATH_MAX - 3;
        for slot in buf[..parent_len].iter_mut() {
            *slot = b'a';
        }
        // Appending "/bb" makes parent_len + 1 + 2 + NUL > PATH_MAX.
        assert!(append_component(&mut buf, parent_len, b"bb").is_none());
    }

    // -- dtype_to_info -- ---------------------------------------------------

    #[test]
    fn test_dtype_to_info_mappings() {
        assert_eq!(dtype_to_info(crate::dirent::DT_REG), FTS_F);
        assert_eq!(dtype_to_info(crate::dirent::DT_DIR), FTS_D);
        assert_eq!(dtype_to_info(crate::dirent::DT_LNK), FTS_SL);
        assert_eq!(dtype_to_info(crate::dirent::DT_UNKNOWN), FTS_DEFAULT);
    }

    // -- is_dot_or_dotdot -- ------------------------------------------------

    #[test]
    fn test_is_dot_or_dotdot() {
        assert!(is_dot_or_dotdot(b".\0".as_ptr(), 1));
        assert!(is_dot_or_dotdot(b"..\0".as_ptr(), 2));
        assert!(!is_dot_or_dotdot(b"a\0".as_ptr(), 1));
        assert!(!is_dot_or_dotdot(b"..a\0".as_ptr(), 3));
    }

    // -- Streams held per level -- ------------------------------------------
    //
    // A frame used to copy 64 children out of its directory and close the
    // stream; the 65th entry was dropped, silently, from every `find`,
    // `rm -rf` and `du` that walked the tree. A frame now holds the stream
    // and reads on demand. None of that can be exercised against a real
    // directory from a host test -- `opendir` here goes through SlateOS
    // syscalls -- so what is pinned below is the set of facts that made the
    // change safe, each of which is a thing a later edit could silently take
    // back.

    #[test]
    fn a_traversal_at_full_depth_cannot_exhaust_the_dir_pool() {
        // The `const _` assertion beside MAX_FTS_DEPTH says this at build
        // time; restated here because a build error names a file and a test
        // names the consequence. Every level holds one stream for as long as
        // it is on the stack, and two FTS streams may be open at once, so
        // the pool has to cover the product -- otherwise a deep walk starts
        // reporting FTS_DNR/EMFILE partway down, which reads like a
        // permissions problem and is not one.
        assert!(MAX_FTS_INSTANCES * MAX_FTS_DEPTH <= crate::dirent::MAX_OPEN_DIRS);
    }

    #[test]
    fn every_name_readdir_can_produce_fits_a_frame() {
        // The component buffer was 64 bytes and anything longer was skipped
        // with no entry and no error -- the same silent loss as the 64-child
        // cap, one directory deeper. Matching `d_name` exactly is what makes
        // the skip unreachable rather than merely unlikely.
        assert!(FTS_NAME_MAX >= crate::dirent::DIRENT_NAME_MAX);
    }

    #[test]
    fn a_fresh_frame_owns_no_stream() {
        // `DIR_FRAME_INIT` is what `INSTANCE_INIT` fills the stack with, so a
        // non-null `dir` here would mean every newly allocated instance
        // believed it owned eight streams it had never opened -- and would
        // close them.
        assert!(DIR_FRAME_INIT.dir.is_null());
    }

    #[test]
    fn closing_a_frame_twice_does_not_close_twice() {
        // Idempotence is not tidiness here: `fts_close` on a partly-walked
        // tree runs `close_open_frames` over frames the pop path may already
        // have closed, and a second `closedir` on the same pointer frees a
        // pool slot that another traversal has since been given.
        let mut frame = DIR_FRAME_INIT;
        close_frame(&mut frame);
        close_frame(&mut frame);
        assert!(frame.dir.is_null());
    }

    #[test]
    fn a_frame_with_no_stream_yields_no_children() {
        // The null check in `next_child` is what stops a failed descent --
        // where `open_frame` returned Err and left `dir` null -- from being
        // read as a directory with entries.
        let frame = DIR_FRAME_INIT;
        assert!(next_child(&frame).is_none());
    }

    #[test]
    fn closing_the_open_frames_empties_the_stack() {
        let mut inst = INSTANCE_INIT;
        inst.depth = MAX_FTS_DEPTH;
        close_open_frames(&mut inst);
        assert_eq!(inst.depth, 0);
        assert!(inst.stack.iter().all(|f| f.dir.is_null()));
    }

    #[test]
    fn a_frame_no_longer_carries_a_listing() {
        // The frame held `[CachedChild; 64]` -- 64 names of 64 bytes plus two
        // counters, about 4.2 KiB, eight of them per instance and two
        // instances per process. It now holds a pointer, two lengths, a flag
        // and a `Stat`. The bound is loose on purpose: it is not a budget,
        // it is a tripwire for someone reintroducing a per-directory buffer,
        // because such a buffer is a cap and a cap is a file quietly not
        // visited.
        assert!(
            core::mem::size_of::<DirFrame>() < 1024,
            "a DirFrame that large is a listing buffer wearing a frame's name"
        );
    }

    // -- A descent that does not happen is reported -- -----------------------
    //
    // Neither failure can be provoked from a host test: one needs a
    // directory `opendir` refuses, the other needs nine real levels. What
    // is checkable is the shape both rely on -- that `descend_into_current`
    // reports rather than repairs, and that the entry the caller re-reads
    // is the same record it just saw, re-typed.

    #[test]
    fn a_failed_descent_pushes_no_frame() {
        // The Err path used to fall through to `step`, which read the
        // parent's next child and overwrote the FTS_DNR mark before the
        // caller could see it -- so an unreadable directory was skipped as
        // silently as an over-full one. What makes the caller's `return`
        // correct is that a failed descent leaves the stack exactly as it
        // was: same depth, no half-pushed frame to trip over later.
        let mut inst = INSTANCE_INIT;
        inst.path[0] = 0;
        inst.path_len = 0;
        let before = inst.depth;
        // An empty path is not a directory, so `opendir` must refuse it.
        // (On the host this goes through the SlateOS syscall shims and
        // fails; the point of the assert is the *stack*, not the errno.)
        let r = descend_into_current(&mut inst);
        assert!(r.is_err(), "opendir(\"\") must not succeed");
        assert_eq!(inst.depth, before);
        assert!(inst.stack.iter().all(|f| f.dir.is_null()));
    }

    #[test]
    fn a_descent_within_the_depth_limit_is_not_refused() {
        assert!(descent_refusal(0).is_none());
        assert!(descent_refusal(MAX_FTS_DEPTH - 1).is_none());
    }

    #[test]
    fn the_depth_limit_is_a_resource_error_not_an_access_error() {
        // ENOMEM rather than EACCES, because nothing is wrong with the
        // directory -- we ran out of traversal stack. A caller that maps
        // errno to a message would otherwise tell the user to check
        // permissions on a directory whose permissions are fine.
        assert_eq!(descent_refusal(MAX_FTS_DEPTH), Some(errno::ENOMEM));
        assert_ne!(errno::ENOMEM, errno::EACCES);
    }
}
