//! The recycle bin: files moved aside, restorable, emptied or aged out.
//!
//! # Why a crate
//!
//! It was the file manager's (`apps/explorer/src/fileops.rs`), and the image
//! viewer's Delete key was a comment reading "would move to trash via OS
//! recycle bin integration" -- a key the shortcut list advertised that did
//! nothing. A second program that moves files to a bin must move them to *the*
//! bin, the one the file manager lists and restores from, or a picture
//! deleted in the viewer would be gone from everywhere the user looks for it.
//! So the bin is a crate both use, moved whole with its tests.
//!
//! # On disk
//!
//! `~/.recycle/<id>/meta.txt` holds the original path (escaped losslessly by
//! `pathcodec`, so a name that is not text restores to itself) and when it was
//! recycled; `~/.recycle/<id>/data` is the file or directory itself. See
//! [`RecycleBin`].

use pathtext::ShowPath;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use pathcodec::{decode_path, encode_path};

/// `name` as the user reads it (`pathtext`): a byte that is not text, or a
/// control character, as an escape -- never a lossy decode, which shows two
/// such names alike.
fn shown(name: &OsStr) -> String {
    name.shown().to_string()
}

/// Marker on the first line of a `meta.txt` whose path line is escaped.
///
/// Entries written before this existed begin with the raw path, so the marker
/// is what tells the two formats apart.
const META_VERSION: &str = "slate-recycle-v2";

/// Generate a non-conflicting destination name.
///
/// Given `/dest/file.txt`, tries `/dest/file (2).txt`, `/dest/file (3).txt`, etc.
///
/// The name is assembled as an [`OsString`](std::ffi::OsString), never as
/// text. A name on this OS may hold any byte but `/` and NUL, and this used to
/// build the new name from `to_string_lossy` of the old one -- so pasting
/// `caf\xE9.txt` into a folder that already had it created the copy as
/// `caf\u{FFFD} (2).txt`: a file under a name the user never gave anything,
/// with the byte that distinguished it gone. The copy and a recycle-bin restore
/// both land here, so both did it.
pub fn resolve_rename(dest: &Path) -> PathBuf {
    let stem = dest.file_stem().unwrap_or_default();
    let ext = dest.extension();
    let parent = dest.parent().unwrap_or(Path::new(""));
    let named = |marker: &str| {
        let mut name = stem.to_os_string();
        name.push(marker);
        if let Some(ext) = ext {
            name.push(".");
            name.push(ext);
        }
        parent.join(name)
    };

    for n in 2u32..10_000 {
        let candidate = named(&format!(" ({n})"));
        if !candidate.exists() {
            return candidate;
        }
    }
    // Extremely unlikely fallback.
    named(" (renamed)")
}

/// Move `src` to `dest`, falling back to copy-then-remove across devices.
///
/// `fs::rename` cannot cross a mount point — it fails with `EXDEV`. The recycle
/// bin lives under the user's home directory, so recycling anything from a
/// separate data partition hit exactly that and simply reported an error.
/// (The file manager's `drives::same_drive` answers this properly now, but attempting
/// the rename and reacting to its failure is still both cheaper in the common
/// case and correct in the cases no resolver can settle -- a path whose device
/// this machine cannot name is exactly a path whose rename might work.)
pub fn move_path(src: &Path, dest: &Path) -> io::Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => return Ok(()),
        Err(e) => {
            // A missing source, or a destination whose parent does not exist,
            // will not be fixed by copying either — report it as-is.
            if e.kind() == io::ErrorKind::NotFound {
                return Err(e);
            }
        }
    }

    // Links are carried as links and never followed -- the move is of the
    // entry the user chose, not of whatever it reaches. `src.is_dir()` used to
    // follow a link to a folder and copy the folder's contents into the bin.
    let meta = fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        copy_link(src, dest)?;
        return remove_link(src);
    }
    if meta.is_dir() {
        // A copy that fails part-way is taken back out of the bin: it is a
        // partial duplicate under a name nobody will look for, and the
        // original has not been touched.
        if let Err(e) = copy_tree(src, dest) {
            // Best effort: the error the caller needs is the copy's.
            let _ = fs::remove_dir_all(dest);
            return Err(e);
        }
        fs::remove_dir_all(src)
    } else {
        fs::copy(src, dest)?;
        fs::remove_file(src)
    }
}

/// Recursively copy a directory tree, links as links.
fn copy_tree(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let child_dest = dest.join(entry.file_name());
        // `DirEntry::file_type` does not follow links.
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            copy_link(&entry.path(), &child_dest)?;
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &child_dest)?;
        } else {
            fs::copy(entry.path(), &child_dest)?;
        }
    }
    Ok(())
}

/// Make at `dest` a link to what the link `src` names.
fn copy_link(src: &Path, dest: &Path) -> io::Result<()> {
    let target = fs::read_link(src)?;
    #[cfg(windows)]
    {
        // Windows makes links to folders and to files differently.
        if fs::metadata(src).is_ok_and(|m| m.is_dir()) {
            std::os::windows::fs::symlink_dir(&target, dest)
        } else {
            std::os::windows::fs::symlink_file(&target, dest)
        }
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(&target, dest)
    }
}

/// Remove the link `path` -- the link, never what it names. A Windows link to
/// a folder (and a junction) goes with `remove_dir`, every other with
/// `remove_file`.
fn remove_link(path: &Path) -> io::Result<()> {
    fs::remove_file(path).or_else(|_| fs::remove_dir(path))
}

/// Metadata for a recycled item.
#[derive(Clone, Debug)]
pub struct RecycleEntry {
    /// Unique identifier for this entry.
    pub id: String,
    /// Original absolute path before recycling, or `None` if this entry's
    /// metadata could not be read.
    ///
    /// `None` is the whole reason this is an `Option`. An entry whose
    /// `meta.txt` is damaged used to be *skipped* by [`RecycleBin::list`], so
    /// the file stayed on disk, kept occupying space, and could not be seen or
    /// emptied through any interface -- undeletable by the only means a user
    /// has. It is listed now, with the one thing that is genuinely unknown
    /// marked unknown, rather than hidden because part of it is.
    ///
    /// What a caller may do with it follows from this field and needs no
    /// second flag: restoring requires somewhere to restore *to*, so it is
    /// refused; deleting requires only the id, so it works.
    pub original_path: Option<PathBuf>,
    /// When the item was recycled, or `None` if the metadata could not be
    /// read.
    pub recycled_at: Option<SystemTime>,
    /// Size in bytes (0 for directories).
    ///
    /// Known either way: it is measured from the data on disk rather than
    /// read out of `meta.txt`, so a damaged entry still accounts for its own
    /// space.
    pub size: u64,
    /// Whether this is a directory. `false` for an unreadable entry, which is
    /// not a claim -- see [`is_readable`](Self::is_readable).
    pub is_dir: bool,
}

impl RecycleEntry {
    /// Whether this entry's metadata was readable.
    ///
    /// An unreadable entry can be deleted and cannot be restored.
    #[must_use]
    pub fn is_readable(&self) -> bool {
        self.original_path.is_some()
    }

    /// What to show a user in place of a name.
    ///
    /// The original file name when it is known, and a fixed label when it is
    /// not -- never a guess derived from the entry id, which is a hash and
    /// would read as though it were the file's name.
    #[must_use]
    pub fn display_name(&self) -> String {
        match &self.original_path {
            Some(path) => path
                .file_name()
                .map_or_else(|| shown(path.as_os_str()), shown),
            None => "Unknown item (damaged entry)".to_string(),
        }
    }
}

/// Manages the recycle bin at `~/.recycle/`.
///
/// Layout on disk:
/// ```text
/// ~/.recycle/
///     <hash>/
///         meta.txt        # original_path, recycled_at
///         data/           # the actual file or directory contents
/// ```
pub struct RecycleBin {
    root: PathBuf,
    /// Items older than this are eligible for auto-purge.
    max_age: Duration,
}

/// Recycled items are eligible for auto-purge after 30 days.
///
/// In hours rather than `Duration::from_days`, which is still nightly-gated
/// (rust-lang/rust#120301).
const DEFAULT_RECYCLE_MAX_AGE: Duration = Duration::from_hours(30 * 24);

/// How many candidate names a new recycle bin entry will try before failing.
///
/// Collisions come from same-named files recycled inside one clock tick, so
/// the realistic worst case is a handful. The bound is generous enough never
/// to be reached by that, and exists only so a pathological bin cannot spin
/// forever.
const MAX_ENTRY_ID_ATTEMPTS: usize = 4096;

/// Nanoseconds since the Unix epoch, or 0 if the clock is before it.
///
/// A clock that cannot be read yields a usable-but-colliding id rather than
/// failing the delete; [`RecycleBin::create_entry_dir`] resolves the collision,
/// so the degraded case costs a suffix, not the user's file.
fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

impl RecycleBin {
    /// Create a new `RecycleBin` rooted at `root`.
    ///
    /// `max_age` is the auto-purge threshold (default 30 days).
    pub fn new(root: PathBuf, max_age: Duration) -> Self {
        Self { root, max_age }
    }

    /// Create a `RecycleBin` at the default location (`~/.recycle/`)
    /// with 30-day auto-purge.
    ///
    /// `var_os`, not `var`. This read `HOME` as UTF-8 and fell back to `/tmp`
    /// when it was not, which put the recycle bin of anyone with a home
    /// directory holding undecodable bytes in a directory that is cleared on
    /// restart -- so "move to recycle bin" became "delete on next boot",
    /// silently, for exactly the users this module is otherwise careful about.
    /// [`Self::send_to_bin`] below goes to real trouble to record an original
    /// path losslessly so a non-UTF-8 name can be restored; that care was
    /// undone one function earlier by the location itself.
    ///
    /// The `/tmp` fallback now applies only when `HOME` is genuinely unset,
    /// which is its own hazard and is left alone here: it is the pre-existing
    /// behaviour for a case this change does not touch, and conflating the two
    /// would hide which one was the bug.
    pub fn default_location() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        Self::new(home.join(".recycle"), DEFAULT_RECYCLE_MAX_AGE)
    }

    /// Move `path` into the recycle bin and return the entry id.
    ///
    /// The original path is recorded losslessly (see [`encode_path`]) so that a
    /// file whose name is not valid UTF-8 can still be restored to where it
    /// came from. Recording it with `Display` would have written U+FFFD in
    /// place of every undecodable byte, and restore would then have recreated
    /// the file under a different name.
    pub fn recycle(&self, path: &Path) -> io::Result<String> {
        self.recycle_at(path, now_nanos())
    }

    /// [`Self::recycle`] with the clock supplied by the caller.
    ///
    /// The seam exists so that the collision handling in
    /// [`Self::create_entry_dir`] can be tested deterministically. Through
    /// `recycle` the timestamps of two successive calls always differ, because
    /// each call does enough filesystem work to advance even a coarse clock —
    /// but that is an accident of timing, not a guarantee, and it is not one
    /// the correctness of the bin should rest on.
    fn recycle_at(&self, path: &Path, ts: u128) -> io::Result<String> {
        let (id, entry_dir) = self.create_entry_dir(path, ts)?;
        let data_path = entry_dir.join("data");

        // Write metadata *before* moving the data: if the move fails, the
        // orphaned metadata is harmless (`read_entry` reports size 0), whereas
        // moved data with no metadata would be unrestorable.
        let meta_path = entry_dir.join("meta.txt");
        let mut meta_file = fs::File::create(&meta_path)?;
        writeln!(meta_file, "{META_VERSION}")?;
        writeln!(meta_file, "{}", encode_path(path))?;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        writeln!(meta_file, "{now}")?;
        meta_file.flush()?;

        // Move the actual data.
        if let Err(e) = move_path(path, &data_path) {
            // Do not leave metadata pointing at data that is not there.
            let _ = fs::remove_file(&meta_path);
            let _ = fs::remove_dir(&entry_dir);
            return Err(e);
        }

        Ok(id)
    }

    /// Restore a recycled item, returning where it actually landed.
    ///
    /// **The return value is the path to show the user**, not
    /// `entry.original_path`: if something else now occupies the original path,
    /// the item is restored beside it under a `name (2)` variant rather than on
    /// top of it. Restoring used to call [`move_path`] straight at the original
    /// path, and `fs::rename` replaces its destination without a word — so
    /// deleting `report.docx`, writing a new `report.docx`, then restoring the
    /// old one from the bin destroyed the new one. It did not go to the bin
    /// either; there was nothing left to recover.
    pub fn restore(&self, entry_id: &str) -> io::Result<PathBuf> {
        let entry = self.read_entry(entry_id)?;
        // An entry whose metadata would not parse has no original path, so
        // there is nowhere to put it back. `read_entry` fails for those, so
        // this is unreachable today -- it is written out anyway because `list`
        // now hands such entries to callers, and the next reader should find
        // the refusal here rather than an `unwrap` that happens to be safe.
        let Some(original_path) = entry.original_path else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "this entry's metadata is damaged, so there is no path to restore it to",
            ));
        };
        let data_path = self.root.join(entry_id).join("data");

        // Ensure parent directory exists.
        if let Some(parent) = original_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // The gap between this check and the move is a race that `std` alone
        // cannot close — there is no "rename only if the destination is free".
        // The same tradeoff is documented on the engine's own conflict handling
        // above; narrowing it needs a platform primitive we do not have here.
        let dest = if original_path.exists() {
            resolve_rename(&original_path)
        } else {
            original_path.clone()
        };

        move_path(&data_path, &dest)?;

        // Clean up the entry directory. Reported rather than ignored: a
        // `meta.txt` that survives its `data` leaves an entry that `list` still
        // shows and `restore` can no longer satisfy, and the user's only clue
        // would be the failure of a restore they try much later.
        let entry_dir = self.root.join(entry_id);
        for path in [entry_dir.join("meta.txt"), entry_dir.clone()] {
            let removed = if path == entry_dir {
                fs::remove_dir(&path)
            } else {
                fs::remove_file(&path)
            };
            if let Err(e) = removed
                && e.kind() != io::ErrorKind::NotFound
            {
                eprintln!(
                    "warning: restored {} but could not clear its recycle bin entry {}: {}",
                    dest.shown(),
                    path.shown(),
                    e
                );
            }
        }

        Ok(dest)
    }

    /// List all items in the recycle bin.
    pub fn list(&self) -> io::Result<Vec<RecycleEntry>> {
        let mut entries = Vec::new();

        if !self.root.exists() {
            return Ok(entries);
        }

        for dir_entry in fs::read_dir(&self.root)? {
            let dir_entry = dir_entry?;
            if !dir_entry.path().is_dir() {
                continue;
            }
            // Every folder this bin makes is named in text (`make_id`); one
            // that is not was not made by it, and reading it by a decoded
            // name would reach a different folder.
            let Some(id) = dir_entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            // An entry whose metadata will not parse is *listed* rather than
            // skipped. Failing the whole listing would make one corrupt
            // `meta.txt` hide every other recycled file; skipping it hid the
            // damaged one, which then occupied space nothing could account for
            // and could not be emptied. Listing it with its unknown parts
            // marked unknown is the only option that loses nothing.
            match self.read_entry(&id) {
                Ok(entry) => entries.push(entry),
                Err(_) => entries.push(RecycleEntry {
                    size: Self::entry_size(&dir_entry.path()),
                    id,
                    original_path: None,
                    recycled_at: None,
                    is_dir: false,
                }),
            }
        }

        // Most recently recycled first. An entry with no readable timestamp
        // sorts last rather than first: it is the one the user is least likely
        // to be looking for, and putting an unnameable row at the top of the
        // bin would bury what they came for.
        entries.sort_by_key(|e| std::cmp::Reverse(e.recycled_at));
        Ok(entries)
    }

    /// Permanently delete all items in the recycle bin.
    pub fn empty(&self) -> io::Result<u32> {
        let entries = self.list()?;
        let mut count = 0u32;
        for entry in &entries {
            let entry_dir = self.root.join(&entry.id);
            if fs::remove_dir_all(&entry_dir).is_ok() {
                count = count.saturating_add(1);
            }
        }
        Ok(count)
    }

    /// Permanently delete items older than `max_age`.
    pub fn purge_old(&self) -> io::Result<u32> {
        let entries = self.list()?;
        let now = SystemTime::now();
        let mut count = 0u32;

        for entry in &entries {
            // An entry with no readable timestamp is never aged out. Its age
            // is unknown, and "unknown" must not be read as "old": purging on
            // a guess would delete a user's file to tidy up a metadata
            // problem. It stays listed, and the user can empty it themselves.
            let Some(recycled_at) = entry.recycled_at else {
                continue;
            };
            let age = now.duration_since(recycled_at).unwrap_or(Duration::ZERO);
            if age > self.max_age {
                let entry_dir = self.root.join(&entry.id);
                if fs::remove_dir_all(&entry_dir).is_ok() {
                    count = count.saturating_add(1);
                }
            }
        }

        Ok(count)
    }

    /// Set the auto-purge age threshold.
    pub fn set_max_age(&mut self, age: Duration) {
        self.max_age = age;
    }

    /// Current auto-purge age threshold.
    pub fn max_age(&self) -> Duration {
        self.max_age
    }

    // ------------------------------------------------------------------
    // Internal
    // ------------------------------------------------------------------

    /// Generate a unique entry id from the file path and current time.
    /// A *candidate* directory name for a new entry.
    ///
    /// This is a hint, not a unique key, and must not be treated as one. Its
    /// only entropy is `ts`; two files that share a `file_name` and are
    /// recycled within one tick of the system clock produce the same string.
    /// That is not far-fetched — `SystemTime::now()` advances in 100 ns steps
    /// on Windows, and deleting `projA/README.md` and `projB/README.md`
    /// together is an ordinary multi-select. Uniqueness is enforced by
    /// [`Self::create_entry_dir`], which asks the filesystem.
    fn make_id(&self, path: &Path, ts: u128) -> String {
        // The file's name, kept readable for someone browsing the bin by
        // hand: letters, digits, `.`, `-` and `_` as they are, anything else
        // -- a byte that is not text, a character some system reads as a
        // separator -- as `_`. The original name itself is in `meta.txt`.
        let name: String = path.file_name().map_or_else(
            || "unknown".to_string(),
            |n| {
                n.as_encoded_bytes()
                    .iter()
                    .map(|&b| {
                        if b.is_ascii_alphanumeric() || b"._-".contains(&b) {
                            char::from(b)
                        } else {
                            '_'
                        }
                    })
                    .collect()
            },
        );
        // Simple hash to keep directory names manageable.
        let hash = ts ^ (name.len() as u128).wrapping_mul(0x517cc1b727220a95);
        format!("{name}_{hash:016x}")
    }

    /// Claim a fresh entry directory, returning its id and path.
    ///
    /// # Why the filesystem decides
    ///
    /// The previous code took [`Self::make_id`]'s output on trust and called
    /// `create_dir_all`, which succeeds when the directory already exists. Two
    /// entries landing on the same id therefore *shared* one directory: the
    /// second `recycle` overwrote the first's `meta.txt` and then renamed its
    /// `data` on top of the first's. One of the two deleted files ceased to
    /// exist, silently — no error, and nothing in the bin to restore it from.
    ///
    /// `fs::create_dir` fails with `AlreadyExists` instead of succeeding, so
    /// an entry directory is only ever used by the caller that created it.
    /// Uniqueness is then a property the filesystem guarantees rather than one
    /// the clock happens to provide, which also makes it hold across two
    /// explorer processes sharing a bin.
    fn create_entry_dir(&self, path: &Path, ts: u128) -> io::Result<(String, PathBuf)> {
        fs::create_dir_all(&self.root)?;
        let base = self.make_id(path, ts);

        for attempt in 0..MAX_ENTRY_ID_ATTEMPTS {
            // The first candidate is the unsuffixed name, so the common case
            // of no collision leaves the on-disk layout exactly as before.
            let id = if attempt == 0 {
                base.clone()
            } else {
                format!("{base}-{attempt}")
            };
            let dir = self.root.join(&id);
            match fs::create_dir(&dir) {
                Ok(()) => return Ok((id, dir)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }

        // Bounded rather than looping forever: a bin whose root has become
        // unwritable in a way that reports `AlreadyExists` would otherwise
        // hang the explorer. Failing the delete leaves the file where it is,
        // which is the recoverable outcome.
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("could not find a free recycle bin entry name for {base}"),
        ))
    }

    /// Read the metadata for a recycled entry.
    /// Where this bin keeps its entries.
    ///
    /// For a caller that needs to reach an entry's directory directly -- the
    /// tests that damage a `meta.txt` on purpose, and anything that later
    /// wants to report *which* file on disk is unreadable.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Bytes on disk under an entry directory, best effort.
    ///
    /// Used for an entry whose `meta.txt` will not parse: the size is the one
    /// fact still knowable about it, and it is the fact that matters, because
    /// the complaint that brings a user to the recycle bin is usually space.
    /// Recursive, and unreadable children count as zero rather than aborting
    /// the walk -- a partial total is more use than none.
    fn entry_size(dir: &Path) -> u64 {
        let Ok(read) = fs::read_dir(dir) else {
            return 0;
        };
        let mut total = 0u64;
        for child in read.flatten() {
            let path = child.path();
            if path.is_dir() {
                total = total.saturating_add(Self::entry_size(&path));
            } else if let Ok(meta) = child.metadata() {
                total = total.saturating_add(meta.len());
            }
        }
        total
    }

    fn read_entry(&self, id: &str) -> io::Result<RecycleEntry> {
        let entry_dir = self.root.join(id);
        let meta_path = entry_dir.join("meta.txt");
        let content = fs::read_to_string(&meta_path)?;
        let mut lines = content.lines();

        let first = lines
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty meta"))?;
        // A bin written before the path was escaped starts straight in with the
        // path. Reading those is still worth doing: the alternative is silently
        // orphaning whatever a user had already deleted.
        let original_path = if first == META_VERSION {
            let encoded = lines.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "missing path in meta")
            })?;
            decode_path(encoded)
        } else {
            PathBuf::from(first)
        };
        let ts_secs: u64 = lines
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing timestamp in meta"))?
            .trim()
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad timestamp"))?;

        let recycled_at = SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_secs(ts_secs))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "timestamp out of range"))?;

        let data_path = entry_dir.join("data");
        let (size, is_dir) = if data_path.exists() {
            let meta = fs::metadata(&data_path)?;
            (meta.len(), meta.is_dir())
        } else {
            (0, false)
        };

        Ok(RecycleEntry {
            id: id.to_string(),
            // `Some` because this function only returns at all when the
            // metadata parsed; the `None` case is built by `list`.
            original_path: Some(original_path),
            recycled_at: Some(recycled_at),
            size,
            is_dir,
        })
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;
    use scratchdir::ScratchDir;

    /// A private temporary directory for one test, removed when it drops.
    fn temp_dir(label: &str) -> ScratchDir {
        ScratchDir::new(&format!("recyclebin_test_{label}"))
    }

    /// Write a file with the given content, making its directory.
    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    /// Read a file to a string.
    fn read_file(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn recycle_and_restore() {
        let scratch = temp_dir("recycle_restore");
        let dir = scratch.dir().to_path_buf();
        let bin_root = dir.join("bin");
        let file_path = dir.join("important.txt");
        write_file(&file_path, "important data");

        let bin = RecycleBin::new(bin_root, Duration::from_hours(24));

        // Recycle.
        let id = bin.recycle(&file_path).unwrap();
        assert!(!file_path.exists());

        // List.
        let entries = bin.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].original_path.as_deref(),
            Some(file_path.as_path())
        );

        // Restore.
        let restored = bin.restore(&id).unwrap();
        assert_eq!(restored, file_path);
        assert!(file_path.exists());
        assert_eq!(read_file(&file_path), "important data");
    }

    /// Delete a file, make a new one with the same name, then restore the old
    /// one from the bin. The new file must survive.
    ///
    /// It did not. `restore` moved the recycled data straight onto
    /// `entry.original_path`, and `fs::rename` replaces its destination
    /// silently — so the newer file was destroyed by an action the user
    /// understands as *recovering* a file, and it did not go to the bin either.
    #[test]
    fn restoring_over_a_newer_file_of_the_same_name_keeps_both() {
        let scratch = temp_dir("restore_conflict");
        let dir = scratch.dir().to_path_buf();
        let bin_root = dir.join("bin");
        let file_path = dir.join("report.docx");
        write_file(&file_path, "the old draft");

        let bin = RecycleBin::new(bin_root, Duration::from_hours(24));
        let id = bin.recycle(&file_path).expect("recycle");
        assert!(!file_path.exists());

        // The user moves on and writes a new file under the same name.
        write_file(&file_path, "the new draft");

        let restored = bin.restore(&id).expect("restore");

        assert_eq!(
            read_file(&file_path),
            "the new draft",
            "restoring must never overwrite a file the user made since"
        );
        assert_ne!(
            restored, file_path,
            "the restored copy has to land somewhere else, and say where"
        );
        assert_eq!(
            read_file(&restored),
            "the old draft",
            "and the recycled contents must be what landed there"
        );
    }

    /// The unoccupied case must be untouched: a restore with nothing in the
    /// way still goes back to exactly where it came from, under its own name.
    #[test]
    fn restoring_to_a_free_path_uses_the_original_name() {
        let scratch = temp_dir("restore_free");
        let dir = scratch.dir().to_path_buf();
        let bin_root = dir.join("bin");
        let file_path = dir.join("notes.txt");
        write_file(&file_path, "notes");

        let bin = RecycleBin::new(bin_root, Duration::from_hours(24));
        let id = bin.recycle(&file_path).expect("recycle");
        let restored = bin.restore(&id).expect("restore");

        assert_eq!(restored, file_path, "no conflict, no renaming");
        assert_eq!(read_file(&file_path), "notes");
    }

    /// A directory in the way is the same hazard with more to lose: the old
    /// code's `move_path` fell back to `copy_tree`, which merges into an
    /// existing directory and overwrites the same-named files inside it.
    #[test]
    fn restoring_a_directory_does_not_merge_into_one_that_is_in_the_way() {
        let scratch = temp_dir("restore_dir_conflict");
        let dir = scratch.dir().to_path_buf();
        let bin_root = dir.join("bin");
        let src_dir = dir.join("project");
        fs::create_dir_all(&src_dir).expect("project");
        write_file(&src_dir.join("main.rs"), "the old source");

        let bin = RecycleBin::new(bin_root, Duration::from_hours(24));
        let id = bin.recycle(&src_dir).expect("recycle");

        // A new, unrelated `project/` with a file of the same name.
        fs::create_dir_all(&src_dir).expect("new project");
        write_file(&src_dir.join("main.rs"), "the new source");

        let restored = bin.restore(&id).expect("restore");

        assert_eq!(
            read_file(&src_dir.join("main.rs")),
            "the new source",
            "the directory in the way must not be merged into"
        );
        assert_eq!(read_file(&restored.join("main.rs")), "the old source");
    }

    /// Two files that share a name but live in different directories are an
    /// entirely ordinary multi-select delete — `projA/README.md` and
    /// `projB/README.md`. They must both survive it.
    ///
    /// They did not. The entry id was `{file_name}_{hash}` where the hash's
    /// only entropy was `SystemTime::now()`, and on Windows that clock
    /// advances in 100 ns ticks — measured here, ~70% of back-to-back readings
    /// are *identical*. Two same-named files recycled in the same tick
    /// therefore got the same id, and the second `recycle` overwrote the
    /// first's metadata and then renamed its data on top of the first's data.
    /// One of the two files was gone, with no error reported anywhere.
    #[test]
    fn two_same_named_files_recycled_together_both_survive() {
        let scratch = temp_dir("recycle_collide");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"), Duration::from_hours(24));

        // Both recycled at the *same* instant. Going through `recycle` would
        // not reproduce this: each call does enough filesystem work to advance
        // the clock, which is why the bug survived so long. The seam removes
        // that accident so the collision handling is what is under test.
        let a = dir.join("projA/README.md");
        let b = dir.join("projB/README.md");
        write_file(&a, "contents of A");
        write_file(&b, "contents of B");
        let id_a = bin
            .recycle_at(&a, 1_700_000_000_000_000_000)
            .expect("recycle a");
        let id_b = bin
            .recycle_at(&b, 1_700_000_000_000_000_000)
            .expect("recycle b");

        assert_ne!(id_a, id_b, "two different files must not share a bin entry");

        let entries = bin.list().expect("list");
        assert_eq!(entries.len(), 2, "both files must be listed in the bin");

        // Both must restore, to their own original paths, with their own data.
        let restored_a = bin.restore(&id_a).expect("restore a");
        let restored_b = bin.restore(&id_b).expect("restore b");
        assert_eq!(restored_a, a);
        assert_eq!(restored_b, b);
        assert_eq!(read_file(&a), "contents of A");
        assert_eq!(read_file(&b), "contents of B");
    }

    /// The same collision, but at the scale a "delete this whole folder"
    /// produces: many identically-named files, all recycled at one instant.
    /// Uniqueness has to hold for all of them, not just for a pair.
    #[test]
    fn a_burst_of_same_named_files_keeps_every_one() {
        let scratch = temp_dir("recycle_burst");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"), Duration::from_hours(24));

        let mut ids = Vec::new();
        for i in 0..64 {
            let path = dir.join(format!("d{i}/notes.txt"));
            write_file(&path, &format!("file {i}"));
            ids.push((i, bin.recycle_at(&path, 4242).expect("recycle")));
        }

        let unique: std::collections::BTreeSet<&String> = ids.iter().map(|(_, id)| id).collect();
        assert_eq!(unique.len(), ids.len(), "every entry needs its own id");
        assert_eq!(bin.list().expect("list").len(), ids.len());

        for (i, id) in &ids {
            let restored = bin.restore(id).expect("restore");
            assert_eq!(restored, dir.join(format!("d{i}/notes.txt")));
            assert_eq!(read_file(&restored), format!("file {i}"));
        }
    }

    #[test]
    fn recycle_bin_empty() {
        let scratch = temp_dir("recycle_empty");
        let dir = scratch.dir().to_path_buf();
        let bin_root = dir.join("bin");

        let bin = RecycleBin::new(bin_root, Duration::from_hours(24));

        write_file(&dir.join("a.txt"), "aaa");
        write_file(&dir.join("b.txt"), "bbb");

        bin.recycle(&dir.join("a.txt")).unwrap();
        bin.recycle(&dir.join("b.txt")).unwrap();

        assert_eq!(bin.list().unwrap().len(), 2);

        let removed = bin.empty().unwrap();
        assert_eq!(removed, 2);
        assert_eq!(bin.list().unwrap().len(), 0);
    }

    #[test]
    fn recycle_bin_list_empty() {
        let scratch = temp_dir("recycle_list_empty");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"), Duration::from_hours(24));
        let entries = bin.list().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn recycle_bin_purge_old() {
        let scratch = temp_dir("recycle_purge");
        let dir = scratch.dir().to_path_buf();
        let bin_root = dir.join("bin");

        // Max age of 0 seconds means everything is "old".
        let bin = RecycleBin::new(bin_root, Duration::from_secs(0));

        write_file(&dir.join("old.txt"), "old");
        bin.recycle(&dir.join("old.txt")).unwrap();

        let purged = bin.purge_old().unwrap();
        assert_eq!(purged, 1);
        assert_eq!(bin.list().unwrap().len(), 0);
    }

    #[test]
    fn a_recycled_non_ascii_name_restores_to_its_original_path() {
        let scratch = temp_dir("recycle_nonascii");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"), Duration::from_hours(24));

        let file_path = dir.join("写真 100%.txt");
        write_file(&file_path, "keep");

        let id = bin.recycle(&file_path).expect("recycle");
        assert!(!file_path.exists());

        let restored = bin.restore(&id).expect("restore");
        assert_eq!(
            restored, file_path,
            "restore must put the file back under its original name"
        );
        assert_eq!(read_file(&file_path), "keep");
    }

    #[test]
    fn a_bin_written_in_the_old_format_is_still_readable() {
        let scratch = temp_dir("recycle_legacy");
        let dir = scratch.dir().to_path_buf();
        let bin_root = dir.join("bin");
        let entry_dir = bin_root.join("legacy_0000000000000001");
        fs::create_dir_all(&entry_dir).expect("entry dir");
        // The pre-versioning layout: raw path on line 1, timestamp on line 2.
        write_file(
            &entry_dir.join("meta.txt"),
            "/home/u/legacy.txt\n1700000000\n",
        );
        write_file(&entry_dir.join("data"), "old contents");

        let bin = RecycleBin::new(bin_root, Duration::from_hours(24));
        let listed = bin.list().expect("list");
        assert_eq!(
            listed.len(),
            1,
            "an already-deleted file must not be orphaned"
        );
        assert_eq!(
            listed.first().expect("one").original_path.as_deref(),
            Some(Path::new("/home/u/legacy.txt"))
        );
    }

    #[test]
    fn a_recycle_that_could_not_move_the_data_leaves_no_entry_behind() {
        let scratch = temp_dir("recycle_orphan");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"), Duration::from_hours(24));

        let err = bin
            .recycle(&dir.join("never_existed.txt"))
            .expect_err("recycling a missing file must fail");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);

        assert!(
            bin.list().expect("list").is_empty(),
            "metadata written before a failed move must be cleaned up, or the \
             bin lists an entry whose data is not there"
        );
    }

    #[test]
    fn move_path_relocates_a_whole_directory_tree() {
        let scratch = temp_dir("move_tree");
        let dir = scratch.dir().to_path_buf();
        let src = dir.join("src");
        fs::create_dir_all(src.join("nested")).expect("nested");
        write_file(&src.join("top.txt"), "top");
        write_file(&src.join("nested/deep.txt"), "deep");

        let dest = dir.join("dest");
        move_path(&src, &dest).expect("move");

        assert!(!src.exists(), "the source must be gone");
        assert_eq!(read_file(&dest.join("top.txt")), "top");
        assert_eq!(read_file(&dest.join("nested/deep.txt")), "deep");
    }

    #[test]
    fn copy_tree_reproduces_every_level() {
        let scratch = temp_dir("copy_tree");
        let dir = scratch.dir().to_path_buf();
        let src = dir.join("src");
        fs::create_dir_all(src.join("a/b")).expect("dirs");
        write_file(&src.join("a/b/leaf.txt"), "leaf");

        let dest = dir.join("dest");
        copy_tree(&src, &dest).expect("copy");

        assert_eq!(read_file(&dest.join("a/b/leaf.txt")), "leaf");
        assert!(src.exists(), "a copy must leave the source in place");
    }

    /// Recycle a file, then corrupt its metadata.
    fn bin_with_a_damaged_entry(root: &Path) -> (RecycleBin, String) {
        let bin = RecycleBin::new(root.join("bin"), Duration::from_hours(1));
        let file = root.join("notes.txt");
        write_file(&file, "some bytes");
        let id = bin.recycle(&file).expect("recycle");
        fs::write(
            bin.root().join(&id).join("meta.txt"),
            "\u{fffd}not a meta file",
        )
        .expect("corrupt the metadata");
        (bin, id)
    }

    #[test]
    fn a_damaged_entry_is_listed_rather_than_hidden() {
        let scratch = temp_dir("recycle_damaged_listed");
        let (bin, id) = bin_with_a_damaged_entry(scratch.dir());

        let listed = bin.list().expect("list");
        assert_eq!(listed.len(), 1, "the damaged entry vanished from the bin");
        let entry = listed.first().expect("one");
        assert_eq!(entry.id, id);
        assert!(!entry.is_readable());
        assert_eq!(entry.original_path, None, "and does not invent a path");
    }

    /// Its size is still known, because that is what a user came to the bin
    /// about.
    #[test]
    fn a_damaged_entry_still_accounts_for_its_space() {
        let scratch = temp_dir("recycle_damaged_size");
        let (bin, _) = bin_with_a_damaged_entry(scratch.dir());

        let listed = bin.list().expect("list");
        assert!(
            listed.first().expect("one").size >= 10,
            "the ten bytes of the recycled file are unaccounted for"
        );
    }

    /// It reads as something, and not as its hash.
    #[test]
    fn a_damaged_entry_has_a_name_that_is_not_its_id() {
        let scratch = temp_dir("recycle_damaged_name");
        let (bin, id) = bin_with_a_damaged_entry(scratch.dir());

        let listed = bin.list().expect("list");
        let name = listed.first().expect("one").display_name();
        assert!(!name.contains(&id), "the id is a hash, not a name: {name}");
        assert!(!name.is_empty());
    }

    /// Emptying the bin removes it: the point of listing it at all.
    #[test]
    fn a_damaged_entry_can_be_emptied() {
        let scratch = temp_dir("recycle_damaged_empty");
        let (bin, _) = bin_with_a_damaged_entry(scratch.dir());

        bin.empty().expect("empty");
        assert!(
            bin.list().expect("list").is_empty(),
            "the damaged entry survived an empty, so its space is still lost"
        );
    }

    /// Restoring it is refused rather than attempted.
    #[test]
    fn a_damaged_entry_cannot_be_restored() {
        let scratch = temp_dir("recycle_damaged_restore");
        let (bin, id) = bin_with_a_damaged_entry(scratch.dir());

        assert!(
            bin.restore(&id).is_err(),
            "restored a file to a path nobody knows"
        );
    }

    /// And it is never aged out.
    ///
    /// Its age is unknown, and unknown must not be read as old: purging on a
    /// guess would delete a user's file to tidy up a metadata problem.
    #[test]
    fn a_damaged_entry_is_not_purged_by_age() {
        let scratch = temp_dir("recycle_damaged_purge");
        let bin = RecycleBin::new(scratch.dir().join("bin"), Duration::from_secs(0));
        let file = scratch.dir().join("notes.txt");
        write_file(&file, "some bytes");
        let id = bin.recycle(&file).expect("recycle");
        fs::write(bin.root().join(&id).join("meta.txt"), "not a meta file").expect("corrupt");

        bin.purge_old().expect("purge");
        assert_eq!(
            bin.list().expect("list").len(),
            1,
            "a damaged entry was aged out on an age nobody knows"
        );
    }

    /// A recycled file's folder in the bin is named in plain characters: a
    /// byte that is not text, or a character some system takes for a
    /// separator, becomes `_`. The name itself is kept in `meta.txt`.
    #[test]
    fn a_recycle_bin_folder_is_named_in_plain_characters() {
        let bin = RecycleBin::new(
            std::env::temp_dir().join("slateos-unused-bin"),
            Duration::from_secs(1),
        );
        let id = bin.make_id(Path::new("/docs/caf\u{e9} notes.txt"), 7);
        assert!(id.starts_with("caf___notes.txt_"), "{id}");
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "{id}"
        );
        assert!(bin.make_id(Path::new("/"), 7).starts_with("unknown_"));
    }

    // The pure encode/decode cases live in `gui/pathcodec`, which owns the
    // encoding and tests every byte value; this one goes through `meta.txt`,
    // which is this crate's own use of it.
    #[test]
    fn a_path_needing_escapes_round_trips_through_the_metadata() {
        // A percent sign (the escape character itself), a space, a non-ASCII
        // name, and a control character — the four things a naive text format
        // gets wrong.
        for original in [
            "/home/u/100% done.txt",
            "/home/u/写真/2024.jpg",
            "/home/u/a\tb",
            "/home/u/plain.txt",
        ] {
            let path = PathBuf::from(original);
            let encoded = encode_path(&path);
            assert!(
                encoded.bytes().all(|b| (0x20..0x7f).contains(&b)),
                "the encoding must stay on one line of printable ASCII: {encoded:?}"
            );
            assert_eq!(
                decode_path(&encoded),
                path,
                "round trip failed for {original:?} (encoded as {encoded:?})"
            );
        }
    }

    /// A link at `link` to the folder `target`: a symbolic link, or on a
    /// Windows host without the privilege a junction. `None` if neither.
    fn folder_link(target: &Path, link: &Path) -> Option<()> {
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(target, link).is_ok() {
                return Some(());
            }
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .ok()?;
            (made.status.success()
                && fs::symlink_metadata(link).is_ok_and(|m| m.file_type().is_symlink()))
            .then_some(())
        }
        #[cfg(not(windows))]
        {
            std::os::unix::fs::symlink(target, link).ok()
        }
    }

    /// The copy a recycle falls back to across drives carries a link as a
    /// link, or fails and takes its partial copy back out -- never a copy of
    /// what the link reaches. (2026-09-27: it followed links.)
    #[test]
    fn the_cross_drive_copy_never_follows_a_link() {
        let scratch = temp_dir("copy_tree_link");
        let root = scratch.dir().to_path_buf();
        fs::create_dir_all(root.join("outside")).unwrap();
        fs::write(root.join("outside").join("keep.txt"), "not selected").unwrap();
        fs::create_dir_all(root.join("chosen")).unwrap();
        fs::write(root.join("chosen").join("mine.txt"), "selected").unwrap();
        if folder_link(&root.join("outside"), &root.join("chosen").join("link")).is_none() {
            eprintln!("skipped: this host can make no link");
            return;
        }
        let copied = root.join("bin-data");
        // A host that can make junctions but not links cannot copy one, and
        // the copy then fails, which is honest; where it succeeds, the link
        // arrived as a link.
        if copy_tree(&root.join("chosen"), &copied).is_ok() {
            assert!(
                fs::symlink_metadata(copied.join("link")).is_ok_and(|m| m.file_type().is_symlink()),
                "the link arrived as something else"
            );
        }
        assert_eq!(
            fs::read_to_string(root.join("outside").join("keep.txt")).unwrap(),
            "not selected"
        );
    }

    /// A move of a link across drives moves the link: what it named stays.
    #[test]
    fn moving_a_link_moves_the_link_not_what_it_names() {
        let scratch = temp_dir("move_link");
        let root = scratch.dir().to_path_buf();
        fs::create_dir_all(root.join("outside")).unwrap();
        fs::write(root.join("outside").join("keep.txt"), "not selected").unwrap();
        if folder_link(&root.join("outside"), &root.join("shortcut")).is_none() {
            eprintln!("skipped: this host can make no link");
            return;
        }
        // Same drive, so the rename path; the fallback is what the test above
        // covers. Either way the folder named must be untouched.
        move_path(&root.join("shortcut"), &root.join("moved")).unwrap();
        assert!(fs::symlink_metadata(root.join("shortcut")).is_err());
        assert!(fs::symlink_metadata(root.join("moved")).is_ok_and(|m| m.file_type().is_symlink()));
        assert_eq!(
            fs::read_to_string(root.join("outside").join("keep.txt")).unwrap(),
            "not selected"
        );
    }
}
