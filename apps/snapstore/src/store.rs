//! The blobs: each file's content, once, named by its SHA-256.
//!
//! `<root>/cas/<first two hex digits>/<the other sixty-two>`, the layout
//! `apps/backup` has always written.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// SHA-256 of `data`, as lowercase hex.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    sha2::sha256_hex(data).as_str().to_string()
}

/// Hash a file without holding it in memory.
///
/// # Errors
///
/// The file cannot be opened or read.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        // `get` rather than `&buf[..n]`: `read` returning more than the buffer
        // would be a broken `Read`, and stopping is the only sane reading.
        let Some(chunk) = buf.get(..n) else { break };
        hasher.update(chunk);
    }
    Ok(sha2::hex(&hasher.finalize()).as_str().to_string())
}

/// What storing one file came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    /// The hash of what the store now holds for it -- the blob's name.
    pub hash: String,
    /// Its size.
    pub size: u64,
    /// Whether the blob was new, rather than already in the store.
    pub new: bool,
}

/// The content-addressed store of blobs.
pub struct ContentStore {
    base_path: PathBuf,
}

impl ContentStore {
    /// The blobs of the store at `root`.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            base_path: root.join("cas"),
        }
    }

    /// Where a blob's content lives.
    #[must_use]
    pub fn blob_path(&self, hash: &str) -> PathBuf {
        // Two hex digits of directory keep any one directory small.
        let (prefix, rest) = hash.split_at(2.min(hash.len()));
        self.base_path.join(prefix).join(rest)
    }

    /// Whether the store holds a blob.
    ///
    /// Existence *is* the deduplication test -- nothing re-verifies a blob on
    /// this path -- so it is only sound if a blob can never appear at its name
    /// partly written. Every way into the store therefore builds the blob
    /// under another name and renames it into place once it is whole.
    #[must_use]
    pub fn has_blob(&self, hash: &str) -> bool {
        self.blob_path(hash).exists()
    }

    /// Copy `source` into the store and return the hash of what was stored.
    ///
    /// `expected` is the hash the caller read the file as. When the store
    /// already holds it, nothing is copied: that blob *is* that content, and
    /// what the caller hashed was the file then. When it does not, the file is
    /// copied to a staging name (through `safeio`, so the copy is whole and
    /// flushed before it is renamed), and **the staged copy is hashed** --
    /// the blob is named by the bytes actually stored. A file changed between
    /// the caller's read and the copy therefore goes in under its new hash,
    /// never under the old one: the store's one invariant is that a blob's
    /// name is its content's hash, and a blob that broke it would be handed
    /// back, wrong, to every later snapshot that deduplicated against it.
    ///
    /// # Errors
    ///
    /// The source cannot be read, or the store cannot be written.
    pub fn ingest(&self, source: &Path, expected: &str) -> io::Result<Stored> {
        if !expected.is_empty() && self.has_blob(expected) {
            let size = fs::metadata(self.blob_path(expected))?.len();
            return Ok(Stored {
                hash: expected.to_string(),
                size,
                new: false,
            });
        }
        let staging = self.base_path.join("staging");
        fs::create_dir_all(&staging)?;
        let staged = staging.join(unique_name());
        let size = safeio::copy_atomically(source, &staged)?;
        let stored = self.adopt(&staged, size);
        if stored.is_err() {
            // Best effort: a staged copy left behind is swept by the next
            // collection, and the adopt error is the one worth reporting.
            let _ = fs::remove_file(&staged);
        }
        stored
    }

    /// Name a staged copy by its hash, or drop it if that blob is already
    /// here.
    fn adopt(&self, staged: &Path, size: u64) -> io::Result<Stored> {
        let hash = sha256_file(staged)?;
        let blob = self.blob_path(&hash);
        if blob.exists() {
            fs::remove_file(staged)?;
            return Ok(Stored {
                hash,
                size,
                new: false,
            });
        }
        if let Some(parent) = blob.parent() {
            fs::create_dir_all(parent)?;
        }
        // Same filesystem, so the rename is atomic: the blob appears whole or
        // not at all. The staged copy was flushed by `copy_atomically`.
        fs::rename(staged, &blob)?;
        Ok(Stored {
            hash,
            size,
            new: true,
        })
    }

    /// A blob's content.
    ///
    /// # Errors
    ///
    /// The blob is missing or unreadable.
    pub fn read_blob(&self, hash: &str) -> io::Result<Vec<u8>> {
        fs::read(self.blob_path(hash))
    }

    /// Whether a blob's content still hashes to its name.
    ///
    /// # Errors
    ///
    /// The blob is missing or unreadable.
    pub fn verify_blob(&self, hash: &str) -> io::Result<bool> {
        Ok(sha256_file(&self.blob_path(hash))? == hash)
    }

    /// Remove a blob, if it is there.
    ///
    /// # Errors
    ///
    /// It is there and cannot be removed.
    pub fn remove_blob(&self, hash: &str) -> io::Result<()> {
        match fs::remove_file(self.blob_path(hash)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    /// Every blob in the store, with when it was last written.
    ///
    /// # Errors
    ///
    /// The store's directories cannot be listed.
    pub fn all_blobs(&self) -> io::Result<Vec<(String, SystemTime)>> {
        let mut blobs = Vec::new();
        if !self.base_path.exists() {
            return Ok(blobs);
        }
        for prefix_entry in fs::read_dir(&self.base_path)? {
            let prefix_entry = prefix_entry?;
            if !prefix_entry.file_type()?.is_dir() {
                continue;
            }
            // A blob's name is its hash, in hex: a name that is not text is
            // not one of the store's, and a lossy decode of it would list a
            // hash that names nothing. `staging` is not a prefix either.
            let Ok(prefix) = prefix_entry.file_name().into_string() else {
                continue;
            };
            if prefix.len() != 2 {
                continue;
            }
            for blob_entry in fs::read_dir(prefix_entry.path())? {
                let blob_entry = blob_entry?;
                let Ok(rest) = blob_entry.file_name().into_string() else {
                    continue;
                };
                let written = blob_entry.metadata()?.modified()?;
                blobs.push((format!("{prefix}{rest}"), written));
            }
        }
        Ok(blobs)
    }

    /// Staged copies older than `grace`: what a capture that died left.
    ///
    /// # Errors
    ///
    /// The staging directory exists and cannot be listed.
    pub fn stale_staging(&self, now: SystemTime, grace: Duration) -> io::Result<Vec<PathBuf>> {
        let dir = self.base_path.join("staging");
        let mut stale = Vec::new();
        let listing = match fs::read_dir(&dir) {
            Ok(listing) => listing,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(stale),
            Err(e) => return Err(e),
        };
        for entry in listing {
            let entry = entry?;
            let written = entry.metadata()?.modified()?;
            if now.duration_since(written).is_ok_and(|age| age >= grace) {
                stale.push(entry.path());
            }
        }
        Ok(stale)
    }
}

/// A name no other staged copy in this process or another will take.
fn unique_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{}-{nanos}-{n}", std::process::id())
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did it
    // -- that is the diagnosis. The defensive lints keep panics out of code
    // that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;

    // --- SHA-256 Tests ---

    #[test]
    fn test_sha256_empty() {
        let hash = sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_abc() {
        let hash = sha256_hex(b"abc");
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_sha256_longer() {
        let hash = sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        assert_eq!(
            hash,
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn test_sha256_multiblock() {
        // 64 bytes exactly — one full block
        let data = vec![0x61u8; 64]; // 'a' * 64
        let hash = sha256_hex(&data);
        assert_eq!(
            hash,
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
    }

    /// A file is hashed from an 8 KiB read buffer, so the pieces `sha256_file`
    /// feeds in have nothing to do with SHA-256's 64-byte blocks. This checks
    /// every split rather than the three arbitrary ones that were here before:
    /// a padding bug lives at one specific offset, so a single split tests
    /// almost nothing.
    #[test]
    fn hashing_in_pieces_matches_hashing_all_at_once() {
        let data = b"The quick brown fox jumps over the lazy dog";
        let expected = sha256_hex(data);
        for split in 0..=data.len() {
            let (head, tail) = data.split_at(split);
            let mut hasher = sha2::Sha256::new();
            hasher.update(head);
            hasher.update(tail);
            let actual = sha2::hex(&hasher.finalize()).as_str().to_string();
            assert_eq!(actual, expected, "split at {split}");
        }
    }
}
