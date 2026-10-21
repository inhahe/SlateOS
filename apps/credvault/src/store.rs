//! Where the vault is kept, and how its file is read and written.
//!
//! The password manager and the credential service read the same file, so
//! where it is and how it is read live here, once: a reader that hard-coded
//! the path or read it its own way would drift from the one that writes it.
//! Only the password manager writes it, through [`write_owner_only`] -- a new
//! file renamed over the old (`safeio::write_atomically`) -- so a reader in
//! another process sees the last save or the one before, never half of one.

use std::io;
use std::path::{Path, PathBuf};

/// The largest file read as a vault. A vault of ten thousand entries is a
/// few MiB; this is room for far more, and a stop for a file that is plainly
/// something else.
pub const MAX_VAULT_BYTES: usize = 256 << 20;

/// Where the vault is kept: `<config>/credmanager/vault`, beside every other
/// program's settings and data (`settingsfile::config_dir`) -- or `None` on
/// a system with no such directory.
#[must_use]
pub fn vault_path() -> Option<PathBuf> {
    settingsfile::config_dir().map(|dir| dir.join("credmanager").join("vault"))
}

/// Why a vault file could not be read: each says it as a clause about the
/// file ("it is ..."), to follow the file's name.
#[derive(Debug)]
pub enum ReadError {
    /// Larger than any vault ([`MAX_VAULT_BYTES`]).
    TooLarge,
    /// The file system's own refusal.
    Io(io::Error),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("it is too large to be a vault"),
            Self::Io(e) => write!(f, "it could not be read: {e}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// The bytes of the vault file at `path`, read whole: `None` when there is
/// no file there -- no vault has been made -- for [`crate::Vault::from_file`].
///
/// # Errors
///
/// The file is larger than any vault, or cannot be read.
pub fn read_vault_file(path: &Path) -> Result<Option<Vec<u8>>, ReadError> {
    read_up_to(path, MAX_VAULT_BYTES)
}

/// [`read_vault_file`] with the cap given, so a test need not write 256 MiB.
fn read_up_to(path: &Path, max: usize) -> Result<Option<Vec<u8>>, ReadError> {
    match safeio::read_capped(path, max) {
        Ok(read) if read.truncated => Err(ReadError::TooLarge),
        Ok(read) => Ok(Some(read.bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ReadError::Io(e)),
    }
}

/// Write `bytes` to `path` whole or not at all, readable by its owner alone:
/// how the vault, its backups and an export of it are written.
///
/// The folder is made owner-only as it is made, so the file is never readable
/// by anyone else even for the moment before its own mode is set. A vault is
/// encrypted either way; this keeps other users from copying it to guess at
/// offline, and keeps an export -- every password in the clear -- from them
/// altogether.
///
/// # Errors
///
/// The folder or the file cannot be made, written or renamed into place.
pub fn write_owner_only(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(dir)?;
    }
    safeio::write_atomically(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// **No file is no vault, a file is read whole, and one larger than any
    /// vault is refused**, not read in part.
    #[test]
    fn a_vault_file_is_read_whole_or_said_to_be_missing_or_too_large() {
        let scratch = scratchdir::ScratchDir::new("credvault_read");
        let path = scratch.path("vault");
        assert!(
            matches!(read_up_to(&path, 8), Ok(None)),
            "a missing file was read"
        );
        std::fs::write(&path, b"eight by").unwrap();
        assert_eq!(
            read_up_to(&path, 8).unwrap().as_deref(),
            Some(&b"eight by"[..])
        );
        std::fs::write(&path, b"nine byte").unwrap();
        assert!(matches!(read_up_to(&path, 8), Err(ReadError::TooLarge)));
    }

    /// **A write lands whole, over what was there, and makes the folders it
    /// needs** -- owner-only where the system has modes.
    #[test]
    fn a_write_lands_whole_and_owner_only() {
        let scratch = scratchdir::ScratchDir::new("credvault_write");
        let path = scratch
            .dir()
            .join("config")
            .join("credmanager")
            .join("vault");
        write_owner_only(&path, b"first").unwrap();
        write_owner_only(&path, b"second, longer").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second, longer");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "the vault is readable by others");
        }
    }

    /// The vault is kept where the password manager has always kept it.
    #[test]
    fn the_vault_is_kept_in_the_password_managers_folder() {
        if let Some(path) = vault_path() {
            assert!(
                path.ends_with(Path::new("credmanager").join("vault")),
                "{path:?}"
            );
        }
    }
}
