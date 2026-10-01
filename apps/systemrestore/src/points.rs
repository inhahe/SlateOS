//! Restore points kept for real: where they are, what they hold, and the
//! work of taking and restoring them.
//!
//! Until 2026-09-27 this program drew five invented restore points under a
//! banner saying they were not real, took a new invented one on a schedule,
//! and compared two of them by making up files, packages and settings. It
//! now keeps real ones, in `apps/snapstore` -- the store the backup tool
//! uses, with its guarantees: a restore point records what it could not
//! read, and a restore never removes that, never writes outside the folder,
//! and never writes through a link.
//!
//! **What a restore point holds.** Every program on this system keeps its
//! settings *and* its data -- the notes library, the calendar, the address
//! book, the ledger -- in one folder, `settingsfile::config_dir()`
//! (`~/.config/slateos`). That folder is the "Programs' settings and data"
//! component, and it is the one this program can capture and put back: it is
//! the user's own. The system's files, its boot configuration, its services
//! and its packages need a permission this program does not have, and say so
//! rather than pretend (design-decisions.md §1217; open-questions.md asks
//! how the system half should be done).
//!
//! **Where it keeps them.** The store, and the list of restore points with
//! the tree they form and the schedule, are in
//! `$XDG_DATA_HOME/slateos/restore-points` (`~/.local/share/...` without it)
//! -- deliberately *outside* the folder a restore point captures. Inside it,
//! restoring an old point would put back an old list of restore points and
//! lose every newer one.

use std::collections::BTreeMap;
use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use snapstore::json::{JsonValue, json_parse, json_pretty};
use snapstore::{BackupType, CaptureOptions, RestoreOptions, Store};

use crate::{
    DiffEntry, RetentionPolicy, ScheduleConfig, ScheduleFrequency, Snapshot, SnapshotComponent,
    SnapshotDiffResult, SnapshotManager, SnapshotTree, SnapshotType,
};

/// Where System Restore reads from and keeps what it keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locations {
    /// Every program's settings and data. `None` when neither
    /// `XDG_CONFIG_HOME` nor `HOME` says where that is.
    pub settings: Option<PathBuf>,
    /// The store and the list of restore points. `None` on the same terms.
    pub store: Option<PathBuf>,
}

impl Locations {
    /// Where this user's things are, from the environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            settings: settingsfile::config_dir(),
            store: data_dir().map(|d| d.join("restore-points")),
        }
    }
}

/// `$XDG_DATA_HOME/slateos`, or `$HOME/.local/share/slateos`.
///
/// The same two spellings and the same refusal as `settingsfile::config_dir`:
/// an empty or relative `XDG_DATA_HOME` is ignored (the XDG specification
/// says to), and with no `HOME` either there is no answer -- not a guess at a
/// system-wide path.
fn data_dir() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_DATA_HOME") {
        let xdg = PathBuf::from(xdg);
        if xdg.is_absolute() {
            return Some(xdg.join("slateos"));
        }
    }
    let home = env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    Some(
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("slateos"),
    )
}

/// Why a component holds nothing yet: a system one.
pub const NEEDS_THE_SYSTEM: &str = "Needs the system's permission to read and replace its files, which System Restore does not have";

/// Why the desktop component is not a component of its own.
pub const KEPT_WITH_PROGRAMS: &str =
    "The desktop's settings are kept with the programs' settings and data";

/// Why nothing can be captured when the environment names no home.
pub const NO_HOME: &str = "No home folder is set (neither XDG_CONFIG_HOME nor HOME)";

impl SnapshotComponent {
    /// The folder this component captures, or why it captures nothing.
    ///
    /// # Errors
    ///
    /// A sentence saying why, for every component but the programs' own.
    pub fn source(self, loc: &Locations) -> Result<PathBuf, &'static str> {
        match self {
            Self::UserSettings => loc.settings.clone().ok_or(NO_HOME),
            Self::DesktopConfig => Err(KEPT_WITH_PROGRAMS),
            Self::SystemFiles
            | Self::InstalledApps
            | Self::BootConfig
            | Self::NetworkConfig
            | Self::ServiceConfig
            | Self::DriverState
            | Self::PackageState
            | Self::SecurityPolicy => Err(NEEDS_THE_SYSTEM),
        }
    }
}

// ============================================================================
// The saved list
// ============================================================================

/// The file the restore points, their tree and the schedule are kept in.
fn points_file(store: &Path) -> PathBuf {
    store.join("points.json")
}

/// Format version of `points.json`.
const POINTS_VERSION: u64 = 1;

fn num(n: u64) -> JsonValue {
    #[allow(
        clippy::cast_precision_loss,
        reason = "ids, sizes and times past 2^53 do not occur; the file is JSON numbers"
    )]
    JsonValue::Number(n as f64)
}

fn point_to_json(p: &Snapshot) -> JsonValue {
    JsonValue::Object(vec![
        ("id".to_string(), num(p.id)),
        ("name".to_string(), JsonValue::Str(p.name.clone())),
        (
            "description".to_string(),
            JsonValue::Str(p.description.clone()),
        ),
        ("timestamp".to_string(), num(p.timestamp)),
        (
            "type".to_string(),
            JsonValue::Str(p.snapshot_type.label().to_string()),
        ),
        (
            "parent".to_string(),
            p.parent_id.map_or(JsonValue::Null, num),
        ),
        ("locked".to_string(), JsonValue::Bool(p.locked)),
        (
            "tags".to_string(),
            JsonValue::Array(p.tags.iter().cloned().map(JsonValue::Str).collect()),
        ),
        ("size".to_string(), num(p.size_bytes)),
        ("unread".to_string(), num(p.unread)),
        (
            "stored".to_string(),
            JsonValue::Object(
                p.stored
                    .iter()
                    .map(|(c, id)| (c.label().to_string(), JsonValue::Str(id.clone())))
                    .collect(),
            ),
        ),
    ])
}

fn point_from_json(v: &JsonValue) -> Option<Snapshot> {
    let mut stored = BTreeMap::new();
    for (label, id) in match v.get("stored")? {
        JsonValue::Object(members) => members,
        _ => return None,
    } {
        stored.insert(
            SnapshotComponent::from_label(label)?,
            id.as_str()?.to_string(),
        );
    }
    let parent_id = match v.get("parent")? {
        JsonValue::Null => None,
        other => Some(other.as_u64()?),
    };
    let mut tags = Vec::new();
    for t in v.get("tags")?.as_array()? {
        tags.push(t.as_str()?.to_string());
    }
    Some(Snapshot {
        id: v.get("id")?.as_u64()?,
        name: v.get("name")?.as_str()?.to_string(),
        description: v.get("description")?.as_str()?.to_string(),
        timestamp: v.get("timestamp")?.as_u64()?,
        snapshot_type: SnapshotType::from_label(v.get("type")?.as_str()?)?,
        size_bytes: v.get("size")?.as_u64()?,
        components: stored.keys().copied().collect(),
        parent_id,
        locked: v.get("locked")?.as_bool()?,
        tags,
        stored,
        unread: v.get("unread")?.as_u64()?,
    })
}

fn schedule_to_json(s: &ScheduleConfig) -> JsonValue {
    JsonValue::Object(vec![
        ("enabled".to_string(), JsonValue::Bool(s.enabled)),
        (
            "frequency".to_string(),
            JsonValue::Str(s.frequency.label().to_string()),
        ),
        (
            "components".to_string(),
            JsonValue::Array(
                s.components
                    .iter()
                    .map(|c| JsonValue::Str(c.label().to_string()))
                    .collect(),
            ),
        ),
        (
            "keep_count".to_string(),
            num(u64::try_from(s.retention.max_count).unwrap_or(u64::MAX)),
        ),
        ("keep_seconds".to_string(), num(s.retention.max_age_secs)),
        ("keep_bytes".to_string(), num(s.retention.max_total_bytes)),
        ("last".to_string(), num(s.last_snapshot_timestamp)),
    ])
}

fn schedule_from_json(v: &JsonValue) -> Option<ScheduleConfig> {
    let mut components = Vec::new();
    for c in v.get("components")?.as_array()? {
        components.push(SnapshotComponent::from_label(c.as_str()?)?);
    }
    Some(ScheduleConfig {
        enabled: v.get("enabled")?.as_bool()?,
        frequency: ScheduleFrequency::from_label(v.get("frequency")?.as_str()?)?,
        components,
        retention: RetentionPolicy::new(
            usize::try_from(v.get("keep_count")?.as_u64()?).ok()?,
            v.get("keep_seconds")?.as_u64()?,
            v.get("keep_bytes")?.as_u64()?,
        ),
        last_snapshot_timestamp: v.get("last")?.as_u64()?,
    })
}

/// The file's text for `manager`.
#[must_use]
pub fn to_text(manager: &SnapshotManager) -> String {
    let tree = &manager.tree;
    let points = tree
        .all_ids_by_timestamp()
        .into_iter()
        .filter_map(|id| tree.get_snapshot(id))
        .map(point_to_json)
        .collect();
    let root = JsonValue::Object(vec![
        ("version".to_string(), num(POINTS_VERSION)),
        ("next_id".to_string(), num(tree.next_id())),
        (
            "current".to_string(),
            manager.current.map_or(JsonValue::Null, num),
        ),
        ("schedule".to_string(), schedule_to_json(&manager.schedule)),
        ("points".to_string(), JsonValue::Array(points)),
    ]);
    json_pretty(&root, 2)
}

/// Read the file's text.
///
/// # Errors
///
/// Says what is wrong. The whole file is refused rather than read in part:
/// saving what was understood would throw the rest away.
pub fn parse(text: &str) -> Result<SnapshotManager, String> {
    let root = json_parse(text)?;
    let bad = |what: &str| format!("the list of restore points is damaged ({what})");
    if root.get("version").and_then(JsonValue::as_u64) != Some(POINTS_VERSION) {
        return Err(bad("a version this program does not read"));
    }
    let mut points = Vec::new();
    for v in root
        .get("points")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| bad("no list"))?
    {
        points.push(point_from_json(v).ok_or_else(|| bad("a point is incomplete"))?);
    }
    let next_id = root
        .get("next_id")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| bad("no next id"))?;
    let tree = SnapshotTree::from_points(points, next_id).map_err(|e| bad(&e.to_string()))?;
    let current = match root.get("current") {
        None | Some(JsonValue::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .ok_or_else(|| bad("a current point that is not one"))?,
        ),
    };
    if current.is_some_and(|id| tree.get_snapshot(id).is_none()) {
        return Err(bad("the current point is not in the list"));
    }
    let schedule = root
        .get("schedule")
        .and_then(schedule_from_json)
        .ok_or_else(|| bad("the schedule"))?;
    Ok(SnapshotManager {
        tree,
        schedule,
        current,
    })
}

/// Read the list from the store at `store`: none there is an empty list.
///
/// # Errors
///
/// The file is there and does not read.
pub fn load(store: &Path) -> Result<SnapshotManager, String> {
    match std::fs::read_to_string(points_file(store)) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(SnapshotManager::new()),
        Err(e) => Err(format!("the list of restore points cannot be read: {e}")),
    }
}

/// Write the list to the store at `store`, whole or not at all.
///
/// # Errors
///
/// It cannot be written.
pub fn save(store: &Path, manager: &SnapshotManager) -> io::Result<()> {
    std::fs::create_dir_all(store)?;
    safeio::write_str_atomically(&points_file(store), &to_text(manager))
}

// ============================================================================
// Comparing and measuring
// ============================================================================

/// What changed from `older` to `newer`, file by file, in every component
/// either holds.
///
/// # Errors
///
/// A snapshot's list of files cannot be read.
pub fn compare(
    store: &Store,
    older: &Snapshot,
    newer: &Snapshot,
) -> io::Result<SnapshotDiffResult> {
    let mut entries = Vec::new();
    for c in SnapshotComponent::all() {
        match (older.stored.get(c), newer.stored.get(c)) {
            (None, None) => {}
            (None, Some(_)) => entries.push(DiffEntry::ComponentAdded(*c)),
            (Some(_), None) => entries.push(DiffEntry::ComponentRemoved(*c)),
            (Some(a), Some(b)) => {
                let diff = snapstore::diff(&store.files(a)?, &store.files(b)?);
                let shown = |p: &Path| pathtext::ShowPath::shown(p).to_string();
                entries.extend(
                    diff.added
                        .iter()
                        .map(|f| DiffEntry::FileAdded(shown(&f.path))),
                );
                entries.extend(
                    diff.modified
                        .iter()
                        .map(|(_, f)| DiffEntry::FileModified(shown(&f.path))),
                );
                entries.extend(
                    diff.deleted
                        .iter()
                        .map(|f| DiffEntry::FileRemoved(shown(&f.path))),
                );
            }
        }
    }
    Ok(SnapshotDiffResult {
        older_id: older.id,
        newer_id: newer.id,
        entries,
    })
}

/// How much disk the store takes: every blob and every record.
///
/// # Errors
///
/// The store cannot be listed.
pub fn store_bytes(store: &Store) -> io::Result<u64> {
    let mut total = 0u64;
    for (hash, _) in store.blobs().all_blobs()? {
        total = total.saturating_add(
            std::fs::metadata(store.blobs().blob_path(&hash)).map_or(0, |m| m.len()),
        );
    }
    Ok(total)
}

// ============================================================================
// The work, off the window's thread
// ============================================================================

/// One piece of work on restore points.
#[derive(Clone, Debug)]
pub enum Job {
    /// Take a restore point of `components`.
    Create {
        /// The components asked for; those that cannot be captured are left
        /// out, and the result says which.
        components: Vec<SnapshotComponent>,
    },
    /// Put the files back as `point` holds them, after taking a point of the
    /// files as they are now.
    Restore {
        /// The restore point.
        point: Snapshot,
    },
    /// Remove these snapshots from the store.
    Delete {
        /// The store's ids.
        stored: Vec<String>,
    },
}

/// What was captured of one set of components.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Captured {
    /// The store's id for each component captured.
    pub stored: BTreeMap<SnapshotComponent, String>,
    /// The total size of what they hold.
    pub size: u64,
    /// What could not be read, and so is not in them.
    pub unread: Vec<(PathBuf, String)>,
    /// The components asked for that cannot be captured, with why.
    pub skipped: Vec<(SnapshotComponent, &'static str)>,
    /// When it was taken, in seconds since the epoch: the store's date.
    pub taken_at: u64,
}

/// What the work reports as it goes.
#[derive(Clone, Debug)]
pub enum Update {
    /// A step begun: its name, and how far through the files it is.
    Step {
        /// What is being done.
        name: String,
        /// Files done so far, and how many there are.
        done: u64,
        /// How many there are.
        total: u64,
    },
    /// A restore point taken -- asked for, or the one a restore takes first.
    Captured(Captured),
    /// A restore finished: what could not be put back, by path, with why.
    Restored {
        /// Files written or removed.
        changed: u64,
        /// What could not be done.
        errors: Vec<(PathBuf, String)>,
    },
    /// Snapshots removed from the store.
    Deleted,
    /// The work stopped, and why. Nothing after this.
    Failed(String),
}

/// Work running on a thread of its own.
pub struct Worker {
    rx: mpsc::Receiver<Update>,
    // Held, not joined: the thread ends when its work does, and dropping the
    // handle detaches rather than blocks the window.
    _thread: thread::JoinHandle<()>,
}

impl Worker {
    /// Start `job` against the store and the folders `loc` names.
    #[must_use]
    pub fn start(job: Job, loc: Locations) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || run(job, &loc, &tx));
        Self {
            rx,
            _thread: handle,
        }
    }

    /// Every update sent since the last call.
    pub fn updates(&self) -> Vec<Update> {
        self.rx.try_iter().collect()
    }
}

/// Send an update; a window that has gone has nobody to tell.
fn send(tx: &mpsc::Sender<Update>, u: Update) {
    // A closed receiver means the window closed mid-work; the work itself
    // is already on disk or not, and there is nobody left to report to.
    let _ = tx.send(u);
}

fn run(job: Job, loc: &Locations, tx: &mpsc::Sender<Update>) {
    let Some(root) = loc.store.clone() else {
        send(tx, Update::Failed(NO_HOME.to_string()));
        return;
    };
    let store = match Store::open(&root) {
        Ok(store) => store,
        Err(e) => {
            send(
                tx,
                Update::Failed(format!("the store cannot be opened: {e}")),
            );
            return;
        }
    };
    match job {
        Job::Create { components } => match capture(&store, loc, &components, tx) {
            Ok(c) => send(tx, Update::Captured(c)),
            Err(e) => send(tx, Update::Failed(e)),
        },
        Job::Restore { point } => {
            // First, the files as they are: the restore's own undo. Without
            // it a restore to the wrong point would lose everything since.
            let components: Vec<SnapshotComponent> = point.stored.keys().copied().collect();
            match capture(&store, loc, &components, tx) {
                Ok(c) => send(tx, Update::Captured(c)),
                Err(e) => {
                    send(
                        tx,
                        Update::Failed(format!(
                            "nothing was restored: the files as they are could not be kept first ({e})"
                        )),
                    );
                    return;
                }
            }
            let mut changed = 0u64;
            let mut errors = Vec::new();
            for (c, id) in &point.stored {
                let dest = match c.source(loc) {
                    Ok(dest) => dest,
                    Err(why) => {
                        errors.push((PathBuf::from(c.label()), why.to_string()));
                        continue;
                    }
                };
                send(
                    tx,
                    Update::Step {
                        name: format!("Putting back {}", c.label()),
                        done: 0,
                        total: 1,
                    },
                );
                match store.restore(
                    id,
                    &dest,
                    &RestoreOptions {
                        filter: None,
                        mirror: true,
                    },
                ) {
                    Ok(report) => {
                        changed = changed
                            .saturating_add(report.written)
                            .saturating_add(report.removed);
                        errors.extend(report.errors);
                    }
                    Err(e) => errors.push((dest, e.to_string())),
                }
            }
            send(tx, Update::Restored { changed, errors });
        }
        Job::Delete { stored } => {
            for id in &stored {
                // A snapshot already gone is what deleting wanted.
                if let Err(e) = store.remove(id)
                    && e.kind() != io::ErrorKind::NotFound
                {
                    send(
                        tx,
                        Update::Failed(format!("{id} could not be removed: {e}")),
                    );
                    return;
                }
            }
            // Space comes back once nothing names it; an hour's grace, as the
            // backup tool gives, for a capture running beside this.
            if let Err(e) = store.collect_garbage(Duration::from_hours(1)) {
                send(
                    tx,
                    Update::Failed(format!(
                        "the restore point is gone, but its space is not yet free: {e}"
                    )),
                );
                return;
            }
            send(tx, Update::Deleted);
        }
    }
}

/// Capture every component asked for that can be captured.
fn capture(
    store: &Store,
    loc: &Locations,
    components: &[SnapshotComponent],
    tx: &mpsc::Sender<Update>,
) -> Result<Captured, String> {
    let mut out = Captured::default();
    for c in components {
        let source = match c.source(loc) {
            Ok(source) => source,
            Err(why) => {
                out.skipped.push((*c, why));
                continue;
            }
        };
        // A folder that does not exist yet holds nothing: made, so the
        // restore point says "empty" rather than failing.
        if let Err(e) = std::fs::create_dir_all(&source) {
            return Err(format!("{}: {e}", c.label()));
        }
        let name = format!("Keeping {}", c.label());
        let mut report = |p: &snapstore::Progress| {
            send(
                tx,
                Update::Step {
                    name: name.clone(),
                    done: p.processed_files,
                    total: p.total_files,
                },
            );
        };
        let cap = store
            .capture(
                &source,
                &CaptureOptions {
                    kind: BackupType::Full,
                    excludes: Vec::new(),
                    follow_symlinks: false,
                },
                &mut report,
            )
            .map_err(|e| format!("{}: {e}", c.label()))?;
        if out.stored.is_empty() {
            out.taken_at = cap.meta.timestamp;
        }
        out.size = out.size.saturating_add(cap.meta.total_size);
        out.unread.extend(cap.unread);
        out.stored.insert(*c, cap.meta.id);
    }
    if out.stored.is_empty() {
        return Err("none of the chosen components can be kept on this system".to_string());
    }
    Ok(out)
}
