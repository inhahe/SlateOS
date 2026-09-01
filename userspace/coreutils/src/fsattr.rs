//! Writing a file's metadata *by path* — its timestamps, its mode, its owner and
//! its extended attributes — and reading the same four off the source they are
//! being copied from.
//!
//! `std` can read all three and write almost none of them. [`std::fs::set_permissions`]
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
//! # They are one module because `-p` writes all of them
//!
//! `cp -p` restores timestamps, ownership, extended attributes and mode, in
//! that order and for the two reasons GNU gives where it does it: "chown turns
//! off set\[ug\]id bits for non-root, so do the chmod last" (`copy.c:3245`), and
//! "set xattrs after ownership as changing owners will clear capabilities"
//! (`copy.c:3279`). Splitting them across four modules would put the halves of
//! one ordering constraint in four files, and the ordering is the part that is
//! easy to get wrong — a `chmod` before the `chown` compiles, runs, and quietly
//! drops the setuid bit off every copy made by a non-root user, and a
//! `setxattr` before the `chown` loses `security.capability` the same way.
//!
//! [`Link`] is the other thing they share: each has to be able to land on a
//! symbolic link rather than on what it names, because `cp -P -p` stamps the
//! link it just made. One enum answers that question for all of them rather
//! than four booleans spelled four ways.
//!
//! # The read half is here because it is the write half's argument
//!
//! [`times_of`], [`owner_of`] and [`permission_bits`] read a source's
//! attributes; the rest of the module writes them. They belong together because
//! each exists solely to produce what one of the writes consumes, and because
//! each is a *portability* question with the same two answers as its write:
//! `MetadataExt::mode` and `MetadataExt::uid` do not exist off unix, so both
//! come in a `#[cfg]` pair whose non-unix arm has to agree with the non-unix arm
//! of [`set_mode`] and [`set_owner`] about what a host with no modes and no uids
//! should do. Splitting them would put the two halves of that agreement in two
//! files. [`is_denied_ownership`] and [`chown_privileges`] are here for the same
//! reason from the other end: they interpret [`set_owner`]'s failure, and an
//! `EPERM` from a `chown` means "you are not root", which is a fact about the
//! syscall rather than about any one caller.
//!
//! They were `cp`'s private functions until `mv` needed them: its cross-device
//! fallback is a copy, so it preserves exactly what `cp -p` does and must get
//! the same ordering and the same non-unix answers. Copying five functions into
//! a second binary would have been five more chances for the two to drift.
//!
//! The one asymmetry is that the mode is written *twice* where ACLs are
//! involved, and deliberately: [`Xattrs::Permissions`] carries the access ACL,
//! which on this filesystem *is* `system.posix_acl_access`, and writing an ACL
//! rewrites the mode bits it encodes. gnulib's `qcopy_acl` handles this by
//! chmod-ing first and copying the ACL second, so that the ACL wins where the
//! two disagree and the mode still lands where there is no ACL at all.
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
use crate::pathname::c_path;
use crate::quote::{quoteaf, quoteaf_os};

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

/// The two timestamps of `meta`, in the form [`set_times`] takes.
///
/// GNU reads these out of the `struct stat` it is already holding, where they
/// cannot fail. `std` hands them back as a `Result` because a platform can
/// genuinely lack one, so the failure is reported rather than turned into
/// [`When::Omit`] — a copy silently keeping *its own* modification time is
/// exactly the wrong answer `-p` was given to prevent.
///
/// # Errors
///
/// Whatever `std` said about reading either stamp.
pub fn times_of(meta: &std::fs::Metadata) -> io::Result<Times> {
    Ok(Times {
        accessed: When::Set(meta.accessed()?),
        modified: When::Set(meta.modified()?),
    })
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

/// The owner and group of `meta`, as [`set_owner`] wants them.
#[cfg(unix)]
#[must_use]
pub fn owner_of(meta: &std::fs::Metadata) -> Owner {
    use std::os::unix::fs::MetadataExt;
    Owner::of(meta.uid(), meta.gid())
}

/// Windows files have no numeric owner. An empty [`Owner`] is one [`set_owner`]
/// returns from without a syscall, which is the honest answer on a host where
/// there is nothing to set.
#[cfg(not(unix))]
#[must_use]
pub fn owner_of(_meta: &std::fs::Metadata) -> Owner {
    Owner::default()
}

/// Whether the destination's owner or group is not already the source's.
///
/// GNU's `SAME_OWNER_AND_GROUP`, negated, and it is an optimisation with teeth:
/// the `chown` it skips would strip the set-user-ID bit off a destination that
/// needed no change at all.
#[cfg(unix)]
#[must_use]
pub fn owner_differs(on: On<'_>, src_meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    let dst = match on {
        On::File(f) => f.metadata(),
        On::Path(path, Link::Follow) => std::fs::metadata(path),
        On::Path(path, Link::NoFollow) => std::fs::symlink_metadata(path),
    };
    // A destination that cannot be stat'd counts as differing: the `chown` is
    // then attempted and reports its own failure, which says more than skipping
    // it silently on the strength of a lookup that failed.
    !dst.is_ok_and(|d| d.uid() == src_meta.uid() && d.gid() == src_meta.gid())
}

/// See [`owner_of`]'s non-unix arm: there is no ownership to compare.
#[cfg(not(unix))]
#[must_use]
pub fn owner_differs(_on: On<'_>, _src_meta: &std::fs::Metadata) -> bool {
    false
}

/// Whether a refused `chown` is followed by the half of it that needs no
/// privilege.
///
/// Not a `bool`, for [`Link`]'s reason: `take_ownership(on, want, true)` reads
/// as neither the question nor the answer, and the two things it could be read
/// as — "retry" and "this is a symlink" — happen to be opposites here.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum GroupRetry {
    /// After both halves were refused, try `chown(-1, gid)`. An ordinary user
    /// may not *give a file away* but may put it in a group they belong to, so
    /// this recovers the half that was never the privileged one. GNU does it in
    /// `set_owner` (`copy.c:932` and `:947`), ignoring the result and keeping
    /// the original `errno`, because whether it worked changes nothing about
    /// what to say next.
    Yes,
    /// Do not. GNU's symlink arm (`copy.c:3180`) is a bare `lchownat` with no
    /// retry, unlike `copy_reg` and the shared tail, and the difference is
    /// visible in `ls -l` on the copied link.
    No,
}

/// What an attempt to give a copy its source's owner came to.
///
/// GNU's `set_owner` returns 1, 0 or -1 and the three are three different
/// things rather than a success and a failure. The -1 arm is not here: upstream
/// reaches it only under `require_preserve`, which is the *caller's* policy —
/// `cp --preserve=ownership` sets it and `mv` never does — so this reports what
/// happened and the caller decides whether that ends the copy.
#[cfg_attr(test, derive(Debug))]
pub enum Ownership {
    /// The owner and group are the source's now. GNU's 1.
    Taken,
    /// They are not, and the kernel was right: an ordinary user cannot give a
    /// file away, and this is the overwhelmingly common outcome of `cp -p`.
    /// Nothing to report — a diagnostic here would print on every `cp -p` any
    /// non-root user has ever run.
    ///
    /// **The caller must drop the set-user-ID, set-group-ID and sticky bits**
    /// from the mode it is about to restore (GNU's `src_mode &= ~(S_ISUID |
    /// S_ISGID | S_ISVTX)`): a set-user-ID bit on a file that is now *theirs*
    /// would be a privilege nobody granted.
    Denied,
    /// It failed for a reason worth a diagnostic — either this process is root,
    /// for whom "not permitted" is a real fault, or the errno was not a refusal
    /// at all. The caller writes the sentence, which is not the same in the two
    /// programs that need one. The set-ID bits must be dropped for this outcome
    /// too; GNU falls through to the same `return 0`.
    Failed(io::Error),
}

/// Give `on` the owner and group in `want`, with GNU's retry and GNU's reading
/// of what a refusal means.
///
/// The whole of `set_owner` (`copy.c:897`) bar two things the caller keeps: the
/// diagnostic, whose wording differs between programs, and the narrowing of an
/// *existing* destination's mode before the handover, which `cp` alone reaches
/// — `mv`'s cross-device fallback has always just unlinked the destination, so
/// its `new_dst` is unconditionally true.
///
/// Shared rather than written twice for the reason the readers above it are:
/// the retry, the `EPERM`-or-`EINVAL` test and the root check are one decision
/// with three moving parts, and a second copy of it would be three more chances
/// for `cp -p` and `mv` across devices to disagree about who owns the result.
pub fn take_ownership(on: On<'_>, want: Owner, retry: GroupRetry) -> Ownership {
    let Err(e) = set_owner(on, want) else {
        return Ownership::Taken;
    };
    if is_denied_ownership(&e) {
        if retry == GroupRetry::Yes {
            // Deliberately discarded, and the original error deliberately kept:
            // see [`GroupRetry::Yes`].
            let _ = set_owner(on, want.group_only());
        }
        if !chown_privileges() {
            return Ownership::Denied;
        }
    }
    Ownership::Failed(e)
}

/// Whether a [`set_owner`] was refused for want of privilege rather than for a
/// reason worth reporting.
///
/// GNU tests `errno == EPERM || errno == EINVAL` in both `chown_failure_ok` and
/// `owner_failure_ok`. `EINVAL` is there because some systems answer a request
/// to set an unsupported owner that way rather than with `EPERM`.
///
/// Deliberately not [`io::ErrorKind::PermissionDenied`], which also covers
/// `EACCES` — and `EACCES` from a `chown` means a directory on the path is not
/// searchable, which is a real fault and must be reported.
#[must_use]
pub fn is_denied_ownership(e: &io::Error) -> bool {
    /// `EPERM`.
    const OPERATION_NOT_PERMITTED: i32 = 1;
    /// `EINVAL`.
    const INVALID_ARGUMENT: i32 = 22;
    matches!(
        e.raw_os_error(),
        Some(OPERATION_NOT_PERMITTED | INVALID_ARGUMENT)
    )
}

/// GNU's `chown_privileges` and `owner_privileges`, which on anything but
/// Solaris are one question: is this root?
///
/// It decides whether a refused [`set_owner`] is reported. As root it is a real
/// failure — root's `chown` does not fail for want of permission — and for
/// everyone else it is the ordinary state of affairs.
///
/// Its own `geteuid` binding, rather than a call to
/// [`can_write_any_file`](crate::overwrite::can_write_any_file), because
/// upstream keeps them apart too — this is `copy.c:3447` and that is
/// `lib/write-any-file.c`, each with its own `geteuid () == ROOT_UID`. The
/// expressions coincide; the questions do not, and folding them would make a
/// future divergence in either one silently change the other.
#[cfg(unix)]
#[must_use]
pub fn chown_privileges() -> bool {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }

    // SAFETY: `geteuid` takes no arguments, dereferences nothing, and cannot
    // fail — POSIX gives it no error return.
    unsafe { geteuid() == 0 }
}

/// Windows has no `chown` for [`set_owner`] to fail at, so nothing consults
/// this. Answering "not privileged" keeps any future caller on the quiet path
/// rather than the reporting one.
#[cfg(not(unix))]
#[must_use]
pub fn chown_privileges() -> bool {
    false
}

/// The permission and special bits — `07777` — of `meta`.
#[cfg(unix)]
#[must_use]
pub fn permission_bits(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode() & 0o7777
}

/// Windows has no mode bits. `0o777` rather than `0` so that the arithmetic in
/// `cp`'s directory walk — withhold group/other write, put it back if the
/// directory did not get it — cancels out to no change at all, which is the
/// right answer on a host where every `chmod` is a no-op anyway.
#[cfg(not(unix))]
#[must_use]
pub fn permission_bits(_meta: &std::fs::Metadata) -> u32 {
    0o777
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

/// Give a file exactly this mode, and no access-control list that could grant
/// more than it says.
///
/// gnulib's `qset_acl`, whose comment is the reason this is not [`set_mode`]:
/// *"Set the access control lists of a file to match **exactly** MODE (this
/// might remove inherited ACLs). Note chmod() tends to honor inherited/default
/// ACLs."* A chmod does not take an access ACL off — the kernel rewrites the
/// list's owner, mask and other entries to match the new bits and **keeps every
/// named-user and named-group entry**. So `chmod 0700` on a file somebody had
/// granted a named user access to leaves that grant standing, and a mode of
/// `0700` that reads as "only the owner" is not what the kernel will enforce.
///
/// gnulib reaches the same state by writing a three-entry ACL built from the
/// mode; Linux stores such a list as plain mode bits and drops the attribute, so
/// writing the minimal list and deleting the attribute are the same operation.
/// Deleting it is the one this OS can express without an ACL encoder.
///
/// The **default** ACL is deliberately left alone: it decides what a directory
/// hands to files created inside it later, not who may use the directory now,
/// and gnulib does not touch it here either. (Its `S_ISDIR (ctx->mode)` test
/// cannot fire from `cp`, which passes permission bits with no file-type bits
/// in them.) [`copy_permissions`] is where the default ACL does change.
///
/// A filesystem with no extended attributes at all answers the removal with
/// `ENOTSUP`, which is success: a file that cannot have an ACL does not have
/// one. That is gnulib's `acl_errno_valid` arm, which clears the error when the
/// list being written came from the mode.
///
/// # Errors
///
/// Whatever the `chmod` said, or whatever the removal said other than "there was
/// no such attribute" and "this filesystem has no attributes".
#[cfg(unix)]
pub fn set_mode_exactly(on: On<'_>, mode: u32) -> io::Result<()> {
    set_mode(on, mode)?;
    clear_acl(on, b"system.posix_acl_access")
}

/// Give a file exactly this mode.
///
/// # Errors
///
/// As [`set_mode`]; this platform has no access-control lists to remove.
#[cfg(not(unix))]
pub fn set_mode_exactly(on: On<'_>, mode: u32) -> io::Result<()> {
    set_mode(on, mode)
}

/// Make one file's permissions another's — the mode and the access-control
/// lists together.
///
/// gnulib's `qcopy_acl`, in the shape it takes when libattr is present
/// (`USE_XATTR`): chmod first, then carry the permission-class attributes
/// across. The order is gnulib's and its comment says why — *"we chmod before
/// setting ACLs as doing it after could overwrite them"*. Writing an access ACL
/// rewrites the file's mode bits from the list's own entries, so a chmod
/// afterwards would undo it.
///
/// **The clearing step in the middle is not in that branch of gnulib, and is
/// deliberate.** `attr_copy_file` copies the names the *source* has; a name the
/// source lacks and the destination has is left standing. Copy a file with no
/// ACL over one that grants a named user access, and under `--preserve=mode`
/// that grant survives on a file whose permissions the user just asked to be
/// made identical to the source's. gnulib's own non-libattr branch does not
/// have that hole — it goes through `set_permissions`, which replaces the
/// destination's access ACL and deletes its default one — so this follows the
/// branch that is right rather than the branch that is faster. See
/// `design-decisions.md` §738.
///
/// Both names are cleared unconditionally, including on a regular file, which
/// cannot have a default ACL: the removal of a name that is not there costs one
/// syscall and returns success, and a caller that had to know whether its
/// destination was a directory would be a caller that could get it wrong.
///
/// The two sides are not symmetric about symbolic links, and are not meant to
/// be: `from` may be [`Link::NoFollow`], because reading a link's own
/// attributes is meaningful and is what `cp` asks for, while `to` may not,
/// because [`set_mode`] refuses a link — a symbolic link has no permissions of
/// its own to write. `cp` never reaches here with a link destination, which is
/// also why GNU returns before its `copy_acl` when `dest_is_symlink`
/// (`copy.c:3286`).
///
/// # Errors
///
/// The first thing that failed: the chmod, a clearing step, or the copy. `cp`
/// reports all three with the same sentence, which is also gnulib's behaviour
/// under `USE_XATTR` — its `-2` "the source is at fault" code is reachable only
/// from the other branch.
#[cfg(unix)]
pub fn copy_permissions(from: On<'_>, to: On<'_>, mode: u32) -> io::Result<()> {
    set_mode(to, mode)?;
    for name in PERMISSION_XATTRS {
        clear_acl(to, name)?;
    }
    match copy_xattrs(from, to, Xattrs::Permissions)
        .into_iter()
        .next()
    {
        Some(failure) => Err(failure.err),
        None => Ok(()),
    }
}

/// Make one file's permissions another's.
///
/// # Errors
///
/// As [`set_mode`]; this platform has no access-control lists to carry.
#[cfg(not(unix))]
pub fn copy_permissions(_from: On<'_>, to: On<'_>, mode: u32) -> io::Result<()> {
    set_mode(to, mode)
}

/// Take an access-control list off, treating a filesystem that has no extended
/// attributes as one that had no list. See [`set_mode_exactly`].
#[cfg(unix)]
fn clear_acl(on: On<'_>, name: &[u8]) -> io::Result<()> {
    match remove_xattr(on, name) {
        Err(e) if !absent_everywhere(&e) => Err(e),
        _ => Ok(()),
    }
}

/// Which of a file's extended attributes a copy carries.
///
/// The split has to exist because `system.posix_acl_access` is *both* an
/// extended attribute and the file's permissions — this kernel stores POSIX
/// ACLs in exactly the ext4 form Linux does (`kernel/src/fs/acl.rs`,
/// `posix/src/linux_acl.rs`). Copying it under `--preserve=xattr` would make
/// that option change who may read the file, which is `--preserve=mode`'s job
/// and which a user asking only for extended attributes did not ask for.
///
/// GNU reaches the same split through `/etc/xattr.conf`, whose `permissions`
/// action marks exactly these names (libattr `attr_copy_action`): gnulib's
/// `qcopy_acl` copies the names that action selects, and coreutils' `copy_attr`
/// copies the rest. This OS has no `/etc/xattr.conf` — nothing ships one and
/// nothing reads one — so the classification is [`PERMISSION_XATTRS`] rather
/// than a parsed file. The cost is that a site cannot add its own `skip` rules;
/// the benefit is that the two halves cannot drift apart, which is exactly what
/// happens on a Linux box whose `/etc/xattr.conf` is missing — there
/// `--preserve=xattr` silently starts copying ACLs, because the classification
/// lives in a file rather than in the program.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Xattrs {
    /// Everything except the permission-class names. What `--preserve=xattr`
    /// carries.
    Ordinary,
    /// Only the permission-class names — the access ACL and the default ACL.
    /// What `--preserve=mode` carries, *after* the mode itself is written.
    Permissions,
}

/// The attribute names that are a file's permissions rather than data about it.
///
/// These are the `permissions` lines of the `/etc/xattr.conf` that distributions
/// ship, narrowed to the two this kernel actually stores. The NFSv4 and SGI
/// names in that file name ACL flavours nothing in this tree implements, and
/// listing them would be guessing at spellings no code here can produce.
const PERMISSION_XATTRS: [&[u8]; 2] = [b"system.posix_acl_access", b"system.posix_acl_default"];

impl Xattrs {
    /// Whether a copy of this class carries the attribute called `name`.
    #[must_use]
    pub fn carries(self, name: &[u8]) -> bool {
        let is_permission = PERMISSION_XATTRS.contains(&name);
        match self {
            Xattrs::Ordinary => !is_permission,
            Xattrs::Permissions => is_permission,
        }
    }
}

/// Which step of an extended-attribute copy failed.
///
/// The variant picks the message, because GNU's four wordings are not
/// interchangeable: two name the attribute and two do not, and two blame the
/// source while two blame the destination. A caller that had only an `io::Error`
/// could not reconstruct which.
///
/// The two that name an attribute carry it, rather than the whole error carrying
/// an `Option<Vec<u8>>` beside the step. Both spellings hold the same
/// information for the four states that can happen; only this one refuses to
/// hold the four that cannot, and so leaves the caller's `match` with no arm
/// for a `Get` with no name to be mislabelled in.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum XattrStep {
    /// Reading the source's list of attribute names — libattr's
    /// `listing attributes of %s`, about the *source*.
    List,
    /// Reading one attribute's value from the source — `getting attribute %s of
    /// %s`, about the source.
    Get(Vec<u8>),
    /// Writing one attribute to the destination — `setting attribute %s for %s`,
    /// about the *destination*.
    Set(Vec<u8>),
    /// Writing attributes to the destination failed in a way that is about the
    /// destination as a whole rather than about one name — `setting attributes
    /// for %s`. Two things reach it: `ENOSYS`, where libattr gives up on the
    /// rest ("no hope of getting any further"), and a run in which one or more
    /// individual writes returned `ENOTSUP`, which libattr collects into this
    /// single report instead of one per attribute.
    SetAll,
}

/// One thing that went wrong while copying a file's extended attributes.
///
/// A copy reports a *list* of these rather than stopping at the first, because
/// libattr does not stop: a `getxattr` that fails is recorded and the loop moves
/// to the next name. Collapsing them to one error would hide every attribute
/// after the first unreadable one, and "the copy is missing an attribute nobody
/// mentioned" is the failure mode the whole option exists to prevent.
pub struct XattrError {
    /// Which step failed, and so which wording applies.
    pub at: XattrStep,
    /// What the kernel said.
    pub err: io::Error,
}

impl XattrStep {
    /// libattr's four sentences, filled in.
    ///
    /// They are not interchangeable: two name the attribute and two do not, and
    /// two blame the source while two blame the destination. The caller supplies
    /// both names and this picks between them; handing them over the wrong way
    /// round produces a sentence that is grammatical and names the wrong file.
    ///
    /// The attribute name goes through `quoteaf` for the same reason the file
    /// names do — coreutils hands libattr `copy_attr_quote`, which *is*
    /// `quoteaf`, and libattr quotes the attribute name with it as well as the
    /// path.
    ///
    /// Here rather than in a utility because both `cp` and `mv` copy extended
    /// attributes and neither may word the failure differently: `mv` reaches
    /// this through `cp_option_init`'s `preserve_xattr = true` (`mv.c:145`), so
    /// the two are the *same* gnulib call site, and a script that matches one
    /// has to match the other.
    #[must_use]
    pub fn sentence(&self, src: &Path, dst: &Path) -> String {
        match self {
            XattrStep::List => format!("listing attributes of {}", quoteaf_os(src)),
            XattrStep::Get(name) => {
                format!("getting attribute {} of {}", quoteaf(name), quoteaf_os(src))
            }
            XattrStep::Set(name) => {
                format!(
                    "setting attribute {} for {}",
                    quoteaf(name),
                    quoteaf_os(dst)
                )
            }
            XattrStep::SetAll => format!("setting attributes for {}", quoteaf_os(dst)),
        }
    }
}

/// gnulib's `errno_unsupported` (`copy.c:700`): the two errors that mean the
/// filesystem has nothing to say rather than that something went wrong.
///
/// This is *not* [`absent_everywhere`], and the difference between them is the
/// reason both exist. That one decides whether there was a failure at all; this
/// one decides whether to mention one there certainly was. `ENODATA` is here and
/// not there: it cannot come from the initial `listxattr` — a filesystem does
/// not answer "no such attribute" to a request for the list — but it can come
/// from a `getxattr` for a name removed between the listing and the read, and a
/// copy losing a race with `setfattr -x` is not worth a diagnostic.
///
/// *Which* of three volumes a caller wants is not decided here, because it is
/// not the same question for the two callers. gnulib picks one of three error
/// callbacks (`copy.c:782`), which reads as two booleans and is three
/// behaviours:
///
/// | Asked for | Printed | Exit status |
/// |---|---|---|
/// | `cp --preserve=xattr` | every failure | 1 |
/// | `cp --preserve=all`, and **all of `mv`** | all but this predicate | 0 |
/// | `cp -a` | nothing at all | 0 |
///
/// `mv` has no option that moves it off the middle row: `cp_option_init` sets
/// `require_preserve_xattr = false` and `reduce_diagnostics = false`
/// (`mv.c:145`, `mv.c:141`), and nothing in `mv`'s getopt writes either field.
#[must_use]
pub fn errno_unsupported(e: &io::Error) -> bool {
    e.raw_os_error()
        .is_some_and(|n| n == ENOTSUP || n == ENODATA)
}

/// Copy extended attributes from one file to another.
///
/// Returns the failures in the order they happened; an empty vector is success.
/// There is no `Result`, because "some attributes copied and one did not" is the
/// ordinary outcome and is not representable as one — see [`XattrError`].
///
/// # Not finding any is not a failure
///
/// A source on a filesystem with no attribute support at all answers the initial
/// `listxattr` with `ENOTSUP` or `ENOSYS`, and that is reported as success with
/// nothing to copy, exactly as libattr's `attr_copy_file` does (`goto getout`
/// without setting `ret`). The alternative — reporting it — would make `cp -a`
/// noisy on every filesystem that has no xattrs, which is most of them.
///
/// The *destination* refusing is different and is reported, because there the
/// source did have attributes and they have been dropped. `cp` then decides what
/// to make of it: `--preserve=xattr` treats it as fatal, plain `-a` suppresses
/// the message (GNU's `copy_attr_error` vs `copy_attr_allerror`, `copy.c:708`).
///
/// # The link is never followed, for `cp`
///
/// libattr's path form is `l*` throughout — `llistxattr`, `lgetxattr`,
/// `lsetxattr` — with no option to follow, so `cp` passes [`Link::NoFollow`].
/// [`Link::Follow`] is honoured here anyway rather than refused: the `l` and
/// non-`l` pair exists in the ABI, and a module that owns the syscall
/// declarations should not be the place a caller's choice is quietly narrowed.
#[cfg(unix)]
#[must_use]
pub fn copy_xattrs(from: On<'_>, to: On<'_>, which: Xattrs) -> Vec<XattrError> {
    let mut failures = Vec::new();

    let names = match list_xattrs(from) {
        Ok(names) => names,
        Err(err) => {
            if !absent_everywhere(&err) {
                failures.push(XattrError {
                    at: XattrStep::List,
                    err,
                });
            }
            return failures;
        }
    };

    // libattr collects every `ENOTSUP` from the write loop into one report at
    // the end (`setxattr_ENOTSUP++`) rather than emitting one per attribute,
    // because the condition is a property of the destination filesystem and
    // repeating it once per name says nothing new.
    let mut unsupported = false;

    for name in names {
        if name.is_empty() || !which.carries(&name) {
            continue;
        }
        let value = match get_xattr(from, &name) {
            Ok(value) => value,
            Err(err) => {
                failures.push(XattrError {
                    at: XattrStep::Get(name),
                    err,
                });
                continue;
            }
        };
        let Err(err) = set_xattr(to, &name, &value) else {
            continue;
        };
        match err.raw_os_error() {
            Some(ENOTSUP) => unsupported = true,
            // "no hope of getting any further": `ENOSYS` is the C library
            // saying the call does not exist, which the next name will not
            // change.
            Some(ENOSYS) => {
                failures.push(XattrError {
                    at: XattrStep::SetAll,
                    err,
                });
                break;
            }
            _ => failures.push(XattrError {
                at: XattrStep::Set(name),
                err,
            }),
        }
    }

    if unsupported {
        failures.push(XattrError {
            at: XattrStep::SetAll,
            err: io::Error::from_raw_os_error(ENOTSUP),
        });
    }
    failures
}

/// Copy extended attributes from one file to another.
///
/// Windows has no extended attributes that this crate models — NTFS alternate
/// data streams are a different thing with a different naming scheme — so there
/// is never anything to copy and the answer is always "no failures". Same
/// reasoning as [`set_owner`]'s non-unix arm: the target OS is the
/// `#[cfg(unix)]` arm, and a `cp -a` that failed here would fail in the test
/// suite and nowhere else.
#[cfg(not(unix))]
#[must_use]
pub fn copy_xattrs(_from: On<'_>, _to: On<'_>, _which: Xattrs) -> Vec<XattrError> {
    Vec::new()
}

/// Read a file's list of extended-attribute names.
///
/// Where [`copy_xattrs`]'s non-unix arm answers "there was nothing to copy",
/// these three answer "you cannot ask that here". The difference is that a copy
/// of no attributes is a real outcome a caller can act on, while a *list* of no
/// attributes would be a claim about the file that this host cannot make. See
/// that function for why the `#[cfg(unix)]` arm is the one that ships.
///
/// # Errors
///
/// Always [`io::ErrorKind::Unsupported`], on this platform.
#[cfg(not(unix))]
pub fn list_xattrs(_on: On<'_>) -> io::Result<Vec<Vec<u8>>> {
    Err(no_xattrs_here())
}

/// Read one extended attribute's value.
///
/// # Errors
///
/// Always [`io::ErrorKind::Unsupported`], on this platform. See
/// [`list_xattrs`].
#[cfg(not(unix))]
pub fn get_xattr(_on: On<'_>, _name: &[u8]) -> io::Result<Vec<u8>> {
    Err(no_xattrs_here())
}

/// Write one extended attribute's value.
///
/// # Errors
///
/// Always [`io::ErrorKind::Unsupported`], on this platform. See
/// [`list_xattrs`].
#[cfg(not(unix))]
pub fn set_xattr(_on: On<'_>, _name: &[u8], _value: &[u8]) -> io::Result<()> {
    Err(no_xattrs_here())
}

/// The one error the three non-unix arms return.
#[cfg(not(unix))]
fn no_xattrs_here() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "this platform has no extended attributes",
    )
}

/// `ENOTSUP` (== `EOPNOTSUPP`) on Linux, the only ABI this ships on. This crate
/// has no `libc` dependency to read it from; named for the same reason
/// `cp`'s `libc_eloop` is.
///
/// Ungated, unlike two of its three neighbours, because [`errno_unsupported`] is
/// ungated and has to compile on the Windows *host* the tests run on. Comparing
/// a Windows error number against 95 is meaningless — and unreachable: the
/// non-unix [`copy_xattrs`] returns no failures, so nothing is ever handed to
/// that predicate there. A second `cfg(not(unix))` arm answering a fixed `false`
/// would put one decision in two places to avoid a comparison that never
/// happens. [`ENODATA`] is ungated for the same reason.
const ENOTSUP: i32 = 95;

/// `ENOSYS` on Linux — "the kernel does not implement this call".
#[cfg(unix)]
const ENOSYS: i32 = 38;

/// `ERANGE` — "the buffer you offered is too small", which for the `*xattr`
/// family means the value grew between the size probe and the read.
#[cfg(unix)]
const ERANGE: i32 = 34;

/// `ENODATA` (== `ENOATTR`) on Linux — "the file exists, that attribute does
/// not". The two spellings are one number; there is no second code.
const ENODATA: i32 = 61;

/// Whether a failure means "this filesystem simply has no extended attributes",
/// as opposed to "the attributes exist and something went wrong".
///
/// libattr's own test at the head of `attr_copy_file`: `errno != ENOSYS &&
/// errno != ENOTSUP`. Note it is *not* coreutils' `errno_unsupported`, which is
/// `ENOTSUP || ENODATA` — that one decides whether to *print* a failure, this one
/// decides whether there is a failure at all.
#[cfg(unix)]
fn absent_everywhere(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(ENOTSUP | ENOSYS))
}

/// How many times a size probe may be re-run before its `ERANGE` is reported.
///
/// libattr probes once and reads once, and reports the `ERANGE` if the value
/// grew in between (`attr_copy_file.c:90`). That is a race, not a design: the
/// attribute is perfectly copyable and the report is wrong. Retrying costs one
/// extra syscall in a case that essentially never happens and removes a
/// spurious "cp: getting attribute 'user.x' of 'f': Numerical result out of
/// range" from a concurrent writer's way. The bound exists so that a value being
/// rewritten in a tight loop cannot spin here forever; the `ERANGE` is then
/// reported, which is what libattr would have done immediately.
#[cfg(unix)]
const XATTR_SIZE_RETRIES: u32 = 4;

/// Read a file's list of extended-attribute names, NUL-separated by the kernel
/// and split here.
///
/// # Errors
///
/// Whatever `listxattr`/`llistxattr`/`flistxattr` said — including `ENOTSUP`
/// from a filesystem that has no extended attributes at all, which is a
/// question this returns rather than answers. [`copy_xattrs`] is where that
/// particular error becomes "nothing to copy".
#[cfg(unix)]
pub fn list_xattrs(on: On<'_>) -> io::Result<Vec<Vec<u8>>> {
    // The three spellings differ only in what they are given; keeping the choice
    // in one place keeps the probe-then-read loop from repeating it four times.
    //
    // # Safety
    //
    // Either `cpath` is `Some` NUL-terminated bytes that outlive the call, or
    // `on` is `On::File` holding a live descriptor.
    unsafe fn call(on: On<'_>, cpath: Option<&[u8]>, buf: &mut [u8]) -> isize {
        unsafe extern "C" {
            fn listxattr(path: *const u8, list: *mut u8, size: usize) -> isize;
            fn llistxattr(path: *const u8, list: *mut u8, size: usize) -> isize;
            fn flistxattr(fd: i32, list: *mut u8, size: usize) -> isize;
        }
        use std::os::unix::io::AsRawFd;

        // A null pointer with a zero size is the documented way to ask only for
        // the length. An empty slice's `as_mut_ptr` is dangling rather than
        // null, which is not the same request.
        let (ptr, len) = if buf.is_empty() {
            (core::ptr::null_mut(), 0)
        } else {
            (buf.as_mut_ptr(), buf.len())
        };
        match (on, cpath) {
            // SAFETY: forwarded from this function's own contract. `buf` is
            // `len` bytes and is written, not read, so its previous contents are
            // never observed by the kernel.
            (On::File(f), _) => unsafe { flistxattr(f.as_raw_fd(), ptr, len) },
            (On::Path(_, Link::Follow), Some(p)) => unsafe { listxattr(p.as_ptr(), ptr, len) },
            (On::Path(_, Link::NoFollow), Some(p)) => unsafe { llistxattr(p.as_ptr(), ptr, len) },
            // Unreachable: `cpath` is `Some` for exactly the `On::Path` arms.
            (On::Path(..), None) => -1,
        }
    }

    let cpath = match on {
        On::Path(path, _) => Some(c_path(path)?),
        On::File(_) => None,
    };

    let mut buf = Vec::new();
    for _ in 0..=XATTR_SIZE_RETRIES {
        // SAFETY: `cpath` is `Some` for exactly the `On::Path` arms, and holds a
        // NUL-terminated copy of the path that outlives the call; the `On::File`
        // arm holds a live `File`.
        let want = unsafe { call(on, cpath.as_deref(), &mut buf) };
        let Ok(want) = usize::try_from(want) else {
            return Err(io::Error::last_os_error());
        };
        if want == 0 {
            return Ok(Vec::new());
        }
        buf.resize(want, 0);
        // SAFETY: as above; `buf` now has `want` bytes for the kernel to fill.
        let got = unsafe { call(on, cpath.as_deref(), &mut buf) };
        if let Ok(got) = usize::try_from(got) {
            buf.truncate(got);
            return Ok(split_names(&buf));
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(ERANGE) {
            return Err(err);
        }
        buf.clear();
    }
    Err(io::Error::from_raw_os_error(ERANGE))
}

/// Split the kernel's NUL-separated name list.
///
/// A trailing NUL terminates the last name rather than introducing an empty one,
/// which is why this splits on NUL and drops a single trailing empty rather than
/// using `split` naively. An interior empty name would be a kernel bug; it is
/// dropped rather than trusted, because [`copy_xattrs`] would otherwise ask for
/// an attribute called `""`.
#[cfg_attr(not(unix), allow(dead_code))]
fn split_names(list: &[u8]) -> Vec<Vec<u8>> {
    list.split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

/// Read one extended attribute's value.
///
/// A present attribute with no bytes is `Ok(vec![])` and an absent one is
/// `ENODATA`; the two are different states and the kernel distinguishes them.
///
/// # Errors
///
/// Whatever `getxattr`/`lgetxattr`/`fgetxattr` said, or [`io::ErrorKind::
/// InvalidInput`] for a name containing a NUL.
#[cfg(unix)]
pub fn get_xattr(on: On<'_>, name: &[u8]) -> io::Result<Vec<u8>> {
    /// # Safety
    ///
    /// As [`list_xattrs`]'s `call`, plus: `cname` is NUL-terminated and outlives
    /// the call.
    unsafe fn call(on: On<'_>, cpath: Option<&[u8]>, cname: &[u8], buf: &mut [u8]) -> isize {
        unsafe extern "C" {
            fn getxattr(path: *const u8, name: *const u8, value: *mut u8, size: usize) -> isize;
            fn lgetxattr(path: *const u8, name: *const u8, value: *mut u8, size: usize) -> isize;
            fn fgetxattr(fd: i32, name: *const u8, value: *mut u8, size: usize) -> isize;
        }
        use std::os::unix::io::AsRawFd;

        let (ptr, len) = if buf.is_empty() {
            (core::ptr::null_mut(), 0)
        } else {
            (buf.as_mut_ptr(), buf.len())
        };
        let n = cname.as_ptr();
        match (on, cpath) {
            // SAFETY: forwarded from this function's own contract.
            (On::File(f), _) => unsafe { fgetxattr(f.as_raw_fd(), n, ptr, len) },
            (On::Path(_, Link::Follow), Some(p)) => unsafe { getxattr(p.as_ptr(), n, ptr, len) },
            (On::Path(_, Link::NoFollow), Some(p)) => unsafe { lgetxattr(p.as_ptr(), n, ptr, len) },
            // Unreachable: `cpath` is `Some` for exactly the `On::Path` arms.
            (On::Path(..), None) => -1,
        }
    }

    let cpath = match on {
        On::Path(path, _) => Some(c_path(path)?),
        On::File(_) => None,
    };
    let cname = c_name(name)?;

    let mut buf = Vec::new();
    for _ in 0..=XATTR_SIZE_RETRIES {
        // SAFETY: `cname` and `cpath` are NUL-terminated buffers that outlive
        // the call; the `On::File` arm holds a live `File`.
        let want = unsafe { call(on, cpath.as_deref(), &cname, &mut buf) };
        let Ok(want) = usize::try_from(want) else {
            return Err(io::Error::last_os_error());
        };
        if want == 0 {
            // A zero-length value is a real, storable thing, not an absence.
            return Ok(Vec::new());
        }
        buf.resize(want, 0);
        // SAFETY: as above; `buf` now has `want` bytes for the kernel to fill.
        let got = unsafe { call(on, cpath.as_deref(), &cname, &mut buf) };
        if let Ok(got) = usize::try_from(got) {
            buf.truncate(got);
            return Ok(buf);
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(ERANGE) {
            return Err(err);
        }
        buf.clear();
    }
    Err(io::Error::from_raw_os_error(ERANGE))
}

/// Write one extended attribute's value.
///
/// The flag word is zero — "create it or replace it" — which is what libattr
/// passes and what a copy wants: the destination is either new, or is being
/// overwritten on purpose.
///
/// # Errors
///
/// Whatever `setxattr`/`lsetxattr`/`fsetxattr` said, or [`io::ErrorKind::
/// InvalidInput`] for a name containing a NUL.
#[cfg(unix)]
pub fn set_xattr(on: On<'_>, name: &[u8], value: &[u8]) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    unsafe extern "C" {
        fn setxattr(
            path: *const u8,
            name: *const u8,
            value: *const u8,
            size: usize,
            flags: i32,
        ) -> i32;
        fn lsetxattr(
            path: *const u8,
            name: *const u8,
            value: *const u8,
            size: usize,
            flags: i32,
        ) -> i32;
        fn fsetxattr(fd: i32, name: *const u8, value: *const u8, size: usize, flags: i32) -> i32;
    }

    let cname = c_name(name)?;
    // A zero-length value is stored as a present attribute with no bytes, so the
    // pointer must still be non-null for the kernel's `copy_from_user`; an empty
    // slice's `as_ptr` is a dangling-but-aligned address, which is what every
    // other caller of a `(ptr, 0)` pair passes and what the kernel never reads.
    let (vptr, vlen) = (value.as_ptr(), value.len());

    let rc = match on {
        // SAFETY: `f` is a live `File`, so its descriptor is open for the whole
        // call; `cname` is NUL-terminated; `vptr`/`vlen` describe `value`, which
        // outlives the call. Nothing is retained.
        On::File(f) => unsafe { fsetxattr(f.as_raw_fd(), cname.as_ptr(), vptr, vlen, 0) },
        On::Path(path, link) => {
            let cpath = c_path(path)?;
            // SAFETY: as above, with `cpath` NUL-terminated and outliving the
            // call.
            unsafe {
                match link {
                    Link::Follow => setxattr(cpath.as_ptr(), cname.as_ptr(), vptr, vlen, 0),
                    Link::NoFollow => lsetxattr(cpath.as_ptr(), cname.as_ptr(), vptr, vlen, 0),
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

/// Take one extended attribute off a file, treating "it was not there" as done.
///
/// The absent case is folded into success on purpose. Every caller here is
/// clearing a name it does not know to be present — gnulib's `qset_acl` deletes
/// a directory's default ACL whether or not it has one, because "this file has
/// no default ACL" is the state it is trying to reach, and a file that was
/// already in that state has not failed to get there. Reporting it would make
/// `cp -p` onto an ordinary file print a diagnostic about an attribute nobody
/// asked for.
///
/// `ENODATA` is the answer, on Linux and now here too. It used to accept
/// `ENOENT` as well, because the kernel had one error for "no such file" and
/// "no such attribute" and there was no way to fold in the second without also
/// folding in the first — swallowing a genuinely missing *path*. Lane A's
/// `32f35d46b` split them (`NoAttribute`, mapped to `ENODATA` in
/// `posix::errno`), so the `ENOENT` is gone and a vanished path is reported
/// again.
///
/// # Errors
///
/// Whatever `removexattr`/`lremovexattr`/`fremovexattr` said, except `ENODATA`,
/// or [`io::ErrorKind::InvalidInput`] for a name containing a NUL.
#[cfg(unix)]
pub fn remove_xattr(on: On<'_>, name: &[u8]) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    unsafe extern "C" {
        fn removexattr(path: *const u8, name: *const u8) -> i32;
        fn lremovexattr(path: *const u8, name: *const u8) -> i32;
        fn fremovexattr(fd: i32, name: *const u8) -> i32;
    }

    let cname = c_name(name)?;

    let rc = match on {
        // SAFETY: `f` is a live `File`, so its descriptor is open for the whole
        // call, and `cname` is NUL-terminated and outlives it. Nothing is
        // retained.
        On::File(f) => unsafe { fremovexattr(f.as_raw_fd(), cname.as_ptr()) },
        On::Path(path, link) => {
            let cpath = c_path(path)?;
            // SAFETY: as above, with `cpath` NUL-terminated and outliving the
            // call.
            unsafe {
                match link {
                    Link::Follow => removexattr(cpath.as_ptr(), cname.as_ptr()),
                    Link::NoFollow => lremovexattr(cpath.as_ptr(), cname.as_ptr()),
                }
            }
        }
    };
    if rc == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(ENODATA) {
        return Ok(());
    }
    Err(err)
}

/// Take one extended attribute off a file.
///
/// # Errors
///
/// Always [`io::ErrorKind::Unsupported`], on this platform. See [`list_xattrs`].
#[cfg(not(unix))]
pub fn remove_xattr(_on: On<'_>, _name: &[u8]) -> io::Result<()> {
    Err(no_xattrs_here())
}

/// An attribute name as C wants it. The same NUL rule as
/// [`c_path`](crate::pathname::c_path), and for the same reason: a name
/// truncated at an embedded NUL would silently address a *different* attribute.
#[cfg(unix)]
fn c_name(name: &[u8]) -> io::Result<Vec<u8>> {
    if name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "attribute name contains a NUL byte",
        ));
    }
    let mut buf = Vec::with_capacity(name.len().saturating_add(1));
    buf.extend_from_slice(name);
    buf.push(0);
    Ok(buf)
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
    ///
    /// The nanoseconds are a multiple of 100 because a `SystemTime` is only as
    /// fine as the host clock underneath it, and on Windows that is a FILETIME
    /// — 100ns ticks. `UNIX_EPOCH + Duration::new(7, 8)` is not a representable
    /// instant there: it rounds to a flat 7s, and the test then read the host's
    /// dropped nanosecond field as a bug in the subject. The subject carries
    /// `u32` nanoseconds through unexamined and does not care which multiple
    /// this is, so a representable one tests the same thing on both hosts.
    #[test]
    fn both_sets_the_two_to_one_instant() {
        let ts = to_timespecs(Times::both(at_epoch_plus(7, 800)));
        assert_eq!(ts[0], ts[1]);
        assert_eq!(ts[0].tv_sec, 7);
        assert_eq!(ts[0].tv_nsec, 800);
    }

    // ------------------------------------------------ extended attributes --

    /// The two classes partition the namespace: every name is carried by
    /// exactly one of them. If a name were carried by both, `cp -a` would write
    /// the ACL twice; if by neither, `cp -a` would drop it and say nothing.
    #[test]
    fn the_two_xattr_classes_partition_every_name() {
        use super::Xattrs;

        for name in [
            &b"user.comment"[..],
            b"security.capability",
            b"system.posix_acl_access",
            b"system.posix_acl_default",
            b"trusted.whatever",
            b"user.system.posix_acl_access",
            b"",
        ] {
            assert!(
                Xattrs::Ordinary.carries(name) ^ Xattrs::Permissions.carries(name),
                "{} is in both classes or neither",
                String::from_utf8_lossy(name)
            );
        }
    }

    /// The permission class is the ACL and nothing else. A name that merely
    /// *contains* one of the two — `user.system.posix_acl_access` is a legal
    /// attribute a user can set — is ordinary data, so the test is equality and
    /// not a prefix or substring match.
    #[test]
    fn the_permission_class_is_exactly_the_two_acl_names() {
        use super::Xattrs;

        assert!(Xattrs::Permissions.carries(b"system.posix_acl_access"));
        assert!(Xattrs::Permissions.carries(b"system.posix_acl_default"));
        assert!(!Xattrs::Permissions.carries(b"user.system.posix_acl_access"));
        assert!(!Xattrs::Permissions.carries(b"system.posix_acl_acces"));
        assert!(!Xattrs::Permissions.carries(b"system.posix_acl_accessx"));
        assert!(Xattrs::Ordinary.carries(b"user.comment"));
        assert!(!Xattrs::Ordinary.carries(b"system.posix_acl_access"));
    }

    /// The kernel's list is NUL-*terminated*, not NUL-separated, so a naive
    /// split yields a trailing empty name — and asking for an attribute called
    /// `""` is an `ERANGE`/`ENODATA` that would be reported as a failed copy of
    /// an attribute that never existed.
    #[test]
    fn the_name_list_is_split_without_a_phantom_trailing_entry() {
        use super::split_names;

        assert_eq!(split_names(b""), Vec::<Vec<u8>>::new());
        assert_eq!(split_names(b"user.a\0"), vec![b"user.a".to_vec()]);
        assert_eq!(
            split_names(b"user.a\0user.b\0"),
            vec![b"user.a".to_vec(), b"user.b".to_vec()]
        );
        // A list the kernel forgot to terminate still yields the last name.
        assert_eq!(
            split_names(b"user.a\0user.b"),
            vec![b"user.a".to_vec(), b"user.b".to_vec()]
        );
        // Names are bytes, not text: a non-UTF-8 name must survive.
        assert_eq!(split_names(b"user.\xff\0"), vec![b"user.\xff".to_vec()]);
    }

    /// An attribute name with a NUL is refused rather than truncated, for a
    /// sharper reason than the path case: the truncated name is a *different,
    /// valid* attribute, so the copy would silently read and write the wrong one
    /// rather than fail.
    #[test]
    #[cfg(unix)]
    fn an_attribute_name_with_a_nul_is_refused_not_truncated() {
        use super::c_name;

        assert_eq!(c_name(b"user.a").unwrap(), b"user.a\0".to_vec());
        assert_eq!(
            c_name(b"user.a\0evil").unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
    }

    /// "This filesystem has no extended attributes" is not a failure, and the
    /// test is libattr's — `ENOSYS` and `ENOTSUP` only. `ENODATA` is
    /// deliberately absent even though coreutils' own `errno_unsupported`
    /// includes it: that one decides whether to *print*, this one decides
    /// whether anything went wrong.
    #[test]
    #[cfg(unix)]
    fn only_enosys_and_enotsup_mean_there_was_nothing_to_copy() {
        use super::absent_everywhere;
        use std::io::Error;

        assert!(absent_everywhere(&Error::from_raw_os_error(95)));
        assert!(absent_everywhere(&Error::from_raw_os_error(38)));
        assert!(!absent_everywhere(&Error::from_raw_os_error(61))); // ENODATA
        assert!(!absent_everywhere(&Error::from_raw_os_error(13))); // EACCES
        assert!(!absent_everywhere(&Error::other("not from the OS")));
    }

    // The live round-trip below needs a filesystem that stores `user.*`
    // attributes. `/tmp` on the development host is such a filesystem and
    // `/mnt/*` is not, so the helper reports which it got rather than failing:
    // a missing feature in the *test environment* must not read as a bug in the
    // code under test.
    #[cfg(unix)]
    fn scratch(stem: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("fsattr_test_{stem}_{pid}_{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Set one attribute, and say whether the filesystem took it.
    #[cfg(unix)]
    fn seed(path: &std::path::Path, name: &[u8], value: &[u8]) -> bool {
        use super::{Link, On, set_xattr};
        match set_xattr(On::Path(path, Link::NoFollow), name, value) {
            Ok(()) => true,
            Err(e) => {
                assert!(
                    super::absent_everywhere(&e),
                    "seeding {} failed for a reason other than \"unsupported\": {e}",
                    String::from_utf8_lossy(name)
                );
                false
            }
        }
    }

    /// The whole point, end to end: attributes present on the source are present
    /// on the destination afterwards, with their bytes intact.
    ///
    /// Values are checked byte-for-byte including an empty one, because a
    /// zero-length attribute is a real, storable thing that the size-probe loop
    /// could easily confuse with "not there".
    #[test]
    #[cfg(unix)]
    fn every_ordinary_attribute_crosses_with_its_bytes_intact() {
        use super::{Link, On, Xattrs, copy_xattrs, get_xattr, list_xattrs};

        let dir = scratch("roundtrip");
        let (src, dst) = (dir.join("src"), dir.join("dst"));
        std::fs::write(&src, b"body").unwrap();
        std::fs::write(&dst, b"body").unwrap();

        if !seed(&src, b"user.one", b"\x00\x01\xfe\xff") {
            return; // No xattr support here; the target OS has it.
        }
        assert!(seed(&src, b"user.empty", b""));

        let failures = copy_xattrs(
            On::Path(&src, Link::NoFollow),
            On::Path(&dst, Link::NoFollow),
            Xattrs::Ordinary,
        );
        assert!(
            failures.is_empty(),
            "unexpected failures copying attributes"
        );

        let mut names = list_xattrs(On::Path(&dst, Link::NoFollow)).unwrap();
        names.sort();
        assert_eq!(names, vec![b"user.empty".to_vec(), b"user.one".to_vec()]);
        assert_eq!(
            get_xattr(On::Path(&dst, Link::NoFollow), b"user.one").unwrap(),
            b"\x00\x01\xfe\xff".to_vec()
        );
        assert_eq!(
            get_xattr(On::Path(&dst, Link::NoFollow), b"user.empty").unwrap(),
            Vec::<u8>::new()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `Permissions` copy of a file whose only attributes are ordinary copies
    /// nothing — which is what keeps `--preserve=mode` from carrying data the
    /// user asked `--preserve=xattr` about, and vice versa.
    #[test]
    #[cfg(unix)]
    fn a_permissions_copy_leaves_ordinary_attributes_behind() {
        use super::{Link, On, Xattrs, copy_xattrs, list_xattrs};

        let dir = scratch("classes");
        let (src, dst) = (dir.join("src"), dir.join("dst"));
        std::fs::write(&src, b"body").unwrap();
        std::fs::write(&dst, b"body").unwrap();

        if !seed(&src, b"user.one", b"v") {
            return;
        }

        let failures = copy_xattrs(
            On::Path(&src, Link::NoFollow),
            On::Path(&dst, Link::NoFollow),
            Xattrs::Permissions,
        );
        assert!(failures.is_empty());
        assert!(
            list_xattrs(On::Path(&dst, Link::NoFollow))
                .unwrap()
                .is_empty(),
            "a permissions-only copy carried an ordinary attribute"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A source with no attributes at all is a successful copy of nothing, not
    /// a failure — and neither is a source on a filesystem that has no
    /// attributes. `cp -a` runs this case on almost every file it copies.
    #[test]
    #[cfg(unix)]
    fn a_source_with_no_attributes_is_a_success() {
        use super::{Link, On, Xattrs, copy_xattrs};

        let dir = scratch("empty");
        let (src, dst) = (dir.join("src"), dir.join("dst"));
        std::fs::write(&src, b"body").unwrap();
        std::fs::write(&dst, b"body").unwrap();

        assert!(
            copy_xattrs(
                On::Path(&src, Link::NoFollow),
                On::Path(&dst, Link::NoFollow),
                Xattrs::Ordinary,
            )
            .is_empty()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A destination that is not there fails once, naming the attribute, rather
    /// than looping or reporting every name.
    #[test]
    #[cfg(unix)]
    fn a_missing_destination_reports_the_attribute_it_could_not_write() {
        use super::{Link, On, XattrStep, Xattrs, copy_xattrs};

        let dir = scratch("nodest");
        let src = dir.join("src");
        std::fs::write(&src, b"body").unwrap();
        if !seed(&src, b"user.one", b"v") {
            return;
        }

        let failures = copy_xattrs(
            On::Path(&src, Link::NoFollow),
            On::Path(&dir.join("absent"), Link::NoFollow),
            Xattrs::Ordinary,
        );
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].at, XattrStep::Set(b"user.one".to_vec()));
        assert_eq!(failures[0].err.kind(), std::io::ErrorKind::NotFound);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A descriptor reaches the same attributes as the name that opened it.
    /// `cp` uses the descriptor form for regular files, so a divergence here
    /// would show up as `cp -a` copying attributes for directories and symlinks
    /// but not for the ordinary case.
    #[test]
    #[cfg(unix)]
    fn a_descriptor_and_a_name_see_the_same_attributes() {
        use super::{On, Xattrs, copy_xattrs, list_xattrs};

        let dir = scratch("byfd");
        let (src, dst) = (dir.join("src"), dir.join("dst"));
        std::fs::write(&src, b"body").unwrap();
        std::fs::write(&dst, b"body").unwrap();
        if !seed(&src, b"user.one", b"v") {
            return;
        }

        let src_file = std::fs::File::open(&src).unwrap();
        let dst_file = std::fs::OpenOptions::new().write(true).open(&dst).unwrap();
        let failures = copy_xattrs(On::File(&src_file), On::File(&dst_file), Xattrs::Ordinary);
        assert!(failures.is_empty());
        assert_eq!(
            list_xattrs(On::File(&dst_file)).unwrap(),
            vec![b"user.one".to_vec()]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Removal takes the named attribute and only the named one, by path and by
    /// descriptor alike — and removing one that is not there succeeds.
    ///
    /// That last clause is the whole reason the function exists in this shape:
    /// the caller (`cp -p` narrowing a destination's permissions) deletes the
    /// two ACL names on every file it copies, and almost no file has them. If
    /// absence were an error, the ordinary case would be the failing one.
    #[test]
    #[cfg(unix)]
    fn removal_takes_one_name_and_absence_is_not_a_failure() {
        use super::{Link, On, list_xattrs, remove_xattr};

        let dir = scratch("remove");
        let f = dir.join("f");
        std::fs::write(&f, b"body").unwrap();

        // Absent on a filesystem that has attributes, and absent because the
        // filesystem has none, both have to come back `Ok`.
        assert!(remove_xattr(On::Path(&f, Link::NoFollow), b"user.never").is_ok());

        if !seed(&f, b"user.one", b"v") {
            return;
        }
        assert!(seed(&f, b"user.two", b"w"));

        remove_xattr(On::Path(&f, Link::NoFollow), b"user.one").unwrap();
        assert_eq!(
            list_xattrs(On::Path(&f, Link::NoFollow)).unwrap(),
            vec![b"user.two".to_vec()],
            "removal took a name it was not given"
        );

        // Twice in a row is the same as once: this is the state the caller wants.
        remove_xattr(On::Path(&f, Link::NoFollow), b"user.one").unwrap();

        let file = std::fs::OpenOptions::new().write(true).open(&f).unwrap();
        remove_xattr(On::File(&file), b"user.two").unwrap();
        assert!(list_xattrs(On::File(&file)).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A POSIX access ACL in the on-disk form Linux and this kernel both store
    /// (`kernel/src/fs/acl.rs`): a four-byte version 2, then five-byte-aligned
    /// eight-byte entries of `(tag: u16, perm: u16, id: u32)`, all
    /// little-endian, in ascending tag order.
    ///
    /// The entry that matters is the `ACL_USER` one in the middle: a grant to a
    /// named user, which is what a chmod cannot take away and what every test
    /// below is about. Built by hand because the development host has no
    /// `setfacl` and this crate has no ACL encoder — it needs none, since the
    /// only thing it ever does to an ACL is delete it.
    #[cfg(unix)]
    fn acl_granting_a_named_user(uid: u32) -> Vec<u8> {
        const VERSION: u32 = 2;
        const USER_OBJ: u16 = 0x01;
        const USER: u16 = 0x02;
        const GROUP_OBJ: u16 = 0x04;
        const MASK: u16 = 0x10;
        const OTHER: u16 = 0x20;
        const UNDEFINED: u32 = u32::MAX;

        let mut acl = VERSION.to_le_bytes().to_vec();
        for (tag, perm, id) in [
            (USER_OBJ, 6u16, UNDEFINED),
            (USER, 6, uid),
            (GROUP_OBJ, 0, UNDEFINED),
            (MASK, 6, UNDEFINED),
            (OTHER, 0, UNDEFINED),
        ] {
            acl.extend_from_slice(&tag.to_le_bytes());
            acl.extend_from_slice(&perm.to_le_bytes());
            acl.extend_from_slice(&id.to_le_bytes());
        }
        acl
    }

    /// Put an access ACL on a file, and say whether the filesystem took it.
    ///
    /// A host kernel built without POSIX ACLs, or a filesystem mounted without
    /// them, refuses — and that is a fact about the test environment, not about
    /// the code, so it reports rather than fails. The target OS has them.
    #[cfg(unix)]
    fn seed_acl(path: &std::path::Path) -> bool {
        use super::{Link, On, get_xattr, set_xattr};
        let acl = acl_granting_a_named_user(1234);
        if set_xattr(
            On::Path(path, Link::NoFollow),
            b"system.posix_acl_access",
            &acl,
        )
        .is_err()
        {
            return false;
        }
        // The kernel may normalise what it stored; all these tests need is that
        // the name is now present, so that its absence afterwards means something.
        get_xattr(On::Path(path, Link::NoFollow), b"system.posix_acl_access").is_ok()
    }

    /// The point of [`super::set_mode_exactly`]: a mode of 0600 has to mean
    /// 0600, and a chmod alone does not deliver that. The kernel rewrites an
    /// access ACL's owner, mask and other entries to match a new mode and keeps
    /// every named entry, so a `chmod 0600` over `user:1234:rw` leaves 1234
    /// still able to write — which is the window `cp -p` opens just before it
    /// hands the file to a new owner.
    #[test]
    #[cfg(unix)]
    fn an_exact_mode_takes_the_access_list_off_where_a_chmod_would_not() {
        use super::{Link, On, list_xattrs, set_mode, set_mode_exactly};

        let dir = scratch("exactmode");
        let (chmodded, exact) = (dir.join("chmodded"), dir.join("exact"));
        std::fs::write(&chmodded, b"body").unwrap();
        std::fs::write(&exact, b"body").unwrap();

        if !seed_acl(&chmodded) {
            let _ = std::fs::remove_dir_all(&dir);
            return; // No POSIX ACLs here; the target OS has them.
        }
        assert!(seed_acl(&exact));

        // The control: a plain chmod leaves the list — and so the grant — on.
        set_mode(On::Path(&chmodded, Link::Follow), 0o600).unwrap();
        assert!(
            list_xattrs(On::Path(&chmodded, Link::NoFollow))
                .unwrap()
                .contains(&b"system.posix_acl_access".to_vec()),
            "the host kernel dropped the ACL on chmod, so this test proves nothing"
        );

        set_mode_exactly(On::Path(&exact, Link::Follow), 0o600).unwrap();
        assert!(
            !list_xattrs(On::Path(&exact, Link::NoFollow))
                .unwrap()
                .contains(&b"system.posix_acl_access".to_vec()),
            "an exact mode left an access list that can grant more than it says"
        );
        assert_eq!(mode_of(&exact) & 0o7777, 0o600);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An exact mode on a file that never had a list is the ordinary case — the
    /// one `cp -p` runs on almost every file — and must succeed silently rather
    /// than report the attribute it did not find.
    #[test]
    #[cfg(unix)]
    fn an_exact_mode_on_a_file_with_no_list_is_a_success() {
        use super::{Link, On, set_mode_exactly};

        let dir = scratch("exactplain");
        let f = dir.join("f");
        std::fs::write(&f, b"body").unwrap();

        set_mode_exactly(On::Path(&f, Link::Follow), 0o640).unwrap();
        assert_eq!(mode_of(&f) & 0o7777, 0o640);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A copy of permissions is a *replacement*: the destination ends with the
    /// source's access list and no other, so a grant the destination carried and
    /// the source did not comes off. Copying only what the source has — which is
    /// what gnulib's libattr branch does — would leave it standing on a file the
    /// user just asked to be given the source's permissions.
    #[test]
    #[cfg(unix)]
    fn copied_permissions_replace_the_destination_list_rather_than_merge() {
        use super::{Link, On, copy_permissions, list_xattrs};

        let dir = scratch("copyperm");
        let (src, dst) = (dir.join("src"), dir.join("dst"));
        std::fs::write(&src, b"body").unwrap();
        std::fs::write(&dst, b"body").unwrap();

        if !seed_acl(&dst) {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        // The source has an ordinary attribute and no list. The ordinary one is
        // here to prove the permission copy does not carry it.
        assert!(seed(&src, b"user.one", b"v"));

        copy_permissions(
            On::Path(&src, Link::NoFollow),
            On::Path(&dst, Link::Follow),
            0o644,
        )
        .unwrap();

        assert!(
            list_xattrs(On::Path(&dst, Link::NoFollow))
                .unwrap()
                .is_empty(),
            "a permission copy from a source with no list left one behind"
        );
        assert_eq!(mode_of(&dst) & 0o7777, 0o644);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The other direction: a list the *source* has is carried, and the mode
    /// goes on first so that writing the list is the last word on the bits.
    #[test]
    #[cfg(unix)]
    fn copied_permissions_carry_the_source_list() {
        use super::{Link, On, copy_permissions, list_xattrs};

        let dir = scratch("copyperm2");
        let (src, dst) = (dir.join("src"), dir.join("dst"));
        std::fs::write(&src, b"body").unwrap();
        std::fs::write(&dst, b"body").unwrap();

        if !seed_acl(&src) {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }

        copy_permissions(
            On::Path(&src, Link::NoFollow),
            On::Path(&dst, Link::Follow),
            0o644,
        )
        .unwrap();

        assert_eq!(
            list_xattrs(On::Path(&dst, Link::NoFollow)).unwrap(),
            vec![b"system.posix_acl_access".to_vec()],
            "a permission copy dropped the source's access list"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The mode of a file, for the tests above.
    #[cfg(unix)]
    fn mode_of(path: &std::path::Path) -> u32 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).unwrap().mode()
    }

    /// A name with a NUL is refused before it reaches the kernel, exactly as in
    /// [`super::set_xattr`] — truncating it would delete a *different*
    /// attribute, which is the one error here that destroys data.
    #[test]
    #[cfg(unix)]
    fn removal_refuses_a_name_with_a_nul() {
        use super::{Link, On, remove_xattr};

        let dir = scratch("removenul");
        let f = dir.join("f");
        std::fs::write(&f, b"body").unwrap();

        assert_eq!(
            remove_xattr(On::Path(&f, Link::NoFollow), b"user.a\0b")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
