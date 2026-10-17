//! Namespaces, as a native program sees them: `unshare(2)` and `setns(2)`,
//! and the descriptors `open` makes of `/proc/<pid>/ns/<kind>`.
//!
//! The kernel keeps each process's namespaces -- a UTS namespace so far,
//! its private host and domain names (lane A's design-decisions 1554) -- and
//! reaches them through six native calls (1161-1166,
//! `requests/a-d-the-namespace-calls-have-native-numbers.md`). A namespace's
//! descriptor is [`HandleKind::Namespace`](crate::fdtable::HandleKind): a
//! kernel handle that holds the namespace alive until its last close, which
//! `fstat`, `ioctl(NS_GET_NSTYPE)`, `setns` and `/proc/self/fd`'s link read.
//! Like Linux's nsfs file it cannot be read or written (`EINVAL`).
//!
//! Namespaces are a process's here, not a thread's: one thread's `unshare`
//! or `setns` moves every thread, where Linux moves the caller alone.
//!
//! Not yet: `setns` with a pidfd, which native programs cannot make
//! (`pidfd_open` answers `ENOSYS`); and a namespace descriptor crossing
//! `posix_spawn` or `exec`, for which the kernel's spawn has no handle type.

use crate::errno::{self, native};
use crate::fdtable::{self, HandleKind};
use crate::linux_clone_args::{
    CLONE_NEWCGROUP, CLONE_NEWIPC, CLONE_NEWNET, CLONE_NEWNS, CLONE_NEWPID, CLONE_NEWTIME,
    CLONE_NEWUSER, CLONE_NEWUTS,
};

/// `ioctl(fd, NS_GET_NSTYPE)`: the namespace's `CLONE_NEW*` bit
/// (`<linux/nsfs.h>`, `_IO(0xb7, 3)`).
pub const NS_GET_NSTYPE: u64 = 0xb703;

/// Each kind of namespace: its `CLONE_NEW*` bit and the name its
/// `/proc/<pid>/ns` link has.
const KINDS: [(u64, &[u8]); 8] = [
    (CLONE_NEWNS, b"mnt"),
    (CLONE_NEWUTS, b"uts"),
    (CLONE_NEWIPC, b"ipc"),
    (CLONE_NEWNET, b"net"),
    (CLONE_NEWPID, b"pid"),
    (CLONE_NEWUSER, b"user"),
    (CLONE_NEWCGROUP, b"cgroup"),
    (CLONE_NEWTIME, b"time"),
];

/// Whether `path` -- absolute and normalised -- has the shape of a
/// namespace's link, `/proc/<pid, self or thread-self>/ns/<name>`: the
/// paths [`open`] asks the kernel about, so that no other open pays for a
/// call. The kernel decides what the path really names.
pub(crate) fn has_a_namespace_shape(path: &[u8]) -> bool {
    let Some(rest) = path.strip_prefix(b"/proc/") else {
        return false;
    };
    let mut parts = rest.split(|&b| b == b'/');
    let (Some(pid), Some(ns), Some(name), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let pid_ok = pid == b"self"
        || pid == b"thread-self"
        || (!pid.is_empty() && pid.iter().all(u8::is_ascii_digit));
    pid_ok && ns == b"ns" && !name.is_empty()
}

/// What the namespace calls' refusals are, as Linux's `unshare` and `setns`
/// answer them: `EPERM` without the right (`PermissionDenied`, not the usual
/// `EACCES`), `EINVAL` for a kind not built yet (`NotSupported`, Linux's
/// answer without that `CONFIG_*_NS`), `ENOSPC` past the limit on
/// namespaces (Linux's), and the rest as they come.
fn changing_errno(ret: i64) -> i32 {
    match ret {
        native::PERMISSION_DENIED => errno::EPERM,
        native::NOT_SUPPORTED => errno::EINVAL,
        native::RESOURCE_EXHAUSTED => errno::ENOSPC,
        _ => errno::errno_for(ret),
    }
}

/// `unshare(2)`'s namespace bits, `kinds` (`CLONE_NEW*`): the calling
/// process into new namespaces of those kinds (`SYS_NAMESPACE_UNSHARE`).
/// `ENOSYS` from a kernel without the call.
pub(crate) fn unshare(kinds: u64) -> Result<(), i32> {
    let ret = crate::syscall::syscall1(crate::syscall::SYS_NAMESPACE_UNSHARE, kinds);
    if crate::syscall::lacks_call(ret) {
        Err(errno::ENOSYS)
    } else if ret < 0 {
        Err(changing_errno(ret))
    } else {
        Ok(())
    }
}

/// `setns(fd, nstype)` on a namespace's descriptor, whose kernel handle is
/// `handle`: the calling process into that namespace
/// (`SYS_NAMESPACE_ENTER`). `nstype` 0 takes it whatever its kind;
/// otherwise a kind that is not the descriptor's is `EINVAL`.
pub(crate) fn enter(handle: u64, nstype: u64) -> Result<(), i32> {
    let ret = crate::syscall::syscall2(crate::syscall::SYS_NAMESPACE_ENTER, handle, nstype);
    if crate::syscall::lacks_call(ret) {
        Err(errno::ENOSYS)
    } else if ret < 0 {
        Err(changing_errno(ret))
    } else {
        Ok(())
    }
}

/// `open` of `path` -- absolute and normalised -- when it is a namespace's
/// link: `Some` of the new descriptor, or of -1 with `errno` set (`ENOENT`
/// for a process that is gone, `EACCES` for one the caller may not
/// inspect); `None` for any other path, or a kernel without the call, for
/// the ordinary open to answer.
pub(crate) fn open(path: &[u8], flags: i32) -> Option<i32> {
    if !has_a_namespace_shape(path) {
        return None;
    }
    let ret = crate::syscall::syscall2(
        crate::syscall::SYS_NAMESPACE_OPEN,
        path.as_ptr() as u64,
        path.len() as u64,
    );
    if crate::syscall::lacks_call(ret) || ret == native::INVALID_ARGUMENT {
        return None;
    }
    if ret < 0 {
        errno::set_errno(errno::errno_for(ret));
        return Some(-1);
    }
    let handle = ret.cast_unsigned();
    let status = flags & crate::fcntl::O_ACCMODE;
    let Some(fd) = fdtable::alloc_fd_with_flags(HandleKind::Namespace, handle, status) else {
        if !fdtable::is_handle_referenced(HandleKind::Namespace, handle) {
            // Ours alone, and never handed out: its close cannot fail in a
            // way anyone could act on.
            let _ = close(handle);
        }
        errno::set_errno(errno::EMFILE);
        return Some(-1);
    };
    if flags & crate::fcntl::O_CLOEXEC != 0 {
        let _ = fdtable::set_fd_flags(fd, fdtable::FD_CLOEXEC);
    }
    Some(fd)
}

/// Give back namespace handle `handle` (`SYS_NAMESPACE_CLOSE`): the kernel's
/// answer, as a close of a descriptor's last holder.
pub(crate) fn close(handle: u64) -> i64 {
    crate::syscall::syscall1(crate::syscall::SYS_NAMESPACE_CLOSE, handle)
}

/// What the kernel says of namespace handle `handle`
/// (`SYS_NAMESPACE_INFO`): its `CLONE_NEW*` bit, its inode number, and
/// nsfs's device's minor number -- or the errno.
pub(crate) fn info(handle: u64) -> Result<[u64; 3], i32> {
    let mut out = [0u64; 3];
    let ret = crate::syscall::syscall2(
        crate::syscall::SYS_NAMESPACE_INFO,
        handle,
        out.as_mut_ptr() as u64,
    );
    if crate::syscall::lacks_call(ret) {
        Err(errno::ENOSYS)
    } else if ret < 0 {
        Err(errno::errno_for(ret))
    } else {
        Ok(out)
    }
}

/// The `struct stat` of a namespace with `info` (`[kind, inode, minor]`), as
/// Linux's nsfs makes it: a regular file, read-only for everyone, one link,
/// on nsfs's device (major 0).
pub(crate) fn fill_stat(st: &mut crate::stat::Stat, info: [u64; 3]) {
    let [_, ino, minor] = info;
    *st = crate::stat::Stat::zeroed();
    st.st_mode = crate::fcntl::S_IFREG | 0o444;
    st.st_ino = ino;
    st.st_dev = crate::crt::gnu_dev_makedev(0, u32::try_from(minor).unwrap_or(u32::MAX));
    st.st_nlink = 1;
    st.st_blksize = 4096;
}

/// `stat` -- following the link -- of `path`, absolute and normalised, when
/// it is a namespace's link: the namespace's own record, as Linux's is
/// (`Some` of 0, or of -1 with `errno` set); `None` for any other path.
///
/// # Safety
///
/// `buf` must be writable for a `struct stat`.
pub(crate) unsafe fn stat_path(path: &[u8], buf: *mut crate::stat::Stat) -> Option<i32> {
    let fd = open(path, crate::fcntl::O_RDONLY | crate::fcntl::O_CLOEXEC)?;
    if fd < 0 {
        return Some(-1);
    }
    // SAFETY: forwarded.
    let ret = crate::file::fstat(fd, buf);
    let saved = errno::get_errno();
    let _ = crate::file::close(fd);
    errno::set_errno(saved);
    Some(ret)
}

/// The text of a namespace descriptor's `/proc/self/fd` link, Linux's
/// `<name>:[<inode>]` -- `uts:[4026531838]` -- into `out`: the full length,
/// as much as fits written. `None` when the kernel cannot say.
pub(crate) fn link_text(handle: u64, out: &mut [u8]) -> Option<usize> {
    let [kind, ino, _] = info(handle).ok()?;
    format_link(kind, ino, out)
}

/// [`link_text`] for a namespace of `kind` (its `CLONE_NEW*` bit) and inode
/// `ino`; `None` for a kind with no name.
fn format_link(kind: u64, ino: u64, out: &mut [u8]) -> Option<usize> {
    let name = KINDS.iter().find(|&&(bit, _)| bit == kind)?.1;
    let mut digits = [0u8; 20];
    let mut n = ino;
    let mut d = digits.len();
    loop {
        d = d.saturating_sub(1);
        if let Some(slot) = digits.get_mut(d) {
            *slot = b'0'.saturating_add(u8::try_from(n % 10).unwrap_or(0));
        }
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let mut len = 0usize;
    for &b in name
        .iter()
        .chain(b":[")
        .chain(digits.get(d..).unwrap_or_default())
        .chain(b"]")
    {
        if let Some(slot) = out.get_mut(len) {
            *slot = b;
        }
        len = len.saturating_add(1);
    }
    Some(len)
}

/// The `ioctl`s a namespace's descriptor answers: `NS_GET_NSTYPE`, its
/// kind's bit. The other nsfs requests ask for namespaces this kernel does
/// not keep (an owning user namespace, a parent), and every other request
/// is not a namespace's: `ENOTTY`.
pub(crate) fn ioctl(handle: u64, request: u64) -> i32 {
    if request != NS_GET_NSTYPE {
        errno::set_errno(errno::ENOTTY);
        return -1;
    }
    match info(handle) {
        Ok([kind, _, _]) => i32::try_from(kind).unwrap_or(-1),
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the shape of a namespace's link is asked about.
    #[test]
    fn a_namespace_link_has_its_shape() {
        for yes in [
            &b"/proc/self/ns/uts"[..],
            b"/proc/thread-self/ns/net",
            b"/proc/42/ns/mnt",
        ] {
            assert!(has_a_namespace_shape(yes), "{yes:?}");
        }
        for no in [
            &b"/proc/self/ns"[..],
            b"/proc/self/ns/",
            b"/proc/self/status",
            b"/proc/x1/ns/uts",
            b"/proc//ns/uts",
            b"/proc/self/ns/uts/more",
            b"/tmp/ns/uts",
            b"/proc/self/fd/3",
        ] {
            assert!(!has_a_namespace_shape(no), "{no:?}");
        }
    }

    /// The refusals of the calls that change namespaces are Linux's.
    #[test]
    fn changing_a_namespace_answers_as_linux() {
        assert_eq!(changing_errno(native::PERMISSION_DENIED), errno::EPERM);
        assert_eq!(changing_errno(native::NOT_SUPPORTED), errno::EINVAL);
        assert_eq!(changing_errno(native::RESOURCE_EXHAUSTED), errno::ENOSPC);
        assert_eq!(changing_errno(native::BAD_HANDLE), errno::EBADF);
        assert_eq!(changing_errno(native::INVALID_ARGUMENT), errno::EINVAL);
    }

    /// A namespace stats as nsfs's file.
    #[test]
    fn a_namespace_stats_as_nsfs_file() {
        let mut st = crate::stat::Stat::zeroed();
        fill_stat(&mut st, [CLONE_NEWUTS, 4_026_531_838, 4]);
        assert_eq!(st.st_mode, crate::fcntl::S_IFREG | 0o444);
        assert_eq!(st.st_ino, 4_026_531_838);
        assert_eq!(st.st_dev, crate::crt::gnu_dev_makedev(0, 4));
        assert_eq!(st.st_nlink, 1);
    }

    /// A namespace's link reads `<name>:[<inode>]`, cut short as any link
    /// is, its full length answered.
    #[test]
    fn a_namespace_link_names_its_kind_and_inode() {
        let mut out = [0u8; 32];
        let n = format_link(CLONE_NEWUTS, 4_026_531_838, &mut out).unwrap();
        assert_eq!(&out[..n], b"uts:[4026531838]");
        assert_eq!(format_link(CLONE_NEWNET, 0, &mut out), Some(7));
        assert_eq!(&out[..7], b"net:[0]");
        let mut short = [0u8; 4];
        assert_eq!(format_link(CLONE_NEWUTS, 7, &mut short), Some(7));
        assert_eq!(&short, b"uts:");
        assert_eq!(format_link(1, 7, &mut out), None, "no such kind");
    }

    /// On the host the kernel cannot be asked: no namespace path opens as
    /// one, and the ordinary open answers instead.
    #[test]
    fn without_the_calls_a_namespace_path_is_any_path() {
        assert_eq!(open(b"/proc/self/ns/uts", crate::fcntl::O_RDONLY), None);
        assert_eq!(unshare(CLONE_NEWUTS), Err(errno::ENOSYS));
        assert_eq!(ioctl(7, 0x5401), -1);
        assert_eq!(errno::get_errno(), errno::ENOTTY);
    }
}
