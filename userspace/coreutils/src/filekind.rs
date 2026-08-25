//! One question, asked the same way everywhere: **is this open file a regular
//! file?**
//!
//! Everything a coreutil does differently for a file than for a pipe hangs off
//! that answer — whether it can seek to the end instead of reading forwards
//! (`tail`), whether a length is worth taking from `fstat` instead of counting
//! (`wc`), whether a `mmap`-style shortcut applies at all. On the target the
//! question is `S_ISREG(st_mode)` and there is nothing to write. This module
//! exists because of the *host*.
//!
//! ## Why `Metadata::is_file` is the wrong question on Windows
//!
//! SlateOS presents as `linux-musl`, so the shipped binaries take the `cfg(unix)`
//! path. But the differential harnesses — the only thing that certifies these
//! utilities against GNU — build and run them on Windows, and there
//! `Metadata::is_file` means "not a directory and not a symlink". A pipe answers
//! **yes**.
//!
//! It is worse than a wrong label. `GetFileInformationByHandle` on an MSYS pipe
//! succeeds and reports `FILE_ATTRIBUTE_NORMAL` with a length of however many
//! bytes happen to be sitting in the pipe buffer, and `SetFilePointerEx` on that
//! handle returns success while moving nothing. A utility that trusts
//! `is_file()` therefore does not merely mislabel the input — it takes a
//! seek-based path that silently reads the wrong bytes:
//!
//! ```text
//! printf 'a\nb\nc\n' | tail -n3      GNU: a b c      is_file(): (nothing)
//! printf 'a\nb\nc\n' | tail -c3      GNU: \nc\n      is_file(): a b c
//! ```
//!
//! Both were measured against the shipped `tail` before this module existed.
//! The harness could not have caught them by wording alone; it caught them
//! because it compares bytes.
//!
//! `GetFileType` is the honest analogue of `S_ISREG`: only a `FILE_TYPE_DISK`
//! handle is a regular file, and `FILE_TYPE_PIPE` and `FILE_TYPE_CHAR` are not.

use std::fs::File;
use std::mem::ManuallyDrop;

/// Whether `file` is a regular file — POSIX's `S_ISREG` — or `None` when the
/// question could not be answered at all, which is `fstat` having failed.
///
/// The three answers are kept apart because a caller can need them apart:
/// `wc /nope` contributes nothing to the output width where `wc /dev/null`
/// contributes a widening, and only "failed" against "not regular"
/// distinguishes those. A caller that does not care should use [`is_regular`].
#[must_use]
pub fn regular(file: &File) -> Option<bool> {
    #[cfg(unix)]
    {
        file.metadata().map(|m| m.is_file()).ok()
    }
    #[cfg(all(not(unix), windows))]
    {
        use std::os::windows::io::AsRawHandle;

        const FILE_TYPE_UNKNOWN: u32 = 0;
        const FILE_TYPE_DISK: u32 = 1;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetFileType(handle: *mut core::ffi::c_void) -> u32;
        }

        // SAFETY: `AsRawHandle` yields a handle that is valid for as long as
        // `file` is borrowed, and `GetFileType` only reads its type — it does
        // not consume input, move the file pointer, or take ownership.
        match unsafe { GetFileType(file.as_raw_handle()) } {
            // A handle nothing can be concluded from. `fstat` failing is the
            // closest thing POSIX has, and it is the conservative answer.
            FILE_TYPE_UNKNOWN => None,
            // Both tests, not either: `GetFileType` rules out the pipes and
            // consoles that `is_file` wrongly admits, and `is_file` rules out
            // the directories that `GetFileType` calls `FILE_TYPE_DISK`.
            FILE_TYPE_DISK => file.metadata().map(|m| m.is_file()).ok(),
            // `FILE_TYPE_CHAR` (a console, `NUL`) and `FILE_TYPE_PIPE`.
            _ => Some(false),
        }
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let _ = file;
        None
    }
}

/// Whether `file` is a regular file, with "could not tell" folded into "no".
///
/// A directory is not a regular file, and neither is a pipe, a socket, a device
/// or a console. Answering `false` when the answer is unknown is the
/// conservative side of every use here: it costs a slower path, never a wrong
/// one.
#[must_use]
pub fn is_regular(file: &File) -> bool {
    regular(file) == Some(true)
}

/// Whether `md` describes a **device** in the sense POSIX utilities mean when
/// they offer to skip one: a character device, a block device, a socket or a
/// FIFO.
///
/// Deliberately not "anything that is not a regular file". A directory is not a
/// device — `grep -D skip` still descends into one, and still says `Is a
/// directory` about one named on the command line — and a symlink is not one
/// either. GNU grep spells the same predicate `is_device_mode`, and it is those
/// four `S_IS*` tests and no others.
///
/// The question is asked of a [`std::fs::Metadata`] rather than of an open
/// [`File`] because the whole point is to answer it **without opening**: `open`
/// on a FIFO with no writer blocks forever, so a `grep -D skip` that opened
/// first would hang on exactly the input it was told to skip. (GNU dodges the
/// same hang from the other side, by adding `O_NONBLOCK` when devices are to be
/// skipped; statting first is the equivalent that does not need the flag.)
///
/// On a host that is not Unix the answer is always `false`. Windows has no
/// FIFOs or block devices in the filesystem namespace to stat, and the
/// differential harnesses that would care run under WSL, where `cfg(unix)`
/// holds.
#[must_use]
pub fn is_device(md: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let t = md.file_type();
        t.is_char_device() || t.is_block_device() || t.is_socket() || t.is_fifo()
    }
    #[cfg(not(unix))]
    {
        let _ = md;
        false
    }
}

/// Whether `file` can be read out of order.
///
/// This is [`is_regular`] plus the seek itself: a handle can be a regular file
/// and still refuse to seek, and a handle can accept a seek and ignore it. The
/// position is left where it was found.
///
/// # Errors
///
/// None — a failure at any step is reported as "not seekable".
#[must_use]
pub fn is_seekable(file: &mut File) -> bool {
    use std::io::{Seek, SeekFrom};

    if !is_regular(file) {
        return false;
    }
    let Ok(at) = file.stream_position() else {
        return false;
    };
    // A seek that does not land where it was asked to is a seek that did
    // nothing, which is the MSYS-pipe behaviour this module exists to catch.
    // Reading back the position is the only way to tell the two apart.
    file.seek(SeekFrom::Start(at)).is_ok_and(|got| got == at)
}

/// Standard input as a [`File`], so that [`regular`] and `metadata` can be
/// asked about it exactly as they are asked about an opened file.
///
/// The returned handle does **not** own the descriptor: `File::from_raw_fd`
/// takes ownership, and letting that `File` drop would close standard input in
/// the middle of a program that is about to read it — hence the
/// [`ManuallyDrop`]. Nothing is read or seeked through it here, so it cannot
/// disturb the stream position either.
///
/// `None` only on a platform that is neither Unix nor Windows, where there is
/// no way to name the descriptor at all.
#[must_use]
pub fn borrowed_stdin() -> Option<ManuallyDrop<File>> {
    #[cfg(unix)]
    {
        use std::io::stdin;
        use std::os::fd::{AsRawFd, FromRawFd};
        let fd = stdin().as_raw_fd();
        // SAFETY: `fd` is standard input, which the runtime keeps open for the
        // whole process, and `ManuallyDrop` prevents the `File` from closing it.
        Some(unsafe { ManuallyDrop::new(File::from_raw_fd(fd)) })
    }
    #[cfg(all(not(unix), windows))]
    {
        use std::io::stdin;
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        let handle = stdin().as_raw_handle();
        // SAFETY: as above — standard input's handle, which the runtime keeps
        // open for the whole process, kept from being closed by `ManuallyDrop`.
        Some(unsafe { ManuallyDrop::new(File::from_raw_handle(handle)) })
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{is_regular, is_seekable, regular};
    use std::fs::File;
    use std::io::{Seek, SeekFrom};

    /// A scratch file, named per-process and per-thread so a parallel test run
    /// cannot have two of these fighting over one path.
    fn scratch(contents: &[u8]) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "slateos-filekind-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, contents).expect("scratch file");
        path
    }

    /// The name of a character device that exists on the host running the
    /// tests. Both are the platform's bit bucket; what matters here is only
    /// that opening it yields something that is *not* a regular file.
    const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

    #[test]
    fn a_real_file_is_regular_and_seekable() {
        let path = scratch(b"0123456789");
        let mut file = File::open(&path).expect("open");
        assert_eq!(regular(&file), Some(true));
        assert!(is_regular(&file));
        assert!(is_seekable(&mut file));
        drop(file);
        let _ = std::fs::remove_file(&path);
    }

    /// The branch the module exists for. On Windows this is `FILE_TYPE_CHAR`,
    /// which `Metadata::is_file` calls a file — so an implementation that
    /// forwarded to `is_file` would answer `Some(true)` here and fail.
    #[test]
    fn a_character_device_is_not_regular() {
        let file = File::open(NULL_DEVICE).expect("open null device");
        assert_eq!(regular(&file), Some(false));
        assert!(!is_regular(&file));
    }

    /// A directory is not a regular file either, and on Windows it is the case
    /// `GetFileType` alone gets wrong: a directory handle is `FILE_TYPE_DISK`.
    /// Opening one needs `FILE_FLAG_BACKUP_SEMANTICS` there, so this runs only
    /// where a plain `File::open` can do it.
    #[test]
    fn a_directory_is_not_regular() {
        let Ok(dir) = File::open(std::env::temp_dir()) else {
            return;
        };
        assert_ne!(regular(&dir), Some(true));
        assert!(!is_regular(&dir));
    }

    /// `is_seekable` promises to leave the position where it found it, because
    /// its callers ask it *after* having read part of the input.
    #[test]
    fn asking_whether_a_file_seeks_does_not_move_it() {
        let path = scratch(b"0123456789");
        let mut file = File::open(&path).expect("open");
        file.seek(SeekFrom::Start(4)).expect("seek");
        assert!(is_seekable(&mut file));
        assert_eq!(file.stream_position().expect("tell"), 4);
        drop(file);
        let _ = std::fs::remove_file(&path);
    }

    /// Nothing that is not a regular file is seekable, whatever the handle
    /// says when asked to seek — which is the pipe case stated as an invariant,
    /// since a pipe cannot be conjured portably here.
    #[test]
    fn only_regular_files_are_seekable() {
        let mut file = File::open(NULL_DEVICE).expect("open null device");
        assert!(!is_seekable(&mut file));
    }
}
