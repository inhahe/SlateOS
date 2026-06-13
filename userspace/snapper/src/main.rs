//! SlateOS Filesystem Snapshot Management Utility (`snapper`)
//!
//! Multi-personality binary providing:
//! - **snapper** (default) — snapshot management CLI (create, list, delete,
//!   diff, status, undochange, rollback, config management)
//! - **snapper-timeline** — automatic timeline snapshot daemon that creates
//!   hourly snapshots and cleans up per retention policies
//! - **snapper-cleanup** — cleanup old snapshots per retention policies
//!   (timeline, number, empty-pre-post algorithms)
//!
//! Personality is detected via argv[0] basename (stripping path and `.exe` suffix).
//!
//! # Configuration
//!
//! Per-subvolume configuration files live under `/etc/snapper/configs/<name>`.
//! Each config specifies the subvolume path, snapshot directory, timeline
//! settings, cleanup algorithms, and retention limits.
//!
//! # Snapshot Types
//!
//! - **single** — standalone snapshot
//! - **pre** — "before" snapshot of a pre/post pair (transactional changes)
//! - **post** — "after" snapshot of a pre/post pair, references a pre snapshot
//!
//! # Usage
//!
//! ```text
//! snapper [--config <name>] [--csvout] <subcommand> [args...]
//!
//! Config management:
//!   list-configs                    List all configured subvolumes
//!   create-config -n <name> -s <subvolume>  Create a new config
//!   delete-config -n <name>         Delete a config
//!   get-config -n <name>            Show config values
//!   set-config -n <name> <key>=<value>...   Set config values
//!
//! Snapshot management:
//!   list                            List snapshots
//!   create [-t single|pre|post] [-d desc] [-c algo] [-u key=val]...
//!   modify <num> [-d desc] [-c algo] [-u key=val]...
//!   delete <num>...                 Delete snapshots by number
//!   status <num1>..<num2>           Show changes between two snapshots
//!   diff <num1>..<num2>             Show diff between two snapshots
//!   undochange <num1>..<num2>       Undo changes between two snapshots
//!   rollback [<num>]                Rollback to a previous snapshot
//!   cleanup <algorithm>             Run cleanup algorithm
//! ```

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const _CONFIG_DIR: &str = "/etc/snapper/configs";
const SNAPSHOT_DIR_NAME: &str = ".snapshots";

// Default timeline retention limits
const DEFAULT_TIMELINE_HOURLY: u32 = 10;
const DEFAULT_TIMELINE_DAILY: u32 = 10;
const DEFAULT_TIMELINE_WEEKLY: u32 = 0;
const DEFAULT_TIMELINE_MONTHLY: u32 = 10;
const DEFAULT_TIMELINE_YEARLY: u32 = 10;

// Default number cleanup limit
const DEFAULT_NUMBER_LIMIT: u32 = 50;

// Timeline interval in seconds (1 hour)
const TIMELINE_INTERVAL_SECS: u64 = 3600;

// ============================================================================
// Time helpers
// ============================================================================

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Broken-down time from Unix seconds (UTC).
#[derive(Debug, Clone, Copy, PartialEq)]
struct BrokenTime {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    _minute: u32,
    _second: u32,
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn secs_to_broken(secs: u64) -> BrokenTime {
    let mut rem = secs as i64;
    let secs_per_day: i64 = 86400;

    let mut days = rem / secs_per_day;
    rem %= secs_per_day;
    if rem < 0 {
        days -= 1;
        rem += secs_per_day;
    }

    let hour = (rem / 3600) as u32;
    rem %= 3600;
    let minute = (rem / 60) as u32;
    let second = (rem % 60) as u32;

    // Days since epoch (1970-01-01 is day 0)
    let mut year: i64 = 1970;
    loop {
        let ydays: i64 = if is_leap_year(year) { 366 } else { 365 };
        if days < ydays {
            break;
        }
        days -= ydays;
        year += 1;
    }

    let mut month: u32 = 1;
    loop {
        let mdays = days_in_month(year, month) as i64;
        if days < mdays {
            break;
        }
        days -= mdays;
        month += 1;
    }

    let day = days as u32 + 1;

    BrokenTime {
        year,
        month,
        day,
        hour,
        _minute: minute,
        _second: second,
    }
}

/// Format a unix timestamp as "YYYY-MM-DD HH:MM:SS".
fn format_timestamp(secs: u64) -> String {
    let bt = secs_to_broken(secs);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        bt.year, bt.month, bt.day, bt.hour, bt._minute, bt._second
    )
}

/// ISO week number (1-53) from a BrokenTime. Simplified calculation.
fn week_of_year(bt: &BrokenTime) -> u32 {
    // Approximate: day-of-year / 7 + 1
    let mut doy: u32 = bt.day;
    for m in 1..bt.month {
        doy += days_in_month(bt.year, m);
    }
    (doy.saturating_sub(1)) / 7 + 1
}

// ============================================================================
// Snapshot types and metadata
// ============================================================================

/// The type of a snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SnapType {
    Single,
    Pre,
    Post,
}

impl SnapType {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "single" => Some(Self::Single),
            "pre" => Some(Self::Pre),
            "post" => Some(Self::Post),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Pre => "pre",
            Self::Post => "post",
        }
    }
}

/// Cleanup algorithm for a snapshot.
#[derive(Debug, Clone, PartialEq)]
enum CleanupAlgo {
    Timeline,
    Number,
    EmptyPrePost,
    None,
}

impl CleanupAlgo {
    fn from_str(s: &str) -> Self {
        match s {
            "timeline" => Self::Timeline,
            "number" => Self::Number,
            "empty-pre-post" => Self::EmptyPrePost,
            "" | "none" => Self::None,
            _ => Self::None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Timeline => "timeline",
            Self::Number => "number",
            Self::EmptyPrePost => "empty-pre-post",
            Self::None => "",
        }
    }
}

/// Snapshot metadata.
#[derive(Debug, Clone)]
struct Snapshot {
    /// Snapshot number (monotonically increasing per config).
    number: u64,
    /// Creation timestamp (unix seconds).
    date: u64,
    /// Snapshot type.
    snap_type: SnapType,
    /// For post snapshots, the corresponding pre snapshot number.
    pre_number: Option<u64>,
    /// Human-readable description.
    description: String,
    /// Cleanup algorithm assigned to this snapshot.
    cleanup: CleanupAlgo,
    /// User-defined key=value metadata.
    userdata: HashMap<String, String>,
}

impl Snapshot {
    fn new(number: u64, snap_type: SnapType) -> Self {
        Self {
            number,
            date: now_secs(),
            snap_type,
            pre_number: None,
            description: String::new(),
            cleanup: CleanupAlgo::None,
            userdata: HashMap::new(),
        }
    }

    fn new_with_date(number: u64, snap_type: SnapType, date: u64) -> Self {
        Self {
            number,
            date,
            snap_type,
            pre_number: None,
            description: String::new(),
            cleanup: CleanupAlgo::None,
            userdata: HashMap::new(),
        }
    }

    /// Serialize snapshot metadata to a string (one field per line).
    #[allow(dead_code)]
    fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("number={}\n", self.number));
        out.push_str(&format!("date={}\n", self.date));
        out.push_str(&format!("type={}\n", self.snap_type.as_str()));
        if let Some(pre) = self.pre_number {
            out.push_str(&format!("pre_number={}\n", pre));
        }
        out.push_str(&format!("description={}\n", self.description));
        out.push_str(&format!("cleanup={}\n", self.cleanup.as_str()));
        for (k, v) in &self.userdata {
            out.push_str(&format!("userdata[{}]={}\n", k, v));
        }
        out
    }

    /// Parse snapshot metadata from serialized lines.
    #[allow(dead_code)]
    fn parse(data: &str) -> Option<Self> {
        let mut number: Option<u64> = None;
        let mut date: u64 = 0;
        let mut snap_type = SnapType::Single;
        let mut pre_number: Option<u64> = None;
        let mut description = String::new();
        let mut cleanup = CleanupAlgo::None;
        let mut userdata = HashMap::new();

        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                match key {
                    "number" => number = val.parse().ok(),
                    "date" => date = val.parse().unwrap_or(0),
                    "type" => {
                        snap_type = SnapType::from_str(val).unwrap_or(SnapType::Single)
                    }
                    "pre_number" => pre_number = val.parse().ok(),
                    "description" => description = val.to_string(),
                    "cleanup" => cleanup = CleanupAlgo::from_str(val),
                    k if k.starts_with("userdata[") => {
                        if let Some(ukey) = k
                            .strip_prefix("userdata[")
                            .and_then(|s| s.strip_suffix(']'))
                        {
                            userdata.insert(ukey.to_string(), val.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        let num = number?;
        Some(Snapshot {
            number: num,
            date,
            snap_type,
            pre_number,
            description,
            cleanup,
            userdata,
        })
    }
}

// ============================================================================
// File change tracking
// ============================================================================

/// Type of change between two snapshots.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum ChangeType {
    Created,
    Modified,
    Deleted,
    TypeChanged,
}

impl ChangeType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::TypeChanged => "type changed",
        }
    }

    fn short(self) -> &'static str {
        match self {
            Self::Created => "+",
            Self::Modified => "c",
            Self::Deleted => "-",
            Self::TypeChanged => "t",
        }
    }
}

/// A file change between two snapshots.
#[derive(Debug, Clone)]
struct FileChange {
    change_type: ChangeType,
    path: String,
}

// ============================================================================
// Configuration
// ============================================================================

/// Per-subvolume snapper configuration.
#[derive(Debug, Clone)]
struct SnapperConfig {
    /// Config name (used as filename under CONFIG_DIR).
    name: String,
    /// Subvolume path (e.g., /).
    subvolume: String,
    /// Snapshot directory (usually <subvolume>/.snapshots).
    snapshot_dir: String,
    /// Whether timeline creation is enabled.
    timeline_create: bool,
    /// Whether timeline cleanup is enabled.
    timeline_cleanup: bool,
    /// Timeline retention: hourly limit.
    timeline_hourly: u32,
    /// Timeline retention: daily limit.
    timeline_daily: u32,
    /// Timeline retention: weekly limit.
    timeline_weekly: u32,
    /// Timeline retention: monthly limit.
    timeline_monthly: u32,
    /// Timeline retention: yearly limit.
    timeline_yearly: u32,
    /// Whether number cleanup is enabled.
    number_cleanup: bool,
    /// Number cleanup: keep at most this many.
    number_limit: u32,
    /// Whether empty-pre-post cleanup is enabled.
    empty_pre_post_cleanup: bool,
}

impl SnapperConfig {
    fn new(name: &str, subvolume: &str) -> Self {
        let snapshot_dir = if subvolume.ends_with('/') {
            format!("{}{}", subvolume, SNAPSHOT_DIR_NAME)
        } else {
            format!("{}/{}", subvolume, SNAPSHOT_DIR_NAME)
        };
        Self {
            name: name.to_string(),
            subvolume: subvolume.to_string(),
            snapshot_dir,
            timeline_create: true,
            timeline_cleanup: true,
            timeline_hourly: DEFAULT_TIMELINE_HOURLY,
            timeline_daily: DEFAULT_TIMELINE_DAILY,
            timeline_weekly: DEFAULT_TIMELINE_WEEKLY,
            timeline_monthly: DEFAULT_TIMELINE_MONTHLY,
            timeline_yearly: DEFAULT_TIMELINE_YEARLY,
            number_cleanup: true,
            number_limit: DEFAULT_NUMBER_LIMIT,
            empty_pre_post_cleanup: true,
        }
    }

    /// Serialize to config file format (KEY=VALUE lines).
    fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("SUBVOLUME={}\n", self.subvolume));
        out.push_str(&format!("SNAPSHOT_DIR={}\n", self.snapshot_dir));
        out.push_str(&format!(
            "TIMELINE_CREATE={}\n",
            if self.timeline_create { "yes" } else { "no" }
        ));
        out.push_str(&format!(
            "TIMELINE_CLEANUP={}\n",
            if self.timeline_cleanup { "yes" } else { "no" }
        ));
        out.push_str(&format!(
            "TIMELINE_LIMIT_HOURLY={}\n",
            self.timeline_hourly
        ));
        out.push_str(&format!("TIMELINE_LIMIT_DAILY={}\n", self.timeline_daily));
        out.push_str(&format!(
            "TIMELINE_LIMIT_WEEKLY={}\n",
            self.timeline_weekly
        ));
        out.push_str(&format!(
            "TIMELINE_LIMIT_MONTHLY={}\n",
            self.timeline_monthly
        ));
        out.push_str(&format!(
            "TIMELINE_LIMIT_YEARLY={}\n",
            self.timeline_yearly
        ));
        out.push_str(&format!(
            "NUMBER_CLEANUP={}\n",
            if self.number_cleanup { "yes" } else { "no" }
        ));
        out.push_str(&format!("NUMBER_LIMIT={}\n", self.number_limit));
        out.push_str(&format!(
            "EMPTY_PRE_POST_CLEANUP={}\n",
            if self.empty_pre_post_cleanup {
                "yes"
            } else {
                "no"
            }
        ));
        out
    }

    /// Parse config from KEY=VALUE text.
    #[allow(dead_code)]
    fn parse(name: &str, data: &str) -> Option<Self> {
        let mut cfg = Self::new(name, "/");
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim();
                match key {
                    "SUBVOLUME" => cfg.subvolume = val.to_string(),
                    "SNAPSHOT_DIR" => cfg.snapshot_dir = val.to_string(),
                    "TIMELINE_CREATE" => cfg.timeline_create = val == "yes",
                    "TIMELINE_CLEANUP" => cfg.timeline_cleanup = val == "yes",
                    "TIMELINE_LIMIT_HOURLY" => {
                        cfg.timeline_hourly = val.parse().unwrap_or(DEFAULT_TIMELINE_HOURLY)
                    }
                    "TIMELINE_LIMIT_DAILY" => {
                        cfg.timeline_daily = val.parse().unwrap_or(DEFAULT_TIMELINE_DAILY)
                    }
                    "TIMELINE_LIMIT_WEEKLY" => {
                        cfg.timeline_weekly = val.parse().unwrap_or(DEFAULT_TIMELINE_WEEKLY)
                    }
                    "TIMELINE_LIMIT_MONTHLY" => {
                        cfg.timeline_monthly = val.parse().unwrap_or(DEFAULT_TIMELINE_MONTHLY)
                    }
                    "TIMELINE_LIMIT_YEARLY" => {
                        cfg.timeline_yearly = val.parse().unwrap_or(DEFAULT_TIMELINE_YEARLY)
                    }
                    "NUMBER_CLEANUP" => cfg.number_cleanup = val == "yes",
                    "NUMBER_LIMIT" => {
                        cfg.number_limit = val.parse().unwrap_or(DEFAULT_NUMBER_LIMIT)
                    }
                    "EMPTY_PRE_POST_CLEANUP" => {
                        cfg.empty_pre_post_cleanup = val == "yes"
                    }
                    _ => {}
                }
            }
        }
        Some(cfg)
    }

    /// Get a config value by key name.
    fn get_value(&self, key: &str) -> Option<String> {
        match key {
            "SUBVOLUME" => Some(self.subvolume.clone()),
            "SNAPSHOT_DIR" => Some(self.snapshot_dir.clone()),
            "TIMELINE_CREATE" => Some(bool_to_yesno(self.timeline_create).to_string()),
            "TIMELINE_CLEANUP" => Some(bool_to_yesno(self.timeline_cleanup).to_string()),
            "TIMELINE_LIMIT_HOURLY" => Some(self.timeline_hourly.to_string()),
            "TIMELINE_LIMIT_DAILY" => Some(self.timeline_daily.to_string()),
            "TIMELINE_LIMIT_WEEKLY" => Some(self.timeline_weekly.to_string()),
            "TIMELINE_LIMIT_MONTHLY" => Some(self.timeline_monthly.to_string()),
            "TIMELINE_LIMIT_YEARLY" => Some(self.timeline_yearly.to_string()),
            "NUMBER_CLEANUP" => Some(bool_to_yesno(self.number_cleanup).to_string()),
            "NUMBER_LIMIT" => Some(self.number_limit.to_string()),
            "EMPTY_PRE_POST_CLEANUP" => {
                Some(bool_to_yesno(self.empty_pre_post_cleanup).to_string())
            }
            _ => None,
        }
    }

    /// Set a config value by key name. Returns true if the key was recognized.
    fn set_value(&mut self, key: &str, val: &str) -> bool {
        match key {
            "SUBVOLUME" => self.subvolume = val.to_string(),
            "SNAPSHOT_DIR" => self.snapshot_dir = val.to_string(),
            "TIMELINE_CREATE" => self.timeline_create = val == "yes",
            "TIMELINE_CLEANUP" => self.timeline_cleanup = val == "yes",
            "TIMELINE_LIMIT_HOURLY" => {
                if let Ok(v) = val.parse() {
                    self.timeline_hourly = v;
                }
            }
            "TIMELINE_LIMIT_DAILY" => {
                if let Ok(v) = val.parse() {
                    self.timeline_daily = v;
                }
            }
            "TIMELINE_LIMIT_WEEKLY" => {
                if let Ok(v) = val.parse() {
                    self.timeline_weekly = v;
                }
            }
            "TIMELINE_LIMIT_MONTHLY" => {
                if let Ok(v) = val.parse() {
                    self.timeline_monthly = v;
                }
            }
            "TIMELINE_LIMIT_YEARLY" => {
                if let Ok(v) = val.parse() {
                    self.timeline_yearly = v;
                }
            }
            "NUMBER_CLEANUP" => self.number_cleanup = val == "yes",
            "NUMBER_LIMIT" => {
                if let Ok(v) = val.parse() {
                    self.number_limit = v;
                }
            }
            "EMPTY_PRE_POST_CLEANUP" => self.empty_pre_post_cleanup = val == "yes",
            _ => return false,
        }
        true
    }
}

fn bool_to_yesno(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

// ============================================================================
// Snapshot store (in-memory model of on-disk snapshot state)
// ============================================================================

/// Bucket key used by `keep_by_bucket`: (year, month/quarter, day, hour).
type BucketKey = (i64, i64, i64, i64);
/// Bucket value: (snapshot number, snapshot date).
type BucketVal = (u64, u64);

/// In-memory representation of all snapshots for a configuration.
struct SnapshotStore {
    config: SnapperConfig,
    snapshots: Vec<Snapshot>,
    next_number: u64,
}

impl SnapshotStore {
    fn new(config: SnapperConfig) -> Self {
        Self {
            config,
            snapshots: Vec::new(),
            next_number: 1,
        }
    }

    /// Create a new snapshot with the given type, returning its number.
    #[allow(dead_code)]
    fn create(&mut self, snap_type: SnapType) -> u64 {
        let num = self.next_number;
        self.next_number += 1;
        let snap = Snapshot::new(num, snap_type);
        self.snapshots.push(snap);
        num
    }

    /// Create a snapshot with explicit parameters.
    fn create_full(
        &mut self,
        snap_type: SnapType,
        description: &str,
        cleanup: CleanupAlgo,
        userdata: HashMap<String, String>,
        pre_number: Option<u64>,
    ) -> u64 {
        let num = self.next_number;
        self.next_number += 1;
        let mut snap = Snapshot::new(num, snap_type);
        snap.description = description.to_string();
        snap.cleanup = cleanup;
        snap.userdata = userdata;
        snap.pre_number = pre_number;
        self.snapshots.push(snap);
        num
    }

    /// Create a snapshot with a specific timestamp (for timeline).
    fn create_with_date(
        &mut self,
        snap_type: SnapType,
        date: u64,
        description: &str,
        cleanup: CleanupAlgo,
    ) -> u64 {
        let num = self.next_number;
        self.next_number += 1;
        let mut snap = Snapshot::new_with_date(num, snap_type, date);
        snap.description = description.to_string();
        snap.cleanup = cleanup;
        self.snapshots.push(snap);
        num
    }

    /// Get a snapshot by number.
    fn get(&self, number: u64) -> Option<&Snapshot> {
        self.snapshots.iter().find(|s| s.number == number)
    }

    /// Get a mutable snapshot by number.
    fn get_mut(&mut self, number: u64) -> Option<&mut Snapshot> {
        self.snapshots.iter_mut().find(|s| s.number == number)
    }

    /// Delete a snapshot by number. Returns true if it existed.
    fn delete(&mut self, number: u64) -> bool {
        let len_before = self.snapshots.len();
        self.snapshots.retain(|s| s.number != number);
        self.snapshots.len() < len_before
    }

    /// Delete multiple snapshots by number. Returns count deleted.
    fn delete_many(&mut self, numbers: &[u64]) -> usize {
        let len_before = self.snapshots.len();
        self.snapshots.retain(|s| !numbers.contains(&s.number));
        len_before - self.snapshots.len()
    }

    /// List all snapshots, sorted by number.
    fn list(&self) -> Vec<&Snapshot> {
        let mut snaps: Vec<&Snapshot> = self.snapshots.iter().collect();
        snaps.sort_by_key(|s| s.number);
        snaps
    }

    /// Count snapshots.
    #[allow(dead_code)]
    fn count(&self) -> usize {
        self.snapshots.len()
    }

    /// Run the number cleanup algorithm: keep the last N snapshots with
    /// cleanup=Number, delete the rest (oldest first).
    fn cleanup_number(&mut self) -> Vec<u64> {
        if !self.config.number_cleanup {
            return Vec::new();
        }
        let limit = self.config.number_limit as usize;
        let mut numbered: Vec<u64> = self
            .snapshots
            .iter()
            .filter(|s| s.cleanup == CleanupAlgo::Number)
            .map(|s| s.number)
            .collect();
        numbered.sort();

        let mut to_delete = Vec::new();
        if numbered.len() > limit {
            let excess = numbered.len() - limit;
            to_delete.extend_from_slice(&numbered[..excess]);
        }

        self.delete_many(&to_delete);
        to_delete
    }

    /// Run the timeline cleanup algorithm: for each time bucket (hourly, daily,
    /// weekly, monthly, yearly), keep the configured number of most recent
    /// snapshots, delete the rest.
    fn cleanup_timeline(&mut self) -> Vec<u64> {
        if !self.config.timeline_cleanup {
            return Vec::new();
        }

        let mut timeline_snaps: Vec<(u64, u64)> = self
            .snapshots
            .iter()
            .filter(|s| s.cleanup == CleanupAlgo::Timeline)
            .map(|s| (s.number, s.date))
            .collect();
        timeline_snaps.sort_by_key(|&(_, date)| date);

        // Assign each snapshot to the most granular bucket it falls in.
        // Strategy: keep the most recent N per bucket level.
        // We keep all that are within the hourly, daily, weekly, monthly, yearly
        // limits, and delete the rest.

        let mut to_keep: Vec<u64> = Vec::new();

        // Group by (year, month, day, hour) for hourly
        to_keep.extend(self.keep_by_bucket(&timeline_snaps, self.config.timeline_hourly, |ts| {
            let bt = secs_to_broken(ts);
            (bt.year, bt.month as i64, bt.day as i64, bt.hour as i64)
        }));

        // Group by (year, month, day) for daily
        to_keep.extend(self.keep_by_bucket(&timeline_snaps, self.config.timeline_daily, |ts| {
            let bt = secs_to_broken(ts);
            (bt.year, bt.month as i64, bt.day as i64, 0)
        }));

        // Group by (year, week) for weekly
        to_keep.extend(self.keep_by_bucket(&timeline_snaps, self.config.timeline_weekly, |ts| {
            let bt = secs_to_broken(ts);
            (bt.year, week_of_year(&bt) as i64, 0, 0)
        }));

        // Group by (year, month) for monthly
        to_keep.extend(self.keep_by_bucket(&timeline_snaps, self.config.timeline_monthly, |ts| {
            let bt = secs_to_broken(ts);
            (bt.year, bt.month as i64, 0, 0)
        }));

        // Group by year for yearly
        to_keep.extend(self.keep_by_bucket(&timeline_snaps, self.config.timeline_yearly, |ts| {
            let bt = secs_to_broken(ts);
            (bt.year, 0, 0, 0)
        }));

        to_keep.sort();
        to_keep.dedup();

        let to_delete: Vec<u64> = timeline_snaps
            .iter()
            .map(|&(num, _)| num)
            .filter(|num| !to_keep.contains(num))
            .collect();

        self.delete_many(&to_delete);
        to_delete
    }

    /// Helper: group snapshots by a bucket key, keep the most recent `limit`
    /// unique buckets, returning the snapshot numbers to keep.
    fn keep_by_bucket<F>(
        &self,
        snaps: &[(u64, u64)],
        limit: u32,
        bucket_fn: F,
    ) -> Vec<u64>
    where
        F: Fn(u64) -> BucketKey,
    {
        if limit == 0 {
            return Vec::new();
        }

        // Collect unique buckets with the most recent snapshot in each.
        let mut buckets: HashMap<BucketKey, BucketVal> = HashMap::new();
        for &(num, date) in snaps {
            let key = bucket_fn(date);
            let entry = buckets.entry(key).or_insert((num, date));
            // Keep the one with the latest date in each bucket
            if date > entry.1 {
                *entry = (num, date);
            }
        }

        // Sort buckets by key (most recent first) and keep `limit` of them.
        let mut bucket_list: Vec<(BucketKey, BucketVal)> =
            buckets.into_iter().collect();
        bucket_list.sort_by_key(|b| std::cmp::Reverse(b.0));

        let keep_count = (limit as usize).min(bucket_list.len());
        bucket_list[..keep_count]
            .iter()
            .map(|&(_, (num, _))| num)
            .collect()
    }

    /// Run the empty-pre-post cleanup: delete pre/post pairs where no files
    /// changed between them (i.e., the pair is empty).
    fn cleanup_empty_pre_post(&mut self, changes_fn: &dyn Fn(u64, u64) -> Vec<FileChange>) -> Vec<u64> {
        if !self.config.empty_pre_post_cleanup {
            return Vec::new();
        }

        let mut to_delete = Vec::new();

        // Find all pre/post pairs
        let post_snaps: Vec<(u64, u64)> = self
            .snapshots
            .iter()
            .filter(|s| {
                s.snap_type == SnapType::Post
                    && s.pre_number.is_some()
                    && s.cleanup == CleanupAlgo::EmptyPrePost
            })
            .map(|s| (s.number, s.pre_number.unwrap_or(0)))
            .collect();

        for (post_num, pre_num) in post_snaps {
            // Check if the pre snapshot exists
            if self.get(pre_num).is_none() {
                continue;
            }
            let changes = changes_fn(pre_num, post_num);
            if changes.is_empty() {
                to_delete.push(pre_num);
                to_delete.push(post_num);
            }
        }

        self.delete_many(&to_delete);
        to_delete
    }
}

// ============================================================================
// Status and diff computation
// ============================================================================

/// Compute file changes between two snapshots (stub that would read from
/// the snapshot directories on a real filesystem).
fn compute_status(
    _config: &SnapperConfig,
    snap_a: &Snapshot,
    snap_b: &Snapshot,
) -> Vec<FileChange> {
    // On a real system, this would walk the two snapshot directories and
    // compare file metadata and content. Here we return a placeholder.
    let _ = (snap_a, snap_b);
    Vec::new()
}

/// Format status output for display.
fn format_status_table(changes: &[FileChange]) -> String {
    let mut out = String::new();
    for ch in changes {
        out.push_str(&format!(
            "{} {}\n",
            ch.change_type.short(),
            ch.path
        ));
    }
    if changes.is_empty() {
        out.push_str("No changes.\n");
    }
    out
}

/// Format status output as CSV.
fn format_status_csv(changes: &[FileChange]) -> String {
    let mut out = String::from("change,path\n");
    for ch in changes {
        out.push_str(&format!(
            "{},{}\n",
            ch.change_type.as_str(),
            ch.path
        ));
    }
    out
}

// ============================================================================
// Output formatting
// ============================================================================

/// Format snapshot list as a table.
fn format_snapshot_table(snapshots: &[&Snapshot]) -> String {
    let mut out = String::new();
    // Header
    out.push_str(&format!(
        "{:<6} | {:<19} | {:<7} | {:<4} | {:<10} | {}\n",
        "#", "Date", "Type", "Pre#", "Cleanup", "Description"
    ));
    out.push_str(&format!("{}\n", "-".repeat(78)));

    for snap in snapshots {
        let pre_str = match snap.pre_number {
            Some(n) => n.to_string(),
            None => String::new(),
        };
        out.push_str(&format!(
            "{:<6} | {:<19} | {:<7} | {:<4} | {:<10} | {}\n",
            snap.number,
            format_timestamp(snap.date),
            snap.snap_type.as_str(),
            pre_str,
            snap.cleanup.as_str(),
            snap.description
        ));
    }
    out
}

/// Format snapshot list as CSV.
fn format_snapshot_csv(snapshots: &[&Snapshot]) -> String {
    let mut out = String::from("number,date,type,pre_number,cleanup,description,userdata\n");
    for snap in snapshots {
        let pre_str = match snap.pre_number {
            Some(n) => n.to_string(),
            None => String::new(),
        };
        let ud: Vec<String> = snap
            .userdata
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        out.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            snap.number,
            snap.date,
            snap.snap_type.as_str(),
            pre_str,
            snap.cleanup.as_str(),
            snap.description,
            ud.join(";")
        ));
    }
    out
}

/// Format config list as a table.
fn format_config_table(configs: &[&SnapperConfig]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:<20} | {}\n",
        "Config", "Subvolume"
    ));
    out.push_str(&format!("{}\n", "-".repeat(50)));
    for cfg in configs {
        out.push_str(&format!("{:<20} | {}\n", cfg.name, cfg.subvolume));
    }
    out
}

/// Format config list as CSV.
fn format_config_csv(configs: &[&SnapperConfig]) -> String {
    let mut out = String::from("config,subvolume\n");
    for cfg in configs {
        out.push_str(&format!("{},{}\n", cfg.name, cfg.subvolume));
    }
    out
}

/// Format a single config's key-value pairs.
fn format_config_detail(cfg: &SnapperConfig) -> String {
    let mut out = String::new();
    let keys = [
        "SUBVOLUME",
        "SNAPSHOT_DIR",
        "TIMELINE_CREATE",
        "TIMELINE_CLEANUP",
        "TIMELINE_LIMIT_HOURLY",
        "TIMELINE_LIMIT_DAILY",
        "TIMELINE_LIMIT_WEEKLY",
        "TIMELINE_LIMIT_MONTHLY",
        "TIMELINE_LIMIT_YEARLY",
        "NUMBER_CLEANUP",
        "NUMBER_LIMIT",
        "EMPTY_PRE_POST_CLEANUP",
    ];
    for key in &keys {
        if let Some(val) = cfg.get_value(key) {
            out.push_str(&format!("{} = {}\n", key, val));
        }
    }
    out
}

// ============================================================================
// Argument parsing helpers
// ============================================================================

/// Parse a snapshot range like "1..5" into (from, to).
fn parse_range(s: &str) -> Option<(u64, u64)> {
    let parts: Vec<&str> = s.split("..").collect();
    if parts.len() == 2 {
        let a = parts[0].parse().ok()?;
        let b = parts[1].parse().ok()?;
        Some((a, b))
    } else {
        None
    }
}

/// Parse key=value pairs from arguments.
fn parse_key_value(s: &str) -> Option<(String, String)> {
    s.split_once('=')
        .map(|(k, v)| (k.to_string(), v.to_string()))
}

/// Extract a flag value from args: returns the value after the flag, or None.
fn extract_flag<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag {
            return iter.next().map(|s| s.as_str());
        }
    }
    None
}

/// Check if a flag is present in args.
fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

/// Collect all values for a repeatable flag (e.g., -u key=val -u key2=val2).
fn collect_flag_values<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag
            && let Some(val) = iter.next() {
                values.push(val.as_str());
            }
    }
    values
}

// ============================================================================
// Personality: snapper (main CLI)
// ============================================================================

fn run_snapper(args: &[String]) -> i32 {
    let csvout = has_flag(args, "--csvout");
    let config_name = extract_flag(args, "--config")
        .or_else(|| extract_flag(args, "-c"))
        .unwrap_or("root");

    // Find the subcommand (first arg that doesn't start with -)
    let sub_args: Vec<String> = {
        let mut filtered = Vec::new();
        let mut skip_next = false;
        for arg in args {
            if skip_next {
                skip_next = false;
                continue;
            }
            if arg == "--config" || arg == "-c" || arg == "--csvout" {
                if arg == "--config" || arg == "-c" {
                    skip_next = true;
                }
                continue;
            }
            filtered.push(arg.clone());
        }
        filtered
    };

    if sub_args.is_empty() {
        print_snapper_usage();
        return 0;
    }

    let subcmd = sub_args[0].as_str();
    let subcmd_args = &sub_args[1..];

    match subcmd {
        "--help" | "-h" | "help" => {
            print_snapper_usage();
            0
        }
        "--version" | "-V" => {
            println!("snapper {}", VERSION);
            0
        }
        "list-configs" => cmd_list_configs(csvout),
        "create-config" => cmd_create_config(subcmd_args),
        "delete-config" => cmd_delete_config(subcmd_args),
        "get-config" => cmd_get_config(subcmd_args),
        "set-config" => cmd_set_config(subcmd_args),
        "list" => cmd_list(config_name, csvout),
        "create" => cmd_create(config_name, subcmd_args),
        "modify" => cmd_modify(config_name, subcmd_args),
        "delete" => cmd_delete(config_name, subcmd_args),
        "status" => cmd_status(config_name, subcmd_args, csvout),
        "diff" => cmd_diff(config_name, subcmd_args, csvout),
        "undochange" => cmd_undochange(config_name, subcmd_args),
        "rollback" => cmd_rollback(config_name, subcmd_args),
        "cleanup" => cmd_cleanup(config_name, subcmd_args),
        _ => {
            eprintln!("snapper: unknown subcommand '{}'", subcmd);
            eprintln!("Try 'snapper --help' for more information.");
            1
        }
    }
}

fn print_snapper_usage() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "snapper {} - Filesystem snapshot manager", VERSION);
    let _ = writeln!(out);
    let _ = writeln!(out, "Usage: snapper [--config <name>] [--csvout] <subcommand> [args...]");
    let _ = writeln!(out);
    let _ = writeln!(out, "Global options:");
    let _ = writeln!(out, "  --config <name>, -c <name>  Use config (default: root)");
    let _ = writeln!(out, "  --csvout                    Machine-readable CSV output");
    let _ = writeln!(out);
    let _ = writeln!(out, "Config management:");
    let _ = writeln!(out, "  list-configs                          List configured subvolumes");
    let _ = writeln!(out, "  create-config -n <name> -s <subvolume> Create a new config");
    let _ = writeln!(out, "  delete-config -n <name>               Delete a config");
    let _ = writeln!(out, "  get-config -n <name>                  Show config values");
    let _ = writeln!(out, "  set-config -n <name> KEY=VALUE...     Set config values");
    let _ = writeln!(out);
    let _ = writeln!(out, "Snapshot management:");
    let _ = writeln!(out, "  list                                  List snapshots");
    let _ = writeln!(
        out,
        "  create [-t type] [-d desc] [-c algo] [-u key=val]..."
    );
    let _ = writeln!(
        out,
        "  modify <num> [-d desc] [-c algo] [-u key=val]..."
    );
    let _ = writeln!(out, "  delete <num>...                       Delete snapshots");
    let _ = writeln!(
        out,
        "  status <num1>..<num2>                 Show changes between snapshots"
    );
    let _ = writeln!(
        out,
        "  diff <num1>..<num2>                   Show diff between snapshots"
    );
    let _ = writeln!(
        out,
        "  undochange <num1>..<num2>             Undo changes between snapshots"
    );
    let _ = writeln!(out, "  rollback [<num>]                      Rollback to snapshot");
    let _ = writeln!(out, "  cleanup <algorithm>                   Run cleanup algorithm");
}

/// Stub: load all configs from CONFIG_DIR.
fn load_all_configs() -> Vec<SnapperConfig> {
    // On a real system, this scans CONFIG_DIR. Stub returns empty.
    Vec::new()
}

/// Stub: load a named config.
fn load_config(name: &str) -> Option<SnapperConfig> {
    let _ = name;
    // On a real system, reads CONFIG_DIR/<name>.
    None
}

/// Stub: save a config.
fn save_config(cfg: &SnapperConfig) -> Result<(), String> {
    let _serialized = cfg.serialize();
    // On a real system, writes to CONFIG_DIR/<name>.
    Ok(())
}

/// Stub: delete a config file.
fn delete_config_file(name: &str) -> Result<(), String> {
    let _ = name;
    // On a real system, removes CONFIG_DIR/<name>.
    Ok(())
}

/// Stub: load snapshots for a config.
fn load_snapshots(config: &SnapperConfig) -> SnapshotStore {
    SnapshotStore::new(config.clone())
}

/// Stub: save snapshot metadata.
fn save_snapshot(_config: &SnapperConfig, _snap: &Snapshot) -> Result<(), String> {
    Ok(())
}

/// Stub: delete snapshot data from disk.
fn delete_snapshot_data(_config: &SnapperConfig, _number: u64) -> Result<(), String> {
    Ok(())
}

fn cmd_list_configs(csvout: bool) -> i32 {
    let configs = load_all_configs();
    let refs: Vec<&SnapperConfig> = configs.iter().collect();
    if csvout {
        print!("{}", format_config_csv(&refs));
    } else {
        print!("{}", format_config_table(&refs));
    }
    0
}

fn cmd_create_config(args: &[String]) -> i32 {
    let name = match extract_flag(args, "-n") {
        Some(n) => n,
        None => {
            eprintln!("Error: -n <name> required");
            return 1;
        }
    };
    let subvolume = match extract_flag(args, "-s") {
        Some(s) => s,
        None => {
            eprintln!("Error: -s <subvolume> required");
            return 1;
        }
    };
    let cfg = SnapperConfig::new(name, subvolume);
    match save_config(&cfg) {
        Ok(()) => {
            println!("Config '{}' created.", name);
            0
        }
        Err(e) => {
            eprintln!("Error creating config: {}", e);
            1
        }
    }
}

fn cmd_delete_config(args: &[String]) -> i32 {
    let name = match extract_flag(args, "-n") {
        Some(n) => n,
        None => {
            eprintln!("Error: -n <name> required");
            return 1;
        }
    };
    match delete_config_file(name) {
        Ok(()) => {
            println!("Config '{}' deleted.", name);
            0
        }
        Err(e) => {
            eprintln!("Error deleting config: {}", e);
            1
        }
    }
}

fn cmd_get_config(args: &[String]) -> i32 {
    let name = match extract_flag(args, "-n") {
        Some(n) => n,
        None => {
            eprintln!("Error: -n <name> required");
            return 1;
        }
    };
    match load_config(name) {
        Some(cfg) => {
            print!("{}", format_config_detail(&cfg));
            0
        }
        None => {
            eprintln!("Error: config '{}' not found", name);
            1
        }
    }
}

fn cmd_set_config(args: &[String]) -> i32 {
    let name = match extract_flag(args, "-n") {
        Some(n) => n,
        None => {
            eprintln!("Error: -n <name> required");
            return 1;
        }
    };
    let mut cfg = match load_config(name) {
        Some(c) => c,
        None => {
            eprintln!("Error: config '{}' not found", name);
            return 1;
        }
    };

    let mut found_kv = false;
    for arg in args {
        if let Some((k, v)) = parse_key_value(arg) {
            if cfg.set_value(&k, &v) {
                found_kv = true;
            } else {
                eprintln!("Warning: unknown key '{}'", k);
            }
        }
    }

    if !found_kv {
        eprintln!("Error: no KEY=VALUE pairs provided");
        return 1;
    }

    match save_config(&cfg) {
        Ok(()) => {
            println!("Config '{}' updated.", name);
            0
        }
        Err(e) => {
            eprintln!("Error saving config: {}", e);
            1
        }
    }
}

fn cmd_list(config_name: &str, csvout: bool) -> i32 {
    let cfg = match load_config(config_name) {
        Some(c) => c,
        None => {
            // Use default config if not found on disk.
            SnapperConfig::new(config_name, "/")
        }
    };
    let store = load_snapshots(&cfg);
    let snaps = store.list();
    if csvout {
        print!("{}", format_snapshot_csv(&snaps));
    } else {
        print!("{}", format_snapshot_table(&snaps));
    }
    0
}

fn cmd_create(config_name: &str, args: &[String]) -> i32 {
    let cfg = match load_config(config_name) {
        Some(c) => c,
        None => SnapperConfig::new(config_name, "/"),
    };

    let snap_type_str = extract_flag(args, "-t").unwrap_or("single");
    let snap_type = match SnapType::from_str(snap_type_str) {
        Some(t) => t,
        None => {
            eprintln!("Error: unknown snapshot type '{}'", snap_type_str);
            return 1;
        }
    };

    let description = extract_flag(args, "-d").unwrap_or("");
    let cleanup_str = extract_flag(args, "--cleanup").unwrap_or("");
    let cleanup = CleanupAlgo::from_str(cleanup_str);

    let mut userdata = HashMap::new();
    for uval in collect_flag_values(args, "-u") {
        if let Some((k, v)) = parse_key_value(uval) {
            userdata.insert(k, v);
        }
    }

    let pre_number = if snap_type == SnapType::Post {
        extract_flag(args, "--pre-number")
            .and_then(|s| s.parse().ok())
    } else {
        None
    };

    let mut store = load_snapshots(&cfg);
    let num = store.create_full(snap_type, description, cleanup, userdata, pre_number);

    if let Some(snap) = store.get(num) {
        let _ = save_snapshot(&cfg, snap);
    }

    println!("Created snapshot {}.", num);
    0
}

fn cmd_modify(config_name: &str, args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Error: snapshot number required");
        return 1;
    }
    let num: u64 = match args[0].parse() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("Error: invalid snapshot number '{}'", args[0]);
            return 1;
        }
    };

    let cfg = match load_config(config_name) {
        Some(c) => c,
        None => SnapperConfig::new(config_name, "/"),
    };
    let mut store = load_snapshots(&cfg);

    match store.get_mut(num) {
        Some(snap) => {
            if let Some(d) = extract_flag(args, "-d") {
                snap.description = d.to_string();
            }
            if let Some(c) = extract_flag(args, "--cleanup") {
                snap.cleanup = CleanupAlgo::from_str(c);
            }
            for uval in collect_flag_values(args, "-u") {
                if let Some((k, v)) = parse_key_value(uval) {
                    snap.userdata.insert(k, v);
                }
            }
            let _ = save_snapshot(&cfg, snap);
            println!("Snapshot {} modified.", num);
            0
        }
        None => {
            eprintln!("Error: snapshot {} not found", num);
            1
        }
    }
}

fn cmd_delete(config_name: &str, args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Error: snapshot number(s) required");
        return 1;
    }

    let cfg = match load_config(config_name) {
        Some(c) => c,
        None => SnapperConfig::new(config_name, "/"),
    };
    let mut store = load_snapshots(&cfg);

    let mut errors = 0;
    for arg in args {
        match arg.parse::<u64>() {
            Ok(num) => {
                if store.delete(num) {
                    let _ = delete_snapshot_data(&cfg, num);
                    println!("Snapshot {} deleted.", num);
                } else {
                    eprintln!("Error: snapshot {} not found", num);
                    errors += 1;
                }
            }
            Err(_) => {
                eprintln!("Error: invalid snapshot number '{}'", arg);
                errors += 1;
            }
        }
    }

    if errors > 0 { 1 } else { 0 }
}

fn cmd_status(config_name: &str, args: &[String], csvout: bool) -> i32 {
    if args.is_empty() {
        eprintln!("Error: snapshot range required (e.g., 1..2)");
        return 1;
    }

    let (from, to) = match parse_range(&args[0]) {
        Some(r) => r,
        None => {
            eprintln!("Error: invalid range '{}', expected format: N..M", args[0]);
            return 1;
        }
    };

    let cfg = match load_config(config_name) {
        Some(c) => c,
        None => SnapperConfig::new(config_name, "/"),
    };
    let store = load_snapshots(&cfg);

    let snap_a = match store.get(from) {
        Some(s) => s,
        None => {
            eprintln!("Error: snapshot {} not found", from);
            return 1;
        }
    };
    let snap_b = match store.get(to) {
        Some(s) => s,
        None => {
            eprintln!("Error: snapshot {} not found", to);
            return 1;
        }
    };

    let changes = compute_status(&cfg, snap_a, snap_b);
    if csvout {
        print!("{}", format_status_csv(&changes));
    } else {
        print!("{}", format_status_table(&changes));
    }
    0
}

fn cmd_diff(config_name: &str, args: &[String], csvout: bool) -> i32 {
    // diff and status share the same implementation for now;
    // on a real system, diff would show file content differences.
    cmd_status(config_name, args, csvout)
}

fn cmd_undochange(config_name: &str, args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Error: snapshot range required (e.g., 1..2)");
        return 1;
    }

    let (from, to) = match parse_range(&args[0]) {
        Some(r) => r,
        None => {
            eprintln!("Error: invalid range '{}', expected format: N..M", args[0]);
            return 1;
        }
    };

    let cfg = match load_config(config_name) {
        Some(c) => c,
        None => SnapperConfig::new(config_name, "/"),
    };
    let store = load_snapshots(&cfg);

    if store.get(from).is_none() {
        eprintln!("Error: snapshot {} not found", from);
        return 1;
    }
    if store.get(to).is_none() {
        eprintln!("Error: snapshot {} not found", to);
        return 1;
    }

    println!("Undoing changes between snapshots {} and {}.", from, to);
    // On a real system, this would iterate changes and reverse them.
    0
}

fn cmd_rollback(config_name: &str, args: &[String]) -> i32 {
    let cfg = match load_config(config_name) {
        Some(c) => c,
        None => SnapperConfig::new(config_name, "/"),
    };
    let store = load_snapshots(&cfg);

    let target = if args.is_empty() {
        // Roll back to the most recent snapshot
        match store.list().last() {
            Some(s) => s.number,
            None => {
                eprintln!("Error: no snapshots available for rollback");
                return 1;
            }
        }
    } else {
        match args[0].parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("Error: invalid snapshot number '{}'", args[0]);
                return 1;
            }
        }
    };

    if store.get(target).is_none() {
        eprintln!("Error: snapshot {} not found", target);
        return 1;
    }

    println!("Rolling back to snapshot {}.", target);
    // On a real system, this would create a read-write snapshot from the
    // target and switch the default subvolume.
    0
}

fn cmd_cleanup(config_name: &str, args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("Error: cleanup algorithm required (timeline, number, empty-pre-post)");
        return 1;
    }

    let algo = &args[0];
    let cfg = match load_config(config_name) {
        Some(c) => c,
        None => SnapperConfig::new(config_name, "/"),
    };
    let mut store = load_snapshots(&cfg);

    let deleted = match algo.as_str() {
        "timeline" => store.cleanup_timeline(),
        "number" => store.cleanup_number(),
        "empty-pre-post" => {
            store.cleanup_empty_pre_post(&|a, b| compute_status(&cfg, &Snapshot::new(a, SnapType::Single), &Snapshot::new(b, SnapType::Single)))
        }
        _ => {
            eprintln!("Error: unknown cleanup algorithm '{}'", algo);
            return 1;
        }
    };

    if deleted.is_empty() {
        println!("No snapshots to clean up.");
    } else {
        println!("Deleted {} snapshot(s): {:?}", deleted.len(), deleted);
    }
    0
}

// ============================================================================
// Personality: snapper-timeline
// ============================================================================

fn run_timeline(args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        print_timeline_usage();
        return 0;
    }
    if has_flag(args, "--version") || has_flag(args, "-V") {
        println!("snapper-timeline {}", VERSION);
        return 0;
    }

    let config_name = extract_flag(args, "--config")
        .or_else(|| extract_flag(args, "-c"))
        .unwrap_or("root");

    let once = has_flag(args, "--once");

    println!("snapper-timeline: starting for config '{}'", config_name);

    let cfg = match load_config(config_name) {
        Some(c) => c,
        None => SnapperConfig::new(config_name, "/"),
    };

    if !cfg.timeline_create {
        println!("Timeline creation is disabled for config '{}'.", config_name);
        return 0;
    }

    // Create a timeline snapshot
    let mut store = load_snapshots(&cfg);
    let num = store.create_with_date(
        SnapType::Single,
        now_secs(),
        "timeline",
        CleanupAlgo::Timeline,
    );
    println!("Created timeline snapshot {}.", num);

    // Run timeline cleanup
    if cfg.timeline_cleanup {
        let deleted = store.cleanup_timeline();
        if !deleted.is_empty() {
            println!("Cleaned up {} old timeline snapshot(s).", deleted.len());
        }
    }

    if !once {
        println!(
            "Timeline daemon would sleep {} seconds between snapshots.",
            TIMELINE_INTERVAL_SECS
        );
        // On a real system, this would loop with sleep.
    }

    0
}

fn print_timeline_usage() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(
        out,
        "snapper-timeline {} - Automatic timeline snapshot daemon",
        VERSION
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Usage: snapper-timeline [OPTIONS]");
    let _ = writeln!(out);
    let _ = writeln!(out, "Options:");
    let _ = writeln!(out, "  --config <name>, -c <name>  Config to use (default: root)");
    let _ = writeln!(out, "  --once                      Create one snapshot and exit");
    let _ = writeln!(out, "  --help, -h                  Show this help");
    let _ = writeln!(out, "  --version, -V               Show version");
}

// ============================================================================
// Personality: snapper-cleanup
// ============================================================================

fn run_cleanup(args: &[String]) -> i32 {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        print_cleanup_usage();
        return 0;
    }
    if has_flag(args, "--version") || has_flag(args, "-V") {
        println!("snapper-cleanup {}", VERSION);
        return 0;
    }

    let config_name = extract_flag(args, "--config")
        .or_else(|| extract_flag(args, "-c"))
        .unwrap_or("root");

    let algo = extract_flag(args, "--algorithm")
        .or_else(|| extract_flag(args, "-a"));

    println!("snapper-cleanup: running for config '{}'", config_name);

    let cfg = match load_config(config_name) {
        Some(c) => c,
        None => SnapperConfig::new(config_name, "/"),
    };
    let mut store = load_snapshots(&cfg);

    let mut total_deleted = 0usize;

    match algo {
        Some("timeline") => {
            let deleted = store.cleanup_timeline();
            total_deleted += deleted.len();
        }
        Some("number") => {
            let deleted = store.cleanup_number();
            total_deleted += deleted.len();
        }
        Some("empty-pre-post") => {
            let deleted = store.cleanup_empty_pre_post(&|a, b| {
                compute_status(
                    &cfg,
                    &Snapshot::new(a, SnapType::Single),
                    &Snapshot::new(b, SnapType::Single),
                )
            });
            total_deleted += deleted.len();
        }
        Some(a) => {
            eprintln!("Error: unknown algorithm '{}'", a);
            return 1;
        }
        None => {
            // Run all enabled algorithms
            if cfg.timeline_cleanup {
                let deleted = store.cleanup_timeline();
                total_deleted += deleted.len();
            }
            if cfg.number_cleanup {
                let deleted = store.cleanup_number();
                total_deleted += deleted.len();
            }
            if cfg.empty_pre_post_cleanup {
                let deleted = store.cleanup_empty_pre_post(&|a, b| {
                    compute_status(
                        &cfg,
                        &Snapshot::new(a, SnapType::Single),
                        &Snapshot::new(b, SnapType::Single),
                    )
                });
                total_deleted += deleted.len();
            }
        }
    }

    println!("Cleanup complete: {} snapshot(s) deleted.", total_deleted);
    0
}

fn print_cleanup_usage() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(
        out,
        "snapper-cleanup {} - Cleanup old snapshots per retention policies",
        VERSION
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Usage: snapper-cleanup [OPTIONS]");
    let _ = writeln!(out);
    let _ = writeln!(out, "Options:");
    let _ = writeln!(out, "  --config <name>, -c <name>          Config (default: root)");
    let _ = writeln!(
        out,
        "  --algorithm <algo>, -a <algo>       Run specific algorithm"
    );
    let _ = writeln!(
        out,
        "                                      (timeline, number, empty-pre-post)"
    );
    let _ = writeln!(out, "  --help, -h                          Show this help");
    let _ = writeln!(out, "  --version, -V                       Show version");
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("snapper");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "snapper-timeline" => run_timeline(&rest),
        "snapper-cleanup" => run_cleanup(&rest),
        _ => run_snapper(&rest),
    };

    process::exit(code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Time helpers ----

    #[test]
    fn test_is_leap_year_2000() {
        assert!(is_leap_year(2000));
    }

    #[test]
    fn test_is_leap_year_1900() {
        assert!(!is_leap_year(1900));
    }

    #[test]
    fn test_is_leap_year_2024() {
        assert!(is_leap_year(2024));
    }

    #[test]
    fn test_is_leap_year_2023() {
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn test_days_in_month_jan() {
        assert_eq!(days_in_month(2024, 1), 31);
    }

    #[test]
    fn test_days_in_month_feb_leap() {
        assert_eq!(days_in_month(2024, 2), 29);
    }

    #[test]
    fn test_days_in_month_feb_nonleap() {
        assert_eq!(days_in_month(2023, 2), 28);
    }

    #[test]
    fn test_days_in_month_apr() {
        assert_eq!(days_in_month(2024, 4), 30);
    }

    #[test]
    fn test_days_in_month_dec() {
        assert_eq!(days_in_month(2024, 12), 31);
    }

    #[test]
    fn test_secs_to_broken_epoch() {
        let bt = secs_to_broken(0);
        assert_eq!(bt.year, 1970);
        assert_eq!(bt.month, 1);
        assert_eq!(bt.day, 1);
        assert_eq!(bt.hour, 0);
    }

    #[test]
    fn test_secs_to_broken_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let bt = secs_to_broken(1704067200);
        assert_eq!(bt.year, 2024);
        assert_eq!(bt.month, 1);
        assert_eq!(bt.day, 1);
    }

    #[test]
    fn test_secs_to_broken_midday() {
        // 1970-01-01 12:30:45 = 45045 seconds
        let bt = secs_to_broken(45045);
        assert_eq!(bt.hour, 12);
        assert_eq!(bt._minute, 30);
        assert_eq!(bt._second, 45);
    }

    #[test]
    fn test_format_timestamp_epoch() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00");
    }

    #[test]
    fn test_format_timestamp_known() {
        let ts = format_timestamp(1704067200);
        assert!(ts.starts_with("2024-01-01"));
    }

    #[test]
    fn test_week_of_year_jan_1() {
        let bt = BrokenTime {
            year: 2024,
            month: 1,
            day: 1,
            hour: 0,
            _minute: 0,
            _second: 0,
        };
        assert_eq!(week_of_year(&bt), 1);
    }

    #[test]
    fn test_week_of_year_dec_31() {
        let bt = BrokenTime {
            year: 2024,
            month: 12,
            day: 31,
            hour: 0,
            _minute: 0,
            _second: 0,
        };
        let w = week_of_year(&bt);
        assert!(w >= 52);
    }

    // ---- SnapType ----

    #[test]
    fn test_snap_type_from_str_single() {
        assert_eq!(SnapType::from_str("single"), Some(SnapType::Single));
    }

    #[test]
    fn test_snap_type_from_str_pre() {
        assert_eq!(SnapType::from_str("pre"), Some(SnapType::Pre));
    }

    #[test]
    fn test_snap_type_from_str_post() {
        assert_eq!(SnapType::from_str("post"), Some(SnapType::Post));
    }

    #[test]
    fn test_snap_type_from_str_invalid() {
        assert_eq!(SnapType::from_str("unknown"), None);
    }

    #[test]
    fn test_snap_type_as_str_single() {
        assert_eq!(SnapType::Single.as_str(), "single");
    }

    #[test]
    fn test_snap_type_as_str_pre() {
        assert_eq!(SnapType::Pre.as_str(), "pre");
    }

    #[test]
    fn test_snap_type_as_str_post() {
        assert_eq!(SnapType::Post.as_str(), "post");
    }

    #[test]
    fn test_snap_type_roundtrip() {
        for t in &[SnapType::Single, SnapType::Pre, SnapType::Post] {
            assert_eq!(SnapType::from_str(t.as_str()), Some(*t));
        }
    }

    // ---- CleanupAlgo ----

    #[test]
    fn test_cleanup_algo_timeline() {
        assert_eq!(CleanupAlgo::from_str("timeline"), CleanupAlgo::Timeline);
    }

    #[test]
    fn test_cleanup_algo_number() {
        assert_eq!(CleanupAlgo::from_str("number"), CleanupAlgo::Number);
    }

    #[test]
    fn test_cleanup_algo_empty_pre_post() {
        assert_eq!(
            CleanupAlgo::from_str("empty-pre-post"),
            CleanupAlgo::EmptyPrePost
        );
    }

    #[test]
    fn test_cleanup_algo_none() {
        assert_eq!(CleanupAlgo::from_str(""), CleanupAlgo::None);
    }

    #[test]
    fn test_cleanup_algo_explicit_none() {
        assert_eq!(CleanupAlgo::from_str("none"), CleanupAlgo::None);
    }

    #[test]
    fn test_cleanup_algo_unknown() {
        assert_eq!(CleanupAlgo::from_str("garbage"), CleanupAlgo::None);
    }

    #[test]
    fn test_cleanup_algo_as_str_timeline() {
        assert_eq!(CleanupAlgo::Timeline.as_str(), "timeline");
    }

    #[test]
    fn test_cleanup_algo_as_str_number() {
        assert_eq!(CleanupAlgo::Number.as_str(), "number");
    }

    #[test]
    fn test_cleanup_algo_as_str_empty() {
        assert_eq!(CleanupAlgo::None.as_str(), "");
    }

    // ---- Snapshot ----

    #[test]
    fn test_snapshot_new() {
        let snap = Snapshot::new(1, SnapType::Single);
        assert_eq!(snap.number, 1);
        assert_eq!(snap.snap_type, SnapType::Single);
        assert!(snap.description.is_empty());
        assert_eq!(snap.cleanup, CleanupAlgo::None);
        assert!(snap.userdata.is_empty());
        assert!(snap.pre_number.is_none());
    }

    #[test]
    fn test_snapshot_new_with_date() {
        let snap = Snapshot::new_with_date(5, SnapType::Pre, 1000);
        assert_eq!(snap.number, 5);
        assert_eq!(snap.date, 1000);
        assert_eq!(snap.snap_type, SnapType::Pre);
    }

    #[test]
    fn test_snapshot_serialize_basic() {
        let snap = Snapshot::new_with_date(1, SnapType::Single, 100);
        let s = snap.serialize();
        assert!(s.contains("number=1"));
        assert!(s.contains("date=100"));
        assert!(s.contains("type=single"));
    }

    #[test]
    fn test_snapshot_serialize_with_description() {
        let mut snap = Snapshot::new(1, SnapType::Single);
        snap.description = "test snapshot".to_string();
        let s = snap.serialize();
        assert!(s.contains("description=test snapshot"));
    }

    #[test]
    fn test_snapshot_serialize_with_pre_number() {
        let mut snap = Snapshot::new(2, SnapType::Post);
        snap.pre_number = Some(1);
        let s = snap.serialize();
        assert!(s.contains("pre_number=1"));
    }

    #[test]
    fn test_snapshot_serialize_with_userdata() {
        let mut snap = Snapshot::new(1, SnapType::Single);
        snap.userdata.insert("key1".to_string(), "val1".to_string());
        let s = snap.serialize();
        assert!(s.contains("userdata[key1]=val1"));
    }

    #[test]
    fn test_snapshot_parse_basic() {
        let data = "number=1\ndate=100\ntype=single\ndescription=\ncleanup=\n";
        let snap = Snapshot::parse(data).unwrap();
        assert_eq!(snap.number, 1);
        assert_eq!(snap.date, 100);
        assert_eq!(snap.snap_type, SnapType::Single);
    }

    #[test]
    fn test_snapshot_parse_with_pre_number() {
        let data = "number=2\ndate=200\ntype=post\npre_number=1\ndescription=\ncleanup=\n";
        let snap = Snapshot::parse(data).unwrap();
        assert_eq!(snap.snap_type, SnapType::Post);
        assert_eq!(snap.pre_number, Some(1));
    }

    #[test]
    fn test_snapshot_parse_with_userdata() {
        let data = "number=1\ndate=0\ntype=single\nuserdata[foo]=bar\n";
        let snap = Snapshot::parse(data).unwrap();
        assert_eq!(snap.userdata.get("foo"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_snapshot_parse_missing_number() {
        let data = "date=100\ntype=single\n";
        assert!(Snapshot::parse(data).is_none());
    }

    #[test]
    fn test_snapshot_parse_empty_lines() {
        let data = "\nnumber=1\n\ndate=100\n\ntype=pre\n\n";
        let snap = Snapshot::parse(data).unwrap();
        assert_eq!(snap.number, 1);
        assert_eq!(snap.snap_type, SnapType::Pre);
    }

    #[test]
    fn test_snapshot_roundtrip() {
        let mut snap = Snapshot::new_with_date(42, SnapType::Post, 999);
        snap.pre_number = Some(41);
        snap.description = "hello world".to_string();
        snap.cleanup = CleanupAlgo::Timeline;
        snap.userdata.insert("a".to_string(), "b".to_string());

        let serialized = snap.serialize();
        let parsed = Snapshot::parse(&serialized).unwrap();
        assert_eq!(parsed.number, 42);
        assert_eq!(parsed.date, 999);
        assert_eq!(parsed.snap_type, SnapType::Post);
        assert_eq!(parsed.pre_number, Some(41));
        assert_eq!(parsed.description, "hello world");
        assert_eq!(parsed.cleanup, CleanupAlgo::Timeline);
        assert_eq!(parsed.userdata.get("a"), Some(&"b".to_string()));
    }

    // ---- ChangeType ----

    #[test]
    fn test_change_type_as_str() {
        assert_eq!(ChangeType::Created.as_str(), "created");
        assert_eq!(ChangeType::Modified.as_str(), "modified");
        assert_eq!(ChangeType::Deleted.as_str(), "deleted");
        assert_eq!(ChangeType::TypeChanged.as_str(), "type changed");
    }

    #[test]
    fn test_change_type_short() {
        assert_eq!(ChangeType::Created.short(), "+");
        assert_eq!(ChangeType::Modified.short(), "c");
        assert_eq!(ChangeType::Deleted.short(), "-");
        assert_eq!(ChangeType::TypeChanged.short(), "t");
    }

    // ---- SnapperConfig ----

    #[test]
    fn test_config_new_defaults() {
        let cfg = SnapperConfig::new("root", "/");
        assert_eq!(cfg.name, "root");
        assert_eq!(cfg.subvolume, "/");
        assert_eq!(cfg.snapshot_dir, "/.snapshots");
        assert!(cfg.timeline_create);
        assert!(cfg.timeline_cleanup);
        assert_eq!(cfg.timeline_hourly, DEFAULT_TIMELINE_HOURLY);
        assert_eq!(cfg.timeline_daily, DEFAULT_TIMELINE_DAILY);
        assert_eq!(cfg.timeline_weekly, DEFAULT_TIMELINE_WEEKLY);
        assert_eq!(cfg.timeline_monthly, DEFAULT_TIMELINE_MONTHLY);
        assert_eq!(cfg.timeline_yearly, DEFAULT_TIMELINE_YEARLY);
        assert!(cfg.number_cleanup);
        assert_eq!(cfg.number_limit, DEFAULT_NUMBER_LIMIT);
        assert!(cfg.empty_pre_post_cleanup);
    }

    #[test]
    fn test_config_new_trailing_slash() {
        let cfg = SnapperConfig::new("home", "/home/");
        assert_eq!(cfg.snapshot_dir, "/home/.snapshots");
    }

    #[test]
    fn test_config_new_no_trailing_slash() {
        let cfg = SnapperConfig::new("home", "/home");
        assert_eq!(cfg.snapshot_dir, "/home/.snapshots");
    }

    #[test]
    fn test_config_serialize() {
        let cfg = SnapperConfig::new("root", "/");
        let s = cfg.serialize();
        assert!(s.contains("SUBVOLUME=/"));
        assert!(s.contains("TIMELINE_CREATE=yes"));
        assert!(s.contains("NUMBER_LIMIT=50"));
    }

    #[test]
    fn test_config_parse() {
        let data = "SUBVOLUME=/data\nSNAPSHOT_DIR=/data/.snapshots\nTIMELINE_CREATE=no\nNUMBER_LIMIT=100\n";
        let cfg = SnapperConfig::parse("data", data).unwrap();
        assert_eq!(cfg.subvolume, "/data");
        assert!(!cfg.timeline_create);
        assert_eq!(cfg.number_limit, 100);
    }

    #[test]
    fn test_config_parse_with_comments() {
        let data = "# This is a comment\nSUBVOLUME=/\n# Another comment\nNUMBER_LIMIT=25\n";
        let cfg = SnapperConfig::parse("root", data).unwrap();
        assert_eq!(cfg.subvolume, "/");
        assert_eq!(cfg.number_limit, 25);
    }

    #[test]
    fn test_config_parse_empty() {
        let cfg = SnapperConfig::parse("root", "").unwrap();
        // Should get defaults
        assert_eq!(cfg.subvolume, "/");
    }

    #[test]
    fn test_config_roundtrip() {
        let mut cfg = SnapperConfig::new("test", "/mnt/test");
        cfg.timeline_hourly = 5;
        cfg.number_limit = 20;
        cfg.timeline_create = false;

        let serialized = cfg.serialize();
        let parsed = SnapperConfig::parse("test", &serialized).unwrap();
        assert_eq!(parsed.subvolume, "/mnt/test");
        assert_eq!(parsed.timeline_hourly, 5);
        assert_eq!(parsed.number_limit, 20);
        assert!(!parsed.timeline_create);
    }

    #[test]
    fn test_config_get_value_subvolume() {
        let cfg = SnapperConfig::new("root", "/myfs");
        assert_eq!(cfg.get_value("SUBVOLUME"), Some("/myfs".to_string()));
    }

    #[test]
    fn test_config_get_value_boolean() {
        let cfg = SnapperConfig::new("root", "/");
        assert_eq!(cfg.get_value("TIMELINE_CREATE"), Some("yes".to_string()));
    }

    #[test]
    fn test_config_get_value_number() {
        let cfg = SnapperConfig::new("root", "/");
        assert_eq!(
            cfg.get_value("NUMBER_LIMIT"),
            Some("50".to_string())
        );
    }

    #[test]
    fn test_config_get_value_unknown() {
        let cfg = SnapperConfig::new("root", "/");
        assert_eq!(cfg.get_value("NONEXISTENT"), None);
    }

    #[test]
    fn test_config_set_value_subvolume() {
        let mut cfg = SnapperConfig::new("root", "/");
        assert!(cfg.set_value("SUBVOLUME", "/new"));
        assert_eq!(cfg.subvolume, "/new");
    }

    #[test]
    fn test_config_set_value_boolean() {
        let mut cfg = SnapperConfig::new("root", "/");
        assert!(cfg.set_value("TIMELINE_CREATE", "no"));
        assert!(!cfg.timeline_create);
    }

    #[test]
    fn test_config_set_value_number() {
        let mut cfg = SnapperConfig::new("root", "/");
        assert!(cfg.set_value("NUMBER_LIMIT", "100"));
        assert_eq!(cfg.number_limit, 100);
    }

    #[test]
    fn test_config_set_value_unknown() {
        let mut cfg = SnapperConfig::new("root", "/");
        assert!(!cfg.set_value("NONEXISTENT", "val"));
    }

    #[test]
    fn test_config_set_value_invalid_number() {
        let mut cfg = SnapperConfig::new("root", "/");
        // Invalid parse should leave the value unchanged.
        cfg.set_value("NUMBER_LIMIT", "notanumber");
        assert_eq!(cfg.number_limit, DEFAULT_NUMBER_LIMIT);
    }

    // ---- SnapshotStore ----

    #[test]
    fn test_store_new_empty() {
        let store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn test_store_create_single() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        let num = store.create(SnapType::Single);
        assert_eq!(num, 1);
        assert_eq!(store.count(), 1);
    }

    #[test]
    fn test_store_create_increments() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        let n1 = store.create(SnapType::Single);
        let n2 = store.create(SnapType::Single);
        let n3 = store.create(SnapType::Single);
        assert_eq!(n1, 1);
        assert_eq!(n2, 2);
        assert_eq!(n3, 3);
    }

    #[test]
    fn test_store_create_full() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        let mut ud = HashMap::new();
        ud.insert("k".to_string(), "v".to_string());
        let num = store.create_full(
            SnapType::Pre,
            "before update",
            CleanupAlgo::Number,
            ud,
            None,
        );
        let snap = store.get(num).unwrap();
        assert_eq!(snap.description, "before update");
        assert_eq!(snap.cleanup, CleanupAlgo::Number);
        assert_eq!(snap.userdata.get("k"), Some(&"v".to_string()));
    }

    #[test]
    fn test_store_create_post_with_pre() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        let pre = store.create(SnapType::Pre);
        let post = store.create_full(
            SnapType::Post,
            "after update",
            CleanupAlgo::EmptyPrePost,
            HashMap::new(),
            Some(pre),
        );
        let snap = store.get(post).unwrap();
        assert_eq!(snap.pre_number, Some(pre));
    }

    #[test]
    fn test_store_get_existing() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        let num = store.create(SnapType::Single);
        assert!(store.get(num).is_some());
    }

    #[test]
    fn test_store_get_nonexistent() {
        let store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        assert!(store.get(999).is_none());
    }

    #[test]
    fn test_store_get_mut() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        let num = store.create(SnapType::Single);
        store.get_mut(num).unwrap().description = "modified".to_string();
        assert_eq!(store.get(num).unwrap().description, "modified");
    }

    #[test]
    fn test_store_delete_existing() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        let num = store.create(SnapType::Single);
        assert!(store.delete(num));
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn test_store_delete_nonexistent() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        assert!(!store.delete(999));
    }

    #[test]
    fn test_store_delete_many() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        let n1 = store.create(SnapType::Single);
        let _n2 = store.create(SnapType::Single);
        let n3 = store.create(SnapType::Single);
        let deleted = store.delete_many(&[n1, n3]);
        assert_eq!(deleted, 2);
        assert_eq!(store.count(), 1);
    }

    #[test]
    fn test_store_delete_many_none() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        store.create(SnapType::Single);
        let deleted = store.delete_many(&[999, 1000]);
        assert_eq!(deleted, 0);
    }

    #[test]
    fn test_store_list_sorted() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        store.create(SnapType::Single);
        store.create(SnapType::Single);
        store.create(SnapType::Single);
        let list = store.list();
        assert_eq!(list.len(), 3);
        assert!(list[0].number < list[1].number);
        assert!(list[1].number < list[2].number);
    }

    #[test]
    fn test_store_list_empty() {
        let store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        assert!(store.list().is_empty());
    }

    #[test]
    fn test_store_create_with_date() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        let num = store.create_with_date(
            SnapType::Single,
            12345,
            "timed",
            CleanupAlgo::Timeline,
        );
        let snap = store.get(num).unwrap();
        assert_eq!(snap.date, 12345);
        assert_eq!(snap.description, "timed");
        assert_eq!(snap.cleanup, CleanupAlgo::Timeline);
    }

    // ---- Cleanup: Number ----

    #[test]
    fn test_cleanup_number_under_limit() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.number_limit = 10;
        let mut store = SnapshotStore::new(cfg);
        for _ in 0..5 {
            let n = store.create(SnapType::Single);
            store.get_mut(n).unwrap().cleanup = CleanupAlgo::Number;
        }
        let deleted = store.cleanup_number();
        assert!(deleted.is_empty());
        assert_eq!(store.count(), 5);
    }

    #[test]
    fn test_cleanup_number_at_limit() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.number_limit = 3;
        let mut store = SnapshotStore::new(cfg);
        for _ in 0..3 {
            let n = store.create(SnapType::Single);
            store.get_mut(n).unwrap().cleanup = CleanupAlgo::Number;
        }
        let deleted = store.cleanup_number();
        assert!(deleted.is_empty());
        assert_eq!(store.count(), 3);
    }

    #[test]
    fn test_cleanup_number_over_limit() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.number_limit = 2;
        let mut store = SnapshotStore::new(cfg);
        for _ in 0..5 {
            let n = store.create(SnapType::Single);
            store.get_mut(n).unwrap().cleanup = CleanupAlgo::Number;
        }
        let deleted = store.cleanup_number();
        assert_eq!(deleted.len(), 3);
        assert_eq!(store.count(), 2);
        // Should keep the newest 2
        let remaining: Vec<u64> = store.list().iter().map(|s| s.number).collect();
        assert!(remaining.contains(&4));
        assert!(remaining.contains(&5));
    }

    #[test]
    fn test_cleanup_number_only_affects_numbered() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.number_limit = 1;
        let mut store = SnapshotStore::new(cfg);

        // 2 with Number cleanup, 2 with Timeline
        let n1 = store.create(SnapType::Single);
        store.get_mut(n1).unwrap().cleanup = CleanupAlgo::Number;
        let n2 = store.create(SnapType::Single);
        store.get_mut(n2).unwrap().cleanup = CleanupAlgo::Timeline;
        let n3 = store.create(SnapType::Single);
        store.get_mut(n3).unwrap().cleanup = CleanupAlgo::Number;
        let n4 = store.create(SnapType::Single);
        store.get_mut(n4).unwrap().cleanup = CleanupAlgo::Timeline;

        let deleted = store.cleanup_number();
        assert_eq!(deleted.len(), 1);
        assert!(deleted.contains(&n1));
        assert_eq!(store.count(), 3);
    }

    #[test]
    fn test_cleanup_number_disabled() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.number_cleanup = false;
        cfg.number_limit = 1;
        let mut store = SnapshotStore::new(cfg);
        let n = store.create(SnapType::Single);
        store.get_mut(n).unwrap().cleanup = CleanupAlgo::Number;
        let n2 = store.create(SnapType::Single);
        store.get_mut(n2).unwrap().cleanup = CleanupAlgo::Number;
        let deleted = store.cleanup_number();
        assert!(deleted.is_empty());
    }

    // ---- Cleanup: Timeline ----

    #[test]
    fn test_cleanup_timeline_disabled() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.timeline_cleanup = false;
        let mut store = SnapshotStore::new(cfg);
        let n = store.create(SnapType::Single);
        store.get_mut(n).unwrap().cleanup = CleanupAlgo::Timeline;
        let deleted = store.cleanup_timeline();
        assert!(deleted.is_empty());
    }

    #[test]
    fn test_cleanup_timeline_empty() {
        let cfg = SnapperConfig::new("root", "/");
        let mut store = SnapshotStore::new(cfg);
        let deleted = store.cleanup_timeline();
        assert!(deleted.is_empty());
    }

    #[test]
    fn test_cleanup_timeline_keeps_recent() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.timeline_hourly = 5;
        cfg.timeline_daily = 0;
        cfg.timeline_weekly = 0;
        cfg.timeline_monthly = 0;
        cfg.timeline_yearly = 0;
        let mut store = SnapshotStore::new(cfg);

        // Create 3 snapshots in different hours
        let base = 1704067200u64; // 2024-01-01 00:00:00
        for i in 0..3 {
            let num = store.create_with_date(
                SnapType::Single,
                base + i * 3600,
                "timeline",
                CleanupAlgo::Timeline,
            );
            let _ = num;
        }
        let deleted = store.cleanup_timeline();
        assert!(deleted.is_empty());
    }

    // ---- Cleanup: Empty Pre/Post ----

    #[test]
    fn test_cleanup_empty_pre_post_no_changes() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.empty_pre_post_cleanup = true;
        let mut store = SnapshotStore::new(cfg);

        let pre = store.create(SnapType::Pre);
        store.get_mut(pre).unwrap().cleanup = CleanupAlgo::EmptyPrePost;
        let post = store.create_full(
            SnapType::Post,
            "",
            CleanupAlgo::EmptyPrePost,
            HashMap::new(),
            Some(pre),
        );

        // Empty changes function => pair should be deleted
        let deleted = store.cleanup_empty_pre_post(&|_a, _b| Vec::new());
        assert!(deleted.contains(&pre));
        assert!(deleted.contains(&post));
    }

    #[test]
    fn test_cleanup_empty_pre_post_with_changes() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.empty_pre_post_cleanup = true;
        let mut store = SnapshotStore::new(cfg);

        let pre = store.create(SnapType::Pre);
        store.get_mut(pre).unwrap().cleanup = CleanupAlgo::EmptyPrePost;
        let _post = store.create_full(
            SnapType::Post,
            "",
            CleanupAlgo::EmptyPrePost,
            HashMap::new(),
            Some(pre),
        );

        // Non-empty changes => pair should be kept
        let deleted = store.cleanup_empty_pre_post(&|_a, _b| {
            vec![FileChange {
                change_type: ChangeType::Modified,
                path: "/etc/config".to_string(),
            }]
        });
        assert!(deleted.is_empty());
    }

    #[test]
    fn test_cleanup_empty_pre_post_disabled() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.empty_pre_post_cleanup = false;
        let mut store = SnapshotStore::new(cfg);

        let pre = store.create(SnapType::Pre);
        store.get_mut(pre).unwrap().cleanup = CleanupAlgo::EmptyPrePost;
        store.create_full(
            SnapType::Post,
            "",
            CleanupAlgo::EmptyPrePost,
            HashMap::new(),
            Some(pre),
        );

        let deleted = store.cleanup_empty_pre_post(&|_a, _b| Vec::new());
        assert!(deleted.is_empty());
    }

    // ---- Output formatting ----

    #[test]
    fn test_format_snapshot_table_header() {
        let table = format_snapshot_table(&[]);
        assert!(table.contains("#"));
        assert!(table.contains("Date"));
        assert!(table.contains("Type"));
    }

    #[test]
    fn test_format_snapshot_table_row() {
        let snap = Snapshot::new_with_date(1, SnapType::Single, 0);
        let table = format_snapshot_table(&[&snap]);
        assert!(table.contains("1"));
        assert!(table.contains("single"));
    }

    #[test]
    fn test_format_snapshot_csv_header() {
        let csv = format_snapshot_csv(&[]);
        assert!(csv.starts_with("number,date,type,pre_number,cleanup,description,userdata\n"));
    }

    #[test]
    fn test_format_snapshot_csv_row() {
        let snap = Snapshot::new_with_date(3, SnapType::Pre, 500);
        let csv = format_snapshot_csv(&[&snap]);
        assert!(csv.contains("3,500,pre,"));
    }

    #[test]
    fn test_format_config_table_header() {
        let table = format_config_table(&[]);
        assert!(table.contains("Config"));
        assert!(table.contains("Subvolume"));
    }

    #[test]
    fn test_format_config_table_row() {
        let cfg = SnapperConfig::new("root", "/");
        let table = format_config_table(&[&cfg]);
        assert!(table.contains("root"));
        assert!(table.contains("/"));
    }

    #[test]
    fn test_format_config_csv_header() {
        let csv = format_config_csv(&[]);
        assert!(csv.starts_with("config,subvolume\n"));
    }

    #[test]
    fn test_format_config_csv_row() {
        let cfg = SnapperConfig::new("home", "/home");
        let csv = format_config_csv(&[&cfg]);
        assert!(csv.contains("home,/home"));
    }

    #[test]
    fn test_format_config_detail() {
        let cfg = SnapperConfig::new("root", "/");
        let detail = format_config_detail(&cfg);
        assert!(detail.contains("SUBVOLUME = /"));
        assert!(detail.contains("NUMBER_LIMIT = 50"));
    }

    #[test]
    fn test_format_status_table_empty() {
        let out = format_status_table(&[]);
        assert!(out.contains("No changes."));
    }

    #[test]
    fn test_format_status_table_with_changes() {
        let changes = vec![
            FileChange {
                change_type: ChangeType::Created,
                path: "/new/file".to_string(),
            },
            FileChange {
                change_type: ChangeType::Deleted,
                path: "/old/file".to_string(),
            },
        ];
        let out = format_status_table(&changes);
        assert!(out.contains("+ /new/file"));
        assert!(out.contains("- /old/file"));
    }

    #[test]
    fn test_format_status_csv_header() {
        let csv = format_status_csv(&[]);
        assert!(csv.starts_with("change,path\n"));
    }

    #[test]
    fn test_format_status_csv_with_changes() {
        let changes = vec![FileChange {
            change_type: ChangeType::Modified,
            path: "/etc/passwd".to_string(),
        }];
        let csv = format_status_csv(&changes);
        assert!(csv.contains("modified,/etc/passwd"));
    }

    // ---- Argument parsing ----

    #[test]
    fn test_parse_range_valid() {
        assert_eq!(parse_range("1..5"), Some((1, 5)));
    }

    #[test]
    fn test_parse_range_same() {
        assert_eq!(parse_range("3..3"), Some((3, 3)));
    }

    #[test]
    fn test_parse_range_large() {
        assert_eq!(parse_range("100..999"), Some((100, 999)));
    }

    #[test]
    fn test_parse_range_invalid_no_dots() {
        assert_eq!(parse_range("1-5"), None);
    }

    #[test]
    fn test_parse_range_invalid_non_numeric() {
        assert_eq!(parse_range("a..b"), None);
    }

    #[test]
    fn test_parse_range_empty() {
        assert_eq!(parse_range(""), None);
    }

    #[test]
    fn test_parse_key_value_valid() {
        assert_eq!(
            parse_key_value("foo=bar"),
            Some(("foo".to_string(), "bar".to_string()))
        );
    }

    #[test]
    fn test_parse_key_value_empty_value() {
        assert_eq!(
            parse_key_value("key="),
            Some(("key".to_string(), "".to_string()))
        );
    }

    #[test]
    fn test_parse_key_value_no_equals() {
        assert_eq!(parse_key_value("nope"), None);
    }

    #[test]
    fn test_parse_key_value_multiple_equals() {
        assert_eq!(
            parse_key_value("a=b=c"),
            Some(("a".to_string(), "b=c".to_string()))
        );
    }

    #[test]
    fn test_extract_flag_present() {
        let args: Vec<String> = vec!["-n".into(), "root".into(), "-s".into(), "/".into()];
        assert_eq!(extract_flag(&args, "-n"), Some("root"));
    }

    #[test]
    fn test_extract_flag_absent() {
        let args: Vec<String> = vec!["-n".into(), "root".into()];
        assert_eq!(extract_flag(&args, "-s"), None);
    }

    #[test]
    fn test_extract_flag_last_no_value() {
        let args: Vec<String> = vec!["-n".into()];
        assert_eq!(extract_flag(&args, "-n"), None);
    }

    #[test]
    fn test_has_flag_present() {
        let args: Vec<String> = vec!["--csvout".into(), "list".into()];
        assert!(has_flag(&args, "--csvout"));
    }

    #[test]
    fn test_has_flag_absent() {
        let args: Vec<String> = vec!["list".into()];
        assert!(!has_flag(&args, "--csvout"));
    }

    #[test]
    fn test_collect_flag_values_multiple() {
        let args: Vec<String> = vec![
            "-u".into(), "a=1".into(),
            "-u".into(), "b=2".into(),
            "-d".into(), "desc".into(),
        ];
        let vals = collect_flag_values(&args, "-u");
        assert_eq!(vals, vec!["a=1", "b=2"]);
    }

    #[test]
    fn test_collect_flag_values_none() {
        let args: Vec<String> = vec!["-d".into(), "desc".into()];
        let vals = collect_flag_values(&args, "-u");
        assert!(vals.is_empty());
    }

    #[test]
    fn test_collect_flag_values_trailing() {
        let args: Vec<String> = vec!["-u".into()];
        let vals = collect_flag_values(&args, "-u");
        assert!(vals.is_empty());
    }

    // ---- Personality detection ----

    #[test]
    fn test_personality_bare_snapper() {
        let name = detect_personality("snapper");
        assert_eq!(name, "snapper");
    }

    #[test]
    fn test_personality_with_path() {
        let name = detect_personality("/usr/bin/snapper");
        assert_eq!(name, "snapper");
    }

    #[test]
    fn test_personality_with_windows_path() {
        let name = detect_personality("C:\\Program Files\\snapper.exe");
        assert_eq!(name, "snapper");
    }

    #[test]
    fn test_personality_timeline() {
        let name = detect_personality("snapper-timeline");
        assert_eq!(name, "snapper-timeline");
    }

    #[test]
    fn test_personality_cleanup() {
        let name = detect_personality("/usr/sbin/snapper-cleanup.exe");
        assert_eq!(name, "snapper-cleanup");
    }

    #[test]
    fn test_personality_mixed_separators() {
        let name = detect_personality("/usr/local\\bin/snapper-timeline");
        assert_eq!(name, "snapper-timeline");
    }

    // ---- bool_to_yesno ----

    #[test]
    fn test_bool_to_yesno_true() {
        assert_eq!(bool_to_yesno(true), "yes");
    }

    #[test]
    fn test_bool_to_yesno_false() {
        assert_eq!(bool_to_yesno(false), "no");
    }

    // ---- Edge cases ----

    #[test]
    fn test_snapshot_parse_whitespace_lines() {
        let data = "  number=7  \n  date=0  \n  type=single  \n";
        let snap = Snapshot::parse(data).unwrap();
        assert_eq!(snap.number, 7);
    }

    #[test]
    fn test_config_set_all_timeline_limits() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.set_value("TIMELINE_LIMIT_HOURLY", "1");
        cfg.set_value("TIMELINE_LIMIT_DAILY", "2");
        cfg.set_value("TIMELINE_LIMIT_WEEKLY", "3");
        cfg.set_value("TIMELINE_LIMIT_MONTHLY", "4");
        cfg.set_value("TIMELINE_LIMIT_YEARLY", "5");
        assert_eq!(cfg.timeline_hourly, 1);
        assert_eq!(cfg.timeline_daily, 2);
        assert_eq!(cfg.timeline_weekly, 3);
        assert_eq!(cfg.timeline_monthly, 4);
        assert_eq!(cfg.timeline_yearly, 5);
    }

    #[test]
    fn test_store_multiple_types() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        store.create(SnapType::Single);
        store.create(SnapType::Pre);
        store.create(SnapType::Post);
        assert_eq!(store.count(), 3);
        assert_eq!(store.get(1).unwrap().snap_type, SnapType::Single);
        assert_eq!(store.get(2).unwrap().snap_type, SnapType::Pre);
        assert_eq!(store.get(3).unwrap().snap_type, SnapType::Post);
    }

    #[test]
    fn test_store_delete_then_create() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        let n1 = store.create(SnapType::Single);
        store.delete(n1);
        let n2 = store.create(SnapType::Single);
        // Numbers keep incrementing, never reuse
        assert_eq!(n2, 2);
        assert_eq!(store.count(), 1);
    }

    #[test]
    fn test_store_cleanup_number_zero_limit() {
        let mut cfg = SnapperConfig::new("root", "/");
        cfg.number_limit = 0;
        let mut store = SnapshotStore::new(cfg);
        let n = store.create(SnapType::Single);
        store.get_mut(n).unwrap().cleanup = CleanupAlgo::Number;
        let deleted = store.cleanup_number();
        assert_eq!(deleted.len(), 1);
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn test_snapshot_parse_cleanup_types() {
        let data = "number=1\ndate=0\ntype=single\ncleanup=timeline\n";
        let snap = Snapshot::parse(data).unwrap();
        assert_eq!(snap.cleanup, CleanupAlgo::Timeline);

        let data2 = "number=2\ndate=0\ntype=single\ncleanup=number\n";
        let snap2 = Snapshot::parse(data2).unwrap();
        assert_eq!(snap2.cleanup, CleanupAlgo::Number);
    }

    #[test]
    fn test_snapshot_parse_unknown_keys_ignored() {
        let data = "number=1\ndate=0\ntype=single\nfuture_key=future_val\n";
        let snap = Snapshot::parse(data).unwrap();
        assert_eq!(snap.number, 1);
    }

    #[test]
    fn test_config_parse_unknown_keys_ignored() {
        let data = "SUBVOLUME=/\nFUTURE_KEY=future_val\n";
        let cfg = SnapperConfig::parse("root", data).unwrap();
        assert_eq!(cfg.subvolume, "/");
    }

    #[test]
    fn test_format_snapshot_csv_with_userdata() {
        let mut snap = Snapshot::new_with_date(1, SnapType::Single, 0);
        snap.userdata.insert("important".to_string(), "yes".to_string());
        let csv = format_snapshot_csv(&[&snap]);
        assert!(csv.contains("important=yes"));
    }

    #[test]
    fn test_snapshot_serialize_cleanup_algo() {
        let mut snap = Snapshot::new(1, SnapType::Single);
        snap.cleanup = CleanupAlgo::EmptyPrePost;
        let s = snap.serialize();
        assert!(s.contains("cleanup=empty-pre-post"));
    }

    #[test]
    fn test_config_get_all_known_keys() {
        let cfg = SnapperConfig::new("root", "/");
        let keys = [
            "SUBVOLUME", "SNAPSHOT_DIR", "TIMELINE_CREATE", "TIMELINE_CLEANUP",
            "TIMELINE_LIMIT_HOURLY", "TIMELINE_LIMIT_DAILY", "TIMELINE_LIMIT_WEEKLY",
            "TIMELINE_LIMIT_MONTHLY", "TIMELINE_LIMIT_YEARLY", "NUMBER_CLEANUP",
            "NUMBER_LIMIT", "EMPTY_PRE_POST_CLEANUP",
        ];
        for key in &keys {
            assert!(cfg.get_value(key).is_some(), "key {} should exist", key);
        }
    }

    #[test]
    fn test_format_snapshot_table_post_with_pre() {
        let mut snap = Snapshot::new_with_date(2, SnapType::Post, 0);
        snap.pre_number = Some(1);
        let table = format_snapshot_table(&[&snap]);
        assert!(table.contains("post"));
        assert!(table.contains("1"));
    }

    #[test]
    fn test_days_in_month_all_months() {
        // Non-leap year
        let expected = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for (i, &exp) in expected.iter().enumerate() {
            assert_eq!(days_in_month(2023, (i + 1) as u32), exp);
        }
    }

    #[test]
    fn test_secs_to_broken_end_of_day() {
        // 1970-01-01 23:59:59 = 86399
        let bt = secs_to_broken(86399);
        assert_eq!(bt.hour, 23);
        assert_eq!(bt._minute, 59);
        assert_eq!(bt._second, 59);
    }

    #[test]
    fn test_secs_to_broken_day_boundary() {
        // 1970-01-02 00:00:00 = 86400
        let bt = secs_to_broken(86400);
        assert_eq!(bt.year, 1970);
        assert_eq!(bt.month, 1);
        assert_eq!(bt.day, 2);
        assert_eq!(bt.hour, 0);
    }

    #[test]
    fn test_store_list_after_partial_delete() {
        let mut store = SnapshotStore::new(SnapperConfig::new("root", "/"));
        store.create(SnapType::Single);
        let n2 = store.create(SnapType::Single);
        store.create(SnapType::Single);
        store.delete(n2);
        let list = store.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].number, 1);
        assert_eq!(list[1].number, 3);
    }

    /// Helper for personality detection tests.
    fn detect_personality(argv0: &str) -> String {
        let s = argv0;
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    }
}
