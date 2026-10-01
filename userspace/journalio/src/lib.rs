//! Appending to the journal file, and rewriting or rotating it, without
//! losing a record.
//!
//! Three programs append records to `/var/log/syslog.jsonl` -- `syslogd`,
//! `logger` and `systemd-cat` -- each opening the file, writing one record
//! and closing it. `syslogd` renames the file aside when it grows (rotation),
//! and `journalctl --vacuum-time`/`--vacuum-size` rewrite it. Done without
//! co-ordination, a rewrite loses every record appended between its read and
//! its write, and a rotation strands, in the rotated file, the record of a
//! writer that opened the file just before the rename.
//!
//! # The protocol (design-decisions §1037)
//!
//! * **A writer** ([`append`]) opens the file for appending and takes an
//!   exclusive `flock`. Holding it, it checks that the path still names the
//!   file it opened -- a rewrite or a rotation may have replaced it while it
//!   waited -- and if not, opens the path again. Then it writes its record
//!   and closes the file, which releases the lock.
//! * **A rewriter** ([`Locked::open`], [`Locked::replace`]) takes the same
//!   lock, checked the same way, reads, and writes the new contents to a
//!   temporary file beside the log, renamed over it while the lock is still
//!   held. A writer that was waiting wakes to find that the path names a new
//!   file, and appends to that one. The rename also means a rewrite that
//!   dies half way leaves the old file whole.
//! * **A rotation** ([`Locked::rename_to`]) takes the lock and renames the
//!   file away; the next writer to arrive creates a new one.
//!
//! The lock is advisory: only programs that follow the protocol are held to
//! it, and all three writers do. `flock` locks belong to an open file, not a
//! process, so two threads of one program exclude each other as two programs
//! do.
//!
//! Where `flock` itself is not available (`ENOSYS`, `EOPNOTSUPP`, `ENOLCK`),
//! a writer appends without it, as before -- a record that is written beats
//! one refused -- while a rewrite refuses, since rewriting unlocked is the
//! loss this exists to prevent.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// How many times a writer, or a rewriter, opens a file again after finding
/// it replaced while it waited for the lock, before using the one it holds.
/// A replacement is a rewrite or a rotation, each rare; running out means
/// something replaces the file continuously.
const RETRIES: usize = 64;

/// Append `line` -- one record, its newline included -- to the log at
/// `path`, creating the file if there is none, under the lock.
///
/// # Errors
///
/// The file cannot be opened or written, or locking it fails for a reason
/// other than there being no locks (`EINTR` is retried).
pub fn append(path: &Path, line: &[u8]) -> io::Result<()> {
    let mut attempt = 0usize;
    loop {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        match sys::lock(&file) {
            Ok(()) => {}
            // No locks on this file system: append as before.
            Err(e) if sys::no_locks(&e) => return file.write_all(line),
            Err(e) => return Err(e),
        }
        attempt = attempt.saturating_add(1);
        if attempt >= RETRIES || sys::still_names(path, &file)? {
            return file.write_all(line);
        }
        // Replaced while this waited: the path names another file now.
    }
}

/// A log file held under the lock, for a rewrite, a rotation or a removal.
/// Dropping it releases the lock.
#[derive(Debug)]
pub struct Locked {
    file: File,
    path: PathBuf,
}

impl Locked {
    /// Lock the log at `path` -- the file the path names once the lock is
    /// held. `Ok(None)` when there is no such file.
    ///
    /// # Errors
    ///
    /// The file cannot be opened or locked -- including on a file system
    /// without locks, where a rewrite could lose records -- or kept being
    /// replaced.
    pub fn open(path: &Path) -> io::Result<Option<Locked>> {
        for _ in 0..RETRIES {
            let file = match File::open(path) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e),
            };
            sys::lock(&file)?;
            if sys::still_names(path, &file)? {
                return Ok(Some(Locked {
                    file,
                    path: path.to_path_buf(),
                }));
            }
            if fs::symlink_metadata(path).is_err() {
                // Rotated away, and nothing new there yet.
                return Ok(None);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            "the file kept being replaced",
        ))
    }

    /// The path this holds.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Everything in the file.
    ///
    /// # Errors
    ///
    /// The read's own.
    pub fn read_all(&mut self) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut out = Vec::new();
        self.file.read_to_end(&mut out)?;
        Ok(out)
    }

    /// Replace the file's contents with `contents`: written to a temporary
    /// file beside it, with its mode and owner, synced, and renamed over it
    /// -- the lock held throughout, and released after.
    ///
    /// # Errors
    ///
    /// The temporary cannot be made, written, synced or renamed; the old
    /// file is then left as it was, and the temporary removed.
    pub fn replace(self, contents: &[u8]) -> io::Result<()> {
        let tmp = temp_path(&self.path);
        let written = (|| {
            let mut out = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            sys::copy_mode_and_owner(&self.file, &out);
            out.write_all(contents)?;
            out.sync_all()?;
            fs::rename(&tmp, &self.path)
        })();
        if written.is_err() {
            // Nothing to keep: the old file is untouched.
            let _ = fs::remove_file(&tmp);
        }
        written
    }

    /// Rename the file to `to` -- a rotation -- the lock held throughout.
    ///
    /// # Errors
    ///
    /// The rename's own.
    pub fn rename_to(self, to: &Path) -> io::Result<()> {
        fs::rename(&self.path, to)
    }

    /// Remove the file -- a rotated file vacuumed empty -- under the lock.
    ///
    /// # Errors
    ///
    /// The removal's own.
    pub fn remove(self) -> io::Result<()> {
        fs::remove_file(&self.path)
    }
}

/// Where [`Locked::replace`] writes: a hidden name beside the log, unique to
/// this process.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}

#[cfg(unix)]
mod sys {
    use std::ffi::c_int;
    use std::fs::{self, File, Permissions};
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;

    /// `LOCK_EX`, on Linux and in the SlateOS C library.
    const LOCK_EX: c_int = 2;
    /// `ENOLCK`, `ENOSYS` and `EOPNOTSUPP`: no locks here.
    const ENOLCK: i32 = 37;
    const ENOSYS: i32 = 38;
    const EOPNOTSUPP: i32 = 95;

    mod ffi {
        use std::ffi::c_int;

        unsafe extern "C" {
            pub fn flock(fd: c_int, operation: c_int) -> c_int;
            pub fn fchown(fd: c_int, owner: u32, group: u32) -> c_int;
        }
    }

    /// `flock(fd, LOCK_EX)`, retried when a signal interrupts the wait.
    pub fn lock(file: &File) -> io::Result<()> {
        loop {
            // SAFETY: `flock` takes a descriptor number and an operation and
            // touches no memory of ours; the descriptor is `file`'s, open for
            // the whole call.
            let rc = unsafe { ffi::flock(file.as_raw_fd(), LOCK_EX) };
            if rc == 0 {
                return Ok(());
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }

    /// Whether a locking failure means this file system has no locks.
    pub fn no_locks(e: &io::Error) -> bool {
        matches!(e.raw_os_error(), Some(ENOLCK | ENOSYS | EOPNOTSUPP))
    }

    /// Whether `path` still names the file `file` has open.
    pub fn still_names(path: &Path, file: &File) -> io::Result<bool> {
        let held = file.metadata()?;
        match fs::metadata(path) {
            Ok(now) => Ok(now.dev() == held.dev() && now.ino() == held.ino()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Give `to` the mode and owner of `from`, as far as this process may:
    /// a replacement log should be no more readable than the one it
    /// replaces, and still belong to whoever owned it.
    pub fn copy_mode_and_owner(from: &File, to: &File) {
        let Ok(meta) = from.metadata() else {
            return;
        };
        // Best effort, both: a failure leaves the replacement with this
        // process's own defaults, which is what a new log file gets anyway.
        let _ = to.set_permissions(Permissions::from_mode(meta.mode() & 0o7777));
        // SAFETY: `fchown` takes a descriptor number and two ids and touches
        // no memory of ours; the descriptor is `to`'s, open for the call.
        let _ = unsafe { ffi::fchown(to.as_raw_fd(), meta.uid(), meta.gid()) };
    }
}

/// The Windows host the unit tests also run on: no `flock`, so no lock --
/// each call is what it was before this crate.
#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the same signatures as the unix module's, whose calls can fail"
)]
mod sys {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    pub fn lock(_file: &File) -> io::Result<()> {
        Ok(())
    }

    pub fn no_locks(_e: &io::Error) -> bool {
        false
    }

    pub fn still_names(_path: &Path, _file: &File) -> io::Result<bool> {
        Ok(true)
    }

    pub fn copy_mode_and_owner(_from: &File, _to: &File) {}
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
