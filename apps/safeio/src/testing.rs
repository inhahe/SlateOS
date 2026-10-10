//! Test fixtures for what a program does when a save cannot be written.
//!
//! On only with the `testing` feature, which a program turns on in its own
//! `[dev-dependencies]` (and in this crate's own tests): nothing here belongs
//! in a real build.
//!
//! A save that writes a temporary file beside the old one and renames it over
//! ([`crate::write_atomically`]) can fail at the write while the old file still
//! reads. A test of what a program says then needs exactly that -- and the
//! usual way to fail a save, a folder where the file goes, fails the read too.
//! A program that reads its file before it saves, to keep what another window
//! of it saved (design-decisions §1239), meets that failure first, so its
//! "the save failed" message went untested behind its "the file could not be
//! read" one.

use std::fs;
use std::io;
use std::path::Path;

/// While this lives, the file it was made for can be read but not replaced.
/// Once it is dropped the file can be replaced again.
#[derive(Debug)]
#[must_use = "the file can be replaced again as soon as this is dropped"]
pub struct NoReplacing {
    /// The file, held open without letting it be deleted.
    #[cfg(windows)]
    _held: fs::File,
    /// The file's folder, made read-only, and the permissions it had.
    #[cfg(unix)]
    dir: std::path::PathBuf,
    #[cfg(unix)]
    mode: u32,
}

/// Make the file at `path` one that can be read and not replaced, for as long
/// as the answer lives.
///
/// On Windows the file is held open without sharing the right to delete it,
/// which a rename onto it needs. On Unix its folder is made read-only, so no
/// temporary file can be made beside it -- which does not stop the superuser,
/// so the answer is `None` when this process is not stopped (found by
/// trying), and a test then has nothing it can test. `None` too on a system
/// that is neither.
///
/// # Errors
///
/// The file cannot be opened, or its folder's permissions read or set.
pub fn refuse_replacing(path: &Path) -> io::Result<Option<NoReplacing>> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Readers may open it alongside; a rename onto it, which needs to
        // delete it, may not.
        const FILE_SHARE_READ: u32 = 0x1;
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)?;
        Ok(Some(NoReplacing { _held: held }))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let mode = fs::metadata(&dir)?.permissions().mode();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555))?;
        // Made first, so that every way out from here puts the mode back.
        let refusal = NoReplacing { dir, mode };
        let probe = refusal.dir.join(".safeio-refuse-replacing-probe");
        if fs::File::create(&probe).is_ok() {
            // The superuser: permissions do not stop it. The probe is removed
            // and the folder's mode put back (by the drop).
            // Ignored: a probe left in a test's scratch folder goes with it.
            let _ = fs::remove_file(&probe);
            return Ok(None);
        }
        Ok(Some(refusal))
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        Ok(None)
    }
}

#[cfg(unix)]
impl Drop for NoReplacing {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        // Ignored: a drop cannot report, and a test's scratch folder left
        // read-only is still removed with the rest of its tree by its owner.
        let _ = fs::set_permissions(&self.dir, fs::Permissions::from_mode(self.mode));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_refused_file_reads_and_is_not_replaced_until_the_refusal_goes() {
        let scratch = scratchdir::ScratchDir::new("safeio_refuse_replacing");
        let path = scratch.dir().join("kept.txt");
        crate::write_str_atomically(&path, "old").unwrap();
        let Some(refusal) = refuse_replacing(&path).unwrap() else {
            // Nothing can refuse this process (a Unix superuser).
            return;
        };
        assert!(
            crate::write_str_atomically(&path, "new").is_err(),
            "the file was replaced"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
        let leftovers: Vec<_> = fs::read_dir(scratch.dir())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "a failed save left {leftovers:?}");
        drop(refusal);
        crate::write_str_atomically(&path, "new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }
}
