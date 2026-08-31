//! Writing a file's metadata *by path*: its timestamps, and later its mode and
//! its owner.
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

/// Write a file's timestamps, naming it by path.
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
#[cfg(unix)]
pub fn set_times(path: &Path, times: Times) -> io::Result<()> {
    unsafe extern "C" {
        fn utimensat(dirfd: i32, path: *const u8, times: *const CTimespec, flags: i32) -> i32;
    }

    let cpath = c_path(path)?;
    let spec = to_timespecs(times);

    // SAFETY: `cpath` is NUL-terminated and lives until the end of this
    // statement; `spec` is exactly the two-element array `utimensat` reads;
    // `AT_FDCWD` and a zero flag word are both valid. The call does not retain
    // either pointer.
    let rc = unsafe { utimensat(AT_FDCWD, cpath.as_ptr(), spec.as_ptr(), 0) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Write a file's timestamps, naming it by path.
///
/// There is no path-based equivalent on Windows — `SetFileTime` takes a
/// handle — so this arm opens one asking for the least access that permits a
/// stamp.
///
/// # Errors
///
/// Whatever the open or the stamp said.
#[cfg(not(unix))]
pub fn set_times(path: &Path, times: Times) -> io::Result<()> {
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

/// `AT_FDCWD` — resolve a relative path against the working directory.
/// Matches `posix/src/file.rs`.
#[cfg(unix)]
const AT_FDCWD: i32 = -100;

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
    use super::{CTimespec, Times, UTIME_OMIT, When, to_timespec, to_timespecs};
    use std::time::{Duration, SystemTime};

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
