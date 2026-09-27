//! backup — Slate OS snapshot-based backup system.
//!
//! A CLI tool for creating, managing, and restoring backups with content
//! deduplication via a SHA-256 content-addressed store.
//!
//! Usage:
//!   backup create [--full|--incremental|--differential] --source <PATH> --dest <PATH> [--exclude PATTERN]...
//!   backup restore <BACKUP_ID> --from <PATH> --dest <PATH> [--files PATTERN]
//!   backup list [--source <PATH>]
//!   backup verify <BACKUP_ID>
//!   backup prune --keep-last N [--keep-daily N] [--keep-weekly N] [--keep-monthly N]
//!   backup schedule --source <PATH> --dest <PATH> --interval <daily|weekly|monthly>
//!   backup diff <BACKUP_ID1> <BACKUP_ID2>
//!   backup info <BACKUP_ID>
//!
//! The store itself -- the blobs, the manifests, what a snapshot promises --
//! is `apps/snapstore`, which this program's engine became on 2026-09-27 so
//! that System Restore keeps its copies with the same guarantees. What is
//! left here is the command line: reading it, and saying what happened.

use std::collections::{BTreeMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

use pathtext::ShowPath;
use snapstore::json::{JsonValue, json_parse, json_pretty};
use snapstore::retention::{Policy, compute_retention};
use snapstore::{BackupMeta, BackupType, CaptureOptions, Progress, RestoreOptions, Store};

/// How long a blob no snapshot names is kept before `prune` removes it.
///
/// A capture stores its blobs before it writes the manifest that names them,
/// so a prune running beside one would see them as orphans. An hour is far
/// longer than any capture of this tool's takes to go from its first blob to
/// its manifest, and an orphan left an hour longer costs only its space.
const PRUNE_GRACE: Duration = Duration::from_hours(1);

// ============================================================================
// Formatting
// ============================================================================

/// Format a byte size as a human-readable string.
fn format_size(bytes: u64) -> String {
    textfmt::bytes::iec(bytes)
}

/// Format a UNIX timestamp as `YYYY-MM-DD HH:MM:SS`, in UTC.
fn format_timestamp(ts: u64) -> String {
    let secs_per_day: u64 = 86400;
    let days = ts / secs_per_day;
    let time_of_day = ts % secs_per_day;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    // The exact inverse of `tzrules::days_from_civil`, total over `i64` — not
    // the "simplified Gregorian calculation" that used to live here, whose
    // sibling transcription in the file manager was wrong for every date
    // before 2000-03-01.
    let (year, month, day) = tzrules::civil_from_days(i64::try_from(days).unwrap_or(i64::MAX));
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}")
}

/// Whether `path` contains `needle` as a run of bytes.
///
/// `list --source` is a substring filter the user types. It is asked of the
/// path's own bytes, not of a decoded copy: a directory whose name is not text
/// still matches on the parts of it that are, and no decode can make a filter
/// match a byte the path does not have.
fn path_contains(path: &Path, needle: &str) -> bool {
    let hay = path.as_os_str().as_encoded_bytes();
    let needle = needle.as_bytes();
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle)
}

/// Print a list of paths and why, under a heading, to stderr.
fn report_paths(heading: &str, items: &[(PathBuf, String)]) {
    if items.is_empty() {
        return;
    }
    eprintln!("{heading} ({}):", items.len());
    for (path, why) in items {
        eprintln!("  {}: {why}", path.shown());
    }
}

// ============================================================================
// Command: create
// ============================================================================

struct CreateOptions {
    backup_type: BackupType,
    source: PathBuf,
    dest: PathBuf,
    exclude: Vec<String>,
    follow_symlinks: bool,
}

fn cmd_create(opts: CreateOptions) -> io::Result<()> {
    let store = Store::open(&opts.dest)?;
    println!(
        "Creating {} backup of {} -> {}",
        opts.backup_type,
        opts.source.shown(),
        opts.dest.shown()
    );
    let mut announced = false;
    let mut report = |p: &Progress| {
        if !announced {
            eprintln!(
                "  Found: {} files, {}",
                p.total_files,
                format_size(p.total_bytes)
            );
            announced = true;
        }
        if p.processed_files > 0 && p.processed_files.is_multiple_of(100) {
            let pct = |done: u64, total: u64| {
                done.checked_mul(100)
                    .and_then(|n| n.checked_div(total))
                    .unwrap_or(0)
            };
            eprintln!(
                "  [{}/{}] files ({}%) | [{}/{}] bytes ({}%) | {}",
                p.processed_files,
                p.total_files,
                pct(p.processed_files, p.total_files),
                format_size(p.processed_bytes),
                format_size(p.total_bytes),
                pct(p.processed_bytes, p.total_bytes),
                p.current.shown(),
            );
        }
    };
    let capture = store.capture(
        &opts.source,
        &CaptureOptions {
            kind: opts.backup_type,
            excludes: opts.exclude,
            follow_symlinks: opts.follow_symlinks,
        },
        &mut report,
    )?;
    let meta = &capture.meta;
    if meta.backup_type != opts.backup_type {
        eprintln!("No earlier backup of this source to build on: a full backup was taken instead.");
    }

    println!("\nBackup complete: {}", meta.id);
    println!("  Type: {}", meta.backup_type);
    println!("  Files: {}", meta.file_count);
    println!("  Total size: {}", format_size(meta.total_size));
    println!("  New blobs: {}", meta.new_blobs);
    println!("  Deduplicated: {}", meta.dedup_blobs);
    if meta.dedup_blobs > 0 {
        // Widened to `u128` so the `* 100` is exact rather than saturated.
        let total = u128::from(meta.new_blobs).saturating_add(u128::from(meta.dedup_blobs));
        let saved_pct = u128::from(meta.dedup_blobs)
            .saturating_mul(100)
            .checked_div(total)
            .unwrap_or(0);
        println!("  Dedup ratio: {saved_pct}%");
    }

    // The backup is kept -- everything it could read is in it -- but it is not
    // a backup of the whole source, and the exit status is the only thing a
    // script can see. A file that could not be read used to be a warning and
    // then "Backup complete", exit 0.
    report_paths(
        "Not backed up, because it could not be read",
        &capture.unread,
    );
    if !capture.unread.is_empty() {
        return Err(io::Error::other(format!(
            "{} item(s) could not be read and are not in backup {}",
            capture.unread.len(),
            meta.id
        )));
    }
    Ok(())
}

// ============================================================================
// Command: restore
// ============================================================================

struct RestoreArgs {
    backup_id: String,
    backup_dest: PathBuf,  // Where backups are stored
    restore_dest: PathBuf, // Where to restore files to
    file_pattern: Option<String>,
}

fn cmd_restore(opts: RestoreArgs) -> io::Result<()> {
    let store = Store::at(&opts.backup_dest);
    let meta = store.meta(&opts.backup_id)?;
    println!(
        "Restoring backup {} to {}",
        opts.backup_id,
        opts.restore_dest.shown()
    );
    println!("  Type: {}", meta.backup_type);
    let report = store.restore(
        &opts.backup_id,
        &opts.restore_dest,
        &RestoreOptions {
            filter: opts.file_pattern,
            mirror: false,
        },
    )?;
    println!(
        "\nRestore finished: {} written, {} already as backed up",
        report.written, report.unchanged
    );
    report_paths("Not restored", &report.errors);
    if !report.errors.is_empty() {
        // A restore that could not deliver every file has not restored the
        // backup, and the exit status is the only thing a script can see:
        // `backup restore … && rm -rf original/` must not be a way to lose
        // data.
        return Err(io::Error::other(format!(
            "{} file(s) could not be restored",
            report.errors.len()
        )));
    }
    Ok(())
}

// ============================================================================
// Command: list
// ============================================================================

fn cmd_list(dest: &Path, source_filter: Option<&str>) -> io::Result<()> {
    let listing = Store::at(dest).list()?;
    for (dir, why) in &listing.unreadable {
        eprintln!(
            "warning: {} holds a backup whose record cannot be read: {why}",
            dir.shown()
        );
    }
    if listing.snapshots.is_empty() {
        println!("No backups found.");
        return Ok(());
    }
    let filtered: Vec<&BackupMeta> = listing
        .snapshots
        .iter()
        .filter(|m| source_filter.is_none_or(|s| path_contains(&m.source, s)))
        .collect();

    println!(
        "{:<30} {:<12} {:<20} {:>8} {:>12}",
        "BACKUP ID", "TYPE", "DATE", "FILES", "SIZE"
    );
    println!("{}", "-".repeat(84));
    for meta in &filtered {
        println!(
            "{:<30} {:<12} {:<20} {:>8} {:>12}",
            meta.id,
            meta.backup_type.as_str(),
            format_timestamp(meta.timestamp),
            meta.file_count,
            format_size(meta.total_size),
        );
    }
    println!("\nTotal: {} backups", filtered.len());
    Ok(())
}

// ============================================================================
// Command: verify
// ============================================================================

fn cmd_verify(dest: &Path, backup_id: &str) -> io::Result<()> {
    let store = Store::at(dest);
    let meta = store.meta(backup_id)?;
    println!("Verifying backup: {backup_id}");
    println!("  Type: {}", meta.backup_type);
    println!("  Files: {}", meta.file_count);
    println!();
    let report = store.verify(backup_id)?;
    for path in &report.missing {
        eprintln!("  MISSING: {}", path.shown());
    }
    for (path, why) in &report.corrupt {
        eprintln!("  CORRUPT: {} — {why}", path.shown());
    }
    println!("\nVerification results:");
    println!("  OK: {}", report.ok);
    if !report.missing.is_empty() {
        println!("  Missing blobs: {}", report.missing.len());
    }
    if !report.corrupt.is_empty() {
        println!("  Corrupt blobs: {}", report.corrupt.len());
    }
    if report.missing.is_empty() && report.corrupt.is_empty() {
        println!("  Status: PASSED");
        Ok(())
    } else {
        println!("  Status: FAILED");
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "backup verification failed",
        ))
    }
}

// ============================================================================
// Command: prune
// ============================================================================

struct PruneOptions {
    dest: PathBuf,
    policy: Policy,
}

fn cmd_prune(opts: &PruneOptions) -> io::Result<()> {
    let store = Store::at(&opts.dest);
    let listing = store.list()?;
    if listing.snapshots.is_empty() {
        println!("No backups to prune.");
        return Ok(());
    }
    // Which backups restore on their own: every one written since manifest
    // version 3, and every full one. Read, not assumed -- a store written by
    // an older version of this program holds incrementals that need their
    // chain, and keeping one keeps its chain.
    let mut standalone = HashSet::new();
    for meta in &listing.snapshots {
        if store.manifest(&meta.id)?.complete {
            standalone.insert(meta.id.clone());
        }
    }
    let retention = compute_retention(&listing.snapshots, &opts.policy, &standalone);
    let to_remove: Vec<&BackupMeta> = listing
        .snapshots
        .iter()
        .filter(|m| !retention.keep.contains(&m.id))
        .collect();

    if !retention.held_by_chain.is_empty() {
        println!(
            "Keeping {} older backup(s) that newer ones are built on top of:",
            retention.held_by_chain.len()
        );
        for id in &retention.held_by_chain {
            println!("  = {id}");
        }
    }
    if !retention.broken_chains.is_empty() {
        // Not caused by this prune — the link was already gone. Said out loud
        // because the alternative is keeping a backup that `backup restore`
        // will refuse, and letting the user find out during a recovery.
        eprintln!(
            "warning: {} kept backup(s) cannot be restored: their parent is missing",
            retention.broken_chains.len()
        );
        for (child, parent) in &retention.broken_chains {
            eprintln!(
                "  ! {child} refers to parent {parent}, which is not in {}",
                opts.dest.shown()
            );
        }
    }
    if to_remove.is_empty() {
        println!("No backups to prune (all match retention policy).");
        return Ok(());
    }
    println!("Pruning {} backups:", to_remove.len());
    for meta in &to_remove {
        println!(
            "  - {} ({}, {})",
            meta.id,
            meta.backup_type,
            format_timestamp(meta.timestamp)
        );
    }
    for meta in &to_remove {
        store.remove(&meta.id)?;
    }
    let removed_blobs = store.collect_garbage(PRUNE_GRACE)?;
    println!("\nPrune complete:");
    println!("  Backups removed: {}", to_remove.len());
    println!("  Orphan blobs removed: {removed_blobs}");
    println!(
        "  (Content written in the last hour is kept until a later prune: a \
         backup running now may be about to name it.)"
    );
    Ok(())
}

// ============================================================================
// Command: schedule
// ============================================================================

/// Schedule entry for automated backups.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ScheduleEntry {
    source: String,
    dest: String,
    interval: String,
}

impl ScheduleEntry {
    /// This entry as the object written to the schedule file.
    ///
    /// Destructured, so a new field stops this compiling until someone
    /// decides whether it is written — a schedule field that is not written is
    /// a setting the user chose and the next run will not honour.
    fn to_json(&self) -> JsonValue {
        let Self {
            source,
            dest,
            interval,
        } = self;
        JsonValue::Object(vec![
            ("source".to_string(), JsonValue::Str(source.clone())),
            ("dest".to_string(), JsonValue::Str(dest.clone())),
            ("interval".to_string(), JsonValue::Str(interval.clone())),
        ])
    }

    fn from_json(val: &JsonValue) -> Option<Self> {
        Some(ScheduleEntry {
            source: val.get("source")?.as_str()?.to_string(),
            dest: val.get("dest")?.as_str()?.to_string(),
            interval: val.get("interval")?.as_str()?.to_string(),
        })
    }
}

/// Get the path to the schedules file.
fn schedules_path(dest: &Path) -> PathBuf {
    dest.join("schedules.json")
}

/// The schedules already recorded in `path`, or none if there is no file.
///
/// A file that is there and does not read is an error, not an empty list: the
/// caller rewrites the whole file, and reading garbage as "no schedules" made
/// adding one schedule a way to lose every other.
fn read_schedules(path: &Path) -> io::Result<Vec<ScheduleEntry>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let bad = |why: String| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is not a schedule file ({why}); fix or remove it, and nothing in it is lost",
                path.shown()
            ),
        )
    };
    let val = json_parse(&content).map_err(bad)?;
    let entries = val
        .as_array()
        .ok_or_else(|| bad("it is not a list".to_string()))?;
    entries
        .iter()
        .map(|e| {
            ScheduleEntry::from_json(e).ok_or_else(|| bad("an entry is incomplete".to_string()))
        })
        .collect()
}

fn cmd_schedule(dest: &Path, source: &OsStr, interval: &str) -> io::Result<()> {
    match interval {
        "daily" | "weekly" | "monthly" => {}
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid interval '{interval}': it must be daily, weekly, or monthly"),
            ));
        }
    }

    // Refused rather than converted. `to_string_lossy` would replace any byte
    // that is not UTF-8 with U+FFFD and store *that*, so the schedule would
    // name a directory the user never gave. Our paths allow every byte but
    // `/` and NUL; the schedule file is JSON, which is text, and inventing an
    // escape for these two fields would be a format only this program reads.
    let text = |what: &str, value: &OsStr| {
        value.to_str().map(str::to_string).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "the {what} path is not valid UTF-8, and the schedule file is JSON; give a \
                     {what} whose path is UTF-8, or back up now with `backup create`"
                ),
            )
        })
    };
    let entry = ScheduleEntry {
        source: text("source", source)?,
        dest: text("destination", dest.as_os_str())?,
        interval: interval.to_string(),
    };

    let sched_path = schedules_path(dest);
    let mut schedules = read_schedules(&sched_path)?;
    schedules.retain(|s| !(s.source == entry.source && s.dest == entry.dest));
    schedules.push(entry);

    let json_arr = JsonValue::Array(schedules.iter().map(ScheduleEntry::to_json).collect());
    fs::create_dir_all(dest)?;
    // Crash-safe: this rewrites the whole schedule list, so a truncated write
    // does not lose the one schedule being added — it loses all of them.
    safeio::write_str_atomically(&sched_path, &json_pretty(&json_arr, 2))?;

    println!(
        "Schedule saved: {} -> {} ({interval})",
        Path::new(source).shown(),
        dest.shown(),
    );
    // Said plainly, because the command otherwise reads as a promise. Nothing
    // in this system reads schedules.json: there is no timer, no service and
    // no check at start-up. See known-issues.md
    // BUG-C-BACKUP-SCHEDULE-WRITES-A-FILE-NOTHING-EVER-READS.
    println!(
        "\nNote: this records the schedule; it does not yet cause a backup.\n\
         Nothing on this system starts a backup at a scheduled time, so no\n\
         backup will happen until a scheduler exists. Use `backup create` to\n\
         back up now."
    );
    Ok(())
}

// ============================================================================
// Command: diff
// ============================================================================

fn cmd_diff(dest: &Path, id1: &str, id2: &str) -> io::Result<()> {
    let store = Store::at(dest);
    let (meta1, meta2) = (store.meta(id1)?, store.meta(id2)?);
    println!("Comparing backups:");
    println!("  [1] {id1} ({})", format_timestamp(meta1.timestamp));
    println!("  [2] {id2} ({})", format_timestamp(meta2.timestamp));
    println!();

    // Every file of each, not the manifests as written: an incremental from
    // before manifest version 3 held only its changes, and diffing that
    // listed every file it did not change as deleted.
    let diff = snapstore::diff(&store.files(id1)?, &store.files(id2)?);
    if diff.is_empty() {
        println!("No differences found.");
        return Ok(());
    }
    if !diff.added.is_empty() {
        println!("Added ({}):", diff.added.len());
        for f in &diff.added {
            println!("  + {} ({})", f.path.shown(), format_size(f.size));
        }
        println!();
    }
    if !diff.modified.is_empty() {
        println!("Modified ({}):", diff.modified.len());
        for (old, new) in &diff.modified {
            // `abs_diff` is exact for every pair of `u64`, and the sign is
            // already known from the comparison that chose which way round.
            let (sign, size_change) = if new.size >= old.size {
                ("+", new.size.abs_diff(old.size))
            } else {
                ("-", old.size.abs_diff(new.size))
            };
            println!("  ~ {} ({sign}{size_change} bytes)", new.path.shown());
        }
        println!();
    }
    if !diff.deleted.is_empty() {
        println!("Deleted ({}):", diff.deleted.len());
        for f in &diff.deleted {
            println!("  - {} ({})", f.path.shown(), format_size(f.size));
        }
        println!();
    }

    let added_size: u64 = diff.added.iter().map(|f| f.size).sum();
    let deleted_size: u64 = diff.deleted.iter().map(|f| f.size).sum();
    let modified_new_size: u64 = diff.modified.iter().map(|(_, n)| n.size).sum();
    let modified_old_size: u64 = diff.modified.iter().map(|(o, _)| o.size).sum();
    println!("Summary:");
    println!(
        "  Added: {} files (+{})",
        diff.added.len(),
        format_size(added_size)
    );
    println!(
        "  Modified: {} files (was {}, now {})",
        diff.modified.len(),
        format_size(modified_old_size),
        format_size(modified_new_size),
    );
    println!(
        "  Deleted: {} files (-{})",
        diff.deleted.len(),
        format_size(deleted_size)
    );
    Ok(())
}

// ============================================================================
// Command: info
// ============================================================================

fn cmd_info(dest: &Path, backup_id: &str) -> io::Result<()> {
    let store = Store::at(dest);
    let meta = store.meta(backup_id)?;
    let manifest = store.manifest(backup_id)?;
    let files = if manifest.complete {
        manifest.files.clone()
    } else {
        store.files(backup_id)?
    };

    println!("Backup: {}", meta.id);
    println!("  Type:       {}", meta.backup_type);
    println!("  Created:    {}", format_timestamp(meta.timestamp));
    println!("  Source:     {}", meta.source.shown());
    println!(
        "  Parent:     {}",
        meta.parent_id.as_deref().unwrap_or("(none)")
    );
    println!("  Files:      {}", meta.file_count);
    println!("  Total size: {}", format_size(meta.total_size));
    println!("  New blobs:  {}", meta.new_blobs);
    println!("  Dedup:      {}", meta.dedup_blobs);
    if !manifest.unread.is_empty() {
        println!(
            "  Unread:     {} (not in this backup)",
            manifest.unread.len()
        );
    }
    println!();

    let mut by_ext: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for entry in &files {
        // `Path::extension` (unlike splitting on '.') correctly reports no
        // extension for "README" and for a dotfile like ".gitignore". An
        // extension that is not text is grouped by its bytes (shown as
        // escapes): two such extensions are two groups, not one.
        let ext = entry.path.extension().map_or_else(
            || "(no ext)".to_string(),
            |e| {
                e.to_str().map_or_else(
                    || quoting::escape_unprintable(e.as_encoded_bytes()),
                    str::to_owned,
                )
            },
        );
        let (count, size) = by_ext.entry(ext).or_insert((0, 0));
        *count = count.saturating_add(1);
        *size = size.saturating_add(entry.size);
    }
    if !by_ext.is_empty() {
        println!("  File types:");
        let mut sorted: Vec<_> = by_ext.into_iter().collect();
        sorted.sort_by_key(|item| std::cmp::Reverse(item.1.1));
        for (ext, (count, size)) in sorted.iter().take(10) {
            println!(
                "    .{:<12} {:>6} files  {:>12}",
                ext,
                count,
                format_size(*size)
            );
        }
        if sorted.len() > 10 {
            println!("    ... and {} more types", sorted.len().saturating_sub(10));
        }
    }
    Ok(())
}

// ============================================================================
// Argument Parsing
// ============================================================================

enum Command {
    Create(CreateOptions),
    Restore(RestoreArgs),
    List {
        dest: PathBuf,
        source: Option<String>,
    },
    Verify {
        dest: PathBuf,
        backup_id: String,
    },
    Prune(PruneOptions),
    Schedule {
        dest: PathBuf,
        source: OsString,
        interval: String,
    },
    Diff {
        dest: PathBuf,
        id1: String,
        id2: String,
    },
    Info {
        dest: PathBuf,
        backup_id: String,
    },
    Help,
}

/// A one-pass reader over one command's arguments.
///
/// Holding an iterator makes a counter that runs off the end, or that one
/// branch forgets to advance, unstatable: advancing is the only way to read.
/// [`value`](Self::value) and [`number`](Self::number) put the "flag requires
/// an argument" wording in one place.
///
/// The arguments are `OsString`s, because a path may be any bytes: this read
/// `env::args()`, which panics on an argument that is not UTF-8, so
/// `backup create --source` on such a folder crashed before it did anything.
/// Paths stay as they came; words that must be text -- an id, a pattern, a
/// count -- are asked for as text, and one that is not says so.
struct ArgCursor<'a> {
    rest: std::slice::Iter<'a, OsString>,
}

impl<'a> ArgCursor<'a> {
    fn new(args: &'a [OsString]) -> Self {
        Self { rest: args.iter() }
    }

    /// The next argument, or `None` once they are exhausted.
    fn next(&mut self) -> Option<&'a OsStr> {
        self.rest.next().map(OsString::as_os_str)
    }

    /// The next argument, required, because `flag` carries a value.
    ///
    /// `what` completes the sentence "`--dest` requires …".
    fn value(&mut self, flag: &str, what: &str) -> Result<&'a OsStr, String> {
        self.next().ok_or_else(|| format!("{flag} requires {what}"))
    }

    /// The next argument as a path.
    fn path(&mut self, flag: &str) -> Result<PathBuf, String> {
        self.value(flag, "a path").map(PathBuf::from)
    }

    /// The next argument as text, because `flag`'s value is a word, not a
    /// name on disk.
    fn text(&mut self, flag: &str, what: &str) -> Result<String, String> {
        let value = self.value(flag, what)?;
        value
            .to_str()
            .map(str::to_string)
            .ok_or_else(|| format!("{flag}'s value is not text"))
    }

    /// The next argument parsed as a count.
    fn number(&mut self, flag: &str) -> Result<u64, String> {
        self.value(flag, "a number")?
            .to_str()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| format!("{flag} must be a number"))
    }
}

/// An argument as the flag it may be: the flags are all ASCII, so an argument
/// that is not text is not one of them.
fn word(arg: &OsStr) -> &str {
    arg.to_str().unwrap_or("")
}

/// An operand that must be text: a backup id.
fn operand(arg: Option<&OsStr>, missing: &str) -> Result<String, String> {
    let arg = arg.ok_or(missing)?;
    arg.to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{} is not a backup id", Path::new(arg).shown()))
}

fn parse_args() -> Result<Command, String> {
    let args: Vec<OsString> = std::env::args_os().collect();
    let Some((_program, rest)) = args.split_first() else {
        return Ok(Command::Help);
    };
    let Some((command, options)) = rest.split_first() else {
        return Ok(Command::Help);
    };
    match word(command) {
        "help" | "--help" | "-h" => Ok(Command::Help),
        "create" => parse_create_args(options),
        "restore" => parse_restore_args(options),
        "list" => parse_list_args(options),
        "verify" => parse_verify_args(options),
        "prune" => parse_prune_args(options),
        "schedule" => parse_schedule_args(options),
        "diff" => parse_diff_args(options),
        "info" => parse_info_args(options),
        _ => Err(format!("unknown command: {}", Path::new(command).shown())),
    }
}

/// The message for an argument a command does not take.
fn unknown(command: &str, arg: &OsStr) -> String {
    format!("unknown option for {command}: {}", Path::new(arg).shown())
}

fn parse_create_args(args: &[OsString]) -> Result<Command, String> {
    let mut backup_type = BackupType::Full;
    let mut source: Option<PathBuf> = None;
    let mut dest: Option<PathBuf> = None;
    let mut exclude: Vec<String> = Vec::new();
    let mut follow_symlinks = false;

    let mut cur = ArgCursor::new(args);
    while let Some(arg) = cur.next() {
        match word(arg) {
            "--full" => backup_type = BackupType::Full,
            "--incremental" => backup_type = BackupType::Incremental,
            "--differential" => backup_type = BackupType::Differential,
            "--follow-symlinks" => follow_symlinks = true,
            "--source" => source = Some(cur.path("--source")?),
            "--dest" => dest = Some(cur.path("--dest")?),
            "--exclude" => exclude.push(cur.text("--exclude", "a pattern")?),
            _ => return Err(unknown("create", arg)),
        }
    }
    let source = source.ok_or("--source is required")?;
    let dest = dest.ok_or("--dest is required")?;
    Ok(Command::Create(CreateOptions {
        backup_type,
        source,
        dest,
        exclude,
        follow_symlinks,
    }))
}

fn parse_restore_args(args: &[OsString]) -> Result<Command, String> {
    let mut cur = ArgCursor::new(args);
    let backup_id = operand(cur.next(), "restore requires a BACKUP_ID")?;
    let mut backup_dest: Option<PathBuf> = None;
    let mut restore_dest: Option<PathBuf> = None;
    let mut file_pattern: Option<String> = None;
    while let Some(arg) = cur.next() {
        match word(arg) {
            "--dest" => restore_dest = Some(cur.path("--dest")?),
            "--from" => backup_dest = Some(cur.path("--from")?),
            "--files" => file_pattern = Some(cur.text("--files", "a pattern")?),
            _ => return Err(unknown("restore", arg)),
        }
    }
    let backup_dest = backup_dest.ok_or("--from is required (backup repository path)")?;
    let restore_dest = restore_dest.ok_or("--dest is required (restore destination)")?;
    Ok(Command::Restore(RestoreArgs {
        backup_id,
        backup_dest,
        restore_dest,
        file_pattern,
    }))
}

fn parse_list_args(args: &[OsString]) -> Result<Command, String> {
    let mut dest: Option<PathBuf> = None;
    let mut source: Option<String> = None;
    let mut cur = ArgCursor::new(args);
    while let Some(arg) = cur.next() {
        match word(arg) {
            "--dest" => dest = Some(cur.path("--dest")?),
            "--source" => source = Some(cur.text("--source", "a path")?),
            // The first bare word is the destination, so `backup list /backups`
            // works without the flag. A second one is a typo, not a second
            // destination, and saying so beats silently listing elsewhere.
            _ if dest.is_none() => dest = Some(PathBuf::from(arg)),
            _ => return Err(unknown("list", arg)),
        }
    }
    let dest = dest.ok_or("destination path is required")?;
    Ok(Command::List { dest, source })
}

/// `verify`, `info`: an id, then the repository.
fn parse_id_and_dest(command: &str, args: &[OsString]) -> Result<(String, PathBuf), String> {
    let mut cur = ArgCursor::new(args);
    let backup_id = operand(cur.next(), &format!("{command} requires a BACKUP_ID"))?;
    let mut dest: Option<PathBuf> = None;
    while let Some(arg) = cur.next() {
        match word(arg) {
            "--dest" => dest = Some(cur.path("--dest")?),
            _ if dest.is_none() => dest = Some(PathBuf::from(arg)),
            _ => return Err(unknown(command, arg)),
        }
    }
    let dest = dest.ok_or("--dest is required")?;
    Ok((backup_id, dest))
}

fn parse_verify_args(args: &[OsString]) -> Result<Command, String> {
    let (backup_id, dest) = parse_id_and_dest("verify", args)?;
    Ok(Command::Verify { dest, backup_id })
}

fn parse_info_args(args: &[OsString]) -> Result<Command, String> {
    let (backup_id, dest) = parse_id_and_dest("info", args)?;
    Ok(Command::Info { dest, backup_id })
}

fn parse_prune_args(args: &[OsString]) -> Result<Command, String> {
    let mut dest: Option<PathBuf> = None;
    let mut policy = Policy::default();
    let mut cur = ArgCursor::new(args);
    while let Some(arg) = cur.next() {
        match word(arg) {
            "--dest" => dest = Some(cur.path("--dest")?),
            "--keep-last" => policy.keep_last = Some(cur.number("--keep-last")?),
            "--keep-daily" => policy.keep_daily = Some(cur.number("--keep-daily")?),
            "--keep-weekly" => policy.keep_weekly = Some(cur.number("--keep-weekly")?),
            "--keep-monthly" => policy.keep_monthly = Some(cur.number("--keep-monthly")?),
            _ if dest.is_none() => dest = Some(PathBuf::from(arg)),
            _ => return Err(unknown("prune", arg)),
        }
    }
    let dest = dest.ok_or("--dest is required")?;
    Ok(Command::Prune(PruneOptions { dest, policy }))
}

fn parse_schedule_args(args: &[OsString]) -> Result<Command, String> {
    let mut source: Option<OsString> = None;
    let mut dest: Option<PathBuf> = None;
    let mut interval: Option<String> = None;
    let mut cur = ArgCursor::new(args);
    while let Some(arg) = cur.next() {
        match word(arg) {
            "--source" => source = Some(cur.value("--source", "a path")?.to_os_string()),
            "--dest" => dest = Some(cur.path("--dest")?),
            "--interval" => interval = Some(cur.text("--interval", "a value")?),
            _ => return Err(unknown("schedule", arg)),
        }
    }
    let source = source.ok_or("--source is required")?;
    let dest = dest.ok_or("--dest is required")?;
    let interval = interval.ok_or("--interval is required")?;
    Ok(Command::Schedule {
        dest,
        source,
        interval,
    })
}

fn parse_diff_args(args: &[OsString]) -> Result<Command, String> {
    let mut cur = ArgCursor::new(args);
    let id1 = operand(cur.next(), "diff requires two BACKUP_IDs")?;
    let id2 = operand(cur.next(), "diff requires two BACKUP_IDs")?;
    let mut dest: Option<PathBuf> = None;
    while let Some(arg) = cur.next() {
        match word(arg) {
            "--dest" => dest = Some(cur.path("--dest")?),
            _ if dest.is_none() => dest = Some(PathBuf::from(arg)),
            _ => return Err(unknown("diff", arg)),
        }
    }
    let dest = dest.ok_or("--dest is required")?;
    Ok(Command::Diff { dest, id1, id2 })
}

// ============================================================================
// Help Text
// ============================================================================

fn print_help() {
    println!("backup — Slate OS snapshot-based backup system");
    println!();
    println!("USAGE:");
    println!("  backup <COMMAND> [OPTIONS]");
    println!();
    println!("COMMANDS:");
    println!("  create     Create a new backup");
    println!("  restore    Restore files from a backup");
    println!("  list       List available backups");
    println!("  verify     Verify backup integrity");
    println!("  prune      Remove old backups per retention policy");
    println!("  schedule   Set up a backup schedule");
    println!("  diff       Compare two backups");
    println!("  info       Show detailed backup information");
    println!("  help       Show this help message");
    println!();
    println!("CREATE OPTIONS:");
    println!("  --full             Read every file (default)");
    println!("  --incremental      Reuse the last backup's reading of unchanged files");
    println!("  --differential     Reuse the last full backup's reading of unchanged files");
    println!("  --source <PATH>    Source directory to back up (required)");
    println!("  --dest <PATH>      Destination backup repository (required)");
    println!("  --exclude <PAT>    Exclude files matching glob pattern (repeatable)");
    println!("  --follow-symlinks  Follow symbolic links");
    println!();
    println!("  Every backup lists every file, so any one restores on its own; the");
    println!("  kinds differ only in how much of the source is read again.");
    println!();
    println!("RESTORE OPTIONS:");
    println!("  backup restore <BACKUP_ID> --from <REPO> --dest <PATH> [--files <PATTERN>]");
    println!();
    println!("PRUNE OPTIONS:");
    println!("  --keep-last N      Keep the N most recent backups");
    println!("  --keep-daily N     Keep one backup per day for N days");
    println!("  --keep-weekly N    Keep one backup per week for N weeks");
    println!("  --keep-monthly N   Keep one backup per calendar month for N months");
    println!();
    // The pattern language, spelled out because it is a real one.
    println!("PATTERNS (--exclude, --files):");
    println!("  *          any run of characters, but never crosses a /");
    println!("  ?          any single character, but never a /");
    println!("  **         any number of directories: src/**/mod.rs");
    println!("  [abc]      one character from a set; [a-z] a range; [!a-z] not in it");
    println!();
    println!("  A [ with no closing ] is an ordinary character, so file[1 means");
    println!("  a file called file[1. Matching is per character, not per byte.");
    println!("  A pattern is tried against a file's whole path and its name, so");
    println!("  `cache` leaves out every folder or file called cache.");
    println!();
    println!("EXAMPLES:");
    println!("  backup create --full --source /home/user --dest /mnt/backup");
    println!(
        "  backup create --incremental --source /home/user --dest /mnt/backup --exclude '*.tmp'"
    );
    println!("  backup list --dest /mnt/backup");
    println!("  backup restore 1700000000-full --from /mnt/backup --dest /tmp/restore");
    println!("  backup verify 1700000000-full --dest /mnt/backup");
    println!("  backup prune --dest /mnt/backup --keep-last 5 --keep-weekly 4");
    println!("  backup diff 1700000000-full 1700100000-incremental --dest /mnt/backup");
    println!("  backup info 1700000000-full --dest /mnt/backup");
    println!("  backup schedule --source /home/user --dest /mnt/backup --interval daily");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let cmd = match parse_args() {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("Try 'backup help' for usage information.");
            process::exit(1);
        }
    };
    let result = match cmd {
        Command::Help => {
            print_help();
            Ok(())
        }
        Command::Create(opts) => cmd_create(opts),
        Command::Restore(opts) => cmd_restore(opts),
        Command::List { dest, source } => cmd_list(&dest, source.as_deref()),
        Command::Verify { dest, backup_id } => cmd_verify(&dest, &backup_id),
        Command::Prune(opts) => cmd_prune(&opts),
        Command::Schedule {
            dest,
            source,
            interval,
        } => cmd_schedule(&dest, &source, &interval),
        Command::Diff { dest, id1, id2 } => cmd_diff(&dest, &id1, &id2),
        Command::Info { dest, backup_id } => cmd_info(&dest, &backup_id),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did
    // it — that is the diagnosis. The defensive lints exist to keep panics out
    // of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;
    use scratchdir::ScratchDir;

    /// Serialises the tests that measure `safeio`'s audit counters.
    ///
    /// The counters are process-global, and `cargo test` runs tests in
    /// parallel, so a delta measured across an unsynchronised span is an
    /// *upper* bound on the span's own writes rather than an equality. That
    /// asymmetry points the wrong way: a call site that regressed to
    /// `fs::write` contributes one fewer write, and a concurrent test can make
    /// up the difference and hand back a false pass — silently disarming the
    /// only check that these paths still route through `safeio` at all.
    /// Holding this across the measured span makes the delta exactly the
    /// span's own, so the assertions below can be equalities and a single
    /// regressed call site cannot hide.
    ///
    /// A poisoned lock is recovered rather than propagated: the poison means
    /// some other test panicked, which its own failure already reports, and
    /// re-panicking here would bury that first failure under a second one.
    fn audit_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A private temporary directory for one test, removed when the returned
    /// guard drops.
    ///
    /// The name used to carry the system clock in nanoseconds, which is not
    /// unique. `cargo test` runs a binary's tests as threads of one process,
    /// and the clock a thread reads is only refreshed on a timer interrupt, so
    /// every test that starts within the same tick draws the same tag and they
    /// share one directory. `ScratchDir` names itself from the process id and a
    /// per-process atomic counter, which is unique by construction.
    ///
    /// Bind the guard to a named local, never to `_`: a bare `_` drops it
    /// immediately and the directory is gone before the test's first line.
    fn temp_dir(tag: &str) -> ScratchDir {
        ScratchDir::new(&format!("slate_backup_{tag}"))
    }

    /// `prefix` followed by one unit that makes a name not text: a lone
    /// `0xE9` byte where names are bytes, an unpaired surrogate on the Windows
    /// host this suite also runs on -- so these tests run in the ordinary host
    /// suite rather than only on a Unix machine.
    #[cfg(unix)]
    fn not_text(prefix: &str) -> std::ffi::OsString {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.push(0xE9);
        std::ffi::OsString::from_vec(bytes)
    }

    #[cfg(windows)]
    fn not_text(prefix: &str) -> std::ffi::OsString {
        use std::os::windows::ffi::OsStringExt;
        let mut units: Vec<u16> = prefix.encode_utf16().collect();
        units.push(0xD800);
        std::ffi::OsString::from_wide(&units)
    }

    /// `list --source` matches bytes the path has -- including the text parts
    /// of a name that is not text -- and never a byte it does not have.
    #[test]
    fn the_source_filter_matches_the_bytes_the_path_has() {
        let mut source = PathBuf::from("/srv/photos");
        source.push(not_text("caf"));
        assert!(path_contains(&source, "photos"));
        assert!(path_contains(&source, "/srv/photos"));
        assert!(path_contains(&source, "caf"));
        assert!(path_contains(&source, ""), "an empty filter is no filter");
        assert!(
            !path_contains(&source, "caf\u{FFFD}"),
            "the replacement character a decode would have put there matched"
        );
        assert!(
            !path_contains(Path::new("/a"), "/ab"),
            "a filter longer than the path matched"
        );
    }

    // --- Format/Display Tests ---

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(1048576), "1.0 MiB");
        assert_eq!(format_size(1073741824), "1.0 GiB");
    }

    /// The whole rendering, not just its first five characters.
    ///
    /// This used to assert `starts_with("2023-")` and `contains(":")`, which a
    /// wrong month and a wrong day both pass — and the month and the day are
    /// the two fields the calendar arithmetic can actually get wrong. The
    /// sibling transcription of that arithmetic in the file manager was wrong
    /// for every date before 2000-03-01 and went unnoticed for exactly this
    /// reason: every value it produced was in range, so only a second opinion
    /// could catch it, and no test asked for one.
    #[test]
    fn test_format_timestamp() {
        // 2023-11-14 22:13:20 UTC.
        assert_eq!(format_timestamp(1_700_000_000), "2023-11-14 22:13:20");
    }

    /// Dates chosen where a month estimate drifts, plus the epoch itself.
    ///
    /// `format_timestamp` reads UTC, so these are fixed points independent of
    /// any zone the machine is set to.
    #[test]
    fn a_timestamp_names_the_day_that_contains_it() {
        for (ts, want) in [
            (0_u64, "1970-01-01 00:00:00"),
            (86_399, "1970-01-01 23:59:59"),
            (86_400, "1970-01-02 00:00:00"),
            // 29 February of a leap year that is also a century year — the
            // case the /4 rule alone gets right and the /100 rule alone does
            // not.
            (951_782_400, "2000-02-29 00:00:00"),
            // The two dates the file manager's copy got wrong, kept by name so
            // this reads as the regression test for that bug.
            (489_283_200, "1985-07-04 00:00:00"),
            (929_404_800, "1999-06-15 00:00:00"),
            // The last second of a year, where an off-by-one rolls both the
            // day and the year at once.
            (1_735_689_599, "2024-12-31 23:59:59"),
            (1_735_689_600, "2025-01-01 00:00:00"),
        ] {
            assert_eq!(format_timestamp(ts), want, "at ts {ts}");
        }
    }

    // --- Argument parsing ---
    //
    // The nine `parse_*_args` functions had no tests at all, which is how they
    // came to share forty-five hand-written `args[i]` / `i += 1` pairs: nothing
    // would have noticed a parser that stepped its counter in one branch and
    // forgot it in another. `ArgCursor` removed the counter; these pin the
    // behaviour it replaced, and in particular that a flag's *value* is
    // consumed rather than re-read as the next flag.

    /// Builds an argument slice the way `parse_args` hands one over — the
    /// command word already stripped.
    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// The rejection message from a parse that was supposed to fail.
    ///
    /// `Result::unwrap_err` would need `Command: Debug`, and deriving one on
    /// four production types so that a test can phrase an assertion is the
    /// tail wagging the dog. This asks the same question without the bound.
    fn parse_err(result: Result<Command, String>) -> String {
        match result {
            Ok(_) => panic!("expected the parse to be rejected, but it succeeded"),
            Err(message) => message,
        }
    }

    #[test]
    fn create_reads_every_option() {
        let args = argv(&[
            "--incremental",
            "--source",
            "/home/u",
            "--dest",
            "/backups",
            "--exclude",
            "*.tmp",
            "--exclude",
            "*.log",
            "--follow-symlinks",
        ]);
        let Ok(Command::Create(opts)) = parse_create_args(&args) else {
            panic!("expected a create command");
        };
        assert!(matches!(opts.backup_type, BackupType::Incremental));
        assert_eq!(opts.source, PathBuf::from("/home/u"));
        assert_eq!(opts.dest, PathBuf::from("/backups"));
        assert_eq!(opts.exclude, vec!["*.tmp".to_string(), "*.log".to_string()]);
        assert!(opts.follow_symlinks);
    }

    /// The bug the cursor makes unstatable. A parser that consumed `--dest`'s
    /// value but forgot to step past it would read `/backups` as an option and
    /// reject it — or worse, read a *later* flag's value as a flag and prune
    /// somewhere the user never named. Every value-taking option is followed
    /// here by another option, so a missed advance cannot pass.
    #[test]
    fn a_flag_value_is_consumed_not_reparsed_as_a_flag() {
        let args = argv(&[
            "--keep-last",
            "5",
            "--keep-daily",
            "7",
            "--keep-weekly",
            "4",
            "--keep-monthly",
            "12",
            "--dest",
            "/backups",
        ]);
        let Ok(Command::Prune(opts)) = parse_prune_args(&args) else {
            panic!("expected a prune command");
        };
        assert_eq!(opts.policy.keep_last, Some(5));
        assert_eq!(opts.policy.keep_daily, Some(7));
        assert_eq!(opts.policy.keep_weekly, Some(4));
        assert_eq!(opts.policy.keep_monthly, Some(12));
        assert_eq!(opts.dest, PathBuf::from("/backups"));
    }

    #[test]
    fn a_flag_with_no_value_left_names_itself() {
        assert_eq!(
            parse_err(parse_create_args(&argv(&["--source"]))),
            "--source requires a path"
        );
        assert_eq!(
            parse_err(parse_create_args(&argv(&["--exclude"]))),
            "--exclude requires a pattern"
        );
        assert_eq!(
            parse_err(parse_prune_args(&argv(&["--keep-last"]))),
            "--keep-last requires a number"
        );
        assert_eq!(
            parse_err(parse_schedule_args(&argv(&["--interval"]))),
            "--interval requires a value"
        );
    }

    #[test]
    fn a_retention_count_that_is_not_a_number_is_refused() {
        assert_eq!(
            parse_err(parse_prune_args(&argv(&["--keep-daily", "weekly"]))),
            "--keep-daily must be a number"
        );
        // Negative counts parse as `u64` failures rather than as flags, which
        // is the honest answer: "keep the last -1 backups" has no meaning.
        assert_eq!(
            parse_err(parse_prune_args(&argv(&["--keep-last", "-1"]))),
            "--keep-last must be a number"
        );
    }

    #[test]
    fn create_requires_a_source_and_a_destination() {
        assert_eq!(
            parse_err(parse_create_args(&argv(&["--dest", "/backups"]))),
            "--source is required"
        );
        assert_eq!(
            parse_err(parse_create_args(&argv(&["--source", "/home/u"]))),
            "--dest is required"
        );
    }

    #[test]
    fn an_unknown_option_names_the_command_it_was_given_to() {
        assert_eq!(
            parse_err(parse_create_args(&argv(&["--verbose"]))),
            "unknown option for create: --verbose"
        );
        assert_eq!(
            parse_err(parse_schedule_args(&argv(&["--daily"]))),
            "unknown option for schedule: --daily"
        );
    }

    /// The commands that take an operand must not read the following option as
    /// one. `restore`, `verify`, `info` take one; `diff` takes two.
    #[test]
    fn an_operand_is_read_before_the_options() {
        let Ok(Command::Restore(opts)) = parse_restore_args(&argv(&[
            "backup-42",
            "--from",
            "/backups",
            "--dest",
            "/restore",
            "--files",
            "*.txt",
        ])) else {
            panic!("expected a restore command");
        };
        assert_eq!(opts.backup_id, "backup-42");
        assert_eq!(opts.backup_dest, PathBuf::from("/backups"));
        assert_eq!(opts.restore_dest, PathBuf::from("/restore"));
        assert_eq!(opts.file_pattern, Some("*.txt".to_string()));

        let Ok(Command::Diff { id1, id2, dest }) =
            parse_diff_args(&argv(&["a", "b", "--dest", "/backups"]))
        else {
            panic!("expected a diff command");
        };
        assert_eq!(id1, "a");
        assert_eq!(id2, "b");
        assert_eq!(dest, PathBuf::from("/backups"));
    }

    #[test]
    fn a_missing_operand_says_which_one() {
        assert_eq!(
            parse_err(parse_restore_args(&argv(&[]))),
            "restore requires a BACKUP_ID"
        );
        assert_eq!(
            parse_err(parse_verify_args(&argv(&[]))),
            "verify requires a BACKUP_ID"
        );
        assert_eq!(
            parse_err(parse_info_args(&argv(&[]))),
            "info requires a BACKUP_ID"
        );
        assert_eq!(
            parse_err(parse_diff_args(&argv(&["only-one"]))),
            "diff requires two BACKUP_IDs"
        );
    }

    /// `backup list /backups` and `backup list --dest /backups` mean the same
    /// thing, but a *second* bare word is a typo rather than a second
    /// destination — silently ignoring it would list the wrong repository.
    #[test]
    fn a_bare_word_is_the_destination_but_only_the_first() {
        let Ok(Command::List { dest, source }) = parse_list_args(&argv(&["/backups"])) else {
            panic!("expected a list command");
        };
        assert_eq!(dest, PathBuf::from("/backups"));
        assert_eq!(source, None);

        assert_eq!(
            parse_err(parse_list_args(&argv(&["/backups", "/other"]))),
            "unknown option for list: /other"
        );
    }

    /// `--dest --source` takes `--source` as the destination rather than
    /// reporting a missing value. That is what the hand-written parsers did and
    /// what most Unix tools do: an option's value is the next word, whatever it
    /// looks like, so a path that genuinely begins with `-` stays reachable.
    /// Pinned because it is a decision, not an accident.
    #[test]
    fn a_flags_value_may_itself_look_like_a_flag() {
        let Ok(Command::Create(opts)) =
            parse_create_args(&argv(&["--source", "--dest", "--dest", "/backups"]))
        else {
            panic!("expected a create command");
        };
        assert_eq!(opts.source, PathBuf::from("--dest"));
        assert_eq!(opts.dest, PathBuf::from("/backups"));
    }

    /// A destination the schedule file cannot hold is refused, not mangled.
    ///
    /// Windows-only because that is where a non-UTF-8 path is constructible in
    /// a test: an unpaired surrogate is a legal path component there and has
    /// no UTF-8 encoding. The defect it guards is not platform-specific --
    /// `to_string_lossy` would substitute U+FFFD and store a directory the
    /// user never named, and the backup would land somewhere else with no
    /// error at the time it was set up.
    #[cfg(windows)]
    #[test]
    fn a_destination_that_is_not_utf8_is_refused_rather_than_mangled() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        // Built inside a scratch directory rather than as a bare relative
        // path. The refusal happens before anything is created, so nothing
        // should be written either way -- but when this test was first run
        // against a deliberately broken version, `create_dir_all` reached the
        // path and left a directory named with an unpaired surrogate sitting
        // in the source tree, which `cargo fmt` then tripped over. A test
        // whose failure mode litters the repository is one nobody will want to
        // run twice.
        let scratch = temp_dir("schedule_nonutf8");
        let dest = scratch.dir().join(OsString::from_wide(&[0xD800_u16]));
        assert!(
            dest.to_str().is_none(),
            "the fixture is not the case under test"
        );

        let err = cmd_schedule(&dest, OsStr::new("/some/source"), "daily")
            .expect_err("a destination that cannot be written down must be refused");
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidInput,
            "refused for the wrong reason: {err}"
        );
    }

    /// A path that is not UTF-8 reaches the command intact. The arguments
    /// were read with `env::args()`, which panics on one -- so backing up such
    /// a folder crashed before anything was read.
    #[test]
    fn a_source_that_is_not_text_is_read_whole() {
        let mut args = argv(&["--dest", "/backups", "--source"]);
        args.push(not_text("caf"));
        let Ok(Command::Create(opts)) = parse_create_args(&args) else {
            panic!("expected a create command");
        };
        assert_eq!(opts.source.as_os_str(), not_text("caf").as_os_str());
    }

    /// An id is text; one that is not is refused by name rather than panicking.
    #[test]
    fn an_id_that_is_not_text_is_refused() {
        let args = vec![not_text("id")];
        let message = parse_err(parse_verify_args(&args));
        assert!(message.ends_with("is not a backup id"), "{message}");
    }

    /// Adding a schedule rewrites the whole file, so a file that does not read
    /// must stop the command -- it was read as "no schedules", and adding one
    /// wiped every other.
    #[test]
    fn a_schedule_file_that_does_not_read_is_not_overwritten() {
        let _guard = audit_lock();
        let scratch = temp_dir("schedule_garbage");
        let dest = scratch.dir().join("repo");
        fs::create_dir_all(&dest).unwrap();
        let garbage = b"[{\"source\": \"/home\", \"dest\": \"/r\", \"interval\": \"daily\"}, oops";
        fs::write(schedules_path(&dest), garbage).unwrap();
        let err = cmd_schedule(&dest, OsStr::new("/other"), "weekly").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{err}");
        assert_eq!(fs::read(schedules_path(&dest)).unwrap(), garbage);
    }

    /// Two schedules for different sources are both kept; a second for the
    /// same source and destination replaces the first.
    #[test]
    fn schedules_accumulate_and_replace() {
        let _guard = audit_lock();
        let scratch = temp_dir("schedule_accumulate");
        let dest = scratch.dir().join("repo");
        let dest_text = dest.to_str().unwrap().to_string();
        cmd_schedule(&dest, OsStr::new("/a"), "daily").unwrap();
        cmd_schedule(&dest, OsStr::new("/b"), "weekly").unwrap();
        cmd_schedule(&dest, OsStr::new("/a"), "monthly").unwrap();
        let schedules = read_schedules(&schedules_path(&dest)).unwrap();
        assert_eq!(
            schedules,
            vec![
                ScheduleEntry {
                    source: "/b".to_string(),
                    dest: dest_text.clone(),
                    interval: "weekly".to_string(),
                },
                ScheduleEntry {
                    source: "/a".to_string(),
                    dest: dest_text,
                    interval: "monthly".to_string(),
                },
            ]
        );
    }

    /// The commands write through `safeio`: the schedule file rewrites every
    /// schedule, so a truncating write interrupted loses all of them. (The
    /// store's own writes -- blobs, manifests, restored files -- are pinned in
    /// `apps/snapstore/tests/audit.rs`.)
    #[test]
    fn the_schedule_file_is_written_through_safeio() {
        let _guard = audit_lock();
        let scratch = temp_dir("cmd_routing");
        let dest = scratch.dir().join("backups");
        let before = safeio::writes_performed();
        cmd_schedule(&dest, OsStr::new("/src"), "daily").expect("cmd_schedule");
        assert_eq!(
            safeio::writes_performed() - before,
            1,
            "cmd_schedule must write schedules.json through safeio"
        );
    }

    /// End to end through the commands: a backup, its listing, a restore.
    #[test]
    fn create_then_restore_through_the_commands() {
        let _guard = audit_lock();
        let scratch = temp_dir("end_to_end");
        let src = scratch.dir().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), b"alpha").unwrap();
        fs::write(src.join("sub/b.txt"), b"beta").unwrap();
        let dest = scratch.dir().join("repo");
        cmd_create(CreateOptions {
            backup_type: BackupType::Full,
            source: src.clone(),
            dest: dest.clone(),
            exclude: Vec::new(),
            follow_symlinks: false,
        })
        .unwrap();
        let listing = Store::at(&dest).list().unwrap().snapshots;
        assert_eq!(listing.len(), 1);
        let out = scratch.dir().join("out");
        cmd_restore(RestoreArgs {
            backup_id: listing[0].id.clone(),
            backup_dest: dest.clone(),
            restore_dest: out.clone(),
            file_pattern: None,
        })
        .unwrap();
        assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"alpha");
        assert_eq!(fs::read(out.join("sub/b.txt")).unwrap(), b"beta");
        cmd_verify(&dest, &listing[0].id).unwrap();
        cmd_info(&dest, &listing[0].id).unwrap();
    }

    /// A backup that could not read everything is kept, and says so with its
    /// exit status: it printed "Backup complete" and exited 0.
    #[cfg(unix)]
    #[test]
    fn a_backup_that_could_not_read_everything_fails() {
        let _guard = audit_lock();
        let scratch = temp_dir("unread");
        let src = scratch.dir().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), b"alpha").unwrap();
        std::os::unix::fs::symlink("/nonexistent/backup-test", src.join("dangling")).unwrap();
        let dest = scratch.dir().join("repo");
        let err = cmd_create(CreateOptions {
            backup_type: BackupType::Full,
            source: src,
            dest: dest.clone(),
            exclude: Vec::new(),
            follow_symlinks: true,
        })
        .unwrap_err();
        assert!(err.to_string().contains("could not be read"), "{err}");
        assert_eq!(
            Store::at(&dest).list().unwrap().snapshots.len(),
            1,
            "it is kept"
        );
    }
}
