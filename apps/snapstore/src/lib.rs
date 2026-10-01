//! snapstore -- a content-addressed store of directory snapshots.
//!
//! What the backup tool (`apps/backup`) and System Restore
//! (`apps/systemrestore`) both keep their copies in. It began as the backup
//! tool's engine and moved here on 2026-09-27, when System Restore stopped
//! drawing invented restore points and needed real ones: one store, one set of
//! guarantees, rather than a second copy of them.
//!
//! # On disk
//!
//! The layout the backup tool has always written, so every existing store
//! still reads:
//!
//! ```text
//! <root>/cas/<2 hex>/<62 hex>        each file's content once, named by its SHA-256
//! <root>/cas/staging/                copies on their way in
//! <root>/backups/<id>/manifest.json  what the snapshot holds (see `manifest`)
//! <root>/backups/<id>/meta.json      the record, written last: a snapshot
//!                                    without one did not finish and is not listed
//! ```
//!
//! # What it promises
//!
//! - **A blob's name is its content's hash.** Content goes in under the hash
//!   of the bytes actually stored, so a file that changes while it is copied
//!   cannot put content under another content's name (`store::ContentStore::ingest`).
//! - **A snapshot says what it could not read.** A file that could not be
//!   read is recorded as such, not silently left out -- and a restore that
//!   makes a folder match a snapshot never removes it.
//! - **Every snapshot lists every file** (manifest version 3). Incremental
//!   and differential snapshots differ only in whose hashes they reused for
//!   files that had not changed, so restoring one needs nothing else, and a
//!   file deleted before it stays deleted.
//! - **Nothing is written outside the folder being restored,** by a `..` in
//!   a manifest or by a link met on the way down.
//! - **Collection never deletes what something may still need:** it refuses
//!   while any snapshot's records cannot be read, and leaves blobs younger
//!   than its grace period, which a capture in progress may be about to name.

mod glob;
pub mod json;
pub mod manifest;
pub mod retention;
pub mod scan;
pub mod store;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pathtext::ShowPath;

pub use glob::{glob_match_recursive, is_excluded};
pub use manifest::{BackupMeta, BackupType, FileEntry, Manifest};
pub use store::{ContentStore, sha256_file, sha256_hex};

/// A store of snapshots, at a directory.
pub struct Store {
    root: PathBuf,
    blobs: ContentStore,
}

/// How to take a snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureOptions {
    /// Whose hashes to reuse for unchanged files: see [`BackupType`].
    pub kind: BackupType,
    /// Patterns for what to leave out.
    pub excludes: Vec<String>,
    /// Walk into the directories links point at.
    pub follow_symlinks: bool,
}

/// How far a capture has got.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Files and links found.
    pub total_files: u64,
    /// Files and links done.
    pub processed_files: u64,
    /// Bytes found.
    pub total_bytes: u64,
    /// Bytes done.
    pub processed_bytes: u64,
    /// The one being done.
    pub current: PathBuf,
}

/// What a capture came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capture {
    /// The record written beside it.
    pub meta: BackupMeta,
    /// What could not be read, and so is not in it, with why.
    pub unread: Vec<(PathBuf, String)>,
}

/// How to restore.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RestoreOptions {
    /// Only the files whose path matches this pattern.
    pub filter: Option<String>,
    /// Make the folder match the snapshot: also remove files and links the
    /// snapshot does not have (never what it could not read, or excluded),
    /// and folders it did not have that are left empty.
    pub mirror: bool,
}

/// What a restore did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RestoreReport {
    /// Files and links written.
    pub written: u64,
    /// Files already as the snapshot has them, left alone.
    pub unchanged: u64,
    /// Files, links and empty folders removed (mirror only).
    pub removed: u64,
    /// What could not be done, by path, with why.
    pub errors: Vec<(PathBuf, String)>,
}

/// What checking a snapshot's blobs found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VerifyReport {
    /// Entries whose content is present and whole.
    pub ok: u64,
    /// Entries whose blob is gone, by path.
    pub missing: Vec<PathBuf>,
    /// Entries whose blob no longer hashes to its name, or will not read.
    pub corrupt: Vec<(PathBuf, String)>,
}

/// The snapshots in a store, and the ones whose records will not read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// Oldest first.
    pub snapshots: Vec<BackupMeta>,
    /// Directories holding a record that could not be read, with why. Not
    /// snapshots anyone can restore -- but their blobs are not orphans
    /// either, which is why collection refuses while there are any.
    pub unreadable: Vec<(PathBuf, String)>,
}

/// Result of comparing two file lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffResult {
    /// In the newer, not the older.
    pub added: Vec<FileEntry>,
    /// In both, with different content: `(older, newer)`.
    pub modified: Vec<(FileEntry, FileEntry)>,
    /// In the older, not the newer.
    pub deleted: Vec<FileEntry>,
}

impl DiffResult {
    /// Whether nothing differs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
}

/// Compare two complete file lists.
///
/// A path present in both whose hash differs is a modification. Size and time
/// are not consulted: the hash is authoritative, and a file can be rewritten
/// with the same size and a preserved time.
#[must_use]
pub fn diff(older: &[FileEntry], newer: &[FileEntry]) -> DiffResult {
    let old_map: BTreeMap<&Path, &FileEntry> =
        older.iter().map(|f| (f.path.as_path(), f)).collect();
    let new_map: BTreeMap<&Path, &FileEntry> =
        newer.iter().map(|f| (f.path.as_path(), f)).collect();
    let mut out = DiffResult::default();
    for entry in newer {
        match old_map.get(entry.path.as_path()) {
            None => out.added.push(entry.clone()),
            Some(old) if old.hash != entry.hash => {
                out.modified.push(((*old).clone(), entry.clone()));
            }
            Some(_) => {}
        }
    }
    for entry in older {
        if !new_map.contains_key(entry.path.as_path()) {
            out.deleted.push(entry.clone());
        }
    }
    out
}

/// Join a manifest's path onto the restore destination, refusing anything
/// that would land outside it.
///
/// `Path::join` **replaces** its base when given an absolute path, so an
/// absolute entry would redirect a restore away from the folder the user
/// named and onto that path; a `..` escapes upward the same way. A manifest
/// is parsed data -- hand-editable, corruptible, perhaps someone else's -- so
/// it is checked where it is used. Refused rather than sanitised: quietly
/// relocating a file puts the user's data somewhere they did not ask for.
#[must_use]
pub fn restore_path_within(dest_root: &Path, rel: &Path) -> Option<PathBuf> {
    let mut out = dest_root.to_path_buf();
    let mut pushed = false;
    for comp in rel.components() {
        match comp {
            Component::Normal(part) => {
                out.push(part);
                pushed = true;
            }
            Component::CurDir => {}
            // `RootDir`/`Prefix` are how an absolute path presents itself;
            // `ParentDir` is the classic archive traversal.
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    // An empty or `.`-only path names the destination itself, which is not a
    // file a restore can write.
    pushed.then_some(out)
}

/// Whether `id` can name a snapshot: one path component, no separators.
///
/// Ids come from the command line and from other programs' records; joining
/// one onto the store's path unchecked would let `../../etc` name a directory
/// outside it.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && !id.bytes().any(|b| b == b'/' || b == b'\\' || b == 0)
}

fn bad_id(id: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{id:?} is not a snapshot id"),
    )
}

/// Seconds since the epoch, now.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Store {
    /// The store at `root`, creating its directories if they are not there.
    ///
    /// # Errors
    ///
    /// The directories cannot be created.
    pub fn open(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root.join("backups"))?;
        fs::create_dir_all(root.join("cas"))?;
        Ok(Self::at(root))
    }

    /// The store at `root`, without creating anything: for reading one that
    /// may not exist, which then simply holds nothing.
    #[must_use]
    pub fn at(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            blobs: ContentStore::new(root),
        }
    }

    /// Where the store is.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Its blobs.
    #[must_use]
    pub fn blobs(&self) -> &ContentStore {
        &self.blobs
    }

    fn snapshots_dir(&self) -> PathBuf {
        self.root.join("backups")
    }

    fn snapshot_dir(&self, id: &str) -> io::Result<PathBuf> {
        if valid_id(id) {
            Ok(self.snapshots_dir().join(id))
        } else {
            Err(bad_id(id))
        }
    }

    /// Every finished snapshot, oldest first -- and the directories whose
    /// records will not read.
    ///
    /// # Errors
    ///
    /// The store's directory exists and cannot be listed.
    pub fn list(&self) -> io::Result<Listing> {
        let mut listing = Listing::default();
        let dir = self.snapshots_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(listing),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = PathBuf::from(entry.file_name());
            let meta_path = entry.path().join("meta.json");
            match fs::read_to_string(&meta_path) {
                Ok(text) => match BackupMeta::deserialize(&text) {
                    Ok(meta) => listing.snapshots.push(meta),
                    Err(e) => listing.unreadable.push((name, e)),
                },
                // No record: a capture that did not finish, or one still
                // running. Not a snapshot, and not an unreadable one either.
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => listing.unreadable.push((name, e.to_string())),
            }
        }
        // Ties broken by id, so two snapshots of one second list in the same
        // order every time.
        listing
            .snapshots
            .sort_by(|a, b| (a.timestamp, &a.id).cmp(&(b.timestamp, &b.id)));
        Ok(listing)
    }

    /// One snapshot's record.
    ///
    /// # Errors
    ///
    /// No such snapshot, or its record will not read.
    pub fn meta(&self, id: &str) -> io::Result<BackupMeta> {
        let text = fs::read_to_string(self.snapshot_dir(id)?.join("meta.json"))?;
        BackupMeta::deserialize(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// One snapshot's manifest, as written.
    ///
    /// # Errors
    ///
    /// No such snapshot, or its manifest will not read.
    pub fn manifest(&self, id: &str) -> io::Result<Manifest> {
        let text = fs::read_to_string(self.snapshot_dir(id)?.join("manifest.json"))?;
        let mut manifest = Manifest::deserialize(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        // Before version 3 a full snapshot's manifest was complete and said
        // nothing about it.
        if !manifest.complete && self.meta(id)?.backup_type == BackupType::Full {
            manifest.complete = true;
        }
        Ok(manifest)
    }

    /// Every file in a snapshot.
    ///
    /// From manifest version 3, and for any full snapshot, that is its
    /// manifest. An incremental or differential one from before held only
    /// what changed, and is rebuilt by walking back to a full one -- which
    /// cannot know what was deleted in between: such a snapshot's list holds
    /// every file any link of its chain held.
    ///
    /// # Errors
    ///
    /// A manifest or record of the chain will not read, or a link is gone.
    pub fn files(&self, id: &str) -> io::Result<Vec<FileEntry>> {
        let manifest = self.manifest(id)?;
        if manifest.complete {
            return Ok(manifest.files);
        }
        let mut chain = vec![manifest];
        let mut seen = HashSet::from([id.to_string()]);
        let mut meta = self.meta(id)?;
        while let Some(parent) = meta.parent_id.clone() {
            if !seen.insert(parent.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("snapshot {id}'s chain loops back on {parent}"),
                ));
            }
            let parent_manifest = self.manifest(&parent)?;
            let done = parent_manifest.complete;
            chain.push(parent_manifest);
            if done {
                break;
            }
            meta = self.meta(&parent)?;
        }
        let mut files: BTreeMap<PathBuf, FileEntry> = BTreeMap::new();
        for m in chain.iter().rev() {
            for entry in &m.files {
                files.insert(entry.path.clone(), entry.clone());
            }
        }
        Ok(files.into_values().collect())
    }

    /// The newest snapshot of `source` of the kind `kind` wants reused.
    fn parent_for(&self, source: &Path, kind: BackupType) -> io::Result<Option<BackupMeta>> {
        let want_full = match kind {
            BackupType::Full => return Ok(None),
            BackupType::Incremental => false,
            BackupType::Differential => true,
        };
        // Of the same source: this took the newest snapshot of *any* source,
        // so a store holding two folders compared one with the other.
        Ok(self
            .list()?
            .snapshots
            .into_iter()
            .rev()
            .find(|m| m.source == source && (!want_full || m.backup_type == BackupType::Full)))
    }

    /// Claim a fresh snapshot directory, named for the time and the kind.
    ///
    /// Ids were `<seconds>-<kind>` and nothing checked that one was free, so
    /// two snapshots in one second -- two folders captured together, which
    /// System Restore does every time -- wrote into one directory, and the
    /// second's manifest replaced the first's. `create_dir` claims a name
    /// atomically; a name that is taken gets a counter.
    fn claim_dir(&self, timestamp: u64, kind: BackupType) -> io::Result<(String, PathBuf)> {
        let base = format!("{timestamp}-{}", kind.as_str());
        for n in 1u32..=10_000 {
            let id = if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            };
            let dir = self.snapshots_dir().join(&id);
            match fs::create_dir(&dir) {
                Ok(()) => return Ok((id, dir)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other(format!(
            "ten thousand snapshots named {base} already exist"
        )))
    }

    /// Take a snapshot of `source`.
    ///
    /// Everything under it goes in except what `opts.excludes` match; what is
    /// there and cannot be read is recorded in the snapshot as unread and
    /// returned, rather than dropped with a warning. A file unchanged since
    /// before the parent snapshot was taken -- same size, time and mode -- has
    /// its hash taken from the parent rather than read again; one changed
    /// *during* the parent's second is read, because a time with one-second
    /// grain cannot say whether the parent saw the change.
    ///
    /// # Errors
    ///
    /// The source cannot be resolved, or the store cannot be written. A file
    /// that cannot be read is not an error: it is in the result's `unread`.
    pub fn capture(
        &self,
        source: &Path,
        opts: &CaptureOptions,
        progress: &mut dyn FnMut(&Progress),
    ) -> io::Result<Capture> {
        let source = source
            .canonicalize()
            .map_err(|e| io::Error::new(e.kind(), format!("source {}: {e}", source.shown())))?;
        fs::create_dir_all(self.snapshots_dir())?;
        // The snapshot is of the folder as the capture *began* to read it,
        // and is dated so: the reuse rule below compares a file's time with
        // this, and a date taken at the end would let a file changed while
        // the capture ran pass for one unchanged before it.
        let started = now_secs();

        let parent = self.parent_for(&source, opts.kind)?;
        let reusable: HashMap<PathBuf, FileEntry> = match &parent {
            Some(meta) => self
                .files(&meta.id)?
                .into_iter()
                .filter(|f| !f.is_symlink && f.mtime < meta.timestamp)
                .map(|f| (f.path.clone(), f))
                .collect(),
            None => HashMap::new(),
        };
        let effective = if parent.is_some() {
            opts.kind
        } else {
            BackupType::Full
        };

        let walk = scan::walk(&source, &opts.excludes, opts.follow_symlinks);
        let mut unread = walk.unread;
        let mut p = Progress {
            total_files: u64::try_from(walk.files.len()).unwrap_or(u64::MAX),
            total_bytes: walk.files.iter().map(|f| f.size).sum(),
            ..Progress::default()
        };
        progress(&p);

        let mut manifest = Manifest {
            dirs: walk.dirs,
            excluded: opts.excludes.clone(),
            complete: true,
            dirs_recorded: true,
            ..Manifest::default()
        };
        let (mut new_blobs, mut dedup_blobs, mut total_size) = (0u64, 0u64, 0u64);
        for found in walk.files {
            p.current.clone_from(&found.rel);
            p.processed_files = p.processed_files.saturating_add(1);
            p.processed_bytes = p.processed_bytes.saturating_add(found.size);
            if let Some(target) = found.link_target {
                // A link's target is carried in the manifest, not as a blob.
                let hash = sha256_hex(target.as_os_str().as_encoded_bytes());
                manifest.files.push(FileEntry {
                    path: found.rel,
                    size: 0,
                    mtime: 0,
                    hash,
                    is_symlink: true,
                    link_target: Some(target),
                    mode: None,
                });
                progress(&p);
                continue;
            }
            let reused = reusable.get(&found.rel).filter(|prev| {
                prev.size == found.size
                    && prev.mtime == found.mtime
                    && prev.mode == found.mode
                    && self.blobs.has_blob(&prev.hash)
            });
            let entry = if let Some(prev) = reused {
                dedup_blobs = dedup_blobs.saturating_add(1);
                prev.clone()
            } else {
                let stored =
                    sha256_file(&found.full).and_then(|hash| self.blobs.ingest(&found.full, &hash));
                match stored {
                    Ok(stored) => {
                        if stored.new {
                            new_blobs = new_blobs.saturating_add(1);
                        } else {
                            dedup_blobs = dedup_blobs.saturating_add(1);
                        }
                        FileEntry {
                            path: found.rel,
                            size: stored.size,
                            mtime: found.mtime,
                            hash: stored.hash,
                            is_symlink: false,
                            link_target: None,
                            mode: found.mode,
                        }
                    }
                    Err(e) => {
                        unread.push((found.rel, e.to_string()));
                        progress(&p);
                        continue;
                    }
                }
            };
            total_size = total_size.saturating_add(entry.size);
            manifest.files.push(entry);
            progress(&p);
        }
        manifest.unread.clone_from(&unread);

        let timestamp = started;
        let (id, dir) = self.claim_dir(timestamp, effective)?;
        // The order is load-bearing: `list` counts a directory as a snapshot
        // only once `meta.json` is there and reads, so the record is written
        // last and marks the snapshot complete. Both through `safeio`, so
        // neither is ever seen half-written.
        safeio::write_str_atomically(&dir.join("manifest.json"), &manifest.serialize())?;
        let meta = BackupMeta {
            id,
            backup_type: effective,
            timestamp,
            source,
            parent_id: parent.map(|m| m.id),
            file_count: u64::try_from(manifest.files.len()).unwrap_or(u64::MAX),
            total_size,
            new_blobs,
            dedup_blobs,
            unread_count: u64::try_from(unread.len()).unwrap_or(u64::MAX),
        };
        safeio::write_str_atomically(&dir.join("meta.json"), &meta.serialize())?;
        Ok(Capture { meta, unread })
    }

    /// Restore a snapshot into `dest`.
    ///
    /// Each file is written only if it differs from what is there -- content
    /// or mode -- and through `safeio`, because a restore usually writes over
    /// a file the user still has, and a truncating write interrupted would
    /// lose both. A file the snapshot does not name is left alone unless
    /// `opts.mirror`; see [`RestoreOptions::mirror`] for what that removes and
    /// never removes.
    ///
    /// # Errors
    ///
    /// The snapshot cannot be read, or a mirror restore is asked of a snapshot
    /// that does not list every file. Individual files that cannot be written
    /// are not errors of the call: they are in the report.
    pub fn restore(
        &self,
        id: &str,
        dest: &Path,
        opts: &RestoreOptions,
    ) -> io::Result<RestoreReport> {
        let manifest = self.manifest(id)?;
        if opts.mirror && opts.filter.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a restore that makes the folder match the snapshot restores all of it, \
                 so it takes no filter",
            ));
        }
        if opts.mirror && !manifest.complete {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "snapshot {id} lists only what changed since the one before it, so it \
                     cannot say what the folder held: it cannot be mirrored"
                ),
            ));
        }
        let files = if manifest.complete {
            manifest.files.clone()
        } else {
            self.files(id)?
        };
        let wanted: Vec<&FileEntry> = files
            .iter()
            .filter(|f| {
                opts.filter.as_ref().is_none_or(|pattern| {
                    glob_match_recursive(pattern.as_bytes(), f.path.as_os_str().as_encoded_bytes())
                })
            })
            .collect();

        let mut report = RestoreReport::default();
        fs::create_dir_all(dest)?;
        if opts.mirror {
            self.remove_what_the_snapshot_lacks(dest, &manifest, &mut report);
        }
        for entry in wanted {
            self.put_back(dest, entry, &mut report);
        }
        if opts.mirror {
            for dir in &manifest.dirs {
                match restore_path_within(dest, dir) {
                    Some(path) => {
                        if let Err(e) = fs::create_dir_all(&path) {
                            report.errors.push((dir.clone(), e.to_string()));
                        }
                    }
                    None => report
                        .errors
                        .push((dir.clone(), "names a place outside the folder".to_string())),
                }
            }
        }
        Ok(report)
    }

    /// Write one entry back, unless it is already there as recorded.
    fn put_back(&self, dest: &Path, entry: &FileEntry, report: &mut RestoreReport) {
        let fail = |report: &mut RestoreReport, why: String| {
            report.errors.push((entry.path.clone(), why));
        };
        let Some(target) = restore_path_within(dest, &entry.path) else {
            fail(report, "names a place outside the folder".to_string());
            return;
        };
        if let Err(why) = ensure_real_parents(dest, &target) {
            fail(report, why);
            return;
        }
        let existing = fs::symlink_metadata(&target).ok();
        if entry.is_symlink {
            let Some(link) = &entry.link_target else {
                fail(report, "recorded as a link with no target".to_string());
                return;
            };
            if let Some(meta) = &existing {
                if meta.file_type().is_symlink() && fs::read_link(&target).is_ok_and(|t| &t == link)
                {
                    report.unchanged = report.unchanged.saturating_add(1);
                    return;
                }
                if let Err(e) = remove_entry(&target, meta) {
                    fail(report, format!("cannot replace what is there: {e}"));
                    return;
                }
            }
            match make_link(link, &target) {
                Ok(()) => report.written = report.written.saturating_add(1),
                Err(e) => fail(report, format!("cannot make the link: {e}")),
            }
            return;
        }
        if let Some(meta) = &existing {
            if meta.is_file()
                && meta.len() == entry.size
                && sha256_file(&target).is_ok_and(|h| h == entry.hash)
            {
                // Content as recorded; the mode may still differ.
                match apply_mode(&target, entry.mode) {
                    Ok(()) => report.unchanged = report.unchanged.saturating_add(1),
                    Err(e) => fail(report, format!("cannot set its permissions: {e}")),
                }
                return;
            }
            if !meta.is_file() {
                // A link or an empty folder where the snapshot has a file.
                // (A folder with anything left in it is refused by remove.)
                if let Err(e) = remove_entry(&target, meta) {
                    fail(report, format!("cannot replace what is there: {e}"));
                    return;
                }
            }
        }
        let data = match self.blobs.read_blob(&entry.hash) {
            Ok(data) => data,
            Err(e) => {
                fail(report, format!("its content is not in the store: {e}"));
                return;
            }
        };
        let actual = sha256_hex(&data);
        if actual != entry.hash {
            fail(
                report,
                format!("its content in the store is damaged (hash {actual})"),
            );
            return;
        }
        if let Err(e) = safeio::write_atomically(&target, &data) {
            fail(report, format!("cannot write it: {e}"));
            return;
        }
        match apply_mode(&target, entry.mode) {
            Ok(()) => report.written = report.written.saturating_add(1),
            Err(e) => fail(
                report,
                format!("written, but its permissions were not set: {e}"),
            ),
        }
    }

    /// Remove from `dest` every file and link the snapshot does not have --
    /// except what it could not read or excluded, and anything under those --
    /// and then every folder it did not have that is left empty.
    fn remove_what_the_snapshot_lacks(
        &self,
        dest: &Path,
        manifest: &Manifest,
        report: &mut RestoreReport,
    ) {
        let keep_files: HashSet<&Path> = manifest.files.iter().map(|f| f.path.as_path()).collect();
        let keep_dirs: HashSet<&Path> = manifest.dirs.iter().map(PathBuf::as_path).collect();
        let protected: Vec<&Path> = manifest.unread.iter().map(|(p, _)| p.as_path()).collect();
        let is_protected = |rel: &Path| {
            protected.iter().any(|p| rel.starts_with(p)) || is_excluded(rel, &manifest.excluded)
        };
        let mut dirs_found: BTreeSet<PathBuf> = BTreeSet::new();
        let mut stack = vec![dest.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let listing = match fs::read_dir(&dir) {
                Ok(listing) => listing,
                Err(e) => {
                    report
                        .errors
                        .push((scan::relative_path(&dir, dest), e.to_string()));
                    continue;
                }
            };
            for entry in listing {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        report
                            .errors
                            .push((scan::relative_path(&dir, dest), e.to_string()));
                        break;
                    }
                };
                let path = entry.path();
                let rel = scan::relative_path(&path, dest);
                if is_protected(&rel) {
                    continue;
                }
                let Ok(meta) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if meta.is_dir() {
                    dirs_found.insert(rel);
                    stack.push(path);
                } else if (meta.is_file() || meta.file_type().is_symlink())
                    && !keep_files.contains(rel.as_path())
                {
                    match fs::remove_file(&path) {
                        Ok(()) => report.removed = report.removed.saturating_add(1),
                        Err(e) => report.errors.push((rel, format!("cannot remove it: {e}"))),
                    }
                }
                // Devices, pipes and sockets are never removed: a snapshot
                // cannot hold them, so their absence from it says nothing.
            }
        }
        // Deepest first, so a folder emptied by removing its empty children
        // goes too. Only ever an empty one: `remove_dir` refuses anything
        // else, which is what keeps protected content where it is. Folders
        // are only known from manifest version 3 on; a snapshot that does
        // not record them removes none.
        if manifest.dirs_recorded {
            for rel in dirs_found.iter().rev() {
                if keep_dirs.contains(rel.as_path()) {
                    continue;
                }
                let path = dest.join(rel);
                if fs::remove_dir(&path).is_ok() {
                    report.removed = report.removed.saturating_add(1);
                }
            }
        }
    }

    /// Check that every file of a snapshot is in the store, whole.
    ///
    /// # Errors
    ///
    /// The snapshot cannot be read.
    pub fn verify(&self, id: &str) -> io::Result<VerifyReport> {
        let mut report = VerifyReport::default();
        for entry in self.files(id)? {
            if entry.is_symlink {
                report.ok = report.ok.saturating_add(1);
                continue;
            }
            if !self.blobs.has_blob(&entry.hash) {
                report.missing.push(entry.path);
                continue;
            }
            match self.blobs.verify_blob(&entry.hash) {
                Ok(true) => report.ok = report.ok.saturating_add(1),
                Ok(false) => report
                    .corrupt
                    .push((entry.path, format!("does not hash to {}", entry.hash))),
                Err(e) => report.corrupt.push((entry.path, e.to_string())),
            }
        }
        Ok(report)
    }

    /// Remove a snapshot's records. Its blobs stay until [`Store::collect_garbage`].
    ///
    /// # Errors
    ///
    /// The id is not one, or the directory cannot be removed.
    pub fn remove(&self, id: &str) -> io::Result<()> {
        fs::remove_dir_all(self.snapshot_dir(id)?)
    }

    /// Remove every blob no snapshot names, and staged copies a capture left.
    ///
    /// Refuses while any snapshot's record or manifest cannot be read: the
    /// blobs only it names would look like orphans and be deleted, turning an
    /// unreadable file into lost data. And leaves anything written in the last
    /// `grace`: a capture running now stores its blobs before it writes the
    /// manifest that names them.
    ///
    /// Returns how many blobs were removed.
    ///
    /// # Errors
    ///
    /// A record or manifest cannot be read, or the store cannot be listed.
    pub fn collect_garbage(&self, grace: Duration) -> io::Result<u64> {
        let listing = self.list()?;
        if let Some((dir, why)) = listing.unreadable.first() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "refusing to collect unreferenced blobs: the record of snapshot {} \
                     cannot be read ({why}), and blobs only it names would be deleted",
                    dir.shown()
                ),
            ));
        }
        let mut referenced = HashSet::new();
        for meta in &listing.snapshots {
            // Its own manifest's hashes: a legacy incremental's parents are
            // snapshots in this listing too, and are read in their turn.
            let manifest = self.manifest(&meta.id).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "refusing to collect unreferenced blobs: cannot read the manifest \
                         of snapshot {}: {e}",
                        meta.id
                    ),
                )
            })?;
            referenced.extend(manifest.files.into_iter().map(|f| f.hash));
        }
        let now = SystemTime::now();
        let old_enough =
            |written: SystemTime| now.duration_since(written).is_ok_and(|age| age >= grace);
        let mut removed = 0u64;
        for (hash, written) in self.blobs.all_blobs()? {
            if !referenced.contains(&hash) && old_enough(written) {
                self.blobs.remove_blob(&hash)?;
                removed = removed.saturating_add(1);
            }
        }
        for staged in self.blobs.stale_staging(now, grace)? {
            fs::remove_file(staged)?;
        }
        Ok(removed)
    }
}

/// Make sure every folder between `dest` and `target` is a real folder --
/// creating the missing ones -- and none is a link.
///
/// A link on the way down would carry the write out of the folder being
/// restored: `settings/` replaced by a link to somewhere else, and a restore
/// of `settings/app.conf` writes there.
fn ensure_real_parents(dest: &Path, target: &Path) -> Result<(), String> {
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    let Ok(rel) = parent.strip_prefix(dest) else {
        return Err("names a place outside the folder".to_string());
    };
    let mut here = dest.to_path_buf();
    for comp in rel.components() {
        here.push(comp);
        match fs::symlink_metadata(&here) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!(
                    "{} is a link, and a restore does not write through one",
                    scan::relative_path(&here, dest).shown()
                ));
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "{} is a file where the snapshot has a folder",
                    scan::relative_path(&here, dest).shown()
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&here).map_err(|e| format!("cannot create a folder: {e}"))?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

/// Remove a file, a link or an empty folder.
fn remove_entry(path: &Path, meta: &fs::Metadata) -> io::Result<()> {
    if meta.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

/// Make a symbolic link.
#[cfg(unix)]
fn make_link(target: &Path, at: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

/// No symbolic links here: the target is written as the file's content, so
/// the information survives, though it is no longer a link.
#[cfg(not(unix))]
fn make_link(target: &Path, at: &Path) -> io::Result<()> {
    safeio::write_atomically(at, target.as_os_str().as_encoded_bytes())
}

/// Set a restored file's permission bits, where there are any.
#[cfg(unix)]
fn apply_mode(path: &Path, mode: Option<u32>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    match mode {
        Some(mode) => fs::set_permissions(path, fs::Permissions::from_mode(mode)),
        None => Ok(()),
    }
}

/// No permission bits on this platform.
#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "one signature on every platform; on Unix setting the bits can fail"
)]
fn apply_mode(_path: &Path, _mode: Option<u32>) -> io::Result<()> {
    Ok(())
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

    // --- Change Detection Tests ---

    #[test]
    fn test_detect_no_changes() {
        let files = vec![FileEntry {
            path: PathBuf::from("a.txt"),
            size: 100,
            mtime: 1000,
            hash: "abc123".to_string(),
            is_symlink: false,
            link_target: None,
            mode: None,
        }];
        let manifest = Manifest {
            files: files.clone(),
            ..Manifest::default()
        };
        let diff = diff(&manifest.files, &files);
        assert!(diff.added.is_empty());
        assert!(diff.modified.is_empty());
        assert!(diff.deleted.is_empty());
    }

    #[test]
    fn test_detect_added() {
        let prev = Manifest {
            files: vec![FileEntry {
                path: PathBuf::from("a.txt"),
                size: 100,
                mtime: 1000,
                hash: "aaa".to_string(),
                is_symlink: false,
                link_target: None,
                mode: None,
            }],
            ..Manifest::default()
        };
        let current = vec![
            FileEntry {
                path: PathBuf::from("a.txt"),
                size: 100,
                mtime: 1000,
                hash: "aaa".to_string(),
                is_symlink: false,
                link_target: None,
                mode: None,
            },
            FileEntry {
                path: PathBuf::from("b.txt"),
                size: 200,
                mtime: 2000,
                hash: "bbb".to_string(),
                is_symlink: false,
                link_target: None,
                mode: None,
            },
        ];
        let diff = diff(&prev.files, &current);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].path, Path::new("b.txt"));
        assert!(diff.modified.is_empty());
        assert!(diff.deleted.is_empty());
    }

    #[test]
    fn test_detect_modified() {
        let prev = Manifest {
            files: vec![FileEntry {
                path: PathBuf::from("a.txt"),
                size: 100,
                mtime: 1000,
                hash: "old_hash".to_string(),
                is_symlink: false,
                link_target: None,
                mode: None,
            }],
            ..Manifest::default()
        };
        let current = vec![FileEntry {
            path: PathBuf::from("a.txt"),
            size: 150,
            mtime: 2000,
            hash: "new_hash".to_string(),
            is_symlink: false,
            link_target: None,
            mode: None,
        }];
        let diff = diff(&prev.files, &current);
        assert!(diff.added.is_empty());
        assert_eq!(diff.modified.len(), 1);
        assert_eq!(diff.modified[0].0.hash, "old_hash");
        assert_eq!(diff.modified[0].1.hash, "new_hash");
        assert!(diff.deleted.is_empty());
    }

    #[test]
    fn test_detect_deleted() {
        let prev = Manifest {
            files: vec![
                FileEntry {
                    path: PathBuf::from("a.txt"),
                    size: 100,
                    mtime: 1000,
                    hash: "aaa".to_string(),
                    is_symlink: false,
                    link_target: None,
                    mode: None,
                },
                FileEntry {
                    path: PathBuf::from("b.txt"),
                    size: 200,
                    mtime: 2000,
                    hash: "bbb".to_string(),
                    is_symlink: false,
                    link_target: None,
                    mode: None,
                },
            ],
            ..Manifest::default()
        };
        let current = vec![FileEntry {
            path: PathBuf::from("a.txt"),
            size: 100,
            mtime: 1000,
            hash: "aaa".to_string(),
            is_symlink: false,
            link_target: None,
            mode: None,
        }];
        let diff = diff(&prev.files, &current);
        assert!(diff.added.is_empty());
        assert!(diff.modified.is_empty());
        assert_eq!(diff.deleted.len(), 1);
        assert_eq!(diff.deleted[0].path, Path::new("b.txt"));
    }

    // ---- Restore Path Containment ----

    #[test]
    fn an_ordinary_relative_entry_lands_under_the_destination() {
        let dest = Path::new("/tmp/restore");
        assert_eq!(
            restore_path_within(dest, Path::new("docs/notes.txt")),
            Some(PathBuf::from("/tmp/restore/docs/notes.txt"))
        );
        assert_eq!(
            restore_path_within(dest, Path::new("./a/./b.txt")),
            Some(PathBuf::from("/tmp/restore/a/b.txt")),
            "`.` components are noise, not an escape"
        );
    }

    /// `Path::join` replaces its base when the argument is absolute, so this
    /// used to write straight to `/etc/passwd` no matter what destination the
    /// user gave. `relative_path` emits absolute paths whenever its
    /// `strip_prefix` fails, so this reaches a manifest without anyone
    /// tampering with it.
    #[test]
    fn an_absolute_entry_is_refused_rather_than_redirecting_the_restore() {
        let dest = Path::new("/tmp/restore");
        assert_eq!(restore_path_within(dest, Path::new("/etc/passwd")), None);
        assert_eq!(restore_path_within(dest, Path::new("/")), None);
    }

    #[test]
    fn a_parent_component_cannot_climb_out_of_the_destination() {
        let dest = Path::new("/tmp/restore");
        assert_eq!(
            restore_path_within(dest, Path::new("../../etc/passwd")),
            None
        );
        assert_eq!(
            restore_path_within(dest, Path::new("docs/../../../etc/passwd")),
            None
        );
        assert_eq!(
            restore_path_within(dest, Path::new("docs/../notes.txt")),
            None,
            "a `..` that happens to stay inside is still refused: resolving it \
             would mean trusting the manifest to be honest about symlinks"
        );
    }

    #[test]
    fn an_entry_naming_no_file_at_all_is_refused() {
        let dest = Path::new("/tmp/restore");
        assert_eq!(restore_path_within(dest, Path::new("")), None);
        assert_eq!(restore_path_within(dest, Path::new(".")), None);
    }

    /// Windows drive prefixes are the platform's own way to spell "absolute",
    /// and a manifest written on Windows records them.
    #[cfg(windows)]
    #[test]
    fn a_windows_drive_prefix_is_refused() {
        let dest = Path::new("C:\\restore");
        assert_eq!(
            restore_path_within(
                dest,
                Path::new("C:\\Windows\\System32\\drivers\\etc\\hosts")
            ),
            None
        );
        assert_eq!(
            restore_path_within(dest, Path::new("\\\\server\\share\\file.txt")),
            None
        );
    }
}
