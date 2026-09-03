//! Walking a directory tree through descriptors instead of through path
//! strings.
//!
//! # The bug this module exists to make impossible
//!
//! A recursive utility that builds up path strings — `dir`, then `dir/sub`,
//! then `dir/sub/file` — and hands each whole string to the kernel is asking
//! the kernel to re-walk the path from the top on **every single call**. Each
//! of those walks is a fresh chance for somebody else to have moved something.
//! If a second process can write inside the tree, it can replace a directory
//! with a symbolic link in the gap between the utility deciding to descend into
//! it and the utility acting on what is inside, and the actions then land
//! wherever the link points.
//!
//! That is not a hypothetical shape. It is `known-issues.md` →
//! `TD-B-RM-WALKS-BY-PATH-SO-A-SYMLINK-SWAP-CAN-REDIRECT-A-REMOVAL`, and the
//! same class of defect was already found and fixed once in `tar`
//! (`B-tar-WALKS-THROUGH-A-PRE-EXISTING-SYMLINK-AND-WRITES-OUTSIDE-THE-DESTINATION`).
//! Two utilities having independently grown the same hole is the argument for
//! this being a module rather than a third private copy: per [`crate`]'s own
//! framing, this library is for *"the things where two utilities disagreeing
//! would itself be the bug"*, and two hand-written `O_NOFOLLOW` walks
//! disagreeing about which component the flag guards is exactly that.
//!
//! # The rule
//!
//! One [`Dir`] is an owned, open descriptor on a directory. Everything a walk
//! does below its starting point is expressed as `(dir, single component)` —
//! never as a path — so the kernel resolves the directory from the descriptor
//! it was handed and the name from one lookup in that directory. There is no
//! second component for anyone to swap.
//!
//! The path strings do not go away; they stop being what gets *resolved*. A
//! caller keeps them for its messages, because what a utility prints is a
//! contract of its own — `scripts/rm-diff.sh` certifies `rm`'s spelling case by
//! case against the real GNU binary — and passes them nowhere near a syscall.
//!
//! # Descending is the step that needs the extra check
//!
//! [`Dir::stat`], [`Dir::unlink`] and [`Dir::rmdir`] are `*at` calls and are
//! done when they return. Descending is not, because it produces a *new*
//! descriptor, and a descriptor is only as good as the lookup that made it:
//!
//! 1. `fstatat(dir, name, AT_SYMLINK_NOFOLLOW)` says `name` is a directory.
//! 2. `openat(dir, name, O_DIRECTORY | O_NOFOLLOW)` opens it.
//!
//! Between 1 and 2 the name can be swapped. `O_NOFOLLOW` catches the obvious
//! substitution — a symlink where the directory was — and on a kernel whose
//! `openat` resolves the descriptor rather than the descriptor's remembered
//! path, that is the whole story.
//!
//! **Ours is not yet such a kernel, and this is the reason
//! [`Dir::open_child`] takes the expected identity.** SlateOS pinned most of
//! the `*at` family during August 2026 — `SYS_FS_UNLINKAT_PINNED` (662),
//! `SYS_FS_FSTATAT_PINNED` (663), `SYS_FS_GETDENTS_PINNED` (664) and the
//! creation set 665–670 all resolve the *handle* — but `openat` is not among
//! them: `posix/src/file.rs`'s `openat` still calls `resolve_dirfd_path`, a
//! textual join of the descriptor's remembered path with the caller's name.
//! `O_NOFOLLOW` guards the *final* component of that join and nothing above it,
//! so an attacker who swaps an already-descended *ancestor* still redirects the
//! open.
//!
//! Rather than wait for a pinned `openat`, [`Dir::open_child`] closes it from
//! this side: it `fstat`s the descriptor it just obtained and compares
//! `(st_dev, st_ino)` against the [`Stat`] the caller had already taken. An
//! attacker can still win the race; they cannot make it go unnoticed, because
//! passing the check would mean redirecting the open to a file that *is* the
//! file we meant. A mismatch answers `ESTALE`, which is the errno the pinned
//! family already uses for "the handle no longer names what you asked about",
//! so the two mechanisms report the same condition the same way.
//!
//! The check is one `fstat` per directory entered, it needs nothing from the
//! kernel that is not already there, and it is identical on Linux and on
//! SlateOS — which matters, because a defence that exists only on the
//! certification target is a defence that tests green and ships absent.
//!
//! # Off unix
//!
//! There is no `openat` and no `fstatat`, so the non-unix [`Dir`] holds a path
//! and joins. That is the very thing this module exists to avoid, and it is
//! accepted there for one reason: the Windows build of these utilities is a
//! test vehicle for the parts that are not about syscalls, never a shipping
//! one. The two implementations expose the same API so that no caller has to
//! carry a `cfg`, and the difference is confined to this file.

use std::io;
use std::path::Path;

use crate::quote::{os_bytes, os_from_bytes};

/// What kind of thing an entry is.
///
/// A closed set rather than a mode word, because the callers ask categorical
/// questions ("is it a directory?", "which noun goes in the prompt?") and a
/// mode word would put the `S_IFMT` masking in each of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A directory.
    Directory,
    /// A symbolic link. Every lookup here is `AT_SYMLINK_NOFOLLOW`, so this is
    /// reported rather than followed.
    SymbolicLink,
    /// An ordinary file.
    Regular,
    /// A named pipe.
    Fifo,
    /// A unix-domain socket bound into the filesystem.
    Socket,
    /// A block special file.
    BlockDevice,
    /// A character special file.
    CharDevice,
    /// Something none of the above — a type this build does not know about.
    /// Reported rather than guessed: a walk that mistook one of these for a
    /// directory would try to descend into it.
    Other,
}

/// An entry's type, identity and size — the whole of what a walk reads from a
/// lookup.
///
/// Deliberately not [`std::fs::Metadata`]. `Metadata` cannot be produced from
/// an `fstatat` without going back through a path, and its `st_dev`/`st_ino`
/// live behind a unix-only extension trait, so a caller wanting both ends up
/// writing the `cfg` this type exists to hold once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stat {
    kind: Kind,
    dev: u64,
    ino: u64,
    size: u64,
}

impl Stat {
    /// What the entry is.
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Whether it is a directory. Never true for a symlink to one: every
    /// lookup in this module is `AT_SYMLINK_NOFOLLOW`.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Directory
    }

    /// Whether it is a symbolic link.
    #[must_use]
    pub fn is_symlink(&self) -> bool {
        self.kind == Kind::SymbolicLink
    }

    /// Whether it is an ordinary file.
    #[must_use]
    pub fn is_regular(&self) -> bool {
        self.kind == Kind::Regular
    }

    /// The device this entry lives on, or `None` where the platform does not
    /// say. `None` must be read as "unknown", never as a match against another
    /// `None`: two unknown devices are not the same device.
    #[must_use]
    pub fn dev(&self) -> Option<u64> {
        if cfg!(unix) { Some(self.dev) } else { None }
    }

    /// The `(device, inode)` pair, or `None` where the platform does not say.
    ///
    /// The same warning as [`Stat::dev`]: `None` is not a match.
    #[must_use]
    pub fn identity(&self) -> Option<(u64, u64)> {
        if cfg!(unix) {
            Some((self.dev, self.ino))
        } else {
            None
        }
    }

    /// The entry's length in bytes. Read only to tell GNU's `regular empty
    /// file` from its `regular file`, which is a prompt's wording.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }

    /// The entry a path names, without following a final symlink.
    ///
    /// This is the one lookup a walk does by path — its starting point, which
    /// by definition has no descriptor above it yet. Everything below it goes
    /// through [`Dir::stat`].
    pub fn of_path(path: &Path) -> io::Result<Self> {
        Ok(Self::from_metadata(&std::fs::symlink_metadata(path)?))
    }

    /// Convert a `Metadata` that some other route already produced.
    #[must_use]
    pub fn from_metadata(md: &std::fs::Metadata) -> Self {
        Self {
            kind: kind_of_metadata(md),
            dev: metadata_dev(md),
            ino: metadata_ino(md),
            size: md.len(),
        }
    }
}

fn kind_of_metadata(md: &std::fs::Metadata) -> Kind {
    // Symlink first, and directory second, because those two are the ones a
    // walk acts on and `is_dir` is false for a link to a directory only if the
    // metadata came from an `lstat`. Every caller here uses `symlink_metadata`.
    if md.is_symlink() {
        return Kind::SymbolicLink;
    }
    if md.is_dir() {
        return Kind::Directory;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let t = md.file_type();
        if t.is_block_device() {
            return Kind::BlockDevice;
        }
        if t.is_char_device() {
            return Kind::CharDevice;
        }
        if t.is_fifo() {
            return Kind::Fifo;
        }
        if t.is_socket() {
            return Kind::Socket;
        }
    }
    if md.is_file() {
        return Kind::Regular;
    }
    Kind::Other
}

#[cfg(unix)]
fn metadata_dev(md: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    md.dev()
}

#[cfg(not(unix))]
fn metadata_dev(_md: &std::fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn metadata_ino(md: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    md.ino()
}

#[cfg(not(unix))]
fn metadata_ino(_md: &std::fs::Metadata) -> u64 {
    0
}

/// The error a call gets when the name it was handed cannot be expressed as a
/// C string.
///
/// A NUL inside is refused rather than truncated, because a C call handed one
/// would act on the *prefix* — and the prefix of a name is a different name.
fn embedded_nul() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte")
}

/// `ESTALE`, the answer to "the descriptor I opened is not the entry I looked
/// up".
///
/// Linux's number, and the same one `posix`'s pinned `*at` calls answer when a
/// handle no longer names what the caller asked about — so a caller that
/// already handles one gets the other for free.
const ESTALE: i32 = 116;

/// The refusal [`Dir::open_child`] raises when the identity check fails.
fn swapped_underneath() -> io::Error {
    io::Error::from_raw_os_error(ESTALE)
}

/// A single path component as a NUL-terminated byte string.
///
/// A `/` is refused as firmly as a NUL. This module's whole premise is that a
/// name is *one lookup*, and a name with a separator in it is a walk; letting
/// one through would silently reintroduce the multi-component resolution
/// everything here exists to avoid.
fn c_name(component: &[u8]) -> io::Result<Vec<u8>> {
    if component.is_empty() || component.contains(&0) || component.contains(&b'/') {
        return Err(embedded_nul());
    }
    let mut buf = Vec::with_capacity(component.len().saturating_add(1));
    buf.extend_from_slice(component);
    buf.push(0);
    Ok(buf)
}

// ---------------------------------------------------------------- unix --

/// The syscalls `std` does not wrap, declared beside their callers rather than
/// pulled from a libc binding — this crate depends on none, and the signatures
/// are short enough to check against `posix/src/file.rs` by eye.
///
/// They are the `*at` forms, taking a directory descriptor and a single
/// component. That is not a stylistic preference; it is the entire point of
/// the module.
#[cfg(unix)]
unsafe extern "C" {
    fn openat(dirfd: i32, path: *const u8, flags: i32, mode: u32) -> i32;
    fn close(fd: i32) -> i32;
    fn dup(fd: i32) -> i32;
    fn fstat(fd: i32, buf: *mut CStat) -> i32;
    fn fstatat(dirfd: i32, path: *const u8, buf: *mut CStat, flags: i32) -> i32;
    fn unlinkat(dirfd: i32, path: *const u8, flags: i32) -> i32;
    fn faccessat(dirfd: i32, path: *const u8, mode: i32, flags: i32) -> i32;
    fn fdopendir(fd: i32) -> *mut CDir;
    fn readdir(dirp: *mut CDir) -> *mut CDirent;
    fn closedir(dirp: *mut CDir) -> i32;
}

/// The opaque `DIR`. Only ever held as a pointer.
#[cfg(unix)]
#[repr(C)]
struct CDir {
    _opaque: [u8; 0],
}

/// `struct dirent`, in the layout both `posix/src/dirent.rs` and glibc declare.
///
/// Only `d_name`'s *offset* is load-bearing: the name is read from there as a
/// NUL-terminated string and the rest is never touched. The fields above it are
/// present so that offset is right.
#[cfg(unix)]
#[repr(C)]
struct CDirent {
    d_ino: u64,
    d_off: i64,
    d_reclen: u16,
    d_type: u8,
    d_name: [u8; 256],
}

/// `struct stat`, in the layout `posix/src/stat.rs` declares — which is itself
/// documented as matching Linux x86-64's, so the one declaration serves both
/// the host build and the SlateOS one.
///
/// Only `st_dev`, `st_ino`, `st_mode` and `st_size` are read. The rest is
/// present so the struct is the right *size*: `fstat` writes all of it, and a
/// short buffer would be a stack overwrite rather than a wrong answer.
#[cfg(unix)]
#[repr(C)]
#[derive(Default)]
struct CStat {
    st_dev: u64,
    st_ino: u64,
    st_nlink: u64,
    st_mode: u32,
    st_uid: u32,
    st_gid: u32,
    _pad0: i32,
    st_rdev: u64,
    st_size: i64,
    st_blksize: i64,
    st_blocks: i64,
    st_atim: CTimespec,
    st_mtim: CTimespec,
    st_ctim: CTimespec,
    _reserved: [i64; 3],
}

/// `struct timespec`, in the layout `posix/src/stat.rs` declares.
#[cfg(unix)]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

/// The size [`CStat`] must have, checked here rather than discovered as a
/// corrupted stack: `fstat` writes 144 bytes on x86-64 whatever this file
/// thinks, so a declaration that drifted from the C one has to fail the build.
#[cfg(unix)]
const _: () = assert!(core::mem::size_of::<CStat>() == 144);

/// The `open` flags used here, as Linux numbers them and as
/// `posix/src/fcntl.rs` declares them.
#[cfg(unix)]
mod oflag {
    pub const RDONLY: i32 = 0;
    pub const DIRECTORY: i32 = 0o200_000;
    pub const NOFOLLOW: i32 = 0o400_000;
    pub const CLOEXEC: i32 = 0o2_000_000;
}

/// `AT_SYMLINK_NOFOLLOW` — act on the link, not on what it names.
#[cfg(unix)]
const AT_SYMLINK_NOFOLLOW: i32 = 0x100;

/// `AT_REMOVEDIR` — `unlinkat` should `rmdir` rather than `unlink`.
#[cfg(unix)]
const AT_REMOVEDIR: i32 = 0x200;

/// `AT_EACCESS` — answer for the *effective* ids.
///
/// The same numeric value as [`AT_REMOVEDIR`], which is not a mistake: the two
/// bits belong to different calls and Linux reuses the number. Spelled twice so
/// that neither call reads as though it were passing the other's flag.
#[cfg(unix)]
const AT_EACCESS: i32 = 0x200;

/// `AT_FDCWD` — resolve a relative name against the working directory.
#[cfg(unix)]
const AT_FDCWD: i32 = -100;

/// `W_OK`, the `access` mode bit for "may be written".
#[cfg(unix)]
const W_OK: i32 = 2;

/// The `S_IFMT` field of `st_mode`, and the values in it.
#[cfg(unix)]
mod ifmt {
    pub const MASK: u32 = 0o170_000;
    pub const DIR: u32 = 0o040_000;
    pub const LNK: u32 = 0o120_000;
    pub const REG: u32 = 0o100_000;
    pub const FIFO: u32 = 0o010_000;
    pub const SOCK: u32 = 0o140_000;
    pub const BLK: u32 = 0o060_000;
    pub const CHR: u32 = 0o020_000;
}

#[cfg(unix)]
impl Stat {
    fn from_cstat(st: &CStat) -> Self {
        let kind = match st.st_mode & ifmt::MASK {
            ifmt::DIR => Kind::Directory,
            ifmt::LNK => Kind::SymbolicLink,
            ifmt::REG => Kind::Regular,
            ifmt::FIFO => Kind::Fifo,
            ifmt::SOCK => Kind::Socket,
            ifmt::BLK => Kind::BlockDevice,
            ifmt::CHR => Kind::CharDevice,
            _ => Kind::Other,
        };
        Self {
            kind,
            dev: st.st_dev,
            ino: st.st_ino,
            #[allow(clippy::cast_sign_loss)]
            size: st.st_size.max(0) as u64,
        }
    }
}

/// An open handle on a directory.
///
/// It owns the descriptor and closes it on drop, which is why this is a type
/// rather than a bare `i32`: a walk holds one per level and unwinds them as it
/// returns, including on every error path.
///
/// The `Debug` is the descriptor number, which is all there is to show: the
/// point of the type is that the path it was opened by is *not* retained.
#[cfg(unix)]
#[derive(Debug)]
pub struct Dir(i32);

#[cfg(unix)]
impl Drop for Dir {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `openat` or `open` and is owned solely by
        // this value — `Dir` is not `Copy` and never hands the number out — so
        // this is the one and only close of it.
        unsafe { close(self.0) };
    }
}

#[cfg(unix)]
impl Dir {
    /// Open a walk's starting point, by path. The one place this module
    /// resolves more than a single component.
    ///
    /// `expected` is the [`Stat`] the caller took of this same path — it had to
    /// take one, because "is this a directory at all?" is what decides whether
    /// to come here — and the descriptor is checked against it for the same
    /// reason [`Dir::open_child`] does. The root is the one step where a
    /// multi-component path *must* be resolved, so it is the step where the
    /// verification is worth most.
    ///
    /// `O_NOFOLLOW` is deliberately **not** set here. An operand that is
    /// literally a symlink is refused by the caller's own classification long
    /// before this point, and a symlink that resolves to the very directory
    /// `expected` describes has not redirected anything — the identity check,
    /// not the flag, is what says so.
    pub fn open_root(path: &Path, expected: &Stat) -> io::Result<Self> {
        let bytes = os_bytes(path.as_os_str());
        if bytes.contains(&0) {
            return Err(embedded_nul());
        }
        let mut cpath = Vec::with_capacity(bytes.len().saturating_add(1));
        cpath.extend_from_slice(&bytes);
        cpath.push(0);
        // SAFETY: `cpath` is NUL-terminated, has no interior NUL, and outlives
        // the call, which reads it and does not retain it. The mode argument is
        // unused without `O_CREAT`.
        let fd = unsafe {
            openat(
                AT_FDCWD,
                cpath.as_ptr(),
                oflag::RDONLY | oflag::DIRECTORY | oflag::CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let opened = Self(fd);
        if opened.stat_self()?.identity() != expected.identity() {
            return Err(swapped_underneath());
        }
        Ok(opened)
    }

    /// Descend into `name`, refusing to follow a symlink and refusing to open
    /// anything that is not the entry `expected` describes.
    ///
    /// Both halves are needed and neither implies the other. `O_NOFOLLOW`
    /// answers "this name is a symlink now"; the identity check answers "this
    /// name resolved somewhere else", which is what a swap of an *already
    /// resolved ancestor* looks like from here while `openat` remains textual
    /// on SlateOS. See the module docs.
    ///
    /// `expected` must be the [`Stat`] the caller took of this same name, from
    /// this same [`Dir`], and must be a directory — the check is only as strong
    /// as the lookup it compares against.
    pub fn open_child(&self, name: &[u8], expected: &Stat) -> io::Result<Self> {
        let cname = c_name(name)?;
        // SAFETY: `cname` is NUL-terminated and outlives the call, which does
        // not retain the pointer. The mode argument is unused without
        // `O_CREAT`.
        let fd = unsafe {
            openat(
                self.0,
                cname.as_ptr(),
                oflag::RDONLY | oflag::DIRECTORY | oflag::NOFOLLOW | oflag::CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let opened = Self(fd);
        // Closed by `opened`'s drop on every path from here, which is why the
        // wrapper is built before the check rather than after it.
        if opened.stat_self()?.identity() != expected.identity() {
            return Err(swapped_underneath());
        }
        Ok(opened)
    }

    /// The directory this descriptor names, asked of the descriptor itself.
    ///
    /// Cannot be redirected by anything: there is no name in the question.
    pub fn stat_self(&self) -> io::Result<Stat> {
        let mut st = CStat::default();
        // SAFETY: `st` is a `CStat`, which is the layout both C libraries this
        // links against declare for `struct stat` (see the type's own comment);
        // the call fills it and retains neither pointer nor buffer.
        let rc = unsafe { fstat(self.0, &raw mut st) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Stat::from_cstat(&st))
    }

    /// Look `name` up in this directory, without following it if it is a link.
    pub fn stat(&self, name: &[u8]) -> io::Result<Stat> {
        let cname = c_name(name)?;
        let mut st = CStat::default();
        // SAFETY: `cname` is NUL-terminated and outlives the call; `st` is a
        // `CStat`, the layout declared for `struct stat`. The call fills it and
        // retains nothing.
        let rc = unsafe { fstatat(self.0, cname.as_ptr(), &raw mut st, AT_SYMLINK_NOFOLLOW) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Stat::from_cstat(&st))
    }

    /// Remove a non-directory entry.
    pub fn unlink(&self, name: &[u8]) -> io::Result<()> {
        self.unlink_with(name, 0)
    }

    /// Remove an empty directory entry.
    pub fn rmdir(&self, name: &[u8]) -> io::Result<()> {
        self.unlink_with(name, AT_REMOVEDIR)
    }

    fn unlink_with(&self, name: &[u8], flags: i32) -> io::Result<()> {
        let cname = c_name(name)?;
        // SAFETY: `cname` is NUL-terminated and outlives the call, which reads
        // it and does not retain it.
        let rc = unsafe { unlinkat(self.0, cname.as_ptr(), flags) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Whether `name` may be written by the *effective* user, or `None` if the
    /// question could not be answered.
    ///
    /// The effective id is the one that matters: `access(2)` asks about the
    /// real one, which for a setuid caller would answer a question nobody
    /// asked. `AT_EACCESS` is how the `*at` form says so.
    ///
    /// `None` rather than a bool for a failure, because the two callers of this
    /// so far want to say different things about it, and folding a failure into
    /// `false` here would take that choice away from them.
    #[must_use]
    pub fn writable(&self, name: &[u8]) -> Option<bool> {
        let cname = c_name(name).ok()?;
        // SAFETY: `cname` is NUL-terminated and outlives the call, which reads
        // it and does not retain it.
        let rc = unsafe { faccessat(self.0, cname.as_ptr(), W_OK, AT_EACCESS) };
        Some(rc == 0)
    }

    /// The names in this directory, in `readdir` order, with `.` and `..`
    /// dropped.
    ///
    /// The whole listing is read before it is returned, as `fts` does, so that
    /// a caller may remove entries while iterating what it got.
    ///
    /// Listed *through this descriptor*: on SlateOS `getdents` is pinned
    /// (`SYS_FS_GETDENTS_PINNED`, 664), so the directory being read is the one
    /// this handle names and not whatever its old path names now.
    pub fn names(&self) -> io::Result<Vec<Vec<u8>>> {
        // `fdopendir` takes ownership of the descriptor it is given and
        // `closedir` closes it, so it gets a duplicate. Handing it `self.0`
        // would close this `Dir` out from under its own `Drop`.
        //
        // `dup`, not a re-`openat` of `"."`: a re-open would resolve a name
        // again, which is the one thing this module does not do. A duplicate
        // is the same open file description by construction.
        //
        // SAFETY: `self.0` is an open descriptor owned by this value.
        let copy = unsafe { dup(self.0) };
        if copy < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `copy` is a freshly duplicated, open directory descriptor.
        // On success `fdopendir` owns it; on failure it does not, which is why
        // the failure arm closes it.
        let dirp = unsafe { fdopendir(copy) };
        if dirp.is_null() {
            let e = io::Error::last_os_error();
            // SAFETY: `fdopendir` failed, so `copy` is still ours to close.
            unsafe { close(copy) };
            return Err(e);
        }
        let out = read_all_names(dirp);
        // SAFETY: `dirp` came from `fdopendir` and has not been closed. This is
        // the one and only `closedir` of it, and it also closes `copy`.
        unsafe { closedir(dirp) };
        out
    }
}

/// Drain a `DIR*`, or report why it stopped early.
///
/// `errno` is cleared before each `readdir` because a null return means *either*
/// end-of-directory or an error, and the only thing that tells them apart is
/// whether `errno` moved. A read that failed halfway through must not be
/// mistaken for a short directory: for a caller removing what it lists, that
/// would be a silent partial removal reported as success.
#[cfg(unix)]
fn read_all_names(dirp: *mut CDir) -> io::Result<Vec<Vec<u8>>> {
    let mut names: Vec<Vec<u8>> = Vec::new();
    loop {
        set_errno(0);
        // SAFETY: `dirp` is a live `DIR*` from `fdopendir`. The returned
        // pointer, when non-null, is owned by the stream and is valid until the
        // next `readdir` or `closedir`, both of which happen after this
        // iteration has finished copying out of it.
        let ent = unsafe { readdir(dirp) };
        if ent.is_null() {
            let e = io::Error::last_os_error();
            return match e.raw_os_error() {
                Some(0) | None => Ok(names),
                _ => Err(e),
            };
        }
        // SAFETY: `ent` is non-null and points at a `struct dirent` owned by
        // the stream; `d_name` is a NUL-terminated array inside it.
        let name = unsafe { name_of(ent) };
        if name == b"." || name == b".." {
            continue;
        }
        names.push(name);
    }
}

/// Copy a directory entry's name out of the stream's buffer.
///
/// # Safety
///
/// `ent` must be a non-null pointer to a live `struct dirent`.
#[cfg(unix)]
unsafe fn name_of(ent: *const CDirent) -> Vec<u8> {
    // SAFETY: the caller guarantees `ent` is live; `d_name` is an array within
    // it, so the raw field access is in bounds and the array is NUL-terminated
    // by the C library's contract.
    let bytes = unsafe { &(*ent).d_name };
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    bytes.get(..end).unwrap_or(&[]).to_vec()
}

/// Set `errno`, so that a null `readdir` can be told from a failing one.
#[cfg(unix)]
fn set_errno(value: i32) {
    unsafe extern "C" {
        fn __errno_location() -> *mut i32;
    }
    // SAFETY: `__errno_location` returns a pointer to this thread's `errno`,
    // which is valid for the life of the thread and is what the C library
    // itself writes through.
    unsafe { *__errno_location() = value };
}

// ------------------------------------------------------------ not unix --

/// A directory named by path.
///
/// The join this module exists to avoid, accepted here because there is no
/// `openat` to avoid it with. See the module docs: off unix these utilities are
/// a test vehicle, not a shipping one.
#[cfg(not(unix))]
#[derive(Debug)]
pub struct Dir(std::path::PathBuf);

#[cfg(not(unix))]
impl Dir {
    /// Remember a walk's starting point.
    ///
    /// `expected` is checked as far as this platform can — that it is still a
    /// directory. See [`Dir::open_child`] for why the identity half is missing
    /// rather than approximated.
    pub fn open_root(path: &Path, expected: &Stat) -> io::Result<Self> {
        // Look it up, so that a missing or unreadable directory fails here
        // rather than at the first listing — which is where the unix twin
        // fails, and the callers' error messages depend on the order.
        if Stat::of_path(path)?.kind() != expected.kind() {
            return Err(swapped_underneath());
        }
        Ok(Self(path.to_path_buf()))
    }

    /// Descend into `name`.
    ///
    /// `expected` is checked as far as this platform can: it must still be a
    /// directory. The identity half is not available, and pretending otherwise
    /// with a canonical-path comparison would be a different check wearing this
    /// one's name.
    pub fn open_child(&self, name: &[u8], expected: &Stat) -> io::Result<Self> {
        let _ = c_name(name)?;
        let child = self.child_path(name);
        let now = Stat::of_path(&child)?;
        if now.kind() != expected.kind() {
            return Err(swapped_underneath());
        }
        Ok(Self(child))
    }

    /// The directory this value names.
    pub fn stat_self(&self) -> io::Result<Stat> {
        Stat::of_path(&self.0)
    }

    /// Look `name` up in this directory, without following it if it is a link.
    pub fn stat(&self, name: &[u8]) -> io::Result<Stat> {
        let _ = c_name(name)?;
        Stat::of_path(&self.child_path(name))
    }

    /// Remove a non-directory entry.
    pub fn unlink(&self, name: &[u8]) -> io::Result<()> {
        let _ = c_name(name)?;
        std::fs::remove_file(self.child_path(name))
    }

    /// Remove an empty directory entry.
    pub fn rmdir(&self, name: &[u8]) -> io::Result<()> {
        let _ = c_name(name)?;
        std::fs::remove_dir(self.child_path(name))
    }

    /// Unanswerable off unix: there is no `euidaccess`, and a read-only
    /// attribute is not the same question.
    #[must_use]
    pub fn writable(&self, _name: &[u8]) -> Option<bool> {
        None
    }

    /// The names in this directory, with `.` and `..` already absent from what
    /// `read_dir` yields.
    pub fn names(&self) -> io::Result<Vec<Vec<u8>>> {
        let mut names: Vec<Vec<u8>> = Vec::new();
        for entry in std::fs::read_dir(&self.0)? {
            names.push(os_bytes(&entry?.file_name()).into_owned());
        }
        Ok(names)
    }

    fn child_path(&self, name: &[u8]) -> std::path::PathBuf {
        self.0.join(os_from_bytes(name))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::expect_used
)]
mod tests {
    use super::*;

    /// A scratch directory that removes itself.
    struct Temp(std::path::PathBuf);

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp(tag: &str) -> Temp {
        let mut p = std::env::temp_dir();
        p.push(format!("coreutils-dirfd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }

    /// `Dir::open_root` with the lookup every real caller has already done.
    fn open_root(path: &std::path::Path) -> Dir {
        let st = Stat::of_path(path).unwrap();
        Dir::open_root(path, &st).unwrap()
    }

    #[test]
    fn names_lists_children_and_omits_dot_entries() {
        let t = temp("names");
        std::fs::write(t.0.join("a"), b"x").unwrap();
        std::fs::create_dir(t.0.join("b")).unwrap();
        let dir = open_root(&t.0);
        let mut got = dir.names().unwrap();
        got.sort();
        assert_eq!(got, vec![b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn stat_reports_kind_and_size() {
        let t = temp("stat");
        std::fs::write(t.0.join("a"), b"hello").unwrap();
        std::fs::create_dir(t.0.join("d")).unwrap();
        let dir = open_root(&t.0);
        let a = dir.stat(b"a").unwrap();
        assert_eq!(a.kind(), Kind::Regular);
        assert_eq!(a.size(), 5);
        assert!(dir.stat(b"d").unwrap().is_dir());
    }

    #[test]
    fn stat_of_a_missing_name_is_not_found() {
        let t = temp("missing");
        let dir = open_root(&t.0);
        let e = dir.stat(b"nope").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }

    /// A name with a separator in it is a walk, and this module does not walk.
    /// The refusal is what stops a caller quietly reintroducing the very
    /// multi-component resolution the module exists to remove.
    #[test]
    fn a_name_with_a_slash_is_refused() {
        let t = temp("slash");
        std::fs::create_dir_all(t.0.join("d/e")).unwrap();
        let dir = open_root(&t.0);
        assert_eq!(
            dir.stat(b"d/e").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            dir.unlink(b"d/e").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_name_with_a_nul_is_refused() {
        let t = temp("nul");
        let dir = open_root(&t.0);
        assert_eq!(
            dir.stat(b"a\0b").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn unlink_and_rmdir_remove_the_right_thing() {
        let t = temp("remove");
        std::fs::write(t.0.join("f"), b"").unwrap();
        std::fs::create_dir(t.0.join("d")).unwrap();
        let dir = open_root(&t.0);
        dir.unlink(b"f").unwrap();
        dir.rmdir(b"d").unwrap();
        assert!(dir.names().unwrap().is_empty());
    }

    /// `rmdir` must refuse a plain file and `unlink` must refuse a directory,
    /// because the two errnos are what the callers turn into their messages.
    #[test]
    fn the_two_removals_are_not_interchangeable() {
        let t = temp("removekind");
        std::fs::write(t.0.join("f"), b"").unwrap();
        std::fs::create_dir(t.0.join("d")).unwrap();
        let dir = open_root(&t.0);
        assert!(dir.rmdir(b"f").is_err());
        assert!(dir.unlink(b"d").is_err());
    }

    #[test]
    fn descending_reaches_the_child_and_only_the_child() {
        let t = temp("descend");
        std::fs::create_dir(t.0.join("d")).unwrap();
        std::fs::write(t.0.join("d/inner"), b"").unwrap();
        std::fs::write(t.0.join("outer"), b"").unwrap();
        let dir = open_root(&t.0);
        let expected = dir.stat(b"d").unwrap();
        let child = dir.open_child(b"d", &expected).unwrap();
        assert_eq!(child.names().unwrap(), vec![b"inner".to_vec()]);
    }

    /// The check that makes a textual `openat` safe. Not a race — the swap is
    /// done deliberately between the lookup and the descent, which is exactly
    /// the state an attacker who wins the race leaves behind.
    ///
    /// Unix only, and deliberately so rather than by omission: off unix a
    /// [`Stat`] has no `(dev, ino)` to compare, so `open_child` falls back to
    /// comparing the *kind* — and a directory swapped for another directory is
    /// the one swap that comparison cannot see. Asserting a refusal there would
    /// be asserting a guarantee this module does not make off unix, and the
    /// honest place to say so is here. See
    /// `opening_a_root_that_was_swapped_is_refused`, which pins both halves.
    #[cfg(unix)]
    #[test]
    fn descending_into_a_name_that_was_swapped_is_refused() {
        let t = temp("swap");
        std::fs::create_dir(t.0.join("real")).unwrap();
        std::fs::create_dir(t.0.join("other")).unwrap();
        let dir = open_root(&t.0);
        let expected = dir.stat(b"real").unwrap();
        // The attacker's move: `real` now names a different directory.
        std::fs::remove_dir(t.0.join("real")).unwrap();
        std::fs::rename(t.0.join("other"), t.0.join("real")).unwrap();
        let e = dir.open_child(b"real", &expected).unwrap_err();
        assert_eq!(
            e.raw_os_error(),
            Some(ESTALE),
            "expected the identity check to refuse, got {e:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn descending_into_a_symlink_is_refused() {
        let t = temp("symlink");
        std::fs::create_dir(t.0.join("target")).unwrap();
        std::os::unix::fs::symlink("target", t.0.join("link")).unwrap();
        let dir = open_root(&t.0);
        let st = dir.stat(b"link").unwrap();
        assert!(st.is_symlink(), "the lookup must not follow the link");
        // A caller would not descend into something it just classified as a
        // link; `O_NOFOLLOW` is the backstop for the one that changed since.
        let target = dir.stat(b"target").unwrap();
        assert!(dir.open_child(b"link", &target).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_unlinked_not_followed() {
        let t = temp("unlinklink");
        std::fs::write(t.0.join("victim"), b"still here").unwrap();
        std::os::unix::fs::symlink("victim", t.0.join("link")).unwrap();
        let dir = open_root(&t.0);
        dir.unlink(b"link").unwrap();
        assert_eq!(std::fs::read(t.0.join("victim")).unwrap(), b"still here");
    }

    /// The root is resolved by path — it is the one step that must be — so it
    /// is the step where a swap between the lookup and the open is most worth
    /// catching.
    #[test]
    fn opening_a_root_that_was_swapped_is_refused() {
        let t = temp("swaproot");
        std::fs::create_dir(t.0.join("real")).unwrap();
        std::fs::create_dir(t.0.join("other")).unwrap();
        let expected = Stat::of_path(&t.0.join("real")).unwrap();
        std::fs::remove_dir(t.0.join("real")).unwrap();
        std::fs::rename(t.0.join("other"), t.0.join("real")).unwrap();
        let opened = Dir::open_root(&t.0.join("real"), &expected);
        if cfg!(unix) {
            assert_eq!(opened.err().and_then(|e| e.raw_os_error()), Some(ESTALE));
        } else {
            // Off unix only the kind is comparable, and both are directories.
            assert!(opened.is_ok());
        }
    }

    #[test]
    fn stat_self_names_the_open_directory() {
        let t = temp("statself");
        std::fs::create_dir(t.0.join("d")).unwrap();
        let dir = open_root(&t.0);
        let by_name = dir.stat(b"d").unwrap();
        let child = dir.open_child(b"d", &by_name).unwrap();
        assert!(child.stat_self().unwrap().is_dir());
        assert_eq!(child.stat_self().unwrap().identity(), by_name.identity());
    }
}
