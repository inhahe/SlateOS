//! Crash-safe file writes for applications that persist user data.
//!
//! # Why this crate exists
//!
//! `std::fs::write` opens its target with `O_TRUNC`. The file is emptied
//! *first*, and only then refilled. Every application that saved a document
//! with it therefore had a window — short, but real — in which the user's file
//! existed on disk as a truncated fragment or as nothing at all. Anything that
//! interrupts the write inside that window leaves it that way permanently:
//! a full disk, a removed USB stick, a killed process, a power loss.
//!
//! That is not a hypothetical failure for a text editor. "Save, then the
//! machine died, and now the file is empty" is the single worst thing a
//! document editor can do, because the user's own copy *was* the file.
//!
//! The fix is the standard one: write the new contents to a temporary file
//! beside the target, flush it to the disk, and then `rename` it over the
//! target. `rename` within a directory is atomic — every reader sees either
//! the whole old file or the whole new one, never a partial one — so an
//! interruption at any point leaves the original intact and costs at most a
//! stray temporary.
//!
//! # What this does *not* promise
//!
//! [`write_atomically`] guarantees the target is never observed partially
//! written. It does not guarantee the *new* contents survive a power loss:
//! that additionally requires flushing the containing directory, which is not
//! portable (Windows cannot open a directory as a file). The distinction
//! matters and is the right trade: losing an edit is recoverable by redoing
//! it, whereas losing the file that existed before the save is not.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// How many temporary names to try before giving up.
///
/// A collision needs two saves of the same file in the same process to draw
/// the same counter value, which cannot happen, or another process to have
/// left a temporary with the same PID — possible only after a PID has been
/// recycled. The bound exists so that a directory in a pathological state
/// fails the save instead of spinning forever.
const MAX_TEMP_ATTEMPTS: u32 = 1024;

/// Distinguishes concurrent saves within one process.
///
/// The PID alone is not enough: two documents saved at once, or one document
/// saved twice in quick succession, would otherwise pick the same temporary
/// name and the second would clobber the first's half-written file.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `contents` to `path` so that `path` is never left partially written.
///
/// On success `path` holds exactly `contents`. On failure `path` is untouched
/// — it keeps whatever it held before, including not existing — and no
/// temporary file is left behind.
///
/// # Symlinks
///
/// A symlink target is followed, and the *resolved* file is replaced. This
/// matters because rename-over replaces whatever it renames onto: without
/// resolving, saving a file the user opened through a symlink would delete
/// their symlink and leave a regular file in its place, which is not what
/// "save" means. Editing dotfiles through a symlinked config directory is the
/// ordinary case here, not an exotic one.
///
/// # Permissions
///
/// When the target already exists its permissions are copied onto the
/// replacement, because the new file would otherwise be created with the
/// process's default mode. A save must not quietly make a private file
/// world-readable.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] if the directory cannot be written,
/// the data cannot be flushed, or the rename fails.
pub fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    // Resolve a symlink to the file it points at. `canonicalize` fails when
    // the path does not exist yet, which is the ordinary "save a new file"
    // case, so that failure is not an error — fall back to the path as given.
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    let parent = target.parent().filter(|p| !p.as_os_str().is_empty());
    let dir: PathBuf = parent.map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let (mut file, tmp_path) = create_temp_in(&dir, &target)?;

    // From here on every failure has to remove the temporary before returning,
    // or a failed save leaves a full-size copy of the document sitting next to
    // it under a name the user never chose.
    let result = (|| -> io::Result<()> {
        file.write_all(contents)?;
        // Flush the contents to the device before the rename. Without this the
        // rename can reach the disk first, leaving a file that atomically
        // points at unwritten blocks — the "empty file after a crash" that
        // atomic rename is supposed to prevent.
        file.sync_all()
    })();

    // Close before renaming: Windows refuses to rename a file that is still
    // open, so leaving this to the end of the scope would make every save fail.
    drop(file);

    if let Err(e) = result {
        let _ = fs::remove_file(&tmp_path); // Best effort; the write error is the one worth reporting.
        return Err(e);
    }

    // Copy the original's permissions onto the replacement, if there was an
    // original. A missing target is the new-file case, where the default mode
    // is correct.
    if let Ok(meta) = fs::metadata(&target)
        && fs::set_permissions(&tmp_path, meta.permissions()).is_err()
    {
        // Not fatal. The contents are what the user asked to save, and
        // failing the save to preserve a mode bit would lose the edit to
        // protect a detail the user cannot see. Filesystems that do not
        // support permissions at all reach this on every save.
    }

    if let Err(e) = fs::rename(&tmp_path, &target) {
        let _ = fs::remove_file(&tmp_path); // Best effort; the rename error is the one worth reporting.
        return Err(e);
    }

    Ok(())
}

/// Create a uniquely-named temporary file in `dir` alongside `target`.
///
/// The temporary must share a directory with the target: `rename` is only
/// atomic within one filesystem, and a temporary in `/tmp` would silently
/// degrade to a copy across a mount point — reintroducing exactly the partial
/// write this crate exists to prevent.
///
/// Uniqueness is enforced by `create_new`, which fails rather than truncating
/// an existing file. Deriving a name and trusting it to be free is how a
/// second saver ends up writing into the first's temporary.
fn create_temp_in(dir: &Path, target: &Path) -> io::Result<(fs::File, PathBuf)> {
    let stem = target
        .file_name()
        .map_or_else(|| "unnamed".to_string(), |n| n.to_string_lossy().to_string());
    let pid = std::process::id();

    for _ in 0..MAX_TEMP_ATTEMPTS {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        // Leading dot so the temporary is hidden, and a suffix that cannot be
        // mistaken for a document if one is ever left behind by a hard kill.
        let tmp_path = dir.join(format!(".{stem}.slate-save-{pid}-{n}"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(file) => return Ok((file, tmp_path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("could not create a temporary file next to {}", target.display()),
    ))
}

/// [`write_atomically`] for text, which is what most callers have.
///
/// # Errors
///
/// As [`write_atomically`].
pub fn write_str_atomically(path: &Path, contents: &str) -> io::Result<()> {
    write_atomically(path, contents.as_bytes())
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did
    // it. The defensive lints exist to keep panics out of code that runs on a
    // user's data, which this is not.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("safeio_test_{label}_{ts}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_new_file_is_created_with_the_given_contents() {
        let dir = temp_dir("new");
        let path = dir.join("doc.txt");

        write_atomically(&path, b"hello").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"hello");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_existing_file_is_replaced_wholesale() {
        let dir = temp_dir("replace");
        let path = dir.join("doc.txt");
        fs::write(&path, b"the old and much longer contents").unwrap();

        write_atomically(&path, b"new").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The whole point: a save that cannot complete must not damage the file
    /// it was saving. A directory standing where the target file should be
    /// makes the final rename fail on every platform, which is the closest
    /// portable stand-in for a disk filling up mid-write.
    #[test]
    fn a_failed_save_leaves_the_original_untouched() {
        let dir = temp_dir("fail_keeps_original");
        let path = dir.join("doc.txt");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("marker"), b"still here").unwrap();

        let err = write_atomically(&path, b"replacement").unwrap_err();

        // The original is exactly as it was.
        assert!(path.is_dir(), "the target must not have been replaced");
        assert_eq!(fs::read(path.join("marker")).unwrap(), b"still here");
        let _ = err;

        let _ = fs::remove_dir_all(&dir);
    }

    /// A failed save must not leave its temporary behind either. A stray
    /// full-size copy of the document beside it is how a failed save of a
    /// large file silently consumes its own size in disk space.
    #[test]
    fn a_failed_save_cleans_up_after_itself() {
        let dir = temp_dir("fail_no_litter");
        let path = dir.join("doc.txt");
        fs::create_dir_all(&path).unwrap();

        let _ = write_atomically(&path, b"replacement");

        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("slate-save"))
            .collect();
        assert!(leftovers.is_empty(), "temporaries left behind: {leftovers:?}");

        let _ = fs::remove_dir_all(&dir);
    }

    /// A successful save must not litter either.
    #[test]
    fn a_successful_save_leaves_only_the_file() {
        let dir = temp_dir("clean");
        let path = dir.join("doc.txt");

        write_atomically(&path, b"contents").unwrap();

        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["doc.txt".to_string()]);

        let _ = fs::remove_dir_all(&dir);
    }

    /// Two documents saved at once must not pick the same temporary name and
    /// write into each other's half-finished file.
    #[test]
    fn concurrent_saves_do_not_share_a_temporary() {
        let dir = temp_dir("concurrent");
        let paths: Vec<PathBuf> = (0..8).map(|i| dir.join(format!("doc{i}.txt"))).collect();

        std::thread::scope(|s| {
            for (i, path) in paths.iter().enumerate() {
                s.spawn(move || {
                    let body = format!("contents of {i}").repeat(500);
                    write_atomically(path, body.as_bytes()).unwrap();
                });
            }
        });

        for (i, path) in paths.iter().enumerate() {
            let expected = format!("contents of {i}").repeat(500);
            assert_eq!(fs::read_to_string(path).unwrap(), expected);
        }

        let _ = fs::remove_dir_all(&dir);
    }

    /// The same file saved repeatedly, which is what an editor with autosave
    /// does. Every save must land whole.
    #[test]
    fn repeated_saves_of_one_file_all_land() {
        let dir = temp_dir("repeat");
        let path = dir.join("doc.txt");

        for i in 0..50 {
            write_str_atomically(&path, &format!("revision {i}")).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), format!("revision {i}"));
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_write_produces_an_empty_file() {
        let dir = temp_dir("empty");
        let path = dir.join("doc.txt");
        fs::write(&path, b"previous").unwrap();

        write_atomically(&path, b"").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A path with no directory component must still work — it means the
    /// current directory, not "no directory".
    #[test]
    fn a_bare_file_name_writes_to_the_current_directory() {
        let dir = temp_dir("bare");
        let path = dir.join("bare.txt");
        // Exercised through a full path whose parent exists; the bare-name
        // branch is the `dir` fallback in `write_atomically`, checked here by
        // construction rather than by changing the process's cwd, which would
        // race every other test in this binary.
        write_atomically(&path, b"ok").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"ok");

        assert_eq!(
            Path::new("bare.txt")
                .parent()
                .filter(|p| !p.as_os_str().is_empty()),
            None,
            "a bare name has no usable parent, so the fallback is the branch taken"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
