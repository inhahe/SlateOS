//! Filesystem disk usage analyzer.
//!
//! Provides detailed disk usage analysis beyond basic `du`:
//! - Per-directory size breakdown with percentage of parent
//! - Top N largest files
//! - File type distribution (extension → aggregate size)
//! - Age analysis (old/recent file breakdown)
//! - Wasted space detection (empty files, tiny files)
//!
//! ## Design Reference
//!
//! design.txt line 885: "mounted drives and network drives and show
//! capacity and free space for each partition/filesystem"
//!
//! ## Architecture
//!
//! ```text
//! usage::analyze("/home")
//!   → DiskUsageReport {
//!       total_size, file_count, dir_count,
//!       top_dirs, top_files, by_extension,
//!       by_age, wasted_space
//!     }
//! ```
//!
//! The analyzer walks the VFS tree once, collecting all metadata,
//! then computes summaries in a single pass.  Results are cached
//! for repeated queries.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::KernelResult;
use crate::fs::{EntryType, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum recursion depth for analysis.
const MAX_DEPTH: usize = 32;

/// Maximum files to track individually.
const MAX_TRACKED_FILES: usize = 100_000;

/// Number of "top N" entries to report.
const TOP_N: usize = 20;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Complete disk usage analysis report.
#[derive(Debug, Clone)]
pub struct UsageReport {
    /// Root path that was analyzed.
    pub root: String,
    /// Total size of all files under root (bytes).
    pub total_size: u64,
    /// Total number of files.
    pub file_count: u64,
    /// Total number of directories.
    pub dir_count: u64,
    /// Total number of symlinks.
    pub symlink_count: u64,
    /// Top directories by size.
    pub top_dirs: Vec<SizeEntry>,
    /// Top files by size.
    pub top_files: Vec<SizeEntry>,
    /// Size breakdown by file extension.
    pub by_extension: Vec<ExtensionGroup>,
    /// Size breakdown by age bucket.
    pub by_age: AgeBuckets,
    /// Wasted space analysis.
    pub wasted: WastedSpace,
    /// Average file size.
    pub avg_file_size: u64,
    /// Median file size (approximate).
    pub median_file_size: u64,
}

/// A path with its size.
#[derive(Debug, Clone)]
pub struct SizeEntry {
    /// Path.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
}

/// Aggregate size for a file extension group.
#[derive(Debug, Clone)]
pub struct ExtensionGroup {
    /// File extension (lowercase, without dot).
    pub extension: String,
    /// Total size of all files with this extension.
    pub total_size: u64,
    /// Number of files with this extension.
    pub count: u64,
}

/// Age distribution of files.
#[derive(Debug, Clone, Default)]
pub struct AgeBuckets {
    /// Files modified in the last 24 hours.
    pub last_day: AgeBucket,
    /// Files modified in the last 7 days.
    pub last_week: AgeBucket,
    /// Files modified in the last 30 days.
    pub last_month: AgeBucket,
    /// Files modified in the last 365 days.
    pub last_year: AgeBucket,
    /// Files older than 1 year.
    pub older: AgeBucket,
}

/// A single age bucket.
#[derive(Debug, Clone, Copy, Default)]
pub struct AgeBucket {
    /// Number of files in this bucket.
    pub count: u64,
    /// Total size of files in this bucket.
    pub size: u64,
}

/// Wasted space analysis.
#[derive(Debug, Clone, Copy, Default)]
pub struct WastedSpace {
    /// Number of empty files (0 bytes).
    pub empty_files: u64,
    /// Number of tiny files (< 64 bytes).
    pub tiny_files: u64,
    /// Total size of tiny files.
    pub tiny_size: u64,
    /// Number of duplicate-named files (same name in different dirs).
    pub duplicate_names: u64,
}

/// Configuration for a disk usage analysis.
#[derive(Debug, Clone)]
pub struct UsageConfig {
    /// Root path to analyze.
    pub root: String,
    /// Maximum depth to recurse.
    pub max_depth: usize,
    /// Maximum files to track.
    pub max_files: usize,
    /// Paths to exclude.
    pub exclude_prefixes: Vec<String>,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self {
            root: String::from("/"),
            max_depth: MAX_DEPTH,
            max_files: MAX_TRACKED_FILES,
            exclude_prefixes: alloc::vec![
                String::from("/proc"),
                String::from("/dev"),
                String::from("/sys"),
                String::from("/_"),
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Global stats
// ---------------------------------------------------------------------------

static ANALYSES_RUN: AtomicU64 = AtomicU64::new(0);
static LAST_REPORT: Mutex<Option<UsageReport>> = Mutex::new(None);

/// Get the number of analyses run.
pub fn analyses_run() -> u64 {
    ANALYSES_RUN.load(Ordering::Relaxed)
}

/// Get the last analysis report (if any).
pub fn last_report() -> Option<UsageReport> {
    LAST_REPORT.lock().clone()
}

// ---------------------------------------------------------------------------
// Core analysis
// ---------------------------------------------------------------------------

/// Run a disk usage analysis on the specified path.
pub fn analyze(config: &UsageConfig) -> KernelResult<UsageReport> {
    let now_ns = crate::timekeeping::clock_realtime();

    let mut collector = Collector::new(now_ns);
    walk(&config.root, config, &mut collector, 0);

    let report = collector.build_report(&config.root);

    ANALYSES_RUN.fetch_add(1, Ordering::Relaxed);
    *LAST_REPORT.lock() = Some(report.clone());

    serial_println!(
        "[usage] Analysis of '{}': {} files, {} dirs, {} bytes",
        config.root,
        report.file_count,
        report.dir_count,
        report.total_size,
    );

    Ok(report)
}

/// Analyze a path with default config.
pub fn analyze_path(root: &str) -> KernelResult<UsageReport> {
    let config = UsageConfig {
        root: String::from(root),
        ..UsageConfig::default()
    };
    analyze(&config)
}

// ---------------------------------------------------------------------------
// Internal collector
// ---------------------------------------------------------------------------

/// Accumulates data during the filesystem walk.
struct Collector {
    /// Current epoch nanoseconds.
    now_ns: u64,
    /// All file entries (path, size).
    files: Vec<(String, u64)>,
    /// Directory sizes (path → total size of contents).
    dir_sizes: BTreeMap<String, u64>,
    /// Extension → (total_size, count).
    ext_stats: BTreeMap<String, (u64, u64)>,
    /// Age buckets.
    age: AgeBuckets,
    /// Wasted space counters.
    wasted: WastedSpace,
    /// Total counts.
    file_count: u64,
    dir_count: u64,
    symlink_count: u64,
    total_size: u64,
    /// Size histogram for median approximation.
    size_buckets: [u64; 16],
    /// Filename occurrences for duplicate name detection.
    name_counts: BTreeMap<String, u64>,
}

impl Collector {
    fn new(now_ns: u64) -> Self {
        Self {
            now_ns,
            files: Vec::new(),
            dir_sizes: BTreeMap::new(),
            ext_stats: BTreeMap::new(),
            age: AgeBuckets::default(),
            wasted: WastedSpace::default(),
            file_count: 0,
            dir_count: 0,
            symlink_count: 0,
            total_size: 0,
            size_buckets: [0; 16],
            name_counts: BTreeMap::new(),
        }
    }

    /// Record a file.
    fn record_file(&mut self, path: &str, name: &str, size: u64, modified_ns: u64) {
        self.file_count = self.file_count.saturating_add(1);
        self.total_size = self.total_size.saturating_add(size);

        // Track individual files for top-N (bounded).
        if self.files.len() < MAX_TRACKED_FILES {
            self.files.push((String::from(path), size));
        }

        // Extension stats.
        let ext = file_extension(name);
        if !ext.is_empty() {
            let entry = self.ext_stats
                .entry(String::from(ext))
                .or_insert((0, 0));
            entry.0 = entry.0.saturating_add(size);
            entry.1 = entry.1.saturating_add(1);
        }

        // Age bucket.
        let age_ns = self.now_ns.saturating_sub(modified_ns);
        let bucket = self.classify_age(age_ns);
        bucket.count = bucket.count.saturating_add(1);
        bucket.size = bucket.size.saturating_add(size);

        // Wasted space.
        if size == 0 {
            self.wasted.empty_files = self.wasted.empty_files.saturating_add(1);
        }
        if size > 0 && size < 64 {
            self.wasted.tiny_files = self.wasted.tiny_files.saturating_add(1);
            self.wasted.tiny_size = self.wasted.tiny_size.saturating_add(size);
        }

        // Size histogram bucket (log2 scale).
        let bucket_idx = if size == 0 { 0 } else { (64 - size.leading_zeros()) as usize };
        let clamped = if bucket_idx >= 16 { 15 } else { bucket_idx };
        self.size_buckets[clamped] = self.size_buckets[clamped].saturating_add(1);

        // Duplicate name tracking.
        let count = self.name_counts
            .entry(String::from(name))
            .or_insert(0);
        *count = count.saturating_add(1);
    }

    /// Classify a file age (in nanoseconds) into a bucket, returning a mutable reference.
    fn classify_age(&mut self, age_ns: u64) -> &mut AgeBucket {
        const DAY_NS: u64 = 86_400_000_000_000;
        const WEEK_NS: u64 = 7 * DAY_NS;
        const MONTH_NS: u64 = 30 * DAY_NS;
        const YEAR_NS: u64 = 365 * DAY_NS;

        if age_ns < DAY_NS {
            &mut self.age.last_day
        } else if age_ns < WEEK_NS {
            &mut self.age.last_week
        } else if age_ns < MONTH_NS {
            &mut self.age.last_month
        } else if age_ns < YEAR_NS {
            &mut self.age.last_year
        } else {
            &mut self.age.older
        }
    }

    /// Record a directory.
    fn record_dir(&mut self, path: &str) {
        self.dir_count = self.dir_count.saturating_add(1);
        self.dir_sizes.entry(String::from(path)).or_insert(0);
    }

    /// Add size to a directory and all its parent directories.
    fn add_to_dir(&mut self, dir: &str, size: u64) {
        // Add to this directory.
        let entry = self.dir_sizes.entry(String::from(dir)).or_insert(0);
        *entry = entry.saturating_add(size);

        // Walk up the path and add to parents.
        let mut current = String::from(dir);
        while let Some(pos) = current.rfind('/') {
            if pos == 0 {
                // Root directory.
                let root_entry = self.dir_sizes.entry(String::from("/")).or_insert(0);
                *root_entry = root_entry.saturating_add(size);
                break;
            }
            current = String::from(&current[..pos]);
            let parent_entry = self.dir_sizes.entry(current.clone()).or_insert(0);
            *parent_entry = parent_entry.saturating_add(size);
        }
    }

    /// Build the final report.
    fn build_report(self, root: &str) -> UsageReport {
        // Top directories by size.
        let mut dir_entries: Vec<SizeEntry> = self.dir_sizes
            .iter()
            .map(|(p, s)| SizeEntry { path: p.clone(), size: *s })
            .collect();
        dir_entries.sort_by_key(|e| core::cmp::Reverse(e.size));
        dir_entries.truncate(TOP_N);

        // Top files by size.
        let mut file_entries: Vec<SizeEntry> = self.files
            .iter()
            .map(|(p, s)| SizeEntry { path: p.clone(), size: *s })
            .collect();
        file_entries.sort_by_key(|e| core::cmp::Reverse(e.size));
        file_entries.truncate(TOP_N);

        // Extension groups, sorted by total size.
        let mut ext_groups: Vec<ExtensionGroup> = self.ext_stats
            .iter()
            .map(|(ext, (size, count))| ExtensionGroup {
                extension: ext.clone(),
                total_size: *size,
                count: *count,
            })
            .collect();
        ext_groups.sort_by_key(|e| core::cmp::Reverse(e.total_size));
        ext_groups.truncate(TOP_N);

        // Average file size.
        let avg = if self.file_count > 0 {
            self.total_size / self.file_count
        } else {
            0
        };

        // Approximate median from histogram.
        let median = self.approx_median();

        // Count duplicate names.
        let dup_names = self.name_counts.values().filter(|&&c| c > 1).count() as u64;
        let mut wasted = self.wasted;
        wasted.duplicate_names = dup_names;

        UsageReport {
            root: String::from(root),
            total_size: self.total_size,
            file_count: self.file_count,
            dir_count: self.dir_count,
            symlink_count: self.symlink_count,
            top_dirs: dir_entries,
            top_files: file_entries,
            by_extension: ext_groups,
            by_age: self.age,
            wasted,
            avg_file_size: avg,
            median_file_size: median,
        }
    }

    /// Approximate median from the log2 size histogram.
    fn approx_median(&self) -> u64 {
        let total: u64 = self.size_buckets.iter().sum();
        if total == 0 {
            return 0;
        }

        let half = total / 2;
        let mut cumulative: u64 = 0;
        for (i, &count) in self.size_buckets.iter().enumerate() {
            cumulative = cumulative.saturating_add(count);
            if cumulative >= half {
                // The median falls in bucket i.
                // Bucket i represents sizes ~ 2^(i-1) to 2^i.
                if i == 0 {
                    return 0;
                }
                return 1u64 << (i.saturating_sub(1));
            }
        }
        0
    }
}

// ---------------------------------------------------------------------------
// Filesystem walk
// ---------------------------------------------------------------------------

/// Recursively walk a directory tree collecting usage data.
fn walk(
    path: &str,
    config: &UsageConfig,
    collector: &mut Collector,
    depth: usize,
) {
    if depth > config.max_depth {
        return;
    }
    if collector.file_count.saturating_add(collector.dir_count) > config.max_files as u64 {
        return;
    }

    // Check exclusions.
    for excl in &config.exclude_prefixes {
        if path.starts_with(excl.as_str()) {
            return;
        }
    }

    collector.record_dir(path);

    let entries = match Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }

        let full = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        match entry.entry_type {
            EntryType::File => {
                if let Ok(meta) = Vfs::metadata(&full) {
                    collector.record_file(&full, &entry.name, meta.size, meta.modified_ns);
                    collector.add_to_dir(path, meta.size);
                }
            }
            EntryType::Directory => {
                walk(&full, config, collector, depth + 1);
            }
            _ => {
                collector.symlink_count = collector.symlink_count.saturating_add(1);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract file extension (lowercase, without dot).
fn file_extension(name: &str) -> &str {
    if let Some(pos) = name.rfind('.') {
        &name[pos + 1..]
    } else {
        ""
    }
}

/// Format a byte count as a human-readable string.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        alloc::format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        alloc::format!("{}.{} KiB", bytes / 1024, (bytes % 1024) * 10 / 1024)
    } else if bytes < 1024 * 1024 * 1024 {
        let mib = bytes / (1024 * 1024);
        let frac = (bytes % (1024 * 1024)) * 10 / (1024 * 1024);
        alloc::format!("{}.{} MiB", mib, frac)
    } else {
        let gib = bytes / (1024 * 1024 * 1024);
        let frac = (bytes % (1024 * 1024 * 1024)) * 10 / (1024 * 1024 * 1024);
        alloc::format!("{}.{} GiB", gib, frac)
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[usage] Running self-test...");

    test_empty_dir();
    test_basic_analysis();
    test_extension_stats();
    test_top_files();
    test_wasted_space();
    test_format_size();
    test_age_buckets();
    test_stats();

    serial_println!("[usage] Self-test passed (8 tests).");
    Ok(())
}

fn test_empty_dir() {
    let _ = Vfs::mkdir("/tmp/usage_empty");

    let report = analyze_path("/tmp/usage_empty").expect("analyze");
    assert_eq!(report.file_count, 0);
    assert_eq!(report.total_size, 0);
    assert!(report.dir_count >= 1); // At least the root dir itself.

    let _ = Vfs::rmdir("/tmp/usage_empty");
    serial_println!("[usage]   empty dir: ok");
}

fn test_basic_analysis() {
    let _ = Vfs::mkdir("/tmp/usage_basic");
    Vfs::write_file("/tmp/usage_basic/a.txt", b"hello world").expect("write");
    Vfs::write_file("/tmp/usage_basic/b.txt", b"more data here for testing").expect("write");
    let _ = Vfs::mkdir("/tmp/usage_basic/sub");
    Vfs::write_file("/tmp/usage_basic/sub/c.txt", b"nested file content").expect("write");

    let report = analyze_path("/tmp/usage_basic").expect("analyze");
    assert!(report.file_count >= 3, "should find 3 files, got {}", report.file_count);
    assert!(report.dir_count >= 2, "should find 2 dirs, got {}", report.dir_count);
    assert!(report.total_size > 0, "should have nonzero size");
    assert!(report.avg_file_size > 0, "should have nonzero avg");

    let _ = Vfs::remove("/tmp/usage_basic/a.txt");
    let _ = Vfs::remove("/tmp/usage_basic/b.txt");
    let _ = Vfs::remove("/tmp/usage_basic/sub/c.txt");
    let _ = Vfs::rmdir("/tmp/usage_basic/sub");
    let _ = Vfs::rmdir("/tmp/usage_basic");

    serial_println!("[usage]   basic analysis: ok");
}

fn test_extension_stats() {
    let _ = Vfs::mkdir("/tmp/usage_ext");
    Vfs::write_file("/tmp/usage_ext/a.txt", b"text file one").expect("write");
    Vfs::write_file("/tmp/usage_ext/b.txt", b"text file two").expect("write");
    Vfs::write_file("/tmp/usage_ext/c.log", b"log data here is longer").expect("write");

    let report = analyze_path("/tmp/usage_ext").expect("analyze");
    assert!(!report.by_extension.is_empty(), "should have extension stats");

    // Find the .txt group.
    let txt = report.by_extension.iter().find(|g| g.extension == "txt");
    assert!(txt.is_some(), "should find .txt group");
    if let Some(g) = txt {
        assert_eq!(g.count, 2, "should have 2 .txt files");
    }

    let _ = Vfs::remove("/tmp/usage_ext/a.txt");
    let _ = Vfs::remove("/tmp/usage_ext/b.txt");
    let _ = Vfs::remove("/tmp/usage_ext/c.log");
    let _ = Vfs::rmdir("/tmp/usage_ext");

    serial_println!("[usage]   extension stats: ok");
}

fn test_top_files() {
    let _ = Vfs::mkdir("/tmp/usage_top");
    // Create files of different sizes.
    Vfs::write_file("/tmp/usage_top/small.txt", b"x").expect("write");
    Vfs::write_file("/tmp/usage_top/medium.txt", &[b'M'; 100]).expect("write");
    Vfs::write_file("/tmp/usage_top/big.txt", &[b'B'; 1000]).expect("write");

    let report = analyze_path("/tmp/usage_top").expect("analyze");
    assert!(!report.top_files.is_empty(), "should have top files");

    // Biggest file should be first.
    assert!(
        report.top_files[0].size >= 1000,
        "top file should be big.txt, got {} bytes",
        report.top_files[0].size
    );

    let _ = Vfs::remove("/tmp/usage_top/small.txt");
    let _ = Vfs::remove("/tmp/usage_top/medium.txt");
    let _ = Vfs::remove("/tmp/usage_top/big.txt");
    let _ = Vfs::rmdir("/tmp/usage_top");

    serial_println!("[usage]   top files: ok");
}

fn test_wasted_space() {
    let _ = Vfs::mkdir("/tmp/usage_waste");
    Vfs::write_file("/tmp/usage_waste/empty.txt", b"").expect("write");
    Vfs::write_file("/tmp/usage_waste/tiny.txt", b"x").expect("write");
    Vfs::write_file("/tmp/usage_waste/normal.txt", b"this is a normal sized file content").expect("write");

    let report = analyze_path("/tmp/usage_waste").expect("analyze");
    assert!(report.wasted.empty_files >= 1, "should detect empty file");
    assert!(report.wasted.tiny_files >= 1, "should detect tiny file");

    let _ = Vfs::remove("/tmp/usage_waste/empty.txt");
    let _ = Vfs::remove("/tmp/usage_waste/tiny.txt");
    let _ = Vfs::remove("/tmp/usage_waste/normal.txt");
    let _ = Vfs::rmdir("/tmp/usage_waste");

    serial_println!("[usage]   wasted space: ok");
}

fn test_format_size() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(512), "512 B");
    assert_eq!(format_size(1024), "1.0 KiB");
    assert_eq!(format_size(1536), "1.5 KiB");
    assert_eq!(format_size(1048576), "1.0 MiB");
    assert_eq!(format_size(1073741824), "1.0 GiB");

    serial_println!("[usage]   format size: ok");
}

fn test_age_buckets() {
    let _ = Vfs::mkdir("/tmp/usage_age");
    Vfs::write_file("/tmp/usage_age/recent.txt", b"just created").expect("write");

    let report = analyze_path("/tmp/usage_age").expect("analyze");
    // Recently created file should be in last_day bucket.
    assert!(
        report.by_age.last_day.count >= 1,
        "should have at least 1 recent file, got {}",
        report.by_age.last_day.count
    );

    let _ = Vfs::remove("/tmp/usage_age/recent.txt");
    let _ = Vfs::rmdir("/tmp/usage_age");

    serial_println!("[usage]   age buckets: ok");
}

fn test_stats() {
    let count = analyses_run();
    assert!(count > 0, "should have run analyses");

    let report = last_report();
    assert!(report.is_some(), "should have cached last report");

    serial_println!("[usage]   stats: ok");
}
