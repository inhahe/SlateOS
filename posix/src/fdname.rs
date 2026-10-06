//! The names that stand for this process's own descriptors -- `/dev/fd/N`,
//! `/dev/stdin`, `/dev/stdout`, `/dev/stderr` and `/proc/self/fd/N` --
//! answered by this library, since only it knows a native program's
//! descriptors.
//!
//! On Linux these are the kernel's: `/dev/fd` is a symbolic link to
//! `/proc/self/fd`, whose entries are "magic" links to what each descriptor
//! holds, and `/dev/stdin`, `/dev/stdout` and `/dev/stderr` link to
//! `/proc/self/fd/0`, `1` and `2`. A native SlateOS program's descriptors
//! are this library's table over capability handles, which the kernel does
//! not see: its devfs has no `fd`, its `stdin`, `stdout` and `stderr` are
//! the console, and its `/proc/<pid>/fd` lists nothing for a native process
//! (`known-issues-resolved/D-POSIX-NATIVE-PROGRAMS-HAVE-NO-DEV-FD.md`). So
//! this library answers the names before the kernel is asked, as it answers
//! `/dev/ptmx` and `/dev/pts/<n>` ([`crate::file`]'s `open_pty_device`).
//!
//! # What each call answers: Linux's, measured
//!
//! WSL2, Linux 6.6, 2026-10-05:
//!
//! | call | Linux's answer |
//! |---|---|
//! | `open` of `/dev/fd/N` | a **reopen**, not a duplicate: a new open file description. A file starts at offset 0 whatever N's is, `O_TRUNC` truncates it, and the access mode may differ from N's as far as the file's permissions allow; a pipe end is the same pipe, in either direction |
//! | `open` of a closed descriptor's | `ENOENT` |
//! | `stat` | the object's: a pipe's is a FIFO, so `[ -p /dev/stdin ]` asks the right question |
//! | `lstat` of `/dev/fd/N` | a symbolic link, mode 0500, 0300 or 0700 as N reads, writes or both |
//! | `lstat` of `/dev/fd`, `/dev/stdin` | a symbolic link, 0777 |
//! | `stat` of `/dev/fd` | a directory, 0500 |
//! | `readlink` | `/proc/self/fd` for `/dev/fd`, `/proc/self/fd/0` for `/dev/stdin`; for `/dev/fd/N` the file's path, or `pipe:[ino]`, `socket:[ino]`, `anon_inode:[eventfd]`, ... |
//! | `opendir` of `/dev/fd` | `.`, `..`, and one link per open descriptor, named by its number |
//!
//! # Where it differs, each on purpose
//!
//! * A pipe end reopened for the other direction is `EACCES`: Linux reopens
//!   the pipe's inode, and this library holds only one end's handle.
//! * A file asked for more access than N has, after the file was renamed or
//!   unlinked, is `EACCES`: Linux reopens the inode, and this library can
//!   widen access only by opening the path N was opened by, and only while
//!   that path is still the same file. Asked for no more access than N has,
//!   a renamed or unlinked file reopens as on Linux (`SYS_FS_DUP`).
//! * Terminals, sockets, eventfds and the rest are duplicated, so the new
//!   descriptor shares N's status flags (`O_NONBLOCK`, `O_APPEND`), where
//!   Linux's reopen would give it its own.

use crate::errno;
use crate::fcntl::{O_ACCMODE, O_RDONLY, O_RDWR, O_WRONLY};
use crate::fdtable::{FdEntry, HandleKind};

/// What a resolved path names, among this module's names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FdName<'a> {
    /// `/dev/fd` itself: a symbolic link to `/proc/self/fd`.
    DevFd,
    /// The directory of descriptors: `/proc/self/fd`, `/proc/thread-self/fd`,
    /// `/proc/<this process's id>/fd`, and `/dev/fd/`.
    Dir,
    /// `/dev/stdin`, `/dev/stdout`, `/dev/stderr`: symbolic links to
    /// `/proc/self/fd/0`, `1` and `2`.
    Std(i32),
    /// `/dev/fd/N` or `/proc/self/fd/N`: the link to descriptor N's object.
    Fd(i32),
    /// A name in the directory that is no descriptor's number: `ENOENT`.
    Missing,
    /// A path through descriptor N's link and on below it: what follows the
    /// slash, which may be empty (`/dev/fd/3/` asks that 3 be a directory).
    Through(i32, &'a [u8]),
}

/// The number a `/proc/<pid>/fd` entry's name is, as Linux reads one
/// (`name_to_int`): decimal digits, no sign, no leading zero but `0`
/// itself, within `i32`. `None` names no descriptor.
fn fd_number(name: &[u8]) -> Option<i32> {
    let (&first, rest) = name.split_first()?;
    if first == b'0' && !rest.is_empty() {
        return None;
    }
    name.iter().try_fold(0i32, |n, &b| {
        let d = b.checked_sub(b'0').filter(|d| *d <= 9)?;
        n.checked_mul(10)?.checked_add(i32::from(d))
    })
}

/// An entry of the directory of descriptors: `N`, `N/rest`, or a name that
/// is no descriptor's. Empty is the directory itself (`/dev/fd/`).
fn entry(rest: &[u8]) -> FdName<'_> {
    if rest.is_empty() {
        return FdName::Dir;
    }
    let (num, below) = match rest.iter().position(|&b| b == b'/') {
        Some(i) => (
            rest.get(..i).unwrap_or_default(),
            Some(rest.get(i.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (rest, None),
    };
    match (fd_number(num), below) {
        (Some(n), None) => FdName::Fd(n),
        (Some(n), Some(b)) => FdName::Through(n, b),
        (None, _) => FdName::Missing,
    }
}

/// What `resolved` -- an absolute, normalised path, as this library's
/// resolver makes them -- names among this module's names, or `None` for a
/// path the filesystem answers. `my_pid` is this process's id, which names
/// its own `/proc/<pid>/fd` as `self` does; another process's is the
/// kernel's to answer.
pub(crate) fn classify(resolved: &[u8], my_pid: i32) -> Option<FdName<'_>> {
    if resolved == b"/dev/fd" {
        return Some(FdName::DevFd);
    }
    if let Some(rest) = resolved.strip_prefix(b"/dev/fd/".as_slice()) {
        return Some(entry(rest));
    }
    for (name, fd) in [
        (b"/dev/stdin".as_slice(), 0),
        (b"/dev/stdout".as_slice(), 1),
        (b"/dev/stderr".as_slice(), 2),
    ] {
        if resolved == name {
            return Some(FdName::Std(fd));
        }
        if let Some(rest) = resolved
            .strip_prefix(name)
            .and_then(|r| r.strip_prefix(b"/".as_slice()))
        {
            return Some(FdName::Through(fd, rest));
        }
    }
    let after = resolved.strip_prefix(b"/proc/".as_slice())?;
    let slash = after.iter().position(|&b| b == b'/')?;
    let who = after.get(..slash)?;
    let tail = after.get(slash.saturating_add(1)..)?;
    let mine = who == b"self" || who == b"thread-self" || fd_number(who) == Some(my_pid);
    if !mine {
        return None;
    }
    if tail == b"fd" {
        return Some(FdName::Dir);
    }
    tail.strip_prefix(b"fd/".as_slice()).map(entry)
}

/// The descriptor a resolved path's link leads to: `/dev/fd/N`,
/// `/proc/self/fd/N` and `/dev/stdin` and kin; `None` for anything else.
pub(crate) fn descriptor_of(resolved: &[u8]) -> Option<i32> {
    match classify(resolved, crate::process::getpid())? {
        FdName::Fd(n) | FdName::Std(n) => Some(n),
        _ => None,
    }
}

/// Whether access `want` (an `O_ACCMODE` value) is within what a descriptor
/// opened for `have` may give without asking the file again.
fn access_within(want: i32, have: i32) -> bool {
    want & O_ACCMODE == have & O_ACCMODE || have & O_ACCMODE == O_RDWR
}

/// `lstat`'s permission bits for descriptor N's link, as Linux gives them:
/// read and search for a descriptor open for reading, write and search for
/// writing, all three for both.
fn link_mode(status_flags: i32) -> u32 {
    match status_flags & O_ACCMODE {
        O_RDONLY => 0o500,
        O_WRONLY => 0o300,
        _ => 0o700,
    }
}

/// Copy as much of `text` as fits into `out`; the full length.
fn put(out: &mut [u8], text: &[u8]) -> usize {
    let n = text.len().min(out.len());
    if let (Some(dst), Some(src)) = (out.get_mut(..n), text.get(..n)) {
        dst.copy_from_slice(src);
    }
    text.len()
}

/// `prefix`, `value` in decimal, `suffix`, into `out`; the full length.
fn put_tagged(out: &mut [u8], prefix: &[u8], value: u64, suffix: &[u8]) -> usize {
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    let mut v = value;
    loop {
        i = i.saturating_sub(1);
        if let Some(d) = digits.get_mut(i) {
            // `v % 10` is below 10, so it fits a `u8`, and `b'0'` plus it is
            // at most `b'9'`.
            *d = b'0'.saturating_add(u8::try_from(v % 10).unwrap_or_default());
        }
        v /= 10;
        if v == 0 || i == 0 {
            break;
        }
    }
    let number = digits.get(i..).unwrap_or_default();
    let mut text = [0u8; 64];
    let mut len = 0usize;
    for part in [prefix, number, suffix] {
        for &b in part {
            if let Some(slot) = text.get_mut(len) {
                *slot = b;
                len = len.saturating_add(1);
            }
        }
    }
    put(out, text.get(..len).unwrap_or_default())
}

/// What `readlink` answers for descriptor N's link: Linux's name for the
/// object. A file's path is the caller's (`fdtable::get_fd_path`), since
/// only it has the table; everything else is named here. Writes as much as
/// fits into `out` and returns the full length.
pub(crate) fn link_target_of_kind(entry: &FdEntry, out: &mut [u8]) -> Option<usize> {
    Some(match entry.kind {
        HandleKind::File => return None,
        HandleKind::Pipe => put_tagged(out, b"pipe:[", entry.handle, b"]"),
        HandleKind::TcpStream
        | HandleKind::TcpListener
        | HandleKind::UdpSocket
        | HandleKind::UnixStream => put_tagged(out, b"socket:[", entry.handle, b"]"),
        HandleKind::Console => put(out, b"/dev/console"),
        HandleKind::PtyMaster => put(out, b"/dev/ptmx"),
        HandleKind::PtySlave => match crate::ioctl::slave_id_of(entry.handle) {
            Some(id) => put_tagged(out, b"/dev/pts/", u64::from(id), b""),
            None => put(out, b"/dev/pts/"),
        },
        HandleKind::Eventfd => put(out, b"anon_inode:[eventfd]"),
        HandleKind::Epoll => put(out, b"anon_inode:[eventpoll]"),
        HandleKind::Timerfd => put(out, b"anon_inode:[timerfd]"),
        HandleKind::Inotify => put(out, b"anon_inode:inotify"),
    })
}

/// One record of the packed listing format `crate::dirent` decodes:
/// `u8 type | u32 name_len | name | u64 size | u64 ino`, little-endian.
/// Appended to `buf` at `at`; the next offset, or `None` when it does not
/// fit.
fn put_record(buf: &mut [u8], at: usize, kind: u8, name: &[u8], ino: u64) -> Option<usize> {
    let len = u32::try_from(name.len()).ok()?;
    let end = at.checked_add(21)?.checked_add(name.len())?;
    let rec = buf.get_mut(at..end)?;
    let (head, rest) = rec.split_at_mut(5);
    head.first_mut().map(|b| *b = kind)?;
    head.get_mut(1..5)?.copy_from_slice(&len.to_le_bytes());
    let (nm, tail) = rest.split_at_mut(name.len());
    nm.copy_from_slice(name);
    tail.get_mut(..8)?.copy_from_slice(&0u64.to_le_bytes());
    tail.get_mut(8..16)?.copy_from_slice(&ino.to_le_bytes());
    Some(end)
}

/// The listing of the directory of descriptors, into `buf`: `.` and `..`,
/// then one symbolic link per descriptor in `open`, named by its number --
/// what `opendir("/dev/fd")` reads on Linux. Each has an inode number of its
/// own (0x100 plus the descriptor's), never 0, which some readers take for
/// an empty slot. Returns the length used, or `None` when `buf` is short.
pub(crate) fn listing(open: impl Iterator<Item = i32>, buf: &mut [u8]) -> Option<usize> {
    let mut at = put_record(buf, 0, crate::dirent::KERNEL_TYPE_DIR, b".", 1)?;
    at = put_record(buf, at, crate::dirent::KERNEL_TYPE_DIR, b"..", 2)?;
    for fd in open {
        let mut digits = [0u8; 12];
        let n = put_tagged(&mut digits, b"", u64::try_from(fd).ok()?, b"");
        let name = digits.get(..n)?;
        let ino = 0x100u64.checked_add(u64::try_from(fd).ok()?)?;
        at = put_record(buf, at, crate::dirent::KERNEL_TYPE_SYMLINK, name, ino)?;
    }
    Some(at)
}

/// The bytes a [`listing`] of `count` descriptors needs.
pub(crate) fn listing_len(count: usize) -> Option<usize> {
    // ".", "..", and up to ten digits a name.
    21usize
        .checked_add(1)?
        .checked_add(21 + 2)?
        .checked_add(count.checked_mul(21 + 10)?)
}

/// Set errno to `e` and answer -1.
pub(crate) fn fail(e: i32) -> i32 {
    errno::set_errno(e);
    -1
}

// ---------------------------------------------------------------------------
// The calls (draft: the call sites are in file.rs, dirent.rs)
// ---------------------------------------------------------------------------

/// The path the directory of descriptors is opened as. A descriptor holding
/// it is that directory: `fdopendir` lists this library's descriptors for
/// it, and the `*at` calls resolve names relative to it here.
pub(crate) const DIR_MARK: &[u8] = b"/proc/self/fd";

/// Whether `fd` is a descriptor for the directory of descriptors.
pub(crate) fn is_descriptor_dir(fd: i32) -> bool {
    let mut buf = [0u8; 16];
    let len = crate::fdtable::get_fd_path(fd, &mut buf);
    buf.get(..len) == Some(DIR_MARK)
}

/// `<descriptor n's path>/<rest>`, NUL-terminated, into `buf`: where a name
/// through n's link goes on. Linux's errno when it goes nowhere: `ENOENT`
/// for a closed n, `ENOTDIR` for one with no path (a pipe, a terminal).
/// The slash is always added, so an empty `rest` asks the kernel for n's
/// file as a directory -- `ENOTDIR` if it is not one.
fn through_path(n: i32, rest: &[u8], buf: &mut [u8]) -> Result<(), i32> {
    if crate::fdtable::get_fd(n).is_none() {
        return Err(errno::ENOENT);
    }
    let len = crate::fdtable::get_fd_path(n, buf);
    if len == 0 {
        return Err(errno::ENOTDIR);
    }
    let end = len
        .checked_add(1)
        .and_then(|x| x.checked_add(rest.len()))
        .ok_or(errno::ENAMETOOLONG)?;
    let tail = buf.get_mut(len..=end).ok_or(errno::ENAMETOOLONG)?;
    let (slash, more) = tail.split_at_mut(1);
    slash.copy_from_slice(b"/");
    let (name, nul) = more.split_at_mut(rest.len());
    name.copy_from_slice(rest);
    nul.copy_from_slice(b"\0");
    Ok(())
}

/// `open` of one of this module's names; `None` for a path that is not one.
pub(crate) fn open(resolved: &[u8], flags: i32, mode: crate::types::ModeT) -> Option<i32> {
    use crate::fcntl::{O_CLOEXEC, O_CREAT, O_DIRECTORY, O_EXCL};
    let name = classify(resolved, crate::process::getpid())?;
    Some(match name {
        FdName::DevFd | FdName::Dir => open_dir(flags),
        FdName::Missing => fail(errno::ENOENT),
        FdName::Through(n, rest) => {
            let mut path = [0u8; crate::unistd::PATH_MAX];
            match through_path(n, rest, &mut path) {
                Ok(()) => crate::file::open(path.as_ptr(), flags, mode),
                Err(e) => fail(e),
            }
        }
        FdName::Std(n) | FdName::Fd(n) => {
            let Some(entry) = crate::fdtable::get_fd(n) else {
                return Some(fail(errno::ENOENT));
            };
            // The link exists, so O_CREAT|O_EXCL finds it there.
            if flags & O_CREAT != 0 && flags & O_EXCL != 0 {
                return Some(fail(errno::EEXIST));
            }
            let fd = if entry.kind == HandleKind::File {
                reopen_file(n, &entry, flags)
            } else if flags & O_DIRECTORY != 0 {
                fail(errno::ENOTDIR)
            } else if !access_within(flags, entry.status_flags) {
                fail(errno::EACCES)
            } else {
                crate::file::dup(n)
            };
            if fd >= 0 && flags & O_CLOEXEC != 0 {
                // A descriptor just made is in the table, so this cannot fail.
                let _ = crate::fdtable::set_fd_flags(fd, crate::fdtable::FD_CLOEXEC);
            }
            fd
        }
    })
}

/// Descriptor n's file again, as Linux's reopen gives it: a new open file
/// description at offset 0.
fn reopen_file(n: i32, entry: &FdEntry, flags: i32) -> i32 {
    use crate::fcntl::{
        O_APPEND, O_CREAT, O_DIRECTORY, O_EXCL, O_NOFOLLOW, O_NONBLOCK, O_PATH, O_SYNC, O_TRUNC,
    };
    if flags & O_DIRECTORY != 0 && !is_directory(n) {
        return fail(errno::ENOTDIR);
    }
    if !access_within(flags, entry.status_flags) {
        // More access than n has: only the file can grant it, through the
        // path n was opened by, and only while that path is still the file.
        let mut path = [0u8; crate::unistd::PATH_MAX];
        let len = crate::fdtable::get_fd_path(n, &mut path);
        if len == 0 || path.get(len).is_none() {
            return fail(errno::EACCES);
        }
        if let Some(nul) = path.get_mut(len) {
            *nul = 0;
        }
        if !same_file(n, &path) {
            return fail(errno::EACCES);
        }
        return crate::file::open(path.as_ptr(), flags & !(O_CREAT | O_EXCL), 0);
    }
    // SYS_FS_DUP: a new handle on the same file, with the same access and a
    // cursor of its own -- which starts where n's stands, so it is moved to
    // 0, where Linux's reopen starts.
    let h = crate::syscall::syscall1(crate::syscall::SYS_FS_DUP, entry.handle);
    if h < 0 {
        return errno::translate(h) as i32;
    }
    let status = flags & (O_ACCMODE | O_APPEND | O_NONBLOCK | O_SYNC | O_NOFOLLOW | O_PATH);
    #[allow(clippy::cast_sign_loss)]
    let handle = h as u64;
    let Some(fd) = crate::fdtable::alloc_fd_with_flags(HandleKind::File, handle, status) else {
        // Nothing holds the new handle yet, so closing it loses nothing.
        let _ = crate::syscall::syscall1(crate::syscall::SYS_FS_CLOSE, handle);
        return fail(errno::EMFILE);
    };
    crate::fdtable::copy_fd_path(n, fd);
    // A directory or a file that cannot seek answers an error here, and
    // stands at 0 already.
    let _ = crate::file::lseek(fd, 0, crate::fcntl::SEEK_SET);
    if flags & O_TRUNC != 0 && flags & O_ACCMODE != O_RDONLY && crate::file::ftruncate(fd, 0) != 0 {
        let e = errno::get_errno();
        let _ = crate::file::close(fd);
        return fail(e);
    }
    fd
}

/// Whether descriptor n holds a directory.
fn is_directory(n: i32) -> bool {
    let mut st = crate::stat::Stat::zeroed();
    crate::file::fstat(n, &raw mut st) == 0
        && st.st_mode & crate::fcntl::S_IFMT == crate::fcntl::S_IFDIR
}

/// Whether the NUL-terminated `path` is still descriptor n's file.
fn same_file(n: i32, path: &[u8]) -> bool {
    let mut a = crate::stat::Stat::zeroed();
    let mut b = crate::stat::Stat::zeroed();
    crate::file::fstat(n, &raw mut a) == 0
        && crate::file::stat(path.as_ptr(), &raw mut b) == 0
        && (a.st_dev, a.st_ino) == (b.st_dev, b.st_ino)
}

/// The directory of descriptors, opened: a real directory descriptor -- the
/// kernel's own `/proc/<pid>/fd`, which exists for every process and lists
/// nothing for a native one -- carrying [`DIR_MARK`] as its path, by which
/// `fdopendir` and the `*at` calls know it.
fn open_dir(flags: i32) -> i32 {
    use crate::fcntl::{O_CLOEXEC, O_DIRECTORY};
    if flags & O_ACCMODE != O_RDONLY {
        return fail(errno::EISDIR);
    }
    let mut path = [0u8; 32];
    #[allow(clippy::cast_sign_loss)]
    let pid = crate::process::getpid() as u64;
    let len = put_tagged(&mut path, b"/proc/", pid, b"/fd");
    let Some(resolved) = path.get(..len) else {
        return fail(errno::ENAMETOOLONG);
    };
    let fd = crate::file::open_resolved(resolved, O_RDONLY | O_DIRECTORY | (flags & O_CLOEXEC), 0);
    if fd >= 0 {
        crate::fdtable::store_fd_path(fd, DIR_MARK.as_ptr(), DIR_MARK.len());
    }
    fd
}

/// A `struct stat` for one of the names that is no file: a link or the
/// directory. Owned by this process, on no device.
fn fill(buf: *mut crate::stat::Stat, mode: u32, nlink: u64, size: i64, ino: u64) -> i32 {
    // SAFETY: the caller checked `buf` non-null; the caller of stat/lstat
    // asserts it points to a writable `struct stat`.
    let st = unsafe { &mut *buf };
    *st = crate::stat::Stat::zeroed();
    st.st_mode = mode;
    st.st_nlink = nlink;
    st.st_size = size;
    st.st_ino = ino;
    st.st_uid = crate::unistd::getuid();
    st.st_gid = crate::unistd::getgid();
    st.st_blksize = 1024;
    0
}

/// `stat` (`follow`) or `lstat` of one of this module's names into `buf`;
/// `None` for a path that is not one.
pub(crate) fn stat(resolved: &[u8], buf: *mut crate::stat::Stat, follow: bool) -> Option<i32> {
    use crate::fcntl::{S_IFDIR, S_IFLNK};
    let name = classify(resolved, crate::process::getpid())?;
    if buf.is_null() {
        return Some(fail(errno::EFAULT));
    }
    Some(match name {
        FdName::Fd(n) | FdName::Std(n) => match crate::fdtable::get_fd(n) {
            None => fail(errno::ENOENT),
            // Through the link: the object, exactly as fstat answers for it.
            Some(_) if follow => crate::file::fstat(n, buf),
            // The link itself. /proc/self/fd/N's is 64 bytes long and moded
            // by N's access; /dev/stdin's is "/proc/self/fd/0", 0777.
            Some(e) => match name {
                FdName::Fd(_) => fill(buf, S_IFLNK | link_mode(e.status_flags), 1, 64, fd_ino(n)),
                _ => fill(buf, S_IFLNK | 0o777, 1, 15, fd_ino(n)),
            },
        },
        // "/proc/self/fd", 13 bytes.
        FdName::DevFd if !follow => fill(buf, S_IFLNK | 0o777, 1, 13, 3),
        FdName::DevFd | FdName::Dir => fill(buf, S_IFDIR | 0o500, 2, 0, 1),
        FdName::Missing => fail(errno::ENOENT),
        FdName::Through(n, rest) => {
            let mut path = [0u8; crate::unistd::PATH_MAX];
            match through_path(n, rest, &mut path) {
                Ok(()) if follow => crate::file::stat(path.as_ptr(), buf),
                Ok(()) => crate::file::lstat(path.as_ptr(), buf),
                Err(e) => fail(e),
            }
        }
    })
}

/// The inode number descriptor n's link lists under ([`listing`]).
fn fd_ino(n: i32) -> u64 {
    0x100u64.saturating_add(u64::try_from(n).unwrap_or_default())
}

/// `access` of one of this module's names; `None` for a path that is not
/// one. Through a descriptor's link, Linux asks the object: a file's own
/// permissions (by its path, while it has one), and for a pipe, terminal or
/// socket, read and write but not execute.
pub(crate) fn access(resolved: &[u8], mode: i32) -> Option<i32> {
    use crate::fcntl::{F_OK, W_OK, X_OK};
    let name = classify(resolved, crate::process::getpid())?;
    Some(match name {
        FdName::Fd(n) | FdName::Std(n) => match crate::fdtable::get_fd(n) {
            None => fail(errno::ENOENT),
            Some(_) if mode == F_OK => 0,
            Some(e) if e.kind == HandleKind::File => {
                let mut path = [0u8; crate::unistd::PATH_MAX];
                let len = crate::fdtable::get_fd_path(n, &mut path);
                match path.get_mut(len) {
                    Some(nul) if len > 0 => {
                        *nul = 0;
                        crate::file::access(path.as_ptr(), mode)
                    }
                    _ => fail(errno::EACCES),
                }
            }
            Some(_) if mode & X_OK != 0 => fail(errno::EACCES),
            Some(_) => 0,
        },
        // 0500: read and search, no write.
        FdName::DevFd | FdName::Dir if mode & W_OK != 0 => fail(errno::EACCES),
        FdName::DevFd | FdName::Dir => 0,
        FdName::Missing => fail(errno::ENOENT),
        FdName::Through(n, rest) => {
            let mut path = [0u8; crate::unistd::PATH_MAX];
            match through_path(n, rest, &mut path) {
                Ok(()) => crate::file::access(path.as_ptr(), mode),
                Err(e) => fail(e),
            }
        }
    })
}

/// `readlink` of one of this module's names into `out`; `None` for a path
/// that is not one. Like `readlink`, the answer is cut to `out` and not
/// NUL-terminated, and the count is of the bytes placed.
pub(crate) fn readlink(resolved: &[u8], out: &mut [u8]) -> Option<isize> {
    let name = classify(resolved, crate::process::getpid())?;
    let full = match name {
        FdName::DevFd => put(out, DIR_MARK),
        FdName::Std(n) => put_tagged(
            out,
            b"/proc/self/fd/",
            u64::try_from(n).unwrap_or_default(),
            b"",
        ),
        FdName::Fd(n) => match crate::fdtable::get_fd(n) {
            None => return Some(fail(errno::ENOENT) as isize),
            Some(e) => match link_target_of_kind(&e, out) {
                Some(len) => len,
                None => crate::fdtable::get_fd_path(n, out),
            },
        },
        FdName::Dir => return Some(fail(errno::EINVAL) as isize),
        FdName::Missing => return Some(fail(errno::ENOENT) as isize),
        FdName::Through(n, rest) => {
            let mut path = [0u8; crate::unistd::PATH_MAX];
            return Some(match through_path(n, rest, &mut path) {
                Ok(()) => crate::file::readlink(path.as_ptr(), out.as_mut_ptr(), out.len()),
                Err(e) => fail(e) as isize,
            });
        }
    };
    Some(isize::try_from(full.min(out.len())).unwrap_or(isize::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: i32 = 4242;

    #[test]
    fn classify_names_the_descriptor_names_and_nothing_else() {
        let cases: &[(&[u8], Option<FdName<'_>>)] = &[
            (b"/dev/fd", Some(FdName::DevFd)),
            (b"/dev/fd/", Some(FdName::Dir)),
            (b"/dev/fd/0", Some(FdName::Fd(0))),
            (b"/dev/fd/63", Some(FdName::Fd(63))),
            (b"/dev/fd/2147483647", Some(FdName::Fd(i32::MAX))),
            (b"/dev/fd/2147483648", Some(FdName::Missing)),
            (b"/dev/fd/03", Some(FdName::Missing)),
            (b"/dev/fd/-1", Some(FdName::Missing)),
            (b"/dev/fd/x", Some(FdName::Missing)),
            (b"/dev/fd/3/a/b", Some(FdName::Through(3, b"a/b"))),
            (b"/dev/fd/3/", Some(FdName::Through(3, b""))),
            (b"/dev/stdin", Some(FdName::Std(0))),
            (b"/dev/stdout", Some(FdName::Std(1))),
            (b"/dev/stderr", Some(FdName::Std(2))),
            (b"/dev/stdin/x", Some(FdName::Through(0, b"x"))),
            (b"/proc/self/fd", Some(FdName::Dir)),
            (b"/proc/self/fd/5", Some(FdName::Fd(5))),
            (b"/proc/thread-self/fd/5", Some(FdName::Fd(5))),
            (b"/proc/4242/fd", Some(FdName::Dir)),
            (b"/proc/4242/fd/7", Some(FdName::Fd(7))),
            (b"/proc/4243/fd/7", None),
            (b"/proc/04242/fd/7", None),
            (b"/proc/self/fdinfo/7", None),
            (b"/proc/self", None),
            (b"/dev/fdx", None),
            (b"/dev/stdinx", None),
            (b"/dev/null", None),
            (b"/", None),
        ];
        for (path, want) in cases {
            assert_eq!(
                classify(path, ME),
                *want,
                "{}",
                core::str::from_utf8(path).unwrap_or("(not UTF-8)")
            );
        }
    }

    #[test]
    fn access_within_lets_a_read_write_descriptor_give_either() {
        assert!(access_within(O_RDONLY, O_RDONLY));
        assert!(access_within(O_WRONLY, O_WRONLY));
        assert!(access_within(O_RDONLY, O_RDWR));
        assert!(access_within(O_WRONLY, O_RDWR));
        assert!(access_within(O_RDWR, O_RDWR));
        assert!(!access_within(O_WRONLY, O_RDONLY));
        assert!(!access_within(O_RDWR, O_RDONLY));
        assert!(!access_within(O_RDONLY, O_WRONLY));
    }

    #[test]
    fn link_mode_is_linuxs() {
        assert_eq!(link_mode(O_RDONLY), 0o500);
        assert_eq!(link_mode(O_WRONLY), 0o300);
        assert_eq!(link_mode(O_RDWR), 0o700);
    }

    #[test]
    fn put_tagged_writes_linuxs_pipe_name_and_reports_the_full_length() {
        let mut out = [0u8; 32];
        let n = put_tagged(&mut out, b"pipe:[", 67585, b"]");
        assert_eq!(out.get(..n), Some(&b"pipe:[67585]"[..]));
        let mut short = [0u8; 4];
        assert_eq!(put_tagged(&mut short, b"pipe:[", 0, b"]"), 8);
        assert_eq!(&short, b"pipe");
    }

    #[test]
    fn listing_decodes_as_the_kernels_records_do() {
        let mut buf = [0u8; 256];
        let len = listing([0, 1, 2, 10].into_iter(), &mut buf).unwrap();
        let mut at = 0;
        let mut got = std::vec::Vec::new();
        while at < len {
            let e = crate::dirent::decode_packed_entry(&buf[at..len]).unwrap();
            got.push((e.kernel_type, e.name.to_vec(), e.ino));
            at += e.record_len;
        }
        assert_eq!(
            got,
            [
                (crate::dirent::KERNEL_TYPE_DIR, b".".to_vec(), 1),
                (crate::dirent::KERNEL_TYPE_DIR, b"..".to_vec(), 2),
                (crate::dirent::KERNEL_TYPE_SYMLINK, b"0".to_vec(), 0x100),
                (crate::dirent::KERNEL_TYPE_SYMLINK, b"1".to_vec(), 0x101),
                (crate::dirent::KERNEL_TYPE_SYMLINK, b"2".to_vec(), 0x102),
                (crate::dirent::KERNEL_TYPE_SYMLINK, b"10".to_vec(), 0x10a),
            ]
        );
        assert!(listing_len(4).unwrap() >= len);
        assert_eq!(
            listing([1].into_iter(), &mut [0u8; 30]),
            None,
            "a short buffer is refused"
        );
    }

    // -- through the C entry points: open, stat, lstat, access, readlink --
    //
    // Each test thread's descriptor table starts as a process's does -- 0, 1
    // and 2 the console -- and a pipe end is put in it directly: dup of a
    // pipe shares its handle and asks the kernel nothing, so the host can
    // run these.

    fn cpath(s: &str) -> std::vec::Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        v
    }

    fn pipe_end(handle: u64, mode: i32) -> i32 {
        crate::fdtable::alloc_fd_with_flags(HandleKind::Pipe, handle, mode).unwrap()
    }

    fn open_errno(path: &str, flags: i32) -> (i32, i32) {
        let p = cpath(path);
        errno::set_errno(0);
        let fd = crate::file::open(p.as_ptr(), flags, 0);
        (fd, errno::get_errno())
    }

    /// Linux reopens a pipe end through its link; here the same end is
    /// duplicated -- same pipe, same direction -- with O_CLOEXEC honoured.
    #[test]
    fn open_of_a_pipe_ends_link_is_that_pipe_end() {
        let r = pipe_end(77, O_RDONLY);
        let (fd, _) = open_errno(
            &std::format!("/dev/fd/{r}"),
            O_RDONLY | crate::fcntl::O_CLOEXEC,
        );
        assert!(fd >= 0 && fd != r, "{fd}");
        let e = crate::fdtable::get_fd(fd).unwrap();
        assert_eq!((e.kind, e.handle), (HandleKind::Pipe, 77));
        assert_eq!(
            crate::fdtable::get_fd_flags(fd),
            Some(crate::fdtable::FD_CLOEXEC)
        );
        let (fd2, _) = open_errno(&std::format!("/proc/self/fd/{r}"), O_RDONLY);
        assert_eq!(crate::fdtable::get_fd(fd2).map(|e| e.handle), Some(77));
        assert_eq!(crate::fdtable::get_fd_flags(fd2), Some(0));
    }

    /// The refusals, each Linux's errno: the other end of a pipe (Linux
    /// reopens the inode; this library holds one end -- the recorded
    /// deviation), a directory asked of a pipe, an exclusive create of a
    /// link that is there, and names that are no descriptor's.
    #[test]
    fn open_refuses_what_it_cannot_give_with_linuxs_errno() {
        let r = pipe_end(78, O_RDONLY);
        let link = std::format!("/dev/fd/{r}");
        assert_eq!(open_errno(&link, O_WRONLY), (-1, errno::EACCES));
        assert_eq!(
            open_errno(&link, O_RDONLY | crate::fcntl::O_DIRECTORY),
            (-1, errno::ENOTDIR)
        );
        assert_eq!(
            open_errno(
                &link,
                O_RDONLY | crate::fcntl::O_CREAT | crate::fcntl::O_EXCL
            ),
            (-1, errno::EEXIST)
        );
        for name in [
            "/dev/fd/200",
            "/dev/fd/03",
            "/dev/fd/x",
            "/proc/self/fd/250",
        ] {
            assert_eq!(open_errno(name, O_RDONLY), (-1, errno::ENOENT), "{name}");
        }
        // The console is read-write here as on Linux's terminal.
        let (fd, _) = open_errno("/dev/stdin", O_RDONLY);
        assert_eq!(
            crate::fdtable::get_fd(fd).map(|e| e.kind),
            Some(HandleKind::Console)
        );
    }

    fn stat_of(path: &str, follow: bool) -> Result<crate::stat::Stat, i32> {
        let p = cpath(path);
        let mut st = crate::stat::Stat::zeroed();
        errno::set_errno(0);
        let r = if follow {
            crate::file::stat(p.as_ptr(), &raw mut st)
        } else {
            crate::file::lstat(p.as_ptr(), &raw mut st)
        };
        if r == 0 {
            Ok(st)
        } else {
            Err(errno::get_errno())
        }
    }

    /// `stat` follows the link to the object, so `[ -p /dev/stdin ]` asks
    /// the right question; `lstat` is the link, moded by the descriptor's
    /// access as Linux's are; `/dev/fd` is a link to a directory.
    #[test]
    fn stat_follows_the_link_and_lstat_is_the_link() {
        use crate::fcntl::{S_IFDIR, S_IFIFO, S_IFLNK, S_IFMT};
        let w = pipe_end(79, O_WRONLY);
        let link = std::format!("/dev/fd/{w}");
        assert_eq!(stat_of(&link, true).unwrap().st_mode & S_IFMT, S_IFIFO);
        let l = stat_of(&link, false).unwrap();
        assert_eq!((l.st_mode, l.st_size), (S_IFLNK | 0o300, 64));
        let rw = crate::fdtable::alloc_fd_with_flags(HandleKind::Pipe, 80, O_RDWR).unwrap();
        assert_eq!(
            stat_of(&std::format!("/proc/self/fd/{rw}"), false)
                .unwrap()
                .st_mode,
            S_IFLNK | 0o700
        );
        assert_eq!(
            stat_of("/dev/fd/0", false).unwrap().st_mode,
            S_IFLNK | 0o500
        );
        assert_eq!(stat_of("/dev/fd", true).unwrap().st_mode, S_IFDIR | 0o500);
        let d = stat_of("/dev/fd", false).unwrap();
        assert_eq!((d.st_mode, d.st_size), (S_IFLNK | 0o777, 13));
        let s = stat_of("/dev/stdin", false).unwrap();
        assert_eq!((s.st_mode, s.st_size), (S_IFLNK | 0o777, 15));
        assert_eq!(
            stat_of("/proc/self/fd", false).unwrap().st_mode,
            S_IFDIR | 0o500
        );
        assert_eq!(stat_of("/dev/fd/201", true).err(), Some(errno::ENOENT));
        assert_eq!(stat_of("/dev/fd/201", false).err(), Some(errno::ENOENT));
    }

    fn access_of(path: &str, mode: i32) -> (i32, i32) {
        let p = cpath(path);
        errno::set_errno(0);
        let r = crate::file::access(p.as_ptr(), mode);
        (r, errno::get_errno())
    }

    /// Through a descriptor's link, access asks the object: a pipe or a
    /// terminal reads and writes, and does not execute; the directory reads
    /// and searches, and is not writable.
    #[test]
    fn access_answers_for_the_object() {
        use crate::fcntl::{F_OK, R_OK, W_OK, X_OK};
        let r = pipe_end(81, O_RDONLY);
        let link = std::format!("/dev/fd/{r}");
        assert_eq!(access_of(&link, F_OK), (0, 0));
        assert_eq!(access_of(&link, R_OK | W_OK).0, 0);
        assert_eq!(access_of(&link, X_OK), (-1, errno::EACCES));
        assert_eq!(access_of("/dev/fd/202", F_OK), (-1, errno::ENOENT));
        assert_eq!(access_of("/dev/fd", R_OK | X_OK).0, 0);
        assert_eq!(access_of("/dev/fd", W_OK), (-1, errno::EACCES));
        assert_eq!(access_of("/dev/stdin", R_OK).0, 0);
    }

    fn readlink_of(path: &str, size: usize) -> Result<std::vec::Vec<u8>, i32> {
        let p = cpath(path);
        let mut buf = std::vec![0u8; size];
        errno::set_errno(0);
        let n = crate::file::readlink(p.as_ptr(), buf.as_mut_ptr(), buf.len());
        match usize::try_from(n) {
            Ok(n) => Ok(buf[..n].to_vec()),
            Err(_) => Err(errno::get_errno()),
        }
    }

    /// Linux's link texts, and readlink's truncation without a NUL.
    #[test]
    fn readlink_names_what_linux_names() {
        let r = pipe_end(67585, O_RDONLY);
        assert_eq!(readlink_of("/dev/fd", 64).unwrap(), b"/proc/self/fd");
        assert_eq!(readlink_of("/dev/stdin", 64).unwrap(), b"/proc/self/fd/0");
        assert_eq!(readlink_of("/dev/stderr", 64).unwrap(), b"/proc/self/fd/2");
        assert_eq!(
            readlink_of(&std::format!("/dev/fd/{r}"), 64).unwrap(),
            b"pipe:[67585]"
        );
        assert_eq!(readlink_of("/proc/self/fd/1", 64).unwrap(), b"/dev/console");
        assert_eq!(
            readlink_of(&std::format!("/dev/fd/{r}"), 4).unwrap(),
            b"pipe"
        );
        assert_eq!(readlink_of("/proc/self/fd", 64).err(), Some(errno::EINVAL));
        assert_eq!(readlink_of("/dev/fd/203", 64).err(), Some(errno::ENOENT));
    }
}
