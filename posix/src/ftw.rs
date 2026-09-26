//! POSIX file tree walk (`<ftw.h>`).
//!
//! Provides `ftw` and `nftw` for recursively traversing directory trees.
//! Each file/directory visited triggers a user-supplied callback.
//!
//! ## Implementation
//!
//! glibc 2.39's io/ftw.c, ported onto our dirent module.  Both entry points
//! drive one walker ([`Walker`]), differing only in how an entry is
//! delivered; the walker reaches the filesystem through [`Fs`], so the host
//! tests can give it a tree of their own.
//!
//! **Descriptors, and no depth limit.**  glibc keeps at most `nopenfd`
//! directory streams open; when a new level needs one and all are in use,
//! it reads the rest of the oldest stream's names into memory and closes it
//! (`open_dir_stream`).  So `nopenfd` bounds descriptors, never depth.  Our
//! [`Dir`](crate::dirent::Dir) is a snapshot taken at `opendir`, so reading
//! its names costs nothing that holding it open would not: every level reads
//! its names ([`Names`]) and closes its stream before the walk descends.  The
//! walk holds one descriptor at a time at any depth, which honours every
//! `nopenfd`; and `nopenfd < 1` is 1, as in glibc.  Until 2026-09-26 one
//! stream was held per level, so `nopenfd` was also the depth at which the
//! walk stopped, and never more than 32: `ftw(dir, fn, 1)` saw the top level
//! and called every subdirectory unreadable.
//!
//! **Each directory once.**  Unless [`FTW_PHYS`] is given, a symbolic link to
//! a directory is walked into, so a link to an ancestor would recurse forever
//! and a directory reachable by two names would be walked twice.  glibc
//! records every directory it enters by `(st_dev, st_ino)` and skips —
//! without a callback — one it has entered before ([`Seen`]).  A directory
//! whose `st_ino` is 0 (a filesystem with no stable identity to report,
//! design-decisions.md §740) is not recorded, since every such directory
//! would otherwise be taken for the first; a cycle through such directories
//! ends at [`PATH_MAX`] with `ENAMETOOLONG`.
//!
//! **What is reported, and what ends the walk**, as glibc:
//!
//! - A child `stat` refuses with `EACCES` or `ENOENT` is [`FTW_NS`] — or,
//!   without [`FTW_PHYS`], [`FTW_SLN`] if it is a dangling link.  Any other
//!   `stat` failure (`ELOOP`, say) ends the walk with -1 and that `errno`.
//! - A directory `opendir` refuses with `EACCES` is [`FTW_DNR`].  Any other
//!   failure ends the walk with -1.
//! - A child whose path would not fit [`PATH_MAX`] ends it with
//!   `ENAMETOOLONG`.
//! - A root that cannot be `stat`ed ends it before any callback, unless it is
//!   a dangling link ([`FTW_SLN`]; [`FTW_NS`] for `ftw`).  An empty root is
//!   `ENOENT`; trailing slashes are stripped from the root's path.
//!
//! ## Limitations
//!
//! - Maximum path length is 4096 bytes.  glibc grows its buffer and `stat`s
//!   relative to the parent's descriptor, so it has no such limit.
//! - [`FTW_MOUNT`] and [`FTW_CHDIR`] are **rejected** with `EINVAL`
//!   rather than accepted and ignored — see [`UNSUPPORTED_NFTW_FLAGS`].

use crate::errno;
use crate::fcntl::{S_IFDIR, S_IFLNK, S_IFMT};
use crate::stat::Stat;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Regular file.
pub const FTW_F: i32 = 0;
/// Directory.
pub const FTW_D: i32 = 1;
/// Unreadable directory.
pub const FTW_DNR: i32 = 2;
/// `stat` failed (not a symlink).
pub const FTW_NS: i32 = 3;
/// Symbolic link (nftw only, with FTW_PHYS).
pub const FTW_SL: i32 = 4;
/// Directory, all children processed (nftw with FTW_DEPTH).
pub const FTW_DP: i32 = 5;
/// Symbolic link pointing to nonexistent file (nftw only, without FTW_PHYS).
pub const FTW_SLN: i32 = 6;

/// `nftw` flag: do not follow symbolic links.
pub const FTW_PHYS: i32 = 1;
/// `nftw` flag: stay on the same filesystem.
pub const FTW_MOUNT: i32 = 2;
/// `nftw` flag: change to each directory before reading it.
pub const FTW_CHDIR: i32 = 4;
/// `nftw` flag: do a depth-first search (call callback after children).
pub const FTW_DEPTH: i32 = 8;
/// `nftw` flag (GNU): the callback's result is an action — [`FTW_CONTINUE`],
/// [`FTW_STOP`], [`FTW_SKIP_SUBTREE`] or [`FTW_SKIP_SIBLINGS`] — rather than
/// "zero to go on, anything else to stop".
pub const FTW_ACTIONRETVAL: i32 = 16;

/// [`FTW_ACTIONRETVAL`]: carry on.
pub const FTW_CONTINUE: i32 = 0;
/// [`FTW_ACTIONRETVAL`]: stop the walk, which returns this.
pub const FTW_STOP: i32 = 1;
/// [`FTW_ACTIONRETVAL`], from an [`FTW_D`] callback: do not enter this
/// directory; carry on with the next entry.
pub const FTW_SKIP_SUBTREE: i32 = 2;
/// [`FTW_ACTIONRETVAL`]: skip the rest of this entry's directory; carry on
/// with the directory's parent.
pub const FTW_SKIP_SIBLINGS: i32 = 3;

/// Every flag `nftw` knows.  glibc 2.39's `nftw` (the `GLIBC_2.3.3` symbol)
/// refuses any other bit with `EINVAL`.
const KNOWN_NFTW_FLAGS: i32 = FTW_PHYS | FTW_MOUNT | FTW_CHDIR | FTW_DEPTH | FTW_ACTIONRETVAL;

/// Extra info passed to the `nftw` callback.
#[repr(C)]
pub struct FTW {
    /// Offset of the filename in the pathname.
    pub base: i32,
    /// Depth of this entry relative to the starting path.
    pub level: i32,
}

/// Maximum path length.
const PATH_MAX: usize = 4096;

// ---------------------------------------------------------------------------
// The filesystem, as the walk sees it
// ---------------------------------------------------------------------------

/// What the walk asks of the filesystem.  [`Kernel`] is the real one; the
/// host tests supply a tree of their own, because on the host every path is
/// missing and the walk could not otherwise be exercised at all.
trait Fs {
    /// `stat` of the NUL-terminated `path` — `lstat` when `follow` is false
    /// — or `Err(errno)`.
    fn stat(&mut self, path: *const u8, follow: bool) -> Result<Stat, i32>;

    /// Every name in the directory `path` but `.` and `..`, into `names`;
    /// `Err(errno)` if it cannot be opened, or memory runs out.
    fn list(&mut self, path: *const u8, names: &mut Names) -> Result<(), i32>;
}

/// The filesystem, through this libc's `stat`, `lstat` and `opendir`.
struct Kernel;

impl Fs for Kernel {
    fn stat(&mut self, path: *const u8, follow: bool) -> Result<Stat, i32> {
        // SAFETY: `Stat` is a plain C struct, for which all zeroes is a value.
        let mut sb: Stat = unsafe { core::mem::zeroed() };
        let rc = if follow {
            crate::file::stat(path, &raw mut sb)
        } else {
            crate::file::lstat(path, &raw mut sb)
        };
        if rc < 0 {
            Err(errno::get_errno())
        } else {
            Ok(sb)
        }
    }

    fn list(&mut self, path: *const u8, names: &mut Names) -> Result<(), i32> {
        let dir = crate::dirent::opendir(path);
        if dir.is_null() {
            return Err(errno::get_errno());
        }
        let mut result = Ok(());
        loop {
            let ent = crate::dirent::readdir(dir);
            if ent.is_null() {
                // End of listing.  `readdir` reports errors the same way, but
                // a `Dir` is a snapshot taken at `opendir`, so there is no
                // read left to fail.
                break;
            }
            // SAFETY: `readdir` returned a live entry, whose `d_name` is a
            // NUL-terminated name inside it.
            let name = unsafe { core::ptr::addr_of!((*ent).d_name).cast::<u8>() };
            if is_dot_or_dotdot(name) {
                continue;
            }
            // SAFETY: as above; the name's `len` bytes precede its NUL.
            let bytes = unsafe { core::slice::from_raw_parts(name, crate::string::strlen(name)) };
            if !names.push(bytes) {
                result = Err(errno::ENOMEM);
                break;
            }
        }
        // A failed close changes nothing the walk reports: the names are
        // already read, and the stream is gone either way.
        crate::dirent::closedir(dir);
        result
    }
}

/// The names in one directory, read out of its stream so that the stream can
/// be closed before the walk descends — glibc's `content` list.  Each name is
/// NUL-terminated, back to back, in a `malloc` buffer this owns.
struct Names {
    buf: *mut u8,
    len: usize,
    cap: usize,
}

impl Names {
    const fn new() -> Self {
        Self {
            buf: core::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// Append `name` and a NUL; `false` if memory ran out.
    fn push(&mut self, name: &[u8]) -> bool {
        let Some(end) = self.len.checked_add(name.len()) else {
            return false;
        };
        let Some(need) = end.checked_add(1) else {
            return false;
        };
        if need > self.cap {
            let grown = need.max(self.cap.saturating_mul(2)).max(256);
            // SAFETY: `buf` is null or this list's own allocation.
            let new = unsafe { crate::malloc::realloc(self.buf, grown) };
            if new.is_null() {
                return false;
            }
            self.buf = new;
            self.cap = grown;
        }
        // SAFETY: `need <= cap`, so `len..need` is inside the allocation,
        // which `name` — the caller's — does not overlap.
        unsafe {
            core::ptr::copy_nonoverlapping(name.as_ptr(), self.buf.add(self.len), name.len());
            *self.buf.add(end) = 0;
        }
        self.len = need;
        true
    }

    /// The name that starts at byte `at`, and where the one after it starts;
    /// `None` past the last.
    fn at(&self, at: usize) -> Option<(*const u8, usize)> {
        if at >= self.len {
            return None;
        }
        // SAFETY: `at < len`, and every name before `len` ends in a NUL
        // inside the allocation, so `at` starts one and `strlen` stays in it.
        let (name, n) = unsafe {
            let name = self.buf.add(at);
            (name.cast_const(), crate::string::strlen(name))
        };
        Some((name, at.saturating_add(n).saturating_add(1)))
    }
}

impl Drop for Names {
    fn drop(&mut self) {
        if !self.buf.is_null() {
            // SAFETY: `buf` is this list's own allocation, and this is the
            // only place it is freed.
            unsafe { crate::malloc::free(self.buf) };
        }
    }
}

/// One directory's identity: its device and inode.
#[derive(Clone, Copy, PartialEq, Eq)]
struct DirId {
    dev: u64,
    ino: u64,
}

/// The directories a walk that follows links has entered — glibc's
/// `known_objects`.  Open addressing in a `malloc` table kept at most half
/// full; a slot is empty when its inode is 0, which is one reason an inode of
/// 0 is never recorded (the other is in the module docs).  Nothing is ever
/// removed, as in glibc: a directory is walked once per walk, not once per
/// path to it.
struct Seen {
    slots: *mut DirId,
    /// A power of two, or 0 before the first insertion.
    cap: usize,
    len: usize,
}

impl Seen {
    const fn new() -> Self {
        Self {
            slots: core::ptr::null_mut(),
            cap: 0,
            len: 0,
        }
    }

    /// Record `id`.  `Ok(true)` if it had not been recorded — the directory
    /// is to be walked — `Ok(false)` if it had, `Err(ENOMEM)` if the table
    /// could not grow.  An inode of 0 is always `Ok(true)`, and not recorded.
    fn insert(&mut self, id: DirId) -> Result<bool, i32> {
        if id.ino == 0 {
            return Ok(true);
        }
        if self.len.saturating_mul(2) >= self.cap {
            self.grow()?;
        }
        // SAFETY: `slots` holds `cap` initialised slots, less than half full.
        let placed = unsafe { Self::place(self.slots, self.cap, id) };
        if placed {
            self.len = self.len.saturating_add(1);
        }
        Ok(placed)
    }

    /// Double the table (or make its first 64 slots) and re-place every entry.
    fn grow(&mut self) -> Result<(), i32> {
        let cap = if self.cap == 0 {
            64
        } else {
            self.cap.checked_mul(2).ok_or(errno::ENOMEM)?
        };
        // `calloc` checks `cap * size` for overflow, and zero is an empty slot.
        let slots = crate::malloc::calloc(cap, core::mem::size_of::<DirId>()).cast::<DirId>();
        if slots.is_null() {
            return Err(errno::ENOMEM);
        }
        let mut i: usize = 0;
        while i < self.cap {
            // SAFETY: `i < self.cap`, the old table's size; `slots` is the new
            // table, of `cap` zeroed slots, at most half of which get filled.
            unsafe {
                let old = *self.slots.add(i);
                if old.ino != 0 {
                    Self::place(slots, cap, old);
                }
            }
            i = i.wrapping_add(1);
        }
        if !self.slots.is_null() {
            // SAFETY: the old table is this set's own allocation, now copied.
            unsafe { crate::malloc::free(self.slots.cast::<u8>()) };
        }
        self.slots = slots;
        self.cap = cap;
        Ok(())
    }

    /// Put `id` in `slots` unless it is there; `true` if it was put.
    ///
    /// # Safety
    ///
    /// `slots` must hold `cap` initialised slots, `cap` a power of two, with
    /// at least one empty — which keeps the probe finite.
    unsafe fn place(slots: *mut DirId, cap: usize, id: DirId) -> bool {
        let mask = cap.wrapping_sub(1);
        let h = (id.ino ^ id.dev.rotate_left(32)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut i = ((h ^ h.rotate_right(32)) as usize) & mask;
        loop {
            // SAFETY: `i <= mask < cap`.
            let slot = unsafe { &mut *slots.add(i) };
            if slot.ino == 0 {
                *slot = id;
                return true;
            }
            if *slot == id {
                return false;
            }
            i = i.wrapping_add(1) & mask;
        }
    }
}

impl Drop for Seen {
    fn drop(&mut self) {
        if !self.slots.is_null() {
            // SAFETY: the table is this set's own allocation, freed only here.
            unsafe { crate::malloc::free(self.slots.cast::<u8>()) };
        }
    }
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// What one walk was asked for, carried down the tree unchanged.
#[derive(Clone, Copy)]
struct Walk {
    /// Report a directory *after* its children ([`FTW_DEPTH`]).
    depth_first: bool,
    /// Do not follow symbolic links ([`FTW_PHYS`]).
    physical: bool,
    /// Read the callback's result as an action ([`FTW_ACTIONRETVAL`]).
    action_retval: bool,
}

/// One traversal in progress.
///
/// The path buffer lives here, **once**, and is mutated in place as the
/// walk descends and ascends — the same discipline [`crate::fts`] uses —
/// so recursion carries a few words per level rather than 4 KiB.
struct Walker<F, S> {
    /// NUL-terminated path of the entry currently being considered.
    path: [u8; PATH_MAX],
    opts: Walk,
    /// Where an entry is delivered.  Takes `(path, stat, typeflag,
    /// level)`; `nftw` builds its [`FTW`] from the level and the path,
    /// and `ftw` throws both away.
    emit: F,
    fs: S,
    /// The directories entered so far; unused under [`FTW_PHYS`].
    seen: Seen,
}

impl<F, S> Walker<F, S>
where
    F: FnMut(*const u8, *const Stat, i32, i32) -> i32,
    S: Fs,
{
    /// Deliver one entry to the caller's callback.
    fn call(&mut self, sb: &Stat, typeflag: i32, level: i32) -> i32 {
        // `as_ptr` ends the borrow of `self.path` before `self.emit` is
        // borrowed mutably, which is what lets both live on `self`.
        let p = self.path.as_ptr();
        (self.emit)(p, core::ptr::from_ref(sb), typeflag, level)
    }

    /// Record a directory about to be entered.  `Ok(false)` means it was
    /// entered before, by another name, and is to be skipped.
    fn first_visit(&mut self, sb: &Stat) -> Result<bool, i32> {
        if self.opts.physical {
            return Ok(true);
        }
        self.seen.insert(DirId {
            dev: sb.st_dev,
            ino: sb.st_ino,
        })
    }

    /// glibc's `process_entry`: the child named by `self.path[..len]`.
    ///
    /// Returns 0 when the walk should go on, the callback's non-zero value
    /// when it asked to stop, or -1 with `errno` set on a failure that ends
    /// the walk.
    fn child(&mut self, len: usize, level: i32) -> i32 {
        let p = self.path.as_ptr();
        let (sb, flag) = match self.fs.stat(p, !self.opts.physical) {
            Ok(sb) => {
                let flag = match sb.st_mode & S_IFMT {
                    S_IFDIR => FTW_D,
                    S_IFLNK => FTW_SL,
                    _ => FTW_F,
                };
                (sb, flag)
            }
            Err(e) if e != errno::EACCES && e != errno::ENOENT => {
                errno::set_errno(e);
                return -1;
            }
            Err(e) => {
                // SAFETY: `Stat` is a plain C struct; all zeroes is a value.
                let none: Stat = unsafe { core::mem::zeroed() };
                errno::set_errno(e);
                if self.opts.physical {
                    (none, FTW_NS)
                } else {
                    // The stat followed the link, so this may be a link with
                    // nothing on the far end — which POSIX gives its own flag.
                    match self.fs.stat(p, false) {
                        Ok(lb) if lb.st_mode & S_IFMT == S_IFLNK => (lb, FTW_SLN),
                        Ok(_) => (none, FTW_NS),
                        Err(e2) => {
                            errno::set_errno(e2);
                            (none, FTW_NS)
                        }
                    }
                }
            }
        };

        let ret = if flag == FTW_D {
            match self.first_visit(&sb) {
                Ok(true) => self.dir(&sb, len, level),
                // Entered before, by another name: skipped, without a
                // callback, as glibc does.
                Ok(false) => 0,
                Err(e) => {
                    errno::set_errno(e);
                    -1
                }
            }
        } else {
            self.call(&sb, flag, level)
        };
        if self.opts.action_retval && ret == FTW_SKIP_SUBTREE {
            0
        } else {
            ret
        }
    }

    /// glibc's `ftw_dir`: the directory named by `self.path[..len]`, whose
    /// `stat` is `sb`.
    fn dir(&mut self, sb: &Stat, len: usize, level: i32) -> i32 {
        // Read the names and close the stream before anything is reported,
        // let alone entered: POSIX says an unreadable directory is FTW_DNR
        // *instead of* FTW_D, and a closed stream is what lets the walk go
        // to any depth on one descriptor.
        let mut names = Names::new();
        if let Err(e) = self.fs.list(self.path.as_ptr(), &mut names) {
            errno::set_errno(e);
            return if e == errno::EACCES {
                self.call(sb, FTW_DNR, level)
            } else {
                -1
            };
        }

        if !self.opts.depth_first {
            let ret = self.call(sb, FTW_D, level);
            if ret != 0 {
                return ret;
            }
        }

        let child_level = level.saturating_add(1);
        let mut ret = 0;
        let mut at = 0;
        while let Some((name, next)) = names.at(at) {
            at = next;
            let child_len = append_component(&mut self.path, len, name);
            if child_len == 0 {
                // POSIX has no type flag for "the name is too long to
                // build", so there is no way to report this entry and keep
                // going; skipping it would turn a truncated traversal into a
                // successful one.
                errno::set_errno(errno::ENAMETOOLONG);
                ret = -1;
                break;
            }
            ret = self.child(child_len, child_level);
            // Cut the path back to this directory before the next name.
            if let Some(slot) = self.path.get_mut(len) {
                *slot = 0;
            }
            if ret != 0 {
                break;
            }
        }
        if self.opts.action_retval && ret == FTW_SKIP_SIBLINGS {
            ret = 0;
        }

        if ret == 0 && self.opts.depth_first {
            ret = self.call(sb, FTW_DP, level);
        }
        ret
    }
}

/// glibc's `ftw_startup`: seed a walker with `root` and run it.
///
/// `root` must be non-null.
fn run<F, S>(root: *const u8, opts: Walk, emit: F, fs: S) -> i32
where
    F: FnMut(*const u8, *const Stat, i32, i32) -> i32,
    S: Fs,
{
    // SAFETY: the caller checked `root` non-null; it is a C string.
    let root_len = unsafe { crate::string::strlen(root) };
    if root_len == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    if root_len >= PATH_MAX {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }
    let mut w = Walker {
        path: [0u8; PATH_MAX],
        opts,
        emit,
        fs,
        seen: Seen::new(),
    };
    // SAFETY: `root` holds `root_len` bytes before its NUL.
    let bytes = unsafe { core::slice::from_raw_parts(root, root_len) };
    let Some(dst) = w.path.get_mut(..root_len) else {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    };
    dst.copy_from_slice(bytes);
    // Strip trailing slashes, but never the first byte: "a//" is "a", and
    // "/" stays "/".
    let mut len = root_len;
    while len > 1 && w.path.get(len.wrapping_sub(1)) == Some(&b'/') {
        len = len.wrapping_sub(1);
        if let Some(slot) = w.path.get_mut(len) {
            *slot = 0;
        }
    }

    let p = w.path.as_ptr();
    let ret = match w.fs.stat(p, !opts.physical) {
        Err(e) => {
            // No callback: nothing can be said about the object -- unless it
            // is a dangling link, which has a flag of its own.
            let link = if !opts.physical && e == errno::ENOENT {
                w.fs.stat(p, false)
                    .ok()
                    .filter(|lb| lb.st_mode & S_IFMT == S_IFLNK)
            } else {
                None
            };
            errno::set_errno(e);
            match link {
                Some(lb) => w.call(&lb, FTW_SLN, 0),
                None => -1,
            }
        }
        Ok(sb) if sb.st_mode & S_IFMT == S_IFDIR => match w.first_visit(&sb) {
            Ok(_) => w.dir(&sb, len, 0),
            Err(e) => {
                errno::set_errno(e);
                -1
            }
        },
        Ok(sb) => {
            let flag = if sb.st_mode & S_IFMT == S_IFLNK {
                FTW_SL
            } else {
                FTW_F
            };
            w.call(&sb, flag, 0)
        }
    };
    if opts.action_retval && (ret == FTW_SKIP_SUBTREE || ret == FTW_SKIP_SIBLINGS) {
        0
    } else {
        ret
    }
}

// ---------------------------------------------------------------------------
// ftw
// ---------------------------------------------------------------------------

/// Callback type for `ftw`.
///
/// Parameters: (pathname, stat_buf, typeflag).
/// Return 0 to continue, non-zero to stop.
pub type FtwFn = extern "C" fn(*const u8, *const Stat, i32) -> i32;

/// The type flag an `ftw` callback sees: glibc's `ftw_arr`.  `ftw` has no
/// [`FTW_SL`], [`FTW_DP`] or [`FTW_SLN`] — they are `nftw`'s, and a caller
/// written against `ftw` will not have a case for them — so they read as the
/// nearest flag it does have.
const fn ftw_flag(flag: i32) -> i32 {
    match flag {
        FTW_SL => FTW_F,
        FTW_DP => FTW_D,
        FTW_SLN => FTW_NS,
        other => other,
    }
}

/// Walk a file tree, calling `callback` for each entry.
///
/// `nopenfd` is the most directory streams the walk may hold open at once.
/// It never holds more than one (see the module docs), so every value is
/// honoured — `nopenfd < 1` means 1, as in glibc; it was `EINVAL` until
/// 2026-09-26 — and it limits nothing else: the whole tree is walked.
///
/// Returns 0 on success, -1 with `errno` set on error, or the non-zero
/// value returned by `callback`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftw(dirpath: *const u8, callback: FtwFn, nopenfd: i32) -> i32 {
    // glibc reads `dirpath[0]` first, where a NULL faults; EFAULT is this
    // libc's substitute for the fault (design-decisions.md §303).
    if dirpath.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // See the doc comment: no value of `nopenfd` changes the walk.
    let _ = nopenfd;

    run(
        dirpath,
        Walk {
            depth_first: false,
            physical: false,
            action_retval: false,
        },
        |p, sb, flag, _level| callback(p, sb, ftw_flag(flag)),
        Kernel,
    )
}

// ---------------------------------------------------------------------------
// nftw
// ---------------------------------------------------------------------------

/// Callback type for `nftw`.
///
/// Parameters: (pathname, stat_buf, typeflag, ftwbuf).
pub type NftwFn = extern "C" fn(*const u8, *const Stat, i32, *mut FTW) -> i32;

/// Flags `nftw` cannot honour, and therefore refuses.
///
/// Accepting a flag and ignoring it is the worst of the three options: a
/// caller that passes [`FTW_CHDIR`] uses `ftwbuf->base` as a *relative*
/// filename, so ignoring it silently points every callback at the wrong
/// file; a caller that passes [`FTW_MOUNT`] is asking not to cross into
/// another filesystem, and ignoring that is how a `--one-file-system`
/// delete walks into a network mount.  Refusing is loud, is trivially
/// reversible when the flags are implemented, and cannot corrupt
/// anything.  See design-decisions.md §761.
const UNSUPPORTED_NFTW_FLAGS: i32 = FTW_MOUNT | FTW_CHDIR;

/// Walk a file tree with extended options.
///
/// Supports [`FTW_PHYS`], [`FTW_DEPTH`] and [`FTW_ACTIONRETVAL`].  A bit
/// outside [`KNOWN_NFTW_FLAGS`] is `EINVAL`, as in glibc, before the path is
/// looked at; so are [`FTW_MOUNT`] and [`FTW_CHDIR`] — see
/// [`UNSUPPORTED_NFTW_FLAGS`].  `nopenfd` is as for [`ftw`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nftw(dirpath: *const u8, callback: NftwFn, nopenfd: i32, flags: i32) -> i32 {
    if flags & !KNOWN_NFTW_FLAGS != 0 || flags & UNSUPPORTED_NFTW_FLAGS != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if dirpath.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // See `ftw`: no value of `nopenfd` changes the walk.
    let _ = nopenfd;

    run(
        dirpath,
        Walk {
            depth_first: flags & FTW_DEPTH != 0,
            physical: flags & FTW_PHYS != 0,
            action_retval: flags & FTW_ACTIONRETVAL != 0,
        },
        |p, sb, flag, level| {
            let mut info = FTW {
                base: find_basename_offset(p),
                level,
            };
            callback(p, sb, flag, &raw mut info)
        },
        Kernel,
    )
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Check if a name is "." or "..".
fn is_dot_or_dotdot(name: *const u8) -> bool {
    if name.is_null() {
        return false;
    }
    // SAFETY: name is a valid C string from readdir.
    let c0 = unsafe { *name };
    if c0 != b'.' {
        return false;
    }
    let c1 = unsafe { *name.add(1) };
    if c1 == 0 {
        return true; // "."
    }
    if c1 == b'.' {
        let c2 = unsafe { *name.add(2) };
        return c2 == 0; // ".."
    }
    false
}

/// Append `/name` to the path already in `buf[..parent_len]`.
///
/// Returns the new length, or 0 if it doesn't fit.  Writes in place
/// rather than building into a second buffer, because the caller does
/// this once per level and a per-level `[u8; PATH_MAX]` is 4 KiB of
/// stack that the recursion has to carry all the way down.
fn append_component(buf: &mut [u8; PATH_MAX], parent_len: usize, name: *const u8) -> usize {
    // SAFETY: `name` is a NUL-terminated name from `readdir`.
    let name_len = unsafe { crate::string::strlen(name) };

    // parent + "/" + name + NUL must fit.
    let needs_sep = parent_len > 0
        && buf
            .get(parent_len.wrapping_sub(1))
            .is_some_and(|b| *b != b'/');
    let sep_len: usize = usize::from(needs_sep);
    // Use checked_add to prevent usize overflow on adversarially long paths.
    let Some(total) = parent_len
        .checked_add(sep_len)
        .and_then(|s| s.checked_add(name_len))
    else {
        return 0;
    };

    if total >= PATH_MAX {
        return 0;
    }

    let mut i = parent_len;

    // Add separator if needed.
    if needs_sep {
        if let Some(slot) = buf.get_mut(i) {
            *slot = b'/';
        }
        i = i.wrapping_add(1);
    }

    // Copy name.
    let mut j: usize = 0;
    while j < name_len {
        if let Some(slot) = buf.get_mut(i) {
            // SAFETY: j < strlen(name), so this byte is inside the name.
            *slot = unsafe { *name.add(j) };
        }
        i = i.wrapping_add(1);
        j = j.wrapping_add(1);
    }

    // NUL terminate.
    if let Some(slot) = buf.get_mut(i) {
        *slot = 0;
    }

    i
}

/// Find the offset of the basename component in a path.
fn find_basename_offset(path: *const u8) -> i32 {
    if path.is_null() {
        return 0;
    }

    let len = unsafe { crate::string::strlen(path) };
    if len == 0 {
        return 0;
    }

    // Walk backwards to find the last '/'.
    let mut i = len;
    loop {
        if i == 0 {
            return 0; // No '/' found — basename is at offset 0.
        }
        i = i.wrapping_sub(1);
        if unsafe { *path.add(i) } == b'/' {
            return i.wrapping_add(1) as i32;
        }
    }
}

// ---------------------------------------------------------------------------
// LFS64 aliases — our off_t is already 64-bit
// ---------------------------------------------------------------------------

/// `ftw64` — Large File Support alias for `ftw`.
///
/// On our OS, `off_t` is always 64-bit (LP64 data model), so
/// `struct stat` and `ftw` already handle large files.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftw64(path: *const u8, callback: FtwFn, maxfds: i32) -> i32 {
    ftw(path, callback, maxfds)
}

/// `nftw64` — Large File Support alias for `nftw`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nftw64(path: *const u8, callback: NftwFn, maxfds: i32, flags: i32) -> i32 {
    nftw(path, callback, maxfds, flags)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // -- FTW type flag constants --

    #[test]
    fn test_ftw_type_flags() {
        assert_eq!(FTW_F, 0);
        assert_eq!(FTW_D, 1);
        assert_eq!(FTW_DNR, 2);
        assert_eq!(FTW_NS, 3);
        assert_eq!(FTW_SL, 4);
        assert_eq!(FTW_DP, 5);
        assert_eq!(FTW_SLN, 6);
    }

    #[test]
    fn test_ftw_flag_constants() {
        assert_eq!(FTW_PHYS, 1);
        assert_eq!(FTW_MOUNT, 2);
        assert_eq!(FTW_CHDIR, 4);
        assert_eq!(FTW_DEPTH, 8);
        assert_eq!(FTW_ACTIONRETVAL, 16);
        assert_eq!(
            (FTW_CONTINUE, FTW_STOP, FTW_SKIP_SUBTREE, FTW_SKIP_SIBLINGS),
            (0, 1, 2, 3),
            "glibc's values"
        );
    }

    #[test]
    fn test_ftw_flags_are_distinct_bits() {
        // Each flag should be a distinct power of 2.
        assert_eq!(KNOWN_NFTW_FLAGS, 31);
    }

    // -- is_dot_or_dotdot --

    #[test]
    fn test_is_dot() {
        assert!(is_dot_or_dotdot(b".\0".as_ptr()));
    }

    #[test]
    fn test_is_dotdot() {
        assert!(is_dot_or_dotdot(b"..\0".as_ptr()));
    }

    #[test]
    fn test_not_dot_regular_name() {
        assert!(!is_dot_or_dotdot(b"hello\0".as_ptr()));
    }

    #[test]
    fn test_not_dot_dotfile() {
        // ".bashrc" starts with '.' but is not "." or "..".
        assert!(!is_dot_or_dotdot(b".bashrc\0".as_ptr()));
    }

    #[test]
    fn test_not_dot_triple_dot() {
        // "..." is not "." or "..".
        assert!(!is_dot_or_dotdot(b"...\0".as_ptr()));
    }

    #[test]
    fn test_is_dot_or_dotdot_null() {
        assert!(!is_dot_or_dotdot(core::ptr::null()));
    }

    // -- find_basename_offset --

    #[test]
    fn test_basename_offset_no_slash() {
        assert_eq!(find_basename_offset(b"file.txt\0".as_ptr()), 0);
    }

    #[test]
    fn test_basename_offset_simple() {
        assert_eq!(find_basename_offset(b"/foo/bar\0".as_ptr()), 5);
    }

    #[test]
    fn test_basename_offset_root() {
        assert_eq!(find_basename_offset(b"/file\0".as_ptr()), 1);
    }

    #[test]
    fn test_basename_offset_nested() {
        assert_eq!(find_basename_offset(b"/a/b/c/d\0".as_ptr()), 7);
    }

    #[test]
    fn test_basename_offset_trailing_slash() {
        // "/foo/" → basename offset is 5 (empty basename after last /).
        assert_eq!(find_basename_offset(b"/foo/\0".as_ptr()), 5);
    }

    #[test]
    fn test_basename_offset_empty() {
        assert_eq!(find_basename_offset(b"\0".as_ptr()), 0);
    }

    #[test]
    fn test_basename_offset_null() {
        assert_eq!(find_basename_offset(core::ptr::null()), 0);
    }

    // -- append_component --
    //
    // The buffer is the walker's one path, seeded with the parent and
    // extended in place, so each case sets up the parent bytes first.

    fn seeded(parent: &[u8]) -> ([u8; PATH_MAX], usize) {
        let mut buf = [0u8; PATH_MAX];
        buf[..parent.len()].copy_from_slice(parent);
        (buf, parent.len())
    }

    #[test]
    fn test_append_component_simple() {
        let (mut buf, parent_len) = seeded(b"/foo");
        let len = append_component(&mut buf, parent_len, b"bar\0".as_ptr());
        assert_eq!(len, 8); // "/foo/bar"
        assert_eq!(&buf[..8], b"/foo/bar");
        assert_eq!(buf[8], 0);
    }

    #[test]
    fn test_append_component_trailing_slash() {
        let (mut buf, parent_len) = seeded(b"/foo/");
        let len = append_component(&mut buf, parent_len, b"bar\0".as_ptr());
        // Parent already ends with '/', so no extra separator.
        assert_eq!(len, 8); // "/foo/bar"
        assert_eq!(&buf[..8], b"/foo/bar");
    }

    #[test]
    fn test_append_component_root() {
        let (mut buf, parent_len) = seeded(b"/");
        let len = append_component(&mut buf, parent_len, b"etc\0".as_ptr());
        assert_eq!(len, 4); // "/etc"
        assert_eq!(&buf[..4], b"/etc");
    }

    #[test]
    fn test_append_component_empty_parent() {
        let (mut buf, parent_len) = seeded(b"");
        let len = append_component(&mut buf, parent_len, b"file\0".as_ptr());
        // Empty parent, no separator needed (parent_len == 0).
        assert_eq!(len, 4);
        assert_eq!(&buf[..4], b"file");
    }

    #[test]
    fn test_append_component_overwrites_the_previous_sibling() {
        // The whole point of writing in place: the buffer is reused for
        // every child of a directory, so a shorter name must not leave
        // the tail of a longer one behind it.
        let (mut buf, parent_len) = seeded(b"/d");
        let long = append_component(&mut buf, parent_len, b"aaaaaaaa\0".as_ptr());
        assert_eq!(&buf[..long], b"/d/aaaaaaaa");
        let short = append_component(&mut buf, parent_len, b"b\0".as_ptr());
        assert_eq!(&buf[..short], b"/d/b");
        assert_eq!(buf[short], 0, "the name must end where it says it ends");
    }

    // -- FTW struct layout --

    #[test]
    fn test_ftw_struct_size() {
        // FTW has two i32 fields = 8 bytes.
        assert_eq!(core::mem::size_of::<FTW>(), 8);
    }

    #[test]
    fn test_ftw_struct_fields() {
        let f = FTW { base: 5, level: 3 };
        assert_eq!(f.base, 5);
        assert_eq!(f.level, 3);
    }

    #[test]
    fn test_path_max() {
        assert_eq!(PATH_MAX, 4096);
    }

    // -- append_component overflow --

    #[test]
    fn test_append_component_near_limit() {
        // A parent near PATH_MAX-2 with a 1-byte name should work.
        let mut buf = [b'a'; PATH_MAX];
        let parent_len = PATH_MAX - 3; // 4093 bytes of 'a', null at 4093
        buf[parent_len] = 0;
        let len = append_component(&mut buf, parent_len, b"x\0".as_ptr());
        // 4093 + "/" + "x" = 4095 bytes, which fits in PATH_MAX (4096).
        assert!(len > 0, "should fit within PATH_MAX");
    }

    #[test]
    fn test_append_component_at_limit() {
        // Exactly at PATH_MAX: parent(4093) + "/" + name(2) + NUL = 4097,
        // so the 4096-byte buffer cannot hold it and the append must refuse.
        let mut buf = [b'a'; PATH_MAX];
        let parent_len = PATH_MAX - 3; // length = 4093
        buf[parent_len] = 0;
        let len = append_component(&mut buf, parent_len, b"xy\0".as_ptr());
        assert_eq!(len, 0, "should fail when result hits PATH_MAX");
    }

    // -- find_basename_offset more cases --

    #[test]
    fn test_basename_offset_only_slash() {
        // "/" → basename offset is 1.
        assert_eq!(find_basename_offset(b"/\0".as_ptr()), 1);
    }

    #[test]
    fn test_basename_offset_double_slash() {
        // "//" → last slash at position 1, offset = 2.
        assert_eq!(find_basename_offset(b"//\0".as_ptr()), 2);
    }

    #[test]
    fn test_basename_offset_relative() {
        // "foo/bar" → last slash at 3, offset = 4.
        assert_eq!(find_basename_offset(b"foo/bar\0".as_ptr()), 4);
    }

    // -- is_dot_or_dotdot empty string --

    #[test]
    fn test_is_dot_or_dotdot_empty() {
        // Empty string ('\0') starts with '\0', not '.'.
        assert!(!is_dot_or_dotdot(b"\0".as_ptr()));
    }

    // -- FTW type flags are distinct --

    #[test]
    fn test_ftw_type_flags_distinct() {
        let types = [FTW_F, FTW_D, FTW_DNR, FTW_NS, FTW_SL, FTW_DP, FTW_SLN];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(
                    types[i], types[j],
                    "FTW types at indices {i} and {j} must be distinct"
                );
            }
        }
    }

    // -- Names and Seen --

    #[test]
    fn test_names_round_trip_and_grow() {
        let mut names = Names::new();
        let long = [b'n'; 300]; // past the first 256-byte allocation
        assert!(names.push(b"a"));
        assert!(names.push(&long));
        assert!(names.push(b"bc"));
        let mut got: Vec<Vec<u8>> = Vec::new();
        let mut at = 0;
        while let Some((name, next)) = names.at(at) {
            let n = unsafe { crate::string::strlen(name) };
            got.push(unsafe { core::slice::from_raw_parts(name, n) }.to_vec());
            at = next;
        }
        assert_eq!(got, vec![b"a".to_vec(), long.to_vec(), b"bc".to_vec()]);
        assert!(Names::new().at(0).is_none(), "an empty list has no names");
    }

    #[test]
    fn test_seen_detects_repeats_across_growth() {
        let mut seen = Seen::new();
        for ino in 1..=1000u64 {
            assert_eq!(seen.insert(DirId { dev: 7, ino }), Ok(true), "ino {ino}");
        }
        for ino in 1..=1000u64 {
            assert_eq!(
                seen.insert(DirId { dev: 7, ino }),
                Ok(false),
                "ino {ino} again"
            );
        }
        assert_eq!(
            seen.insert(DirId { dev: 8, ino: 1 }),
            Ok(true),
            "another device"
        );
        // An inode of 0 is no identity: never recorded, always new.
        assert_eq!(seen.insert(DirId { dev: 7, ino: 0 }), Ok(true));
        assert_eq!(seen.insert(DirId { dev: 7, ino: 0 }), Ok(true));
    }

    // -- the walk, over a tree of the tests' own --

    #[derive(Clone)]
    enum Kind {
        /// A directory and its entries, by name and node.
        Dir(Vec<(String, usize)>),
        /// A directory `opendir` refuses with this errno.
        DirListErr(i32),
        File,
        /// A symbolic link to this path.
        Link(String),
        /// Something whose `stat` fails with this errno.
        StatErr(i32),
    }

    struct Node {
        kind: Kind,
        dev: u64,
        ino: u64,
    }

    /// A small filesystem: nodes, and the top-level names paths start from.
    /// Intermediate components follow links, as path resolution does.
    struct FakeFs {
        nodes: Vec<Node>,
        top: HashMap<String, usize>,
    }

    impl FakeFs {
        fn new() -> Self {
            Self {
                nodes: Vec::new(),
                top: HashMap::new(),
            }
        }

        /// Add a node; its inode is its index + 1 unless given.
        fn add(&mut self, kind: Kind) -> usize {
            let id = self.nodes.len();
            self.nodes.push(Node {
                kind,
                dev: 1,
                ino: id as u64 + 1,
            });
            id
        }

        fn dir(&mut self, children: &[(&str, usize)]) -> usize {
            let entries = children
                .iter()
                .map(|(n, id)| ((*n).to_string(), *id))
                .collect();
            self.add(Kind::Dir(entries))
        }

        fn file(&mut self) -> usize {
            self.add(Kind::File)
        }

        fn link(&mut self, target: &str) -> usize {
            self.add(Kind::Link(target.to_string()))
        }

        /// Give directory `dir` one more entry — for links back to ancestors,
        /// which need the ancestor to exist first.
        fn adopt(&mut self, dir: usize, name: &str, child: usize) {
            if let Kind::Dir(entries) = &mut self.nodes[dir].kind {
                entries.push((name.to_string(), child));
            }
        }

        fn lookup(&self, path: &str, follow_last: bool, hops: u32) -> Result<usize, i32> {
            if hops > 8 {
                return Err(errno::ELOOP);
            }
            let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
            let (first, rest) = comps.split_first().ok_or(errno::ENOENT)?;
            let mut cur = *self.top.get(*first).ok_or(errno::ENOENT)?;
            for comp in rest {
                cur = self.follow(cur, hops)?;
                cur = match &self.nodes[cur].kind {
                    Kind::Dir(entries) => entries
                        .iter()
                        .find(|(n, _)| n == comp)
                        .map(|(_, id)| *id)
                        .ok_or(errno::ENOENT)?,
                    Kind::DirListErr(e) => return Err(*e),
                    Kind::StatErr(e) => return Err(*e),
                    _ => return Err(errno::ENOTDIR),
                };
            }
            if follow_last {
                self.follow(cur, hops)
            } else {
                Ok(cur)
            }
        }

        fn follow(&self, node: usize, hops: u32) -> Result<usize, i32> {
            match &self.nodes[node].kind {
                Kind::Link(target) => self.lookup(target, true, hops + 1),
                _ => Ok(node),
            }
        }
    }

    fn path_str(path: *const u8) -> String {
        let n = unsafe { crate::string::strlen(path) };
        String::from_utf8(unsafe { core::slice::from_raw_parts(path, n) }.to_vec()).unwrap()
    }

    impl Fs for &mut FakeFs {
        fn stat(&mut self, path: *const u8, follow: bool) -> Result<Stat, i32> {
            let id = self.lookup(&path_str(path), follow, 0)?;
            let node = &self.nodes[id];
            let mode = match &node.kind {
                Kind::Dir(_) | Kind::DirListErr(_) => S_IFDIR | 0o755,
                Kind::File => crate::fcntl::S_IFREG | 0o644,
                Kind::Link(_) => S_IFLNK | 0o777,
                Kind::StatErr(e) => return Err(*e),
            };
            let mut sb: Stat = unsafe { core::mem::zeroed() };
            sb.st_mode = mode;
            sb.st_dev = node.dev;
            sb.st_ino = node.ino;
            Ok(sb)
        }

        fn list(&mut self, path: *const u8, names: &mut Names) -> Result<(), i32> {
            let id = self.lookup(&path_str(path), true, 0)?;
            match &self.nodes[id].kind {
                Kind::Dir(entries) => {
                    for (n, _) in entries {
                        assert!(names.push(n.as_bytes()));
                    }
                    Ok(())
                }
                Kind::DirListErr(e) => Err(*e),
                _ => Err(errno::ENOTDIR),
            }
        }
    }

    /// One callback, as the tests record it: path, type flag, level.
    type Seen3 = (String, i32, i32);

    /// Walk `root` in `fs` with `opts`, the callback answering `reply(path,
    /// flag)`; returns the walk's result and every callback, in order.
    fn walk(
        fs: &mut FakeFs,
        root: &str,
        opts: Walk,
        mut reply: impl FnMut(&str, i32) -> i32,
    ) -> (i32, Vec<Seen3>) {
        let mut calls = Vec::new();
        let mut c_root = root.as_bytes().to_vec();
        c_root.push(0);
        let ret = run(
            c_root.as_ptr(),
            opts,
            |p, _sb, flag, level| {
                let s = path_str(p);
                let r = reply(&s, flag);
                calls.push((s, flag, level));
                r
            },
            fs,
        );
        (ret, calls)
    }

    const PLAIN: Walk = Walk {
        depth_first: false,
        physical: false,
        action_retval: false,
    };

    fn go_on(_: &str, _: i32) -> i32 {
        0
    }

    fn entry(p: &str, flag: i32, level: i32) -> Seen3 {
        (p.to_string(), flag, level)
    }

    /// r/{f, d/{g}}: the tree most tests start from.
    fn small_tree() -> FakeFs {
        let mut fs = FakeFs::new();
        let f = fs.file();
        let g = fs.file();
        let d = fs.dir(&[("g", g)]);
        let r = fs.dir(&[("f", f), ("d", d)]);
        fs.top.insert("r".into(), r);
        fs
    }

    #[test]
    fn test_walk_pre_order_and_post_order() {
        let mut fs = small_tree();
        let (ret, calls) = walk(&mut fs, "r", PLAIN, go_on);
        assert_eq!(ret, 0);
        assert_eq!(
            calls,
            vec![
                entry("r", FTW_D, 0),
                entry("r/f", FTW_F, 1),
                entry("r/d", FTW_D, 1),
                entry("r/d/g", FTW_F, 2),
            ]
        );
        let depth = Walk {
            depth_first: true,
            ..PLAIN
        };
        let (ret, calls) = walk(&mut fs, "r", depth, go_on);
        assert_eq!(ret, 0);
        assert_eq!(
            calls,
            vec![
                entry("r/f", FTW_F, 1),
                entry("r/d/g", FTW_F, 2),
                entry("r/d", FTW_DP, 1),
                entry("r", FTW_DP, 0),
            ]
        );
    }

    /// The bug this walker replaced: `nopenfd` was also the depth limit, and
    /// never more than 32.  Forty levels, all walked.
    #[test]
    fn test_walk_has_no_depth_limit() {
        let mut fs = FakeFs::new();
        let mut below = fs.file();
        let mut name = "leaf";
        for _ in 0..40 {
            below = fs.dir(&[(name, below)]);
            name = "d";
        }
        fs.top.insert("r".into(), below);
        let (ret, calls) = walk(&mut fs, "r", PLAIN, go_on);
        assert_eq!(ret, 0, "errno {}", errno::get_errno());
        assert_eq!(calls.len(), 41, "forty directories and the leaf");
        let last = calls.last().unwrap();
        assert!(
            last.0.ends_with("/leaf") && last.1 == FTW_F && last.2 == 40,
            "{last:?}"
        );
        assert!(
            calls.iter().all(|c| c.1 != FTW_DNR),
            "nothing is called unreadable"
        );
    }

    #[test]
    fn test_root_that_cannot_be_stated_gets_no_callback() {
        let mut fs = small_tree();
        let (ret, calls) = walk(&mut fs, "missing", PLAIN, go_on);
        assert_eq!((ret, errno::get_errno()), (-1, errno::ENOENT));
        assert!(calls.is_empty(), "{calls:?}");
        let denied = fs.add(Kind::StatErr(errno::EACCES));
        fs.top.insert("denied".into(), denied);
        let (ret, calls) = walk(&mut fs, "denied", PLAIN, go_on);
        assert_eq!((ret, errno::get_errno()), (-1, errno::EACCES));
        assert!(
            calls.is_empty(),
            "a root is not FTW_NS, as a child would be"
        );
    }

    #[test]
    fn test_dangling_root_link_is_ftw_sln() {
        let mut fs = small_tree();
        let l = fs.link("nowhere");
        fs.top.insert("l".into(), l);
        let (ret, calls) = walk(&mut fs, "l", PLAIN, go_on);
        assert_eq!(ret, 0);
        assert_eq!(calls, vec![entry("l", FTW_SLN, 0)]);
        // `ftw` has no FTW_SLN: it reads as FTW_NS.
        assert_eq!(ftw_flag(FTW_SLN), FTW_NS);
    }

    #[test]
    fn test_root_trailing_slashes_are_stripped() {
        let mut fs = small_tree();
        let (ret, calls) = walk(&mut fs, "r///", PLAIN, go_on);
        assert_eq!(ret, 0);
        assert_eq!(calls[0], entry("r", FTW_D, 0));
        assert_eq!(calls[1], entry("r/f", FTW_F, 1));
    }

    #[test]
    fn test_empty_root_is_enoent_and_nopenfd_is_never_refused() {
        extern "C" fn never(_: *const u8, _: *const Stat, _: i32) -> i32 {
            panic!("no callback for an empty root");
        }
        extern "C" fn never_n(_: *const u8, _: *const Stat, _: i32, _: *mut FTW) -> i32 {
            panic!("no callback for an empty root");
        }
        // `nopenfd` 0 and -5 were EINVAL until 2026-09-26; glibc makes them 1,
        // so the empty root's ENOENT is what comes back.
        for n in [0, -5, 1] {
            errno::set_errno(0);
            assert_eq!(ftw(b"\0".as_ptr(), never, n), -1);
            assert_eq!(errno::get_errno(), errno::ENOENT, "ftw, nopenfd {n}");
            errno::set_errno(0);
            assert_eq!(nftw(b"\0".as_ptr(), never_n, n, FTW_ACTIONRETVAL), -1);
            assert_eq!(errno::get_errno(), errno::ENOENT, "nftw, nopenfd {n}");
        }
    }

    #[test]
    fn test_child_stat_failures() {
        let mut fs = FakeFs::new();
        let denied = fs.add(Kind::StatErr(errno::EACCES));
        let dangling = fs.link("nowhere");
        let f = fs.file();
        let r = fs.dir(&[("denied", denied), ("dangling", dangling), ("f", f)]);
        fs.top.insert("r".into(), r);
        let (ret, calls) = walk(&mut fs, "r", PLAIN, go_on);
        assert_eq!(ret, 0);
        assert_eq!(
            calls,
            vec![
                entry("r", FTW_D, 0),
                entry("r/denied", FTW_NS, 1),
                entry("r/dangling", FTW_SLN, 1),
                entry("r/f", FTW_F, 1),
            ]
        );
        // Under FTW_PHYS the link is lstat'ed, so it is a link, not dangling.
        let phys = Walk {
            physical: true,
            ..PLAIN
        };
        let (_, calls) = walk(&mut fs, "r", phys, go_on);
        assert_eq!(calls[2], entry("r/dangling", FTW_SL, 1));
    }

    /// A stat failure other than EACCES or ENOENT ends the walk, as glibc's
    /// `process_entry` does: later siblings are never seen.
    #[test]
    fn test_other_stat_failure_ends_the_walk() {
        let mut fs = FakeFs::new();
        let looped = fs.add(Kind::StatErr(errno::ELOOP));
        let f = fs.file();
        let r = fs.dir(&[("looped", looped), ("f", f)]);
        fs.top.insert("r".into(), r);
        let (ret, calls) = walk(&mut fs, "r", PLAIN, go_on);
        assert_eq!((ret, errno::get_errno()), (-1, errno::ELOOP));
        assert_eq!(calls, vec![entry("r", FTW_D, 0)]);
    }

    #[test]
    fn test_unreadable_directory_is_ftw_dnr_and_other_open_failures_end_the_walk() {
        let mut fs = FakeFs::new();
        let locked = fs.add(Kind::DirListErr(errno::EACCES));
        let f = fs.file();
        let r = fs.dir(&[("locked", locked), ("f", f)]);
        fs.top.insert("r".into(), r);
        // FTW_DNR replaces FTW_D, and under FTW_DEPTH there is no FTW_DP.
        let depth = Walk {
            depth_first: true,
            ..PLAIN
        };
        let (ret, calls) = walk(&mut fs, "r", depth, go_on);
        assert_eq!(ret, 0);
        assert_eq!(
            calls,
            vec![
                entry("r/locked", FTW_DNR, 1),
                entry("r/f", FTW_F, 1),
                entry("r", FTW_DP, 0)
            ]
        );

        let mut fs = FakeFs::new();
        let busy = fs.add(Kind::DirListErr(errno::EMFILE));
        let f = fs.file();
        let r = fs.dir(&[("busy", busy), ("f", f)]);
        fs.top.insert("r".into(), r);
        let (ret, calls) = walk(&mut fs, "r", PLAIN, go_on);
        assert_eq!((ret, errno::get_errno()), (-1, errno::EMFILE));
        assert_eq!(calls, vec![entry("r", FTW_D, 0)]);
    }

    /// A link to an ancestor: followed, it would recurse forever; glibc
    /// skips a directory it has entered, without a callback.  Under
    /// FTW_PHYS it is reported as the link it is.
    #[test]
    fn test_link_to_an_ancestor_is_walked_once() {
        let mut fs = FakeFs::new();
        let f = fs.file();
        let r = fs.dir(&[("f", f)]);
        let up = fs.link("r");
        fs.adopt(r, "up", up);
        fs.top.insert("r".into(), r);
        let (ret, calls) = walk(&mut fs, "r", PLAIN, go_on);
        assert_eq!(ret, 0);
        assert_eq!(calls, vec![entry("r", FTW_D, 0), entry("r/f", FTW_F, 1)]);
        let phys = Walk {
            physical: true,
            ..PLAIN
        };
        let (ret, calls) = walk(&mut fs, "r", phys, go_on);
        assert_eq!(ret, 0);
        assert_eq!(calls[2], entry("r/up", FTW_SL, 1));
    }

    /// A directory reachable by two names is walked under the first only.
    #[test]
    fn test_directory_reached_twice_is_walked_once() {
        let mut fs = FakeFs::new();
        let g = fs.file();
        let d = fs.dir(&[("g", g)]);
        let alias = fs.link("r/d");
        let r = fs.dir(&[("alias", alias), ("d", d)]);
        fs.top.insert("r".into(), r);
        let (ret, calls) = walk(&mut fs, "r", PLAIN, go_on);
        assert_eq!(ret, 0);
        assert_eq!(
            calls,
            vec![
                entry("r", FTW_D, 0),
                entry("r/alias", FTW_D, 1),
                entry("r/alias/g", FTW_F, 2)
            ]
        );
    }

    /// Inode 0 is no identity (§740): two such directories are both walked.
    #[test]
    fn test_directories_with_inode_zero_are_all_walked() {
        let mut fs = FakeFs::new();
        let a = fs.dir(&[]);
        let b = fs.dir(&[]);
        fs.nodes[a].ino = 0;
        fs.nodes[b].ino = 0;
        let r = fs.dir(&[("a", a), ("b", b)]);
        fs.top.insert("r".into(), r);
        let (ret, calls) = walk(&mut fs, "r", PLAIN, go_on);
        assert_eq!(ret, 0);
        assert_eq!(calls.len(), 3, "{calls:?}");
    }

    #[test]
    fn test_callback_result_stops_the_walk() {
        let mut fs = small_tree();
        let (ret, calls) = walk(&mut fs, "r", PLAIN, |p, _| if p == "r/f" { 7 } else { 0 });
        assert_eq!(ret, 7);
        assert_eq!(calls.len(), 2, "nothing after the stop: {calls:?}");
        // Without FTW_ACTIONRETVAL a 2 is not a skip; it stops too.
        let (ret, _) = walk(&mut fs, "r", PLAIN, |p, _| if p == "r/d" { 2 } else { 0 });
        assert_eq!(ret, 2);
    }

    #[test]
    fn test_action_retval() {
        let act = Walk {
            action_retval: true,
            ..PLAIN
        };
        // r/{a/{x}, f, b/{y}}
        let mut fs = FakeFs::new();
        let x = fs.file();
        let a = fs.dir(&[("x", x)]);
        let f = fs.file();
        let y = fs.file();
        let b = fs.dir(&[("y", y)]);
        let r = fs.dir(&[("a", a), ("f", f), ("b", b)]);
        fs.top.insert("r".into(), r);
        let paths = |calls: &[Seen3]| calls.iter().map(|c| c.0.clone()).collect::<Vec<_>>();

        // FTW_SKIP_SUBTREE from a directory's FTW_D: not entered.
        let (ret, calls) = walk(&mut fs, "r", act, |p, _| {
            if p == "r/a" { FTW_SKIP_SUBTREE } else { 0 }
        });
        assert_eq!(ret, 0);
        assert_eq!(paths(&calls), ["r", "r/a", "r/f", "r/b", "r/b/y"]);

        // FTW_SKIP_SIBLINGS: the rest of the directory is skipped.
        let (ret, calls) = walk(&mut fs, "r", act, |p, _| {
            if p == "r/f" { FTW_SKIP_SIBLINGS } else { 0 }
        });
        assert_eq!(ret, 0);
        assert_eq!(paths(&calls), ["r", "r/a", "r/a/x", "r/f"]);

        // ... and under FTW_DEPTH the directory is still reported after.
        let both = Walk {
            depth_first: true,
            ..act
        };
        let (ret, calls) = walk(&mut fs, "r", both, |p, _| {
            if p == "r/a/x" { FTW_SKIP_SIBLINGS } else { 0 }
        });
        assert_eq!(ret, 0);
        assert_eq!(paths(&calls), ["r/a/x", "r/a", "r/f", "r/b/y", "r/b", "r"]);

        // FTW_STOP stops, and is the result.
        let (ret, calls) = walk(
            &mut fs,
            "r",
            act,
            |p, _| if p == "r/a/x" { FTW_STOP } else { 0 },
        );
        assert_eq!(ret, FTW_STOP);
        assert_eq!(paths(&calls), ["r", "r/a", "r/a/x"]);

        // A skip at the root is success.
        let (ret, calls) = walk(&mut fs, "r", act, |_, _| FTW_SKIP_SUBTREE);
        assert_eq!(ret, 0);
        assert_eq!(paths(&calls), ["r"]);
    }

    #[test]
    fn test_a_path_too_long_ends_the_walk() {
        let mut fs = FakeFs::new();
        let long = "n".repeat(250);
        let mut below = fs.file();
        for _ in 0..20 {
            below = fs.dir(&[(long.as_str(), below)]);
        }
        fs.top.insert("r".into(), below);
        let (ret, calls) = walk(&mut fs, "r", PLAIN, go_on);
        assert_eq!((ret, errno::get_errno()), (-1, errno::ENAMETOOLONG));
        assert!(
            calls.len() >= 16,
            "the levels that fit were walked: {}",
            calls.len()
        );
    }

    // -- argument validation, through the real entry points --

    extern "C" fn never_called_ftw(_p: *const u8, _sb: *const Stat, _flag: i32) -> i32 {
        panic!("callback must not run when the arguments are rejected");
    }

    extern "C" fn never_called_nftw(
        _p: *const u8,
        _sb: *const Stat,
        _flag: i32,
        _fb: *mut FTW,
    ) -> i32 {
        panic!("callback must not run when the arguments are rejected");
    }

    #[test]
    fn test_ftw_null_path_efault() {
        assert_eq!(ftw(core::ptr::null(), never_called_ftw, 4), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_nftw_null_path_efault() {
        assert_eq!(nftw(core::ptr::null(), never_called_nftw, 4, 0), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_nftw_rejects_an_unknown_flag_before_the_path() {
        // glibc 2.39's `nftw` tests the flags first; a NULL path is not
        // even looked at.
        for flags in [32, 64 | FTW_PHYS, i32::MIN] {
            errno::set_errno(0);
            assert_eq!(nftw(core::ptr::null(), never_called_nftw, 4, flags), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL, "flags {flags:#x}");
        }
    }

    #[test]
    fn test_nftw_rejects_ftw_mount() {
        // Accepted-and-ignored is how a --one-file-system delete walks
        // into a network mount.  See UNSUPPORTED_NFTW_FLAGS.
        assert_eq!(
            nftw(b"/tmp\0".as_ptr(), never_called_nftw, 4, FTW_MOUNT),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_nftw_rejects_ftw_chdir() {
        assert_eq!(
            nftw(b"/tmp\0".as_ptr(), never_called_nftw, 4, FTW_CHDIR),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_nftw_accepts_the_flags_it_implements() {
        // FTW_PHYS|FTW_DEPTH|FTW_ACTIONRETVAL must *not* be refused: the
        // rejection is a deliberate list, and a test that only checks the
        // refusals would pass just as happily if the check rejected
        // everything.
        let rc = nftw(
            b"\0".as_ptr(),
            passthrough_nftw,
            4,
            FTW_PHYS | FTW_DEPTH | FTW_ACTIONRETVAL,
        );
        assert_ne!(
            errno::get_errno(),
            errno::EINVAL,
            "supported flags must not be refused (rc = {rc})"
        );
    }

    extern "C" fn passthrough_nftw(
        _p: *const u8,
        _sb: *const Stat,
        _flag: i32,
        _fb: *mut FTW,
    ) -> i32 {
        0
    }

    #[test]
    fn test_run_rejects_an_over_long_root() {
        // strlen(root) >= PATH_MAX is refused before anything is copied
        // into the walker's buffer.
        let mut root = [b'a'; PATH_MAX + 9];
        root[PATH_MAX + 8] = 0;
        assert_eq!(ftw(root.as_ptr(), never_called_ftw, 4), -1);
        assert_eq!(errno::get_errno(), errno::ENAMETOOLONG);
    }
}
