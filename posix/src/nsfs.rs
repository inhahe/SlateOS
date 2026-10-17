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
//! `posix_spawn` or `exec`, or being closed by an `exec`, for which the
//! kernel's descriptor map has no handle type
//! (`known-issues/D-POSIX-A-NAMESPACE-DESCRIPTOR-DOES-NOT-CROSS-EXEC.md`).

use crate::errno::{self, native};
use crate::fdtable::{self, HandleKind};
use crate::linux_clone_args::{
    CLONE_NEWCGROUP, CLONE_NEWIPC, CLONE_NEWNET, CLONE_NEWNS, CLONE_NEWPID, CLONE_NEWTIME,
    CLONE_NEWUSER, CLONE_NEWUTS,
};

/// `ioctl(fd, NS_GET_NSTYPE)`: the namespace's `CLONE_NEW*` bit
/// (`<linux/nsfs.h>`, `_IO(0xb7, 3)`).
pub const NS_GET_NSTYPE: u64 = 0xb703;

/// Each kind of namespace: its `CLONE_NEW*` bit and how its descriptor's
/// `/proc/self/fd` link begins, its `/proc/<pid>/ns` name and `:[`.
const KINDS: [(u64, &[u8]); 8] = [
    (CLONE_NEWNS, b"mnt:["),
    (CLONE_NEWUTS, b"uts:["),
    (CLONE_NEWIPC, b"ipc:["),
    (CLONE_NEWNET, b"net:["),
    (CLONE_NEWPID, b"pid:["),
    (CLONE_NEWUSER, b"user:["),
    (CLONE_NEWCGROUP, b"cgroup:["),
    (CLONE_NEWTIME, b"time:["),
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

/// A hold on the namespace `path` -- absolute and normalised -- names
/// (`SYS_NAMESPACE_OPEN`): its handle, or the errno (`ENOENT` for a process
/// that is gone, `EACCES` for one the caller may not inspect); `None` for a
/// path that is no namespace's link, or a kernel without the call. Every
/// open of one namespace answers the same handle, and takes a hold of its
/// own, which a [`close`] gives back.
fn open_handle(path: &[u8]) -> Option<Result<u64, i32>> {
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
    Some(if ret < 0 {
        Err(errno::errno_for(ret))
    } else {
        Ok(ret.cast_unsigned())
    })
}

/// What `open` with `flags` answers for a path that exists and is a link to
/// nsfs's read-only file (mode 0444), as Linux's is: `EEXIST` for
/// `O_CREAT | O_EXCL`, `ELOOP` for `O_NOFOLLOW`, `ENOTDIR` for `O_DIRECTORY`,
/// then `EACCES` for a write -- a write mode or `O_TRUNC` -- unless
/// `may_override` (`CAP_DAC_OVERRIDE`); nothing for the rest.
fn refused_by_flags(flags: i32, may_override: bool) -> Option<i32> {
    use crate::fcntl::{O_ACCMODE, O_CREAT, O_DIRECTORY, O_EXCL, O_NOFOLLOW, O_RDONLY, O_TRUNC};
    if flags & (O_CREAT | O_EXCL) == O_CREAT | O_EXCL {
        Some(errno::EEXIST)
    } else if flags & O_NOFOLLOW != 0 {
        Some(errno::ELOOP)
    } else if flags & O_DIRECTORY != 0 {
        Some(errno::ENOTDIR)
    } else if (flags & O_ACCMODE != O_RDONLY || flags & O_TRUNC != 0) && !may_override {
        Some(errno::EACCES)
    } else {
        None
    }
}

/// `open` of `path` -- absolute and normalised -- when it is a namespace's
/// link: `Some` of the new descriptor, or of -1 with `errno` set; `None` for
/// any other path, a kernel without the call, or `O_PATH | O_NOFOLLOW`,
/// which opens the link itself, for the ordinary open to answer. `O_PATH`
/// alone makes a descriptor that only `fstat` and `close` take, as Linux's.
///
/// Descriptors of one namespace share its handle, and one hold: the hold
/// this open took is given back when another descriptor already has one,
/// and the last close gives that back.
pub(crate) fn open(path: &[u8], flags: i32) -> Option<i32> {
    use crate::fcntl::{O_ACCMODE, O_CLOEXEC, O_NOFOLLOW, O_PATH};
    if flags & (O_PATH | O_NOFOLLOW) == O_PATH | O_NOFOLLOW {
        return None;
    }
    let handle = match open_handle(path)? {
        Ok(handle) => handle,
        Err(e) => {
            errno::set_errno(e);
            return Some(-1);
        }
    };
    let refused = if flags & O_PATH == 0 {
        refused_by_flags(
            flags,
            crate::sys_capability::has_capability(crate::sys_capability::CAP_DAC_OVERRIDE),
        )
    } else {
        None
    };
    if let Some(e) = refused {
        // This open's own hold; a close of a hold cannot fail in a way
        // anyone could act on.
        let _ = close(handle);
        errno::set_errno(e);
        return Some(-1);
    }
    let shared = fdtable::is_handle_referenced(HandleKind::Namespace, handle);
    if shared {
        // As above: another descriptor's hold keeps the namespace.
        let _ = close(handle);
    }
    let status = flags & (O_ACCMODE | O_PATH);
    let Some(fd) = fdtable::alloc_fd_with_flags(HandleKind::Namespace, handle, status) else {
        if !shared {
            // As above: ours alone, and never handed out.
            let _ = close(handle);
        }
        errno::set_errno(errno::EMFILE);
        return Some(-1);
    };
    if flags & O_CLOEXEC != 0 {
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

/// The kind of namespace handle `handle` names -- its `CLONE_NEW*` bit --
/// if the kernel can say.
pub(crate) fn kind(handle: u64) -> Option<u64> {
    info(handle).ok().map(|[kind, _, _]| kind)
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

/// What the namespace `path` -- absolute and normalised, a namespace's link
/// followed -- is ([`info`]): `Some` of it or of the errno; `None` for any
/// other path. A hold taken and given back, and no descriptor, so that a
/// `stat` cannot run out of them.
pub(crate) fn path_info(path: &[u8]) -> Option<Result<[u64; 3], i32>> {
    let handle = match open_handle(path)? {
        Ok(handle) => handle,
        Err(e) => return Some(Err(e)),
    };
    let got = info(handle);
    // This lookup's own hold; a descriptor of the same namespace keeps its
    // own, and a close of a hold cannot fail in a way anyone could act on.
    let _ = close(handle);
    Some(got)
}

/// `stat` -- following the link -- of `path`, absolute and normalised, when
/// it is a namespace's link: the namespace's own record, as Linux's is
/// (`Some` of 0, or of -1 with `errno` set); `None` for any other path.
///
/// # Safety
///
/// `buf` must be writable for a `struct stat`.
pub(crate) unsafe fn stat_path(path: &[u8], buf: *mut crate::stat::Stat) -> Option<i32> {
    match path_info(path)? {
        Ok(info) => {
            // SAFETY: the caller's contract.
            fill_stat(unsafe { &mut *buf }, info);
            Some(0)
        }
        Err(e) => {
            errno::set_errno(e);
            Some(-1)
        }
    }
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
    let prefix = KINDS.iter().find(|&&(bit, _)| bit == kind)?.1;
    Some(crate::fdname::put_tagged(out, prefix, ino, b"]"))
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

    /// Open's rules for a link to a regular file, in Linux's order.
    #[test]
    fn a_namespace_link_refuses_as_a_link_to_a_file() {
        use crate::fcntl::{
            O_CREAT, O_DIRECTORY, O_EXCL, O_NOFOLLOW, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY,
        };
        assert_eq!(refused_by_flags(O_RDONLY, false), None);
        assert_eq!(refused_by_flags(O_RDWR | O_CREAT, true), None, "it exists");
        assert_eq!(
            refused_by_flags(O_CREAT | O_EXCL, true),
            Some(errno::EEXIST)
        );
        assert_eq!(refused_by_flags(O_NOFOLLOW, true), Some(errno::ELOOP));
        assert_eq!(refused_by_flags(O_DIRECTORY, true), Some(errno::ENOTDIR));
        assert_eq!(
            refused_by_flags(O_CREAT | O_EXCL | O_NOFOLLOW | O_DIRECTORY, false),
            Some(errno::EEXIST)
        );
        assert_eq!(
            refused_by_flags(O_NOFOLLOW | O_DIRECTORY, false),
            Some(errno::ELOOP)
        );
        // Mode 0444: a write needs the override, and comes after the rest.
        assert_eq!(refused_by_flags(O_WRONLY, false), Some(errno::EACCES));
        assert_eq!(refused_by_flags(O_RDWR, false), Some(errno::EACCES));
        assert_eq!(
            refused_by_flags(O_RDONLY | O_TRUNC, false),
            Some(errno::EACCES)
        );
        assert_eq!(refused_by_flags(O_WRONLY, true), None);
        assert_eq!(
            refused_by_flags(O_WRONLY | O_DIRECTORY, false),
            Some(errno::ENOTDIR)
        );
    }

    /// On the host the kernel cannot be asked: no namespace path opens as
    /// one, and the ordinary open answers instead.
    #[test]
    fn without_the_calls_a_namespace_path_is_any_path() {
        assert_eq!(open(b"/proc/self/ns/uts", crate::fcntl::O_RDONLY), None);
        assert_eq!(path_info(b"/proc/self/ns/uts"), None);
        assert_eq!(unshare(CLONE_NEWUTS), Err(errno::ENOSYS));
        assert_eq!(ioctl(7, 0x5401), -1);
        assert_eq!(errno::get_errno(), errno::ENOTTY);
    }
}
