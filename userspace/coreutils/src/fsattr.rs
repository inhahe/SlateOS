//! Writing a file's metadata *by path*: its timestamps, its mode and its owner.
//!
//! `std` can read all three and write almost none of them. [`fs::set_permissions`]
//! is the one exception, and even it takes the mode through an opaque
//! [`Permissions`](std::fs::Permissions) that cannot be constructed portably; there is
//! no path form of `utimensat` at all — [`File::set_times`](std::fs::File::set_times)
//! takes a handle. So every utility that stamps a file has had to declare its own
//! `extern "C"` block, and by 2026-08-30 there were two: `touch` and `tar`.
//!
//! Two copies of a syscall declaration is two chances to get the struct layout
//! wrong, and the failure is silent — a `timespec` whose fields are the wrong
//! width does not fail to compile, it writes a wrong time. `cp -p` was about to
//! be the third. This module is the one copy.
//!
//! # The three are one module because `-p` writes all three
//!
//! `cp -p` restores timestamps, ownership and mode, in that order and for the
//! same reason GNU gives (`copy.c:3245`): "chown turns off set[ug]id bits for
//! non-root, so do the chmod last". Splitting them across three modules would
//! put the three halves of one ordering constraint in three files, and the
//! ordering is the part that is easy to get wrong — a `chmod` before the
//! `chown` compiles, runs, and quietly drops the setuid bit off every copy made
//! by a non-root user.
//!
//! [`Link`] is the other thing they share: each of the three has to be able to
//! land on a symbolic link rather than on what it names, because `cp -P -p`
//! stamps the link it just made. One enum answers that question for all three
//! rather than three booleans spelled three ways.
//!
//! # Why a path and not a handle
//!
//! Stamping a path reaches things a handle cannot: a directory, a file whose
//! permissions forbid every kind of open, a unix-domain socket, a device node.
//! `touch somedir` and `touch a-mode-000-file-you-own` both succeed on GNU for
//! exactly that reason, and they succeed here because [`set_times`] is a path
//! operation on unix rather than an open followed by `futimens`. Windows has no
//! path form, so that arm does open a handle and the cases a handle cannot
//! reach stay unreachable there; see `known-issues.md` →
//! `TD-B-TOUCH-CANNOT-STAMP-A-PATH-IT-CANNOT-OPEN`.
//!
//! # Two private copies that deliberately stay private
//!
//! * **`tar`** works through an `openat` chain rooted at the extraction
//!   directory, so every one of its calls is *dirfd-relative* — the whole point
//!   is that the path it passes is a single component resolved against a
//!   descriptor nobody else can re-point. A path-based API cannot express that,
//!   and giving this module a `dirfd` parameter to suit one caller would invite
//!   the others to pass `AT_FDCWD` and forget why the parameter is there.
//! * **`chown`** encodes each operand to a C string once and reuses it across a
//!   `--from` re-check, and reaches for `fchown` on a descriptor it already
//!   holds when the operation is attackable. Both are properties of *its*
//!   traversal, not of the ownership write.
//!
//! Neither is a band-aid: in both cases the shared shape would be the wrong
//! shape. What this module removes is the copies that existed only because
//! there was nowhere else to put them.

use std::io;
use std::path::Path;
use std::time::SystemTime;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

/// Whether a metadata write passes *through* a symbolic link at the end of the
/// path, or lands on the link itself.
///
/// The distinction only ever arises when the final component is a symlink, but
/// it cannot be defaulted, because the two callers who care want opposite
/// answers: `chown -R` walking a tree must never follow (a link planted in a
/// directory it descends into is an invitation to chown `/etc/shadow`), while
/// `touch` on a link is asking about the file, which is why `touch -h` is a
/// separate option rather than the default.
///
/// Not a `bool`: `set_times(p, t, true)` at a call site reads as neither
/// question nor answer, and the two mistakes it invites — reading it as "is a
/// symlink" and as "no-dereference" — are exact opposites of each other.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Link {
    /// Follow a final symbolic link and write the metadata of what it names.
    /// Plain `utimensat` with no flags, plain `chown`, plain `chmod`.
    Follow,
    /// Write the *link's* own metadata. `AT_SYMLINK_NOFOLLOW`, `lchown`. What
    /// `cp -P -p` needs: it has just created a symlink, and the timestamps it is
    /// restoring are the source link's, not the source file's.
    NoFollow,
}

/// What a metadata write is aimed at: a name, or a descriptor already held.
///
/// This is GNU's own shape — `set_owner (x, dst_name, dst_dirfd, drelname,
/// dest_desc, …)` takes both and uses the descriptor whenever it has one — and
/// it is not an optimisation. `cp -p` restores the setuid bit. Restoring it *by
/// name*, after the bytes are written, leaves a window in which the name can be
/// made to mean a different file, and the bit is then granted on that one; the
/// attacker needs only write access to the containing directory, which is the
/// ordinary state of `/tmp`. A descriptor names an inode and cannot be
/// re-pointed, so where one is already open there is no window at all.
///
/// Both forms have to exist because a directory and a symbolic link have no
/// descriptor to use: nothing opens them on the way to copying them.
///
/// `Copy`, because both variants are shared borrows and a caller aims several
/// writes at one target in a row — `cp -p` alone stamps times, then ownership,
/// then mode. Passing by value with no `Copy` would make the second call
/// borrow-check-fail on a value the first consumed, and the workaround —
/// `&On<'_>`, a reference to a pair of references — buys nothing.
#[derive(Clone, Copy)]
pub enum On<'a> {
    /// The file this path names, following a final symbolic link or not.
    Path(&'a Path, Link),
    /// The file this descriptor names, whatever its name is by now.
    File(&'a std::fs::File),
}

/// What to do with one of a file's two timestamps.
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub enum When {
    /// Leave it exactly as it is.
    ///
    /// Not "write back what is there": reading the old value and writing it
    /// back rounds it to whatever the two clocks agree on, and it races anyone
    /// else writing the file. `UTIME_OMIT` asks the kernel not to touch the
    /// field at all, which is what makes `touch -a` able to move the access
    /// time without disturbing the modification time.
    Omit,
    /// Overwrite it with this instant.
    Set(SystemTime),
}

/// What to write to a file's two timestamps.
///
/// This exists rather than a bare [`FileTimes`](std::fs::FileTimes) because
/// `FileTimes` is opaque — it can be built but not read back — and the unix
/// path needs to *read* the request in order to translate it into a `timespec`
/// pair. So the request is carried in a form this crate owns, and converted at
/// the last moment by whichever [`set_times`] arm is compiled in.
#[derive(Clone, Copy)]
pub struct Times {
    /// The access time.
    pub accessed: When,
    /// The modification time.
    pub modified: When,
}

impl Times {
    /// Both timestamps set to the same instant — what a copy that preserves
    /// times reads off its source, and what `touch -r` reads off its reference.
    #[must_use]
    pub fn both(at: SystemTime) -> Self {
        Times {
            accessed: When::Set(at),
            modified: When::Set(at),
        }
    }

    /// The `std` spelling, for the paths that go through a handle: every stamp
    /// on Windows, and `touch -` on both.
    pub fn to_file_times(self) -> std::fs::FileTimes {
        let mut times = std::fs::FileTimes::new();
        if let When::Set(t) = self.accessed {
            times = times.set_accessed(t);
        }
        if let When::Set(t) = self.modified {
            times = times.set_modified(t);
        }
        times
    }
}

/// Write a file's timestamps.
///
/// # Errors
///
/// Whatever the platform said. On unix the `errno` is recovered through
/// [`io::Error::last_os_error`], which is correct because `utimensat` promises
/// to set it on a `-1` return.
///
/// A path containing a NUL is [`io::ErrorKind::InvalidInput`] rather than a
/// syscall: `utimensat` would stamp the prefix before the NUL and report
/// success, which is worse than refusing. This OS's paths allow every byte but
/// `/` and NUL (`design.txt`), so such a path names nothing anyway.
pub fn set_times(on: On<'_>, times: Times) -> io::Result<()> {
    match on {
        // `File::set_times` is `futimens`, which is what a descriptor wants and
        // what `std` already spells portably. [`Times::to_file_times`] loses
        // nothing on the way: an omitted half becomes a `FileTimes` field that
        // was never set, which `std` turns back into `UTIME_OMIT`.
        On::File(f) => f.set_times(times.to_file_times()),
        On::Path(path, link) => set_times_at(path, times, link),
    }
}

/// [`set_times`]'s path arm.
#[cfg(unix)]
fn set_times_at(path: &Path, times: Times, link: Link) -> io::Result<()> {
    unsafe extern "C" {
        fn utimensat(dirfd: i32, path: *const u8, times: *const CTimespec, flags: i32) -> i32;
    }

    let cpath = c_path(path)?;
    let spec = to_timespecs(times);
    let flags = nofollow_flag(link);

    // SAFETY: `cpath` is NUL-terminated and lives until the end of this
    // statement; `spec` is exactly the two-element array `utimensat` reads;
    // `AT_FDCWD` and the flag word are both valid. The call does not retain
    // either pointer.
    let rc = unsafe { utimensat(AT_FDCWD, cpath.as_ptr(), spec.as_ptr(), flags) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// [`set_times`]'s path arm, on a host that has no path form.
///
/// There is no path-based equivalent on Windows — `SetFileTime` takes a
/// handle — so this arm opens one asking for the least access that permits a
/// stamp. Opening is inherently a follow, so [`Link::NoFollow`] cannot be
/// honoured here and is ignored rather than refused: the target OS is the
/// `#[cfg(unix)]` arm, and failing a `cp -P -p` on the development host would
/// make the test suite disagree with the program it is testing.
#[cfg(not(unix))]
fn set_times_at(path: &Path, times: Times, _link: Link) -> io::Result<()> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    /// `FILE_WRITE_ATTRIBUTES` — the one right `SetFileTime` checks for.
    /// `File::open` asks for `GENERIC_READ`, which does not include it, so the
    /// obvious spelling fails with "Access is denied" on a file that is right
    /// there and writable.
    const FILE_WRITE_ATTRIBUTES: u32 = 0x0000_0100;
    /// `FILE_FLAG_BACKUP_SEMANTICS` — without it a *directory* cannot be opened
    /// as a handle at all, and `touch somedir` could not work on this host even
    /// though it does on the target.
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?
        .set_times(times.to_file_times())
}

/// Who a file belongs to, with either half optionally left alone.
///
/// Both are `Option` rather than a pair of numbers because "leave this one as it
/// is" is a real request that `chown(2)` has a spelling for — the `(uid_t)-1`
/// sentinel — and it is not the same request as "set it to what it already is".
/// The read-then-write version has a window in which the file can be replaced,
/// and it turns a field nobody asked about into a real ownership write, which on
/// most kernels clears the setuid bit. `chown :group f` and `cp --preserve=…`
/// against a source whose uid already matches both take this route.
#[derive(Clone, Copy, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub struct Owner {
    /// The owning user, or `None` to leave it.
    pub uid: Option<u32>,
    /// The owning group, or `None` to leave it.
    pub gid: Option<u32>,
}

impl Owner {
    /// The owner and group a copy inherits from its source.
    #[must_use]
    pub fn of(uid: u32, gid: u32) -> Self {
        Owner {
            uid: Some(uid),
            gid: Some(gid),
        }
    }

    /// Just the group — what a `chown` retry falls back to after the user half
    /// was refused, and what `cp -p` gets to keep when it is not root.
    #[must_use]
    pub fn group_only(self) -> Self {
        Owner {
            uid: None,
            gid: self.gid,
        }
    }

    /// Whether this asks for nothing at all, in which case the syscall is worth
    /// skipping: `chown(f, -1, -1)` is not a no-op on every kernel — it is still
    /// a write, and it can still fail with `EPERM`.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.uid.is_none() && self.gid.is_none()
    }

    /// The pair as `chown(2)` wants it, with `None` as the `(uid_t)-1`
    /// sentinel.
    #[cfg_attr(not(unix), allow(dead_code))]
    fn as_ids(self) -> (u32, u32) {
        (self.uid.unwrap_or(UNCHANGED), self.gid.unwrap_or(UNCHANGED))
    }
}

/// POSIX's "leave this field alone" sentinel for `chown(2)`: `(uid_t)-1`.
const UNCHANGED: u32 = u32::MAX;

/// Write a file's owner and group.
///
/// Asking for neither half writes nothing and succeeds — see [`Owner::is_empty`]
/// for why that is a skip rather than a `chown(f, -1, -1)`.
///
/// # Errors
///
/// Whatever `chown`/`lchown`/`fchown` said, and [`io::ErrorKind::InvalidInput`]
/// for a path containing a NUL; see [`set_times`].
///
/// The caller decides what to make of `EPERM`, and `cp` and `chown` decide
/// differently: `cp -p` reports the failure and keeps the copy (GNU's
/// `chown_failure_ok`, `copy.c:3457` — `EPERM` or `EINVAL` while not root is a
/// warning), whereas `chown` treats it as the failure the user asked about. A
/// rule baked in here would have to be one of the two, and would be the wrong
/// one for the other.
#[cfg(unix)]
pub fn set_owner(on: On<'_>, owner: Owner) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    unsafe extern "C" {
        fn chown(path: *const u8, owner: u32, group: u32) -> i32;
        fn lchown(path: *const u8, owner: u32, group: u32) -> i32;
        fn fchown(fd: i32, owner: u32, group: u32) -> i32;
    }

    if owner.is_empty() {
        return Ok(());
    }
    let (uid, gid) = owner.as_ids();

    let rc = match on {
        // SAFETY: `f` is a live `File`, so its descriptor is open for the whole
        // call. `UNCHANGED` is the documented sentinel for each id.
        On::File(f) => unsafe { fchown(f.as_raw_fd(), uid, gid) },
        On::Path(path, link) => {
            let cpath = c_path(path)?;
            // SAFETY: `cpath` is NUL-terminated and outlives the call, which
            // retains nothing.
            unsafe {
                match link {
                    Link::Follow => chown(cpath.as_ptr(), uid, gid),
                    Link::NoFollow => lchown(cpath.as_ptr(), uid, gid),
                }
            }
        }
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Write a file's owner and group.
///
/// Windows has no uid or gid — its access control is ACLs, which are not a pair
/// of integers and which nothing in this crate models. Succeeding without doing
/// anything is the same answer [`set_mode`] gives on this host and for the same
/// reason: the target OS is the `#[cfg(unix)]` arm, and a `cp -p` that failed
/// here would fail in the test suite and nowhere else.
///
/// # Errors
///
/// Never, on this platform.
#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
pub fn set_owner(_on: On<'_>, _owner: Owner) -> io::Result<()> {
    Ok(())
}

/// Write a file's permission and setuid/setgid/sticky bits — `chmod(2)`'s
/// `07777`.
///
/// Ordering matters and is the caller's business: a `chown` after this one
/// clears `S_ISUID`/`S_ISGID` for a non-root process, so anything restoring both
/// must write the mode *last*. GNU says so where it does it (`copy.c:3245`) and
/// so does this module's header.
///
/// # Errors
///
/// Whatever `chmod`/`fchmod` said.
///
/// [`io::ErrorKind::Unsupported`] for `On::Path(_, Link::NoFollow)`, which is
/// refused rather than quietly followed. Linux has no working `lchmod`:
/// `fchmodat` rejects `AT_SYMLINK_NOFOLLOW` with `ENOTSUP`, and a symlink's own
/// mode bits are ignored by every permission check anyway. GNU never asks —
/// `copy_internal` returns before the mode block when the destination is a
/// symlink (`copy.c:3285`) — so this arm has no caller, and a silent follow
/// would be a `chmod` landing on a file nobody named.
#[cfg(unix)]
pub fn set_mode(on: On<'_>, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let perms = std::fs::Permissions::from_mode(mode);
    match on {
        On::File(f) => f.set_permissions(perms),
        On::Path(path, Link::Follow) => std::fs::set_permissions(path, perms),
        On::Path(_, Link::NoFollow) => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "cannot set the mode of a symbolic link itself",
        )),
    }
}

/// Write a file's mode.
///
/// `set_permissions` on Windows only toggles the read-only flag, which is not
/// what POSIX is asking for; doing nothing is the honest answer, and it is what
/// keeps the mode arithmetic in `cp` — withhold group and other bits, put them
/// back at the end — cancelling to no change on a host that has no such bits.
/// The target OS is the `#[cfg(unix)]` arm above.
///
/// # Errors
///
/// [`io::ErrorKind::Unsupported`] for `On::Path(_, Link::NoFollow)`, for the
/// same reason as the unix arm: refusing is the answer, and refusing on both
/// hosts keeps the tests honest about it.
#[cfg(not(unix))]
pub fn set_mode(on: On<'_>, _mode: u32) -> io::Result<()> {
    match on {
        On::Path(_, Link::NoFollow) => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "cannot set the mode of a symbolic link itself",
        )),
        _ => Ok(()),
    }
}

/// `AT_FDCWD` — resolve a relative path against the working directory.
/// Matches `posix/src/file.rs`.
#[cfg(unix)]
const AT_FDCWD: i32 = -100;

/// `AT_SYMLINK_NOFOLLOW`, matching `posix/src/file.rs` and Linux.
#[cfg(unix)]
const AT_SYMLINK_NOFOLLOW: i32 = 0x100;

/// The `flags` word for the `*at` calls that take one.
#[cfg(unix)]
fn nofollow_flag(link: Link) -> i32 {
    match link {
        Link::Follow => 0,
        Link::NoFollow => AT_SYMLINK_NOFOLLOW,
    }
}

/// A path as C wants it: the bytes, then a NUL.
///
/// This deliberately does not go through `str`. A path here is bytes, and the
/// point of the whole argv conversion is that it stays bytes down to the
/// syscall; `CString::new(path.to_str()?)` would reintroduce precisely the
/// UTF-8 assumption being removed.
///
/// # Errors
///
/// [`io::ErrorKind::InvalidInput`] if the path already contains a NUL — see
/// [`set_times`] for why that is refused rather than truncated.
#[cfg(unix)]
fn c_path(path: &Path) -> io::Result<Vec<u8>> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains a NUL byte",
        ));
    }
    let mut buf = Vec::with_capacity(bytes.len().saturating_add(1));
    buf.extend_from_slice(bytes);
    buf.push(0);
    Ok(buf)
}

/// `struct timespec`, in the layout `posix/src/stat.rs` declares.
///
/// Declared here rather than taken from a crate because `coreutils` depends on
/// no libc binding — the shape lives next to the `extern` block that uses it,
/// where the two can be checked against each other by eye.
///
/// It is *not* behind `#[cfg(unix)]`, even though only the unix arm passes one
/// to a syscall, so that [`to_timespec`] and its tests compile and run on the
/// development host as well. A type that exists only where it cannot be tested
/// is how a conversion bug reaches the target unnoticed.
#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
#[cfg_attr(not(unix), allow(dead_code))]
struct CTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

/// `UTIME_OMIT` — the `tv_nsec` sentinel meaning "leave this one alone".
///
/// Defined by POSIX as `(1 << 30) - 2`, and matching `posix/src/file.rs`. This
/// is the mechanism behind [`When::Omit`].
const UTIME_OMIT: i64 = (1 << 30) - 2;

/// Translate a [`Times`] into the pair `utimensat` reads.
///
/// Kept separate from [`set_times`], and free of any `cfg`, because it is the
/// only part of the unix path with arithmetic in it — and the unix path never
/// runs on the development host. A conversion that is wrong here is wrong on
/// the only operating system this program is for, so it is tested everywhere
/// even though it is called nowhere on Windows.
#[cfg_attr(not(unix), allow(dead_code))]
fn to_timespecs(times: Times) -> [CTimespec; 2] {
    [to_timespec(times.accessed), to_timespec(times.modified)]
}

/// One timestamp, as `utimensat` wants it.
///
/// Times before 1970 are the case worth stating: [`SystemTime::duration_since`]
/// reports them as an `Err` carrying the *absolute* distance back from the
/// epoch, so the sign has to be reapplied by hand, and a non-zero nanosecond
/// part has to borrow a second — `timespec` requires `tv_nsec` in `0..1e9` even
/// when `tv_sec` is negative. `touch -r` on a file dated 1969, and `cp -p` of
/// one, are the ways in.
#[cfg_attr(not(unix), allow(dead_code))]
fn to_timespec(when: When) -> CTimespec {
    let When::Set(at) = when else {
        // `tv_sec` is ignored when `tv_nsec` is a sentinel, but zero is what
        // gnulib passes and it keeps the value reproducible for the tests.
        return CTimespec {
            tv_sec: 0,
            tv_nsec: UTIME_OMIT,
        };
    };
    match at.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => CTimespec {
            tv_sec: i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(since.subsec_nanos()),
        },
        Err(before) => {
            let back = before.duration();
            let secs = i64::try_from(back.as_secs()).unwrap_or(i64::MAX);
            let nanos = i64::from(back.subsec_nanos());
            if nanos == 0 {
                CTimespec {
                    tv_sec: secs.checked_neg().unwrap_or(i64::MIN),
                    tv_nsec: 0,
                }
            } else {
                // Borrow a second so `tv_nsec` stays non-negative: 0.5 s before
                // the epoch is (-1 s, +500_000_000 ns), not (0 s, -500_000_000).
                CTimespec {
                    tv_sec: secs.saturating_add(1).checked_neg().unwrap_or(i64::MIN),
                    tv_nsec: 1_000_000_000 - nanos,
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::{CTimespec, Owner, Times, UTIME_OMIT, When, to_timespec, to_timespecs};
    use std::time::{Duration, SystemTime};

    /// The `(uid_t)-1` sentinel is what an absent half becomes, and it must be
    /// the *unsigned* spelling: `chown` takes `uid_t`, so a signed `-1` widened
    /// to 64 bits would arrive as something else entirely on the ABI.
    #[test]
    #[cfg(unix)]
    fn an_absent_owner_half_is_the_unchanged_sentinel() {
        assert_eq!(Owner::of(1000, 100).as_ids(), (1000, 100));
        assert_eq!(Owner::of(1000, 100).group_only().as_ids(), (u32::MAX, 100));
        assert_eq!(Owner::default().as_ids(), (u32::MAX, u32::MAX));
        // Root is a real id and must not be confused with "leave it alone".
        assert_eq!(Owner::of(0, 0).as_ids(), (0, 0));
    }

    /// Asking for neither half is a skip, not a `chown(f, -1, -1)`. The syscall
    /// is still a write — it can fail with `EPERM` on a file nobody asked to
    /// change — so `cp` copying a file whose owner already matches must not make
    /// it.
    #[test]
    fn asking_for_nothing_is_recognisable_as_nothing() {
        assert!(Owner::default().is_empty());
        assert!(!Owner::of(1000, 100).group_only().is_empty());
        assert!(!Owner::of(0, 0).is_empty());
        // The group-only fallback of an owner that had no group is empty, which
        // is what makes `cp -p`'s chown retry a no-op rather than a second
        // failure.
        assert!(
            Owner {
                uid: Some(7),
                gid: None
            }
            .group_only()
            .is_empty()
        );
    }

    /// The flag word is `AT_SYMLINK_NOFOLLOW` and not some other bit: a wrong
    /// value here does not fail, it silently follows the link.
    #[test]
    #[cfg(unix)]
    fn nofollow_is_the_at_flag_and_follow_is_zero() {
        use super::{Link, nofollow_flag};
        assert_eq!(nofollow_flag(Link::Follow), 0);
        assert_eq!(nofollow_flag(Link::NoFollow), 0x100);
    }

    /// Asking to chmod a symbolic link itself is refused on both hosts, and the
    /// refusal comes *before* any syscall — so it does not depend on the path
    /// existing, and it can never degrade into a `chmod` of whatever the link
    /// points at. Linux has no working `lchmod` and GNU never asks; a silent
    /// follow here would land a mode write on a file nobody named.
    #[test]
    fn the_mode_of_a_symlink_itself_is_refused_rather_than_followed() {
        use super::{Link, On, set_mode};
        use std::path::Path;

        let nowhere = Path::new("no/such/path/exists/here");
        assert_eq!(
            set_mode(On::Path(nowhere, Link::NoFollow), 0o644)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::Unsupported
        );
    }

    fn at_epoch_plus(secs: u64, nanos: u32) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::new(secs, nanos)
    }

    fn before_epoch(secs: u64, nanos: u32) -> SystemTime {
        SystemTime::UNIX_EPOCH - Duration::new(secs, nanos)
    }

    /// An omitted time is the `UTIME_OMIT` sentinel, which is what makes
    /// `touch -a` able to move one timestamp and not the other.
    #[test]
    fn an_omitted_time_is_the_sentinel() {
        assert_eq!(
            to_timespec(When::Omit),
            CTimespec {
                tv_sec: 0,
                tv_nsec: UTIME_OMIT,
            }
        );
        assert_eq!(UTIME_OMIT, 1_073_741_822);
    }

    /// A time at or after the epoch converts straight across, nanoseconds
    /// included — a truncation to whole seconds here would be invisible in
    /// every test that did not look at the fractional part.
    #[test]
    fn a_time_after_the_epoch_keeps_its_nanoseconds() {
        assert_eq!(
            to_timespec(When::Set(at_epoch_plus(1_700_000_000, 123_456_700))),
            CTimespec {
                tv_sec: 1_700_000_000,
                tv_nsec: 123_456_700,
            }
        );
        assert_eq!(
            to_timespec(When::Set(SystemTime::UNIX_EPOCH)),
            CTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            }
        );
    }

    /// A time before the epoch has to have its sign reapplied, and a fractional
    /// one has to borrow a second so `tv_nsec` stays non-negative.
    #[test]
    fn a_time_before_the_epoch_borrows_a_second() {
        assert_eq!(
            to_timespec(When::Set(before_epoch(1, 0))),
            CTimespec {
                tv_sec: -1,
                tv_nsec: 0,
            }
        );
        assert_eq!(
            to_timespec(When::Set(before_epoch(0, 500_000_000))),
            CTimespec {
                tv_sec: -1,
                tv_nsec: 500_000_000,
            }
        );
        assert_eq!(
            to_timespec(When::Set(before_epoch(1, 500_000_000))),
            CTimespec {
                tv_sec: -2,
                tv_nsec: 500_000_000,
            }
        );
    }

    /// `tv_nsec` is in range for every input, which is the invariant the kernel
    /// checks: `utimensat` returns `EINVAL` for a `tv_nsec` outside
    /// `0..1_000_000_000` that is not one of the two sentinels.
    #[test]
    fn every_conversion_leaves_tv_nsec_in_range() {
        for case in [
            When::Omit,
            When::Set(SystemTime::UNIX_EPOCH),
            When::Set(at_epoch_plus(1, 1)),
            When::Set(at_epoch_plus(1_700_000_000, 999_999_999)),
            When::Set(before_epoch(0, 1)),
            When::Set(before_epoch(0, 999_999_999)),
            When::Set(before_epoch(86_400 * 365 * 100, 1)),
        ] {
            let ts = to_timespec(case);
            assert!(
                (0..=999_999_999).contains(&ts.tv_nsec) || ts.tv_nsec == UTIME_OMIT,
                "tv_nsec out of range: {ts:?}"
            );
        }
    }

    /// The pair is in the order `utimensat` reads it — access first, then
    /// modification. Swapping them would pass every single-timestamp test.
    #[test]
    fn the_pair_is_access_then_modification() {
        let ts = to_timespecs(Times {
            accessed: When::Set(at_epoch_plus(11, 0)),
            modified: When::Set(at_epoch_plus(22, 0)),
        });
        assert_eq!(ts[0].tv_sec, 11);
        assert_eq!(ts[1].tv_sec, 22);

        let only_access = to_timespecs(Times {
            accessed: When::Set(at_epoch_plus(11, 0)),
            modified: When::Omit,
        });
        assert_eq!(only_access[0].tv_sec, 11);
        assert_eq!(only_access[1].tv_nsec, UTIME_OMIT);
    }

    /// [`Times::both`] sets the two to one instant, which is what a preserving
    /// copy wants and is easy to get half-right.
    #[test]
    fn both_sets_the_two_to_one_instant() {
        let ts = to_timespecs(Times::both(at_epoch_plus(7, 8)));
        assert_eq!(ts[0], ts[1]);
        assert_eq!(ts[0].tv_sec, 7);
        assert_eq!(ts[0].tv_nsec, 8);
    }

    /// A path with a NUL in it is refused rather than truncated. C has no way
    /// to express one, so `utimensat` would stamp the prefix and report
    /// success — `touch "a\0b"` would silently stamp `a`.
    #[test]
    #[cfg(unix)]
    fn a_path_with_a_nul_is_refused_not_truncated() {
        use super::c_path;
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::path::Path;

        assert_eq!(c_path(Path::new("ab")).unwrap(), vec![b'a', b'b', 0]);
        assert_eq!(
            c_path(Path::new(OsStr::from_bytes(b"a\0b")))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        // And a non-UTF-8 path survives the trip, which `CString::new(to_str())`
        // would not.
        assert_eq!(
            c_path(Path::new(OsStr::from_bytes(b"a\xffb"))).unwrap(),
            vec![b'a', 0xff, b'b', 0]
        );
    }
}
