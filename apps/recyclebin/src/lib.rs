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
//! # A bin on every drive
//!
//! Each drive has a bin of its own (design-decisions §1238): the home
//! folder's drive keeps `~/.recycle`, and every other drive keeps one at its
//! top, so a file is moved to a bin without leaving its drive. [`Bins`] says
//! which bin a file goes to and which bins there are; [`RecycleBin`] is one
//! bin. Each bin keeps to its own [`Limits`] -- an age, a size, a number of
//! items -- written in the bin, so they go where the drive goes.
//!
//! # On disk
//!
//! `<bin>/<id>/meta.txt` holds the original path (escaped losslessly by
//! `pathcodec`, so a name that is not text restores to itself) and when it was
//! recycled; `<bin>/<id>/data` is the file or directory itself; and
//! `<bin>/limits.yaml`, when the drive has limits of its own, says what they
//! are. See [`RecycleBin`].

mod bins;
mod drives;
mod limits;

pub use bins::{Bins, DriveBin};
pub use drives::{Drives, SystemDrives, longest_mount, parse_mounts};
pub use limits::{Limits, USER_SETTINGS};

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
    /// Whether what was recycled is a link. The bin keeps a link as the link
    /// (§1220), so this is what a restore puts back: a link, never the folder
    /// or file it names -- which is why neither `is_dir` nor `size` describes
    /// what it names either.
    pub is_link: bool,
}

/// What emptying the bin did.
#[derive(Debug, Default)]
pub struct Emptied {
    /// How many entries were deleted.
    pub deleted: u32,
    /// The entries that could not be, each with the reason. They are still in
    /// the bin, whole or in part, and still listed.
    pub failed: Vec<(RecycleEntry, io::Error)>,
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

/// One recycle bin: the home bin (`~/.recycle/`) or a drive's.
///
/// Layout on disk:
/// ```text
/// <bin>/
///     limits.yaml         # this drive's limits, if it has its own
///     <hash>/
///         meta.txt        # original_path, recycled_at
///         data/           # the actual file or directory contents
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecycleBin {
    root: PathBuf,
}

/// What holding a bin to its limits did ([`RecycleBin::prune`]).
#[derive(Debug, Default)]
pub struct Pruned {
    /// How many entries were deleted.
    pub deleted: u32,
    /// How many bytes they held.
    pub freed: u64,
    /// The entries a limit took that could not be deleted, each with the
    /// reason. They are still in the bin, whole or in part.
    pub failed: Vec<(RecycleEntry, io::Error)>,
}

/// How much a bin holds ([`RecycleBin::usage`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// How many entries.
    pub items: u32,
    /// How many bytes, folders measured whole.
    pub bytes: u64,
}

/// The most of a `limits.yaml` that is read: a few lines are all it holds,
/// and a drive is not trusted to keep it short.
const LIMITS_MAX: usize = 64 * 1024;

/// What a new `limits.yaml` says at its top, for whoever opens it.
const LIMITS_HEADER: &str = "# How long this drive's recycle bin keeps what is deleted.\n\
                             # A limit left out is no limit.\n";

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
    /// The bin at `root`. Nothing is made until something is recycled.
    ///
    /// Which bin a file belongs in is [`Bins`]'s to say: the home bin is
    /// `$HOME/.recycle` -- read with `var_os`, not `var`, which read `HOME` as
    /// UTF-8 and fell back to `/tmp` when it was not, putting the bin of
    /// anyone whose home folder's name is not text in a folder cleared on
    /// restart -- and each other drive's is at its top.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
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
        let entry_dir = self.entry_dir(entry_id)?;
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
        let data_path = entry_dir.join("data");

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
            // The entry's own type, not what it names: `path().is_dir()`
            // followed a link, so a link planted in the bin that named a
            // folder elsewhere was listed as an entry -- and "Delete
            // permanently" on it was a delete aimed out of the bin. The bin
            // makes every entry with `create_dir`, so an entry is a folder.
            if !dir_entry.file_type().is_ok_and(|t| t.is_dir()) {
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
                    is_link: false,
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

    /// Permanently delete one entry: what was recycled and the record of it.
    ///
    /// For "Delete permanently" in the bin's view. Works on a damaged entry
    /// too, since it needs only the id -- which is the one way such an entry
    /// can be got rid of singly.
    ///
    /// An entry that is already gone -- another window emptied the bin a
    /// moment ago -- is not an error: what was asked for is true.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an id that is not one of this bin's entry names
    /// (see [`Self::entry_dir`]); otherwise whatever the removal met. A
    /// removal that fails part-way leaves the entry listed, with what is
    /// left of it.
    pub fn delete(&self, entry_id: &str) -> io::Result<()> {
        let dir = self.entry_dir(entry_id)?;
        match fs::remove_dir_all(&dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    /// Permanently delete everything in the recycle bin.
    ///
    /// Every entry is tried, and each that could not be deleted is reported
    /// with its reason. This used to answer with the number deleted and drop
    /// the failures, so a caller could only say "emptied" of a bin that still
    /// held whatever a locked file or a permission kept in it.
    ///
    /// # Errors
    ///
    /// Only when the bin cannot be listed; a failure to delete one entry is
    /// in [`Emptied::failed`], not here.
    pub fn empty(&self) -> io::Result<Emptied> {
        let mut outcome = Emptied::default();
        for entry in self.list()? {
            match self.delete(&entry.id) {
                Ok(()) => outcome.deleted = outcome.deleted.saturating_add(1),
                Err(e) => outcome.failed.push((entry, e)),
            }
        }
        Ok(outcome)
    }

    /// Hold the bin to `limits` as of `now`: delete what was deleted longer
    /// ago than the age limit, then, oldest first, what takes it over the
    /// size or the number of items.
    ///
    /// Ages are measured from the time recorded with each entry when it was
    /// recycled, so a drive that was away a month is pruned for that month the
    /// moment it is back. An entry whose metadata cannot be read is never
    /// taken by a limit: its age is unknown, and unknown must not be read as
    /// old -- that would delete a file to tidy up a damaged record. It counts
    /// towards the size and the number, as it takes up space, and the user can
    /// delete it themselves. A clock behind an entry's time gives it no age.
    ///
    /// Every entry a limit takes is tried; each that could not be deleted is
    /// in [`Pruned::failed`] -- this used to be `purge_old`, which counted the
    /// ones that went and said nothing of the others, and which nothing ever
    /// called: the thirty days the bin was said to keep were never applied.
    ///
    /// # Errors
    ///
    /// Only when the bin cannot be listed.
    pub fn prune(&self, limits: &Limits, now: SystemTime) -> io::Result<Pruned> {
        let mut outcome = Pruned::default();
        let entries = self.list()?;
        // A folder's size is not in its entry (`RecycleEntry::size`); it is
        // measured here, only when a size limit needs it.
        let measure = |entry: &RecycleEntry| {
            if limits.max_bytes.is_some() {
                Self::entry_size(&self.root.join(&entry.id))
            } else {
                entry.size
            }
        };
        let sizes: Vec<u64> = entries.iter().map(measure).collect();
        // Oldest first; an undated entry last, where no limit reaches it.
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by_key(|&i| {
            entries
                .get(i)
                .and_then(|e| e.recycled_at)
                .map_or((1, SystemTime::UNIX_EPOCH), |at| (0, at))
        });
        let mut items = u64::try_from(entries.len()).unwrap_or(u64::MAX);
        let mut bytes = sizes
            .iter()
            .fold(0_u64, |total, s| total.saturating_add(*s));

        for &i in &order {
            let Some(entry) = entries.get(i) else {
                continue;
            };
            let Some(recycled_at) = entry.recycled_at else {
                // Undated, and every later one is too.
                break;
            };
            let age = now.duration_since(recycled_at).unwrap_or(Duration::ZERO);
            let too_old = limits.max_age.is_some_and(|max| age > max);
            let too_many = limits.max_items.is_some_and(|max| items > u64::from(max));
            let too_big = limits.max_bytes.is_some_and(|max| bytes > max);
            if !(too_old || too_many || too_big) {
                // Everything after this is newer: no age limit takes it, and
                // the bin is within its size and number already.
                break;
            }
            let size = sizes.get(i).copied().unwrap_or(0);
            match self.delete(&entry.id) {
                Ok(()) => {
                    outcome.deleted = outcome.deleted.saturating_add(1);
                    outcome.freed = outcome.freed.saturating_add(size);
                    items = items.saturating_sub(1);
                    bytes = bytes.saturating_sub(size);
                }
                // Still in the bin, and still counted: a newer entry may go
                // in its place to bring the bin within its size or number.
                Err(e) => outcome.failed.push((entry.clone(), e)),
            }
        }
        Ok(outcome)
    }

    /// This bin's limits: those written in it (`limits.yaml`), or `default`
    /// when it has none of its own.
    ///
    /// A file that is there but cannot be read gives [`Limits::NONE`], not
    /// `default`: the drive's own limits may be looser than the default --
    /// "keep a year" -- and applying a tighter one in their place would delete
    /// what the user chose to keep. Keeping is the direction a mistake can be
    /// undone in.
    #[must_use]
    pub fn limits(&self, default: &Limits) -> Limits {
        match self.own_limits() {
            Ok(Some(own)) => own,
            Ok(None) => *default,
            Err(_) => Limits::NONE,
        }
    }

    /// The limits written in this bin, if it has its own: `Ok(None)` when it
    /// has no `limits.yaml` and keeps to the default.
    ///
    /// # Errors
    ///
    /// A `limits.yaml` that is there but cannot be read, or is too long to be
    /// one -- for which [`limits`](Self::limits) keeps everything, and a page
    /// showing the bin should say why.
    pub fn own_limits(&self) -> io::Result<Option<Limits>> {
        let path = self.root.join(limits::FILE_NAME);
        match safeio::read_to_string_capped(&path, LIMITS_MAX) {
            Ok(read) if !read.truncated => Ok(Some(Limits::read(
                &yamldoc::Document::parse(&read.text),
                &[],
            ))),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is too long to be a bin's limits", path.shown()),
            )),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Give this bin limits of its own -- `Some` -- or take them away, so it
    /// keeps to the user's default -- `None`. Written into the bin, so they go
    /// where the drive goes; a file a user has commented keeps its comments.
    ///
    /// # Errors
    ///
    /// What writing or removing the file meets.
    pub fn set_limits(&self, limits: Option<&Limits>) -> io::Result<()> {
        let path = self.root.join(limits::FILE_NAME);
        let Some(limits) = limits else {
            return match fs::remove_file(&path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            };
        };
        let text = match safeio::read_to_string_capped(&path, LIMITS_MAX) {
            Ok(read) if !read.truncated => {
                let mut doc = yamldoc::Document::parse(&read.text);
                limits.write(&mut doc, &[]);
                doc.to_text()
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} is too long to be a bin's limits", path.shown()),
                ));
            }
            // A new file: the keys, under a line saying what they are.
            // (Written into a document holding only that line, they would go
            // above it -- a document's keys come before a comment that
            // follows them.)
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let mut doc = yamldoc::Document::new();
                limits.write(&mut doc, &[]);
                format!("{LIMITS_HEADER}{}", doc.to_text())
            }
            Err(e) => return Err(e),
        };
        fs::create_dir_all(&self.root)?;
        safeio::write_str_atomically(&path, &text)
    }

    /// How much the bin holds: its entries, and their bytes with folders
    /// measured whole.
    ///
    /// # Errors
    ///
    /// Only when the bin cannot be listed.
    pub fn usage(&self) -> io::Result<Usage> {
        let mut usage = Usage::default();
        for entry in self.list()? {
            usage.items = usage.items.saturating_add(1);
            usage.bytes = usage
                .bytes
                .saturating_add(Self::entry_size(&self.root.join(&entry.id)));
        }
        Ok(usage)
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

    /// The folder of the entry called `id`, refusing anything that is not one
    /// plain name.
    ///
    /// An id is a folder name inside the bin, and every one the bin hands out
    /// is (`make_id`). But `restore` and `delete` take it from their caller,
    /// and `root.join("../Documents")` is a path out of the bin: a
    /// "Delete permanently" given one would have deleted a folder the user
    /// never put in it. So the id is checked to be exactly one ordinary name
    /// -- no separator, no `..`, not empty -- before it is joined to anything.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for anything else.
    fn entry_dir(&self, id: &str) -> io::Result<PathBuf> {
        let mut parts = Path::new(id).components();
        match (parts.next(), parts.next()) {
            (Some(std::path::Component::Normal(name)), None) if name == OsStr::new(id) => {
                Ok(self.root.join(name))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a recycle bin entry", shown(OsStr::new(id))),
            )),
        }
    }

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
    /// The whole tree, and unreadable children count as zero rather than
    /// aborting the walk -- a partial total is more use than none.
    ///
    /// A link counts as itself and is never walked through: what it names is
    /// not in the bin. `path.is_dir()` used to follow one, so a link to a
    /// folder added that folder's size, and a link to a folder above it
    /// recursed until the stack ran out. And the walk keeps its own list of
    /// folders still to read rather than recursing, so a tree however deep
    /// cannot run it out either.
    fn entry_size(dir: &Path) -> u64 {
        let mut total = 0u64;
        let mut pending = vec![dir.to_path_buf()];
        while let Some(dir) = pending.pop() {
            let Ok(read) = fs::read_dir(&dir) else {
                continue;
            };
            for child in read.flatten() {
                match child.file_type() {
                    Ok(kind) if kind.is_dir() => pending.push(child.path()),
                    // `DirEntry::metadata` does not follow a link, so a link
                    // is measured as the link.
                    Ok(_) => {
                        if let Ok(meta) = child.metadata() {
                            total = total.saturating_add(meta.len());
                        }
                    }
                    Err(_) => {}
                }
            }
        }
        total
    }

    fn read_entry(&self, id: &str) -> io::Result<RecycleEntry> {
        let entry_dir = self.entry_dir(id)?;
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

        // What is in the bin, not what it names: `exists` and `metadata`
        // both followed a link, so a recycled link to a folder read as the
        // folder -- and one whose target had gone read as nothing at all.
        // A folder's size is 0, as the field says: its own byte count is not
        // what a size means, and its contents are not measured here.
        let data_path = entry_dir.join("data");
        let (size, is_dir, is_link) = match fs::symlink_metadata(&data_path) {
            Ok(meta) if meta.file_type().is_symlink() => (0, false, true),
            Ok(meta) if meta.is_dir() => (0, true, false),
            Ok(meta) => (meta.len(), false, false),
            // Metadata with no data: a recycle that failed after writing it
            // and could not clean up. Listed, so it can be deleted.
            Err(e) if e.kind() == io::ErrorKind::NotFound => (0, false, false),
            Err(e) => return Err(e),
        };

        Ok(RecycleEntry {
            id: id.to_string(),
            // `Some` because this function only returns at all when the
            // metadata parsed; the `None` case is built by `list`.
            original_path: Some(original_path),
            recycled_at: Some(recycled_at),
            size,
            is_dir,
            is_link,
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
        clippy::panic,
        clippy::arithmetic_side_effects
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

        let bin = RecycleBin::new(bin_root);

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

        let bin = RecycleBin::new(bin_root);
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

        let bin = RecycleBin::new(bin_root);
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

        let bin = RecycleBin::new(bin_root);
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
        let bin = RecycleBin::new(dir.join("bin"));

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
        let bin = RecycleBin::new(dir.join("bin"));

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

        let bin = RecycleBin::new(bin_root);

        write_file(&dir.join("a.txt"), "aaa");
        write_file(&dir.join("b.txt"), "bbb");

        bin.recycle(&dir.join("a.txt")).unwrap();
        bin.recycle(&dir.join("b.txt")).unwrap();

        assert_eq!(bin.list().unwrap().len(), 2);

        let emptied = bin.empty().unwrap();
        assert_eq!(emptied.deleted, 2);
        assert!(emptied.failed.is_empty(), "{:?}", emptied.failed);
        assert_eq!(bin.list().unwrap().len(), 0);
    }

    #[test]
    fn recycle_bin_list_empty() {
        let scratch = temp_dir("recycle_list_empty");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"));
        let entries = bin.list().unwrap();
        assert!(entries.is_empty());
    }

    /// Recycle `name` (holding `content`) from `dir` into `bin`, recorded as
    /// recycled `days_ago` days before now. Returns its entry id.
    fn recycled_days_ago(
        bin: &RecycleBin,
        dir: &Path,
        name: &str,
        content: &str,
        days_ago: u64,
    ) -> String {
        let path = dir.join(name);
        write_file(&path, content);
        let id = bin.recycle(&path).unwrap();
        let when = SystemTime::now()
            .checked_sub(Duration::from_hours(days_ago * 24))
            .unwrap()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        set_recycled_at(bin, &id, when);
        id
    }

    /// Record entry `id` as recycled `secs` after the epoch: the third line
    /// of its `meta.txt`.
    fn set_recycled_at(bin: &RecycleBin, id: &str, secs: u64) {
        let meta = bin.root().join(id).join("meta.txt");
        let text = fs::read_to_string(&meta).unwrap();
        let mut lines: Vec<&str> = text.lines().collect();
        let stamp = secs.to_string();
        lines[2] = &stamp;
        fs::write(&meta, format!("{}\n", lines.join("\n"))).unwrap();
    }

    fn names(bin: &RecycleBin) -> Vec<String> {
        let mut names: Vec<String> = bin
            .list()
            .unwrap()
            .iter()
            .map(RecycleEntry::display_name)
            .collect();
        names.sort();
        names
    }

    /// **What was deleted longer ago than the age limit goes; the rest stays.**
    /// The bin said it kept thirty days and nothing ever applied it:
    /// `purge_old` had no caller.
    #[test]
    fn an_entry_past_the_age_limit_is_pruned_and_a_newer_one_kept() {
        let scratch = temp_dir("prune_age");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"));
        recycled_days_ago(&bin, &dir, "old.txt", "twelve bytes", 10);
        recycled_days_ago(&bin, &dir, "new.txt", "new", 2);

        let week = Limits {
            max_age: Some(Duration::from_hours(7 * 24)),
            ..Limits::NONE
        };
        let pruned = bin.prune(&week, SystemTime::now()).unwrap();
        assert_eq!(pruned.deleted, 1);
        assert_eq!(pruned.freed, 12, "what the old file held");
        assert!(pruned.failed.is_empty());
        assert_eq!(names(&bin), ["new.txt"]);

        // No limit, nothing goes.
        let pruned = bin.prune(&Limits::NONE, SystemTime::now()).unwrap();
        assert_eq!(pruned.deleted, 0);
        assert_eq!(names(&bin), ["new.txt"]);
    }

    /// **Over its number or its size, a bin loses its oldest first** -- and
    /// only as many as bring it within the limit.
    #[test]
    fn a_bin_over_its_number_or_size_loses_its_oldest_first() {
        let scratch = temp_dir("prune_count");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"));
        for (name, days) in [
            ("a.txt", 5),
            ("b.txt", 4),
            ("c.txt", 3),
            ("d.txt", 2),
            ("e.txt", 1),
        ] {
            recycled_days_ago(&bin, &dir, name, "x", days);
        }
        let two = Limits {
            max_items: Some(2),
            ..Limits::NONE
        };
        let pruned = bin.prune(&two, SystemTime::now()).unwrap();
        assert_eq!(pruned.deleted, 3);
        assert_eq!(
            names(&bin),
            ["d.txt", "e.txt"],
            "the newest were not the ones kept"
        );

        let scratch = temp_dir("prune_size");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"));
        recycled_days_ago(&bin, &dir, "small.bin", &"s".repeat(10_000), 3);
        recycled_days_ago(&bin, &dir, "middle.bin", &"m".repeat(20_000), 2);
        recycled_days_ago(&bin, &dir, "large.bin", &"l".repeat(30_000), 1);
        let under = Limits {
            max_bytes: Some(45_000),
            ..Limits::NONE
        };
        let pruned = bin.prune(&under, SystemTime::now()).unwrap();
        assert_eq!(
            pruned.deleted, 2,
            "the bin was brought further down than its limit"
        );
        assert!(pruned.freed >= 30_000, "{}", pruned.freed);
        assert_eq!(names(&bin), ["large.bin"]);
        assert!(bin.usage().unwrap().bytes <= 45_000);
    }

    /// A folder in the bin counts at its whole size against a size limit:
    /// its entry says 0 (`RecycleEntry::size`), which would never take it.
    #[test]
    fn a_folder_counts_whole_against_a_size_limit() {
        let scratch = temp_dir("prune_folder");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"));
        write_file(&dir.join("photos/one.jpg"), &"p".repeat(40_000));
        let id = bin.recycle(&dir.join("photos")).unwrap();
        recycled_days_ago(&bin, &dir, "note.txt", "n", 0);
        // The folder is older than the note.
        set_recycled_at(&bin, &id, 1_000_000_000);
        assert_eq!(bin.usage().unwrap().items, 2);
        assert!(bin.usage().unwrap().bytes >= 40_000);
        let pruned = bin
            .prune(
                &Limits {
                    max_bytes: Some(10_000),
                    ..Limits::NONE
                },
                SystemTime::now(),
            )
            .unwrap();
        assert_eq!(pruned.deleted, 1);
        assert_eq!(names(&bin), ["note.txt"]);
    }

    /// An entry recorded as deleted after now -- a clock set back -- has no
    /// age, so no age limit takes it.
    #[test]
    fn an_entry_from_the_future_is_not_old() {
        let scratch = temp_dir("prune_future");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"));
        recycled_days_ago(&bin, &dir, "soon.txt", "x", 0);
        let day = Limits {
            max_age: Some(Duration::from_hours(24)),
            ..Limits::NONE
        };
        let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert_eq!(bin.prune(&day, long_ago).unwrap().deleted, 0);
        assert_eq!(names(&bin), ["soon.txt"]);
    }

    /// **A bin's own limits are read from it; without them, the default.**
    /// One that cannot be read keeps everything: the drive's own may have
    /// been looser than the default, and the default would then delete what
    /// the user chose to keep.
    #[test]
    fn a_bins_limits_are_its_own_or_the_default() {
        let scratch = temp_dir("bin_limits");
        let bin = RecycleBin::new(scratch.dir().join("bin"));
        let default = Limits::default();
        assert_eq!(bin.limits(&default), default, "no file of its own");

        let year = Limits {
            max_age: Some(Duration::from_hours(365 * 24)),
            max_bytes: None,
            max_items: Some(5000),
        };
        bin.set_limits(Some(&year)).unwrap();
        assert_eq!(bin.limits(&default), year);
        let written = fs::read_to_string(bin.root().join("limits.yaml")).unwrap();
        assert!(written.starts_with("# How long"), "{written}");

        // A user's comment survives a change.
        fs::write(
            bin.root().join("limits.yaml"),
            format!("# Mine.\n{written}"),
        )
        .unwrap();
        let month = Limits {
            max_age: Some(Duration::from_hours(30 * 24)),
            ..year
        };
        bin.set_limits(Some(&month)).unwrap();
        assert!(
            fs::read_to_string(bin.root().join("limits.yaml"))
                .unwrap()
                .starts_with("# Mine.\n")
        );
        assert_eq!(bin.limits(&default), month);

        // A file too long to be one keeps everything.
        fs::write(bin.root().join("limits.yaml"), "#".repeat(LIMITS_MAX + 1)).unwrap();
        assert_eq!(bin.limits(&default), Limits::NONE);

        // Taken away: the default again.
        bin.set_limits(None).unwrap();
        assert_eq!(bin.limits(&default), default);
        bin.set_limits(None).unwrap();
        assert!(
            bin.list().unwrap().is_empty(),
            "the limits file was listed as an entry"
        );
    }

    #[test]
    fn a_recycled_non_ascii_name_restores_to_its_original_path() {
        let scratch = temp_dir("recycle_nonascii");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"));

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

        let bin = RecycleBin::new(bin_root);
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
        let bin = RecycleBin::new(dir.join("bin"));

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
        let bin = RecycleBin::new(root.join("bin"));
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
    fn a_damaged_entry_is_not_pruned_by_any_limit() {
        let scratch = temp_dir("recycle_damaged_purge");
        let bin = RecycleBin::new(scratch.dir().join("bin"));
        let file = scratch.dir().join("notes.txt");
        write_file(&file, "some bytes");
        let id = bin.recycle(&file).expect("recycle");
        fs::write(bin.root().join(&id).join("meta.txt"), "not a meta file").expect("corrupt");

        let keep_nothing = Limits {
            max_age: Some(Duration::ZERO),
            max_bytes: Some(1),
            max_items: Some(1),
        };
        let pruned = bin
            .prune(&keep_nothing, SystemTime::now() + Duration::from_secs(1))
            .expect("prune");
        assert_eq!(pruned.deleted, 0);
        assert_eq!(
            bin.list().expect("list").len(),
            1,
            "a damaged entry was pruned on an age nobody knows"
        );
    }

    /// A recycled file's folder in the bin is named in plain characters: a
    /// byte that is not text, or a character some system takes for a
    /// separator, becomes `_`. The name itself is kept in `meta.txt`.
    #[test]
    fn a_recycle_bin_folder_is_named_in_plain_characters() {
        let bin = RecycleBin::new(std::env::temp_dir().join("slateos-unused-bin"));
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

    // == Deleting one entry, and ids (2026-09-27) ===============================

    #[test]
    fn deleting_one_entry_leaves_the_rest() {
        let scratch = temp_dir("recycle_delete_one");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"));
        write_file(&dir.join("a.txt"), "aaa");
        write_file(&dir.join("b.txt"), "bbb");
        let a = bin.recycle(&dir.join("a.txt")).unwrap();
        let b = bin.recycle(&dir.join("b.txt")).unwrap();

        bin.delete(&a).unwrap();

        let left: Vec<String> = bin.list().unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(left, [b]);
        assert!(
            !bin.root().join(&a).exists(),
            "the entry's folder is still there"
        );
        assert!(!dir.join("a.txt").exists(), "deleting it put it back");
    }

    #[test]
    fn a_damaged_entry_can_be_deleted_on_its_own() {
        let scratch = temp_dir("recycle_delete_damaged");
        let (bin, id) = bin_with_a_damaged_entry(scratch.dir());
        bin.delete(&id).unwrap();
        assert!(bin.list().unwrap().is_empty());
    }

    #[test]
    fn deleting_an_entry_that_is_already_gone_is_not_an_error() {
        let scratch = temp_dir("recycle_delete_gone");
        let bin = RecycleBin::new(scratch.dir().join("bin"));
        bin.delete("nothing_0000000000000000").unwrap();
    }

    /// An id is one name inside the bin. Anything else -- above all a path
    /// out of it -- is refused before it reaches the filesystem.
    #[test]
    fn an_id_that_is_not_one_name_is_refused() {
        let scratch = temp_dir("recycle_bad_id");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"));
        write_file(&dir.join("keep.txt"), "not in the bin");
        fs::create_dir_all(dir.join("victim")).unwrap();
        write_file(&dir.join("victim").join("inside.txt"), "not in the bin");
        write_file(&dir.join("a.txt"), "aaa");
        bin.recycle(&dir.join("a.txt")).unwrap();

        for id in [
            "../victim",
            "..",
            ".",
            "",
            "a/b",
            "../keep.txt",
            "/tmp",
            "sub/../x",
        ] {
            let deleted = bin.delete(id);
            assert_eq!(
                deleted.as_ref().map_err(io::Error::kind),
                Err(io::ErrorKind::InvalidInput),
                "delete({id:?})"
            );
            let restored = bin.restore(id);
            assert_eq!(
                restored.as_ref().map_err(io::Error::kind),
                Err(io::ErrorKind::InvalidInput),
                "restore({id:?})"
            );
        }
        assert_eq!(
            read_file(&dir.join("victim").join("inside.txt")),
            "not in the bin"
        );
        assert_eq!(read_file(&dir.join("keep.txt")), "not in the bin");
        assert_eq!(
            bin.list().unwrap().len(),
            1,
            "a refused id touched the real entry"
        );
    }

    /// Emptying says which entries it could not delete; they stay listed.
    #[test]
    fn emptying_reports_what_it_could_not_delete() {
        let scratch = temp_dir("recycle_empty_fails");
        let dir = scratch.dir().to_path_buf();
        let bin = RecycleBin::new(dir.join("bin"));
        write_file(&dir.join("held.txt"), "held open");
        write_file(&dir.join("free.txt"), "free");
        let held = bin.recycle(&dir.join("held.txt")).unwrap();
        bin.recycle(&dir.join("free.txt")).unwrap();

        // Something that stops the one entry being removed: on Windows a
        // handle that shares nothing, on Unix a folder that may not be
        // written. (A Unix superuser is not stopped by the second, which the
        // assertions below allow for.)
        #[cfg(windows)]
        let _lock = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(bin.root().join(&held).join("data"))
                .unwrap()
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(bin.root().join(&held), fs::Permissions::from_mode(0o500)).unwrap();
        }

        let emptied = bin.empty().unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Put back so the scratch folder can be cleared.
            let _ = fs::set_permissions(bin.root().join(&held), fs::Permissions::from_mode(0o700));
        }
        let still_there = bin.root().join(&held).exists();
        assert_eq!(emptied.deleted, if still_there { 1 } else { 2 });
        assert_eq!(emptied.failed.len(), usize::from(still_there));
        if let Some((entry, _)) = emptied.failed.first() {
            assert_eq!(entry.id, held, "blamed the wrong entry");
        }
        #[cfg(windows)]
        assert!(
            still_there,
            "the lock did not hold, so this checked nothing"
        );
    }

    /// A recycled link is listed as a link, and not as what it names.
    #[test]
    fn a_recycled_link_is_listed_as_a_link() {
        let scratch = temp_dir("recycle_link_listed");
        let root = scratch.dir().to_path_buf();
        fs::create_dir_all(root.join("outside")).unwrap();
        write_file(&root.join("outside").join("big.txt"), &"x".repeat(1000));
        if folder_link(&root.join("outside"), &root.join("shortcut")).is_none() {
            eprintln!("skipped: this host can make no link");
            return;
        }
        let bin = RecycleBin::new(root.join("bin"));
        bin.recycle(&root.join("shortcut")).unwrap();

        let listed = bin.list().unwrap();
        let entry = listed.first().expect("one entry");
        assert!(entry.is_link, "{entry:?}");
        assert!(!entry.is_dir, "{entry:?}");
        assert_eq!(entry.size, 0, "{entry:?}");
        assert_eq!(read_file(&root.join("outside").join("big.txt")).len(), 1000);
    }

    /// A link planted in the bin's folder is not an entry, so neither a
    /// delete nor an empty can reach through it.
    #[test]
    fn a_link_in_the_bin_folder_is_not_an_entry() {
        let scratch = temp_dir("recycle_planted_link");
        let root = scratch.dir().to_path_buf();
        fs::create_dir_all(root.join("outside")).unwrap();
        write_file(
            &root.join("outside").join("meta.txt"),
            "slate-recycle-v2\n/x\n5\n",
        );
        write_file(&root.join("outside").join("keep.txt"), "not in the bin");
        let bin = RecycleBin::new(root.join("bin"));
        fs::create_dir_all(bin.root()).unwrap();
        if folder_link(&root.join("outside"), &bin.root().join("planted")).is_none() {
            eprintln!("skipped: this host can make no link");
            return;
        }

        assert!(
            bin.list().unwrap().is_empty(),
            "the link was listed as an entry"
        );
        let emptied = bin.empty().unwrap();
        assert_eq!(emptied.deleted, 0);
        assert_eq!(
            read_file(&root.join("outside").join("keep.txt")),
            "not in the bin"
        );
    }

    /// A damaged entry's size counts what is in the bin, never what a link
    /// inside it names.
    #[test]
    fn a_damaged_entry_is_measured_without_following_links() {
        let scratch = temp_dir("recycle_damaged_link_size");
        let root = scratch.dir().to_path_buf();
        fs::create_dir_all(root.join("outside")).unwrap();
        write_file(&root.join("outside").join("big.txt"), &"x".repeat(100_000));
        fs::create_dir_all(root.join("folder")).unwrap();
        write_file(&root.join("folder").join("small.txt"), "12345");
        if folder_link(&root.join("outside"), &root.join("folder").join("link")).is_none() {
            eprintln!("skipped: this host can make no link");
            return;
        }
        let bin = RecycleBin::new(root.join("bin"));
        let id = bin.recycle(&root.join("folder")).unwrap();
        fs::write(bin.root().join(&id).join("meta.txt"), "damaged").unwrap();

        let listed = bin.list().unwrap();
        let entry = listed.first().expect("one entry");
        assert!(!entry.is_readable());
        assert!(
            entry.size < 100_000,
            "measured through the link: {}",
            entry.size
        );
        assert!(
            entry.size >= 5,
            "missed the file that is in the bin: {}",
            entry.size
        );
    }
}
