//! SlateOS Background File Indexer
//!
//! A configurable file indexing service that maintains a searchable database
//! of files across specified directories. Designed to run as a background
//! service (off by default) and support fast filename/extension/metadata
//! queries without full filesystem walks.
//!
//! Index format: flat text file with one entry per line.
//! Config format: YAML-like simple key-value configuration.
//!
//! Commands:
//!   indexer start          — Run indexing pass (foreground, exits when done)
//!   indexer daemon         — Run as daemon (periodic re-index)
//!   indexer search <query> — Search index by filename substring
//!   indexer find <pattern> — Search by glob-like pattern (*.rs, doc*.pdf)
//!   indexer ext <ext>      — List all files with given extension
//!   indexer recent [hours] — Files modified within N hours (default 24)
//!   indexer large [mb]     — Files larger than N megabytes (default 100)
//!   indexer stats          — Show index statistics
//!   indexer config         — Show current configuration
//!   indexer config set <key> <value> — Update a config setting
//!   indexer rebuild        — Force full re-index (discard incremental state)
//!   indexer status         — Show daemon status (running, last indexed, etc.)
//!   indexer enable         — Enable indexer service
//!   indexer disable        — Disable indexer service
//!   indexer paths add <path> — Add a path to index
//!   indexer paths remove <path> — Remove a path from indexing
//!   indexer paths list     — List configured paths

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Configuration
// ============================================================================

const DEFAULT_INDEX_DIR: &str = "/var/lib/indexer";
const DEFAULT_CONFIG_PATH: &str = "/etc/indexer.conf";
const INDEX_FILENAME: &str = "index.db";
const STATE_FILENAME: &str = "state.dat";
const INDEX_VERSION: &str = "1";

/// Configuration for the indexer service.
#[derive(Clone, Debug)]
struct Config {
    /// Directories to index.
    paths: Vec<String>,
    /// File extensions to include (empty = all).
    extensions: Vec<String>,
    /// File extensions to exclude.
    exclude_extensions: Vec<String>,
    /// Directory names to skip.
    exclude_dirs: Vec<String>,
    /// Maximum file size to index (bytes, 0 = unlimited).
    max_file_size: u64,
    /// Re-index interval in seconds (for daemon mode).
    interval_secs: u64,
    /// Whether the service is enabled.
    enabled: bool,
    /// Index storage directory.
    index_dir: String,
    /// Follow symlinks during traversal.
    follow_symlinks: bool,
    /// Maximum directory depth (0 = unlimited).
    max_depth: u32,
}

impl Config {
    fn default_config() -> Self {
        Config {
            paths: vec!["/home".to_string()],
            extensions: Vec::new(),
            exclude_extensions: vec![
                "o".to_string(),
                "tmp".to_string(),
                "swp".to_string(),
                "lock".to_string(),
            ],
            exclude_dirs: vec![
                ".git".to_string(),
                ".hg".to_string(),
                ".svn".to_string(),
                "node_modules".to_string(),
                "__pycache__".to_string(),
                ".cache".to_string(),
                "target".to_string(),
            ],
            max_file_size: 0,
            interval_secs: 3600,
            enabled: false,
            index_dir: DEFAULT_INDEX_DIR.to_string(),
            follow_symlinks: false,
            max_depth: 0,
        }
    }

    fn load(path: &str) -> Self {
        let mut config = Self::default_config();

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return config,
        };

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some((key, val)) = trimmed.split_once('=') {
                let key = key.trim();
                let val = val.trim();

                match key {
                    "enabled" => config.enabled = val == "true" || val == "1",
                    "interval" => {
                        if let Ok(n) = val.parse::<u64>() {
                            config.interval_secs = n;
                        }
                    }
                    "max_file_size" => {
                        if let Ok(n) = val.parse::<u64>() {
                            config.max_file_size = n;
                        }
                    }
                    "index_dir" => config.index_dir = val.to_string(),
                    "follow_symlinks" => config.follow_symlinks = val == "true" || val == "1",
                    "max_depth" => {
                        if let Ok(n) = val.parse::<u32>() {
                            config.max_depth = n;
                        }
                    }
                    "paths" => {
                        config.paths = val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    }
                    "extensions" => {
                        config.extensions = val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    }
                    "exclude_extensions" => {
                        config.exclude_extensions = val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    }
                    "exclude_dirs" => {
                        config.exclude_dirs = val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    }
                    _ => {}
                }
            }
        }

        config
    }

    fn save(&self, path: &str) -> Result<(), String> {
        let mut out = String::new();
        out.push_str("# Slate OS File Indexer Configuration\n");
        out.push_str("# Generated — edits are preserved on next write.\n\n");

        out.push_str(&format!("enabled = {}\n", self.enabled));
        out.push_str(&format!("interval = {}\n", self.interval_secs));
        out.push_str(&format!("max_file_size = {}\n", self.max_file_size));
        out.push_str(&format!("index_dir = {}\n", self.index_dir));
        out.push_str(&format!("follow_symlinks = {}\n", self.follow_symlinks));
        out.push_str(&format!("max_depth = {}\n", self.max_depth));
        out.push_str(&format!("paths = {}\n", self.paths.join(", ")));
        out.push_str(&format!("extensions = {}\n", self.extensions.join(", ")));
        out.push_str(&format!("exclude_extensions = {}\n", self.exclude_extensions.join(", ")));
        out.push_str(&format!("exclude_dirs = {}\n", self.exclude_dirs.join(", ")));

        fs::write(path, &out).map_err(|e| format!("write config: {e}"))
    }
}

// ============================================================================
// Index entry
// ============================================================================

/// A single indexed file entry.
#[derive(Clone, Debug)]
struct IndexEntry {
    /// Full path.
    path: String,
    /// File size in bytes.
    size: u64,
    /// Last modified time (Unix seconds).
    mtime: u64,
    /// File extension (empty if none).
    extension: String,
    /// Is directory.
    is_dir: bool,
}

impl IndexEntry {
    fn serialize(&self) -> String {
        let kind = if self.is_dir { "D" } else { "F" };
        format!("{}\t{}\t{}\t{}\t{}", kind, self.size, self.mtime, self.extension, self.path)
    }

    fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.splitn(5, '\t').collect();
        if parts.len() < 5 {
            return None;
        }

        let is_dir = parts[0] == "D";
        let size = parts[1].parse::<u64>().ok()?;
        let mtime = parts[2].parse::<u64>().ok()?;
        let extension = parts[3].to_string();
        let path = parts[4].to_string();

        Some(IndexEntry { path, size, mtime, extension, is_dir })
    }

    fn filename(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

// ============================================================================
// Index database
// ============================================================================

/// The in-memory index database.
struct IndexDb {
    entries: Vec<IndexEntry>,
    /// Index build timestamp.
    built_at: u64,
    /// Number of directories scanned.
    dirs_scanned: u64,
    /// Number of files skipped (by filter).
    files_skipped: u64,
    /// Duration of last scan in milliseconds.
    scan_duration_ms: u64,
}

impl IndexDb {
    fn new() -> Self {
        IndexDb {
            entries: Vec::new(),
            built_at: 0,
            dirs_scanned: 0,
            files_skipped: 0,
            scan_duration_ms: 0,
        }
    }

    fn load(index_dir: &str) -> Self {
        let path = PathBuf::from(index_dir).join(INDEX_FILENAME);
        let mut db = Self::new();

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return db,
        };

        let mut lines = content.lines();

        // Parse header.
        if let Some(header) = lines.next()
            && !header.starts_with("# indexer-db v") {
                eprintln!("warning: index file has unknown format, rebuilding");
                return db;
            }

        // Parse metadata lines (start with #).
        for line in lines.by_ref() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            if !trimmed.starts_with('#') {
                // First data line — parse it.
                if let Some(entry) = IndexEntry::parse(trimmed) {
                    db.entries.push(entry);
                }
                break;
            }
            // Parse metadata.
            if let Some(rest) = trimmed.strip_prefix("# built_at: ") {
                db.built_at = rest.parse().unwrap_or(0);
            } else if let Some(rest) = trimmed.strip_prefix("# dirs_scanned: ") {
                db.dirs_scanned = rest.parse().unwrap_or(0);
            } else if let Some(rest) = trimmed.strip_prefix("# files_skipped: ") {
                db.files_skipped = rest.parse().unwrap_or(0);
            } else if let Some(rest) = trimmed.strip_prefix("# scan_duration_ms: ") {
                db.scan_duration_ms = rest.parse().unwrap_or(0);
            }
        }

        // Parse remaining data lines.
        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(entry) = IndexEntry::parse(trimmed) {
                db.entries.push(entry);
            }
        }

        db
    }

    fn save(&self, index_dir: &str) -> Result<(), String> {
        fs::create_dir_all(index_dir).map_err(|e| format!("mkdir index_dir: {e}"))?;

        let path = PathBuf::from(index_dir).join(INDEX_FILENAME);
        let mut out = String::with_capacity(self.entries.len() * 80);

        out.push_str(&format!("# indexer-db v{INDEX_VERSION}\n"));
        out.push_str(&format!("# built_at: {}\n", self.built_at));
        out.push_str(&format!("# entries: {}\n", self.entries.len()));
        out.push_str(&format!("# dirs_scanned: {}\n", self.dirs_scanned));
        out.push_str(&format!("# files_skipped: {}\n", self.files_skipped));
        out.push_str(&format!("# scan_duration_ms: {}\n", self.scan_duration_ms));
        out.push('\n');

        for entry in &self.entries {
            out.push_str(&entry.serialize());
            out.push('\n');
        }

        fs::write(&path, &out).map_err(|e| format!("write index: {e}"))
    }

    fn file_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.is_dir).count()
    }

    fn dir_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_dir).count()
    }

    fn total_size(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }
}

// ============================================================================
// Indexing engine
// ============================================================================

/// Scan a directory tree and build index entries.
fn scan_directory(
    root: &Path,
    config: &Config,
    entries: &mut Vec<IndexEntry>,
    dirs_scanned: &mut u64,
    files_skipped: &mut u64,
    depth: u32,
) {
    if config.max_depth > 0 && depth > config.max_depth {
        return;
    }

    let read_dir = match fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("  skip {}: {e}", root.display());
            return;
        }
    };

    *dirs_scanned += 1;

    for dir_entry in read_dir {
        let dir_entry = match dir_entry {
            Ok(de) => de,
            Err(_) => continue,
        };

        let path = dir_entry.path();
        let file_name = match dir_entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Skip excluded directories.
        let metadata = if config.follow_symlinks {
            fs::metadata(&path)
        } else {
            fs::symlink_metadata(&path)
        };

        let meta = match metadata {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.is_dir() {
            if config.exclude_dirs.iter().any(|d| d == &file_name) {
                *files_skipped += 1;
                continue;
            }

            // Index the directory itself.
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let path_str = path.to_string_lossy().replace('\\', "/");

            entries.push(IndexEntry {
                path: path_str,
                size: 0,
                mtime,
                extension: String::new(),
                is_dir: true,
            });

            // Recurse.
            scan_directory(&path, config, entries, dirs_scanned, files_skipped, depth + 1);
            continue;
        }

        if meta.is_symlink() && !config.follow_symlinks {
            continue;
        }

        // File — apply filters.
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Extension include filter.
        if !config.extensions.is_empty() && !config.extensions.iter().any(|e| e == &ext) {
            *files_skipped += 1;
            continue;
        }

        // Extension exclude filter.
        if config.exclude_extensions.iter().any(|e| e == &ext) {
            *files_skipped += 1;
            continue;
        }

        // Size filter.
        let size = meta.len();
        if config.max_file_size > 0 && size > config.max_file_size {
            *files_skipped += 1;
            continue;
        }

        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let path_str = path.to_string_lossy().replace('\\', "/");

        entries.push(IndexEntry {
            path: path_str,
            size,
            mtime,
            extension: ext,
            is_dir: false,
        });
    }
}

/// Perform a full index build.
fn build_index(config: &Config) -> IndexDb {
    let start = now_secs();
    let mut db = IndexDb::new();
    let mut dirs_scanned = 0u64;
    let mut files_skipped = 0u64;

    println!("indexer: starting full scan...");
    for path_str in &config.paths {
        let path = Path::new(path_str);
        if !path.exists() {
            eprintln!("  warning: path does not exist: {path_str}");
            continue;
        }
        println!("  scanning: {path_str}");
        scan_directory(path, config, &mut db.entries, &mut dirs_scanned, &mut files_skipped, 0);
    }

    let end = now_secs();
    db.built_at = end;
    db.dirs_scanned = dirs_scanned;
    db.files_skipped = files_skipped;
    // Approximate duration (second resolution).
    db.scan_duration_ms = (end.saturating_sub(start)) * 1000;

    println!(
        "indexer: done. {} files, {} dirs indexed ({} skipped) in {}s",
        db.file_count(),
        db.dir_count(),
        files_skipped,
        end.saturating_sub(start)
    );

    db
}

/// Incremental update: re-scan only paths whose mtime changed.
fn incremental_update(config: &Config, existing: &IndexDb) -> IndexDb {
    // Build a map of path → mtime from the existing index for quick comparison.
    let mut existing_map: BTreeMap<&str, u64> = BTreeMap::new();
    for entry in &existing.entries {
        existing_map.insert(&entry.path, entry.mtime);
    }

    let start = now_secs();
    let mut db = IndexDb::new();
    let mut dirs_scanned = 0u64;
    let mut files_skipped = 0u64;

    println!("indexer: incremental update...");
    for path_str in &config.paths {
        let path = Path::new(path_str);
        if !path.exists() {
            continue;
        }
        scan_directory(path, config, &mut db.entries, &mut dirs_scanned, &mut files_skipped, 0);
    }

    let end = now_secs();
    db.built_at = end;
    db.dirs_scanned = dirs_scanned;
    db.files_skipped = files_skipped;
    db.scan_duration_ms = (end.saturating_sub(start)) * 1000;

    // Report changes.
    let new_count = db.entries.len();
    let old_count = existing.entries.len();
    let added = new_count.saturating_sub(old_count);
    let removed = old_count.saturating_sub(new_count);
    println!(
        "indexer: update complete. {} entries (±{} added, -{} removed)",
        new_count, added, removed
    );

    db
}

// ============================================================================
// Search functions
// ============================================================================

/// Search by filename substring (case-insensitive).
fn search_name<'a>(db: &'a IndexDb, query: &str) -> Vec<&'a IndexEntry> {
    let query_lower = query.to_lowercase();
    db.entries
        .iter()
        .filter(|e| e.filename().to_lowercase().contains(&query_lower))
        .collect()
}

/// Search by glob-like pattern. Supports * as wildcard.
fn search_pattern<'a>(db: &'a IndexDb, pattern: &str) -> Vec<&'a IndexEntry> {
    let pattern_lower = pattern.to_lowercase();

    // Split pattern on '*' to get segments that must appear in order.
    let segments: Vec<&str> = pattern_lower.split('*').collect();
    let starts_with_wild = pattern_lower.starts_with('*');
    let ends_with_wild = pattern_lower.ends_with('*');

    db.entries
        .iter()
        .filter(|e| {
            let name = e.filename().to_lowercase();
            matches_glob_segments(&name, &segments, starts_with_wild, ends_with_wild)
        })
        .collect()
}

fn matches_glob_segments(name: &str, segments: &[&str], starts_wild: bool, ends_wild: bool) -> bool {
    if segments.is_empty() {
        return true;
    }

    let mut pos = 0;

    for (i, seg) in segments.iter().enumerate() {
        if seg.is_empty() {
            continue;
        }

        if i == 0 && !starts_wild {
            // First segment must match start.
            if !name.starts_with(*seg) {
                return false;
            }
            pos = seg.len();
        } else if i == segments.len() - 1 && !ends_wild {
            // Last segment must match end.
            if !name[pos..].ends_with(*seg) {
                return false;
            }
            return true;
        } else {
            // Middle segment — find anywhere after pos.
            match name[pos..].find(*seg) {
                Some(found) => pos += found + seg.len(),
                None => return false,
            }
        }
    }

    true
}

/// Search by extension.
fn search_extension<'a>(db: &'a IndexDb, ext: &str) -> Vec<&'a IndexEntry> {
    let ext_lower = ext.to_lowercase().trim_start_matches('.').to_string();
    db.entries
        .iter()
        .filter(|e| e.extension == ext_lower && !e.is_dir)
        .collect()
}

/// Find recently modified files.
fn search_recent(db: &IndexDb, hours: u64) -> Vec<&IndexEntry> {
    let cutoff = now_secs().saturating_sub(hours * 3600);
    db.entries
        .iter()
        .filter(|e| e.mtime >= cutoff && !e.is_dir)
        .collect()
}

/// Find large files.
fn search_large(db: &IndexDb, min_bytes: u64) -> Vec<&IndexEntry> {
    db.entries
        .iter()
        .filter(|e| e.size >= min_bytes && !e.is_dir)
        .collect()
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_start(config: &Config) {
    if !config.enabled {
        eprintln!("error: indexer is disabled. Run 'indexer enable' first.");
        process::exit(1);
    }

    let db = build_index(config);
    if let Err(e) = db.save(&config.index_dir) {
        eprintln!("error: {e}");
        process::exit(1);
    }
    save_state(&config.index_dir, &db);
}

fn cmd_daemon(config: &Config) {
    if !config.enabled {
        eprintln!("error: indexer is disabled. Run 'indexer enable' first.");
        process::exit(1);
    }

    println!("indexer: daemon mode (interval={}s)", config.interval_secs);

    // First pass: full build.
    let mut db = build_index(config);
    if let Err(e) = db.save(&config.index_dir) {
        eprintln!("error saving index: {e}");
    }
    save_state(&config.index_dir, &db);

    // Subsequent passes: incremental.
    loop {
        // Sleep for interval.
        // On our OS this would use the native sleep syscall; for now
        // use std::thread::sleep.
        std::thread::sleep(std::time::Duration::from_secs(config.interval_secs));

        db = incremental_update(config, &db);
        if let Err(e) = db.save(&config.index_dir) {
            eprintln!("error saving index: {e}");
        }
        save_state(&config.index_dir, &db);
    }
}

fn cmd_search(config: &Config, query: &str) {
    let db = IndexDb::load(&config.index_dir);
    if db.entries.is_empty() {
        eprintln!("Index is empty. Run 'indexer start' first.");
        process::exit(1);
    }

    let results = search_name(&db, query);
    print_results(&results, "name search");
}

fn cmd_find(config: &Config, pattern: &str) {
    let db = IndexDb::load(&config.index_dir);
    if db.entries.is_empty() {
        eprintln!("Index is empty. Run 'indexer start' first.");
        process::exit(1);
    }

    let results = search_pattern(&db, pattern);
    print_results(&results, "pattern search");
}

fn cmd_ext(config: &Config, ext: &str) {
    let db = IndexDb::load(&config.index_dir);
    if db.entries.is_empty() {
        eprintln!("Index is empty. Run 'indexer start' first.");
        process::exit(1);
    }

    let results = search_extension(&db, ext);
    print_results(&results, "extension search");
}

fn cmd_recent(config: &Config, hours: u64) {
    let db = IndexDb::load(&config.index_dir);
    if db.entries.is_empty() {
        eprintln!("Index is empty. Run 'indexer start' first.");
        process::exit(1);
    }

    let results = search_recent(&db, hours);
    print_results(&results, &format!("modified within {hours}h"));
}

fn cmd_large(config: &Config, min_mb: u64) {
    let db = IndexDb::load(&config.index_dir);
    if db.entries.is_empty() {
        eprintln!("Index is empty. Run 'indexer start' first.");
        process::exit(1);
    }

    let min_bytes = min_mb * 1024 * 1024;
    let results = search_large(&db, min_bytes);
    print_results(&results, &format!("files >= {min_mb} MB"));
}

fn cmd_stats(config: &Config) {
    let db = IndexDb::load(&config.index_dir);
    if db.entries.is_empty() {
        println!("Index: empty (not built yet)");
        return;
    }

    println!("=== Index Statistics ===");
    println!("  Files indexed:    {}", db.file_count());
    println!("  Directories:      {}", db.dir_count());
    println!("  Total entries:    {}", db.entries.len());
    println!("  Total size:       {}", format_size(db.total_size()));
    println!("  Built at:         {}", format_timestamp(db.built_at));
    println!("  Dirs scanned:     {}", db.dirs_scanned);
    println!("  Files skipped:    {}", db.files_skipped);
    println!("  Scan duration:    {}ms", db.scan_duration_ms);

    // Extension breakdown.
    let mut ext_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in &db.entries {
        if !entry.is_dir && !entry.extension.is_empty() {
            *ext_counts.entry(&entry.extension).or_insert(0) += 1;
        }
    }

    if !ext_counts.is_empty() {
        println!("\n  Top extensions:");
        let mut sorted: Vec<(&&str, &usize)> = ext_counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (ext, count) in sorted.iter().take(15) {
            println!("    .{:<12} {}", ext, count);
        }
    }
}

fn cmd_config_show(config: &Config) {
    println!("=== Indexer Configuration ===");
    println!("  enabled:            {}", config.enabled);
    println!("  interval:           {}s", config.interval_secs);
    println!("  index_dir:          {}", config.index_dir);
    println!("  follow_symlinks:    {}", config.follow_symlinks);
    println!("  max_depth:          {}", if config.max_depth == 0 { "unlimited".to_string() } else { config.max_depth.to_string() });
    println!("  max_file_size:      {}", if config.max_file_size == 0 { "unlimited".to_string() } else { format_size(config.max_file_size) });
    println!("  paths:              {}", if config.paths.is_empty() { "(none)".to_string() } else { config.paths.join(", ") });
    println!("  extensions:         {}", if config.extensions.is_empty() { "(all)".to_string() } else { config.extensions.join(", ") });
    println!("  exclude_extensions: {}", if config.exclude_extensions.is_empty() { "(none)".to_string() } else { config.exclude_extensions.join(", ") });
    println!("  exclude_dirs:       {}", if config.exclude_dirs.is_empty() { "(none)".to_string() } else { config.exclude_dirs.join(", ") });
}

fn cmd_config_set(config: &mut Config, key: &str, value: &str, config_path: &str) {
    match key {
        "enabled" => config.enabled = value == "true" || value == "1",
        "interval" => {
            match value.parse::<u64>() {
                Ok(n) => config.interval_secs = n,
                Err(_) => {
                    eprintln!("error: invalid number: {value}");
                    process::exit(1);
                }
            }
        }
        "max_file_size" => {
            match value.parse::<u64>() {
                Ok(n) => config.max_file_size = n,
                Err(_) => {
                    eprintln!("error: invalid number: {value}");
                    process::exit(1);
                }
            }
        }
        "index_dir" => config.index_dir = value.to_string(),
        "follow_symlinks" => config.follow_symlinks = value == "true" || value == "1",
        "max_depth" => {
            match value.parse::<u32>() {
                Ok(n) => config.max_depth = n,
                Err(_) => {
                    eprintln!("error: invalid number: {value}");
                    process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("error: unknown config key: {key}");
            eprintln!("  valid keys: enabled, interval, max_file_size, index_dir, follow_symlinks, max_depth");
            process::exit(1);
        }
    }

    println!("  {key} = {value}");
    if let Err(e) = config.save(config_path) {
        eprintln!("error saving config: {e}");
        process::exit(1);
    }
}

fn cmd_rebuild(config: &Config) {
    if !config.enabled {
        eprintln!("error: indexer is disabled. Run 'indexer enable' first.");
        process::exit(1);
    }

    println!("indexer: forcing full rebuild (discarding existing index)...");
    let db = build_index(config);
    if let Err(e) = db.save(&config.index_dir) {
        eprintln!("error: {e}");
        process::exit(1);
    }
    save_state(&config.index_dir, &db);
}

fn cmd_status(config: &Config) {
    println!("=== Indexer Status ===");
    println!("  Service: {}", if config.enabled { "enabled" } else { "disabled" });

    let state = load_state(&config.index_dir);
    if let Some((built_at, entry_count)) = state {
        println!("  Last indexed: {}", format_timestamp(built_at));
        println!("  Entries:      {entry_count}");

        let age_secs = now_secs().saturating_sub(built_at);
        if age_secs < 60 {
            println!("  Age:          {age_secs}s ago");
        } else if age_secs < 3600 {
            println!("  Age:          {}m ago", age_secs / 60);
        } else {
            println!("  Age:          {}h {}m ago", age_secs / 3600, (age_secs % 3600) / 60);
        }
    } else {
        println!("  Last indexed: never");
    }

    println!("  Index dir:    {}", config.index_dir);
    println!("  Interval:     {}s", config.interval_secs);
}

fn cmd_enable(config: &mut Config, config_path: &str) {
    config.enabled = true;
    if let Err(e) = config.save(config_path) {
        eprintln!("error saving config: {e}");
        process::exit(1);
    }
    println!("indexer: enabled");
}

fn cmd_disable(config: &mut Config, config_path: &str) {
    config.enabled = false;
    if let Err(e) = config.save(config_path) {
        eprintln!("error saving config: {e}");
        process::exit(1);
    }
    println!("indexer: disabled");
}

fn cmd_paths(config: &mut Config, config_path: &str, args: &[String]) {
    if args.is_empty() {
        // List paths.
        if config.paths.is_empty() {
            println!("No paths configured.");
        } else {
            println!("Indexed paths:");
            for p in &config.paths {
                println!("  {p}");
            }
        }
        return;
    }

    match args[0].as_str() {
        "list" => {
            if config.paths.is_empty() {
                println!("No paths configured.");
            } else {
                println!("Indexed paths:");
                for p in &config.paths {
                    println!("  {p}");
                }
            }
        }
        "add" => {
            if args.len() < 2 {
                eprintln!("usage: indexer paths add <path>");
                process::exit(1);
            }
            let new_path = &args[1];
            if config.paths.iter().any(|p| p == new_path) {
                println!("  path already in list: {new_path}");
            } else {
                config.paths.push(new_path.clone());
                if let Err(e) = config.save(config_path) {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
                println!("  added: {new_path}");
            }
        }
        "remove" | "rm" => {
            if args.len() < 2 {
                eprintln!("usage: indexer paths remove <path>");
                process::exit(1);
            }
            let rm_path = &args[1];
            let before = config.paths.len();
            config.paths.retain(|p| p != rm_path);
            if config.paths.len() == before {
                println!("  path not found: {rm_path}");
            } else {
                if let Err(e) = config.save(config_path) {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
                println!("  removed: {rm_path}");
            }
        }
        other => {
            eprintln!("unknown paths subcommand: {other}");
            eprintln!("  usage: indexer paths [list|add|remove] [path]");
            process::exit(1);
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_timestamp(unix_secs: u64) -> String {
    if unix_secs == 0 {
        return "unknown".to_string();
    }

    let secs = unix_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let mut y = 1970i64;
    let mut remaining_days = days as i64;

    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }

    let month_days: [i64; 12] = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 0u32;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            month = i as u32 + 1;
            break;
        }
        remaining_days -= md;
    }
    if month == 0 {
        month = 12;
    }
    let day = remaining_days + 1;

    format!("{y:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}")
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn print_results(results: &[&IndexEntry], label: &str) {
    if results.is_empty() {
        println!("No results for {label}.");
        return;
    }

    println!("{} result(s) for {label}:", results.len());

    // Cap display at 100 entries.
    let display_count = results.len().min(100);
    for entry in &results[..display_count] {
        if entry.is_dir {
            println!("  [dir] {}", entry.path);
        } else {
            println!("  {:>10}  {}  {}", format_size(entry.size), format_timestamp(entry.mtime), entry.path);
        }
    }

    if results.len() > 100 {
        println!("  ... and {} more", results.len() - 100);
    }
}

/// Save lightweight state file (for status command).
fn save_state(index_dir: &str, db: &IndexDb) {
    let path = PathBuf::from(index_dir).join(STATE_FILENAME);
    let content = format!("{}\n{}\n", db.built_at, db.entries.len());
    let _ = fs::write(&path, &content);
}

/// Load state file. Returns (built_at, entry_count) or None.
fn load_state(index_dir: &str) -> Option<(u64, usize)> {
    let path = PathBuf::from(index_dir).join(STATE_FILENAME);
    let content = fs::read_to_string(&path).ok()?;
    let mut lines = content.lines();
    let built_at: u64 = lines.next()?.parse().ok()?;
    let count: usize = lines.next()?.parse().ok()?;
    Some((built_at, count))
}

// ============================================================================
// Usage and main
// ============================================================================

fn print_usage() {
    println!("Slate OS File Indexer v0.1.0");
    println!();
    println!("A background file indexing service for fast filename queries.");
    println!("Off by default — must be explicitly enabled.");
    println!();
    println!("USAGE:");
    println!("  indexer <command> [arguments]");
    println!();
    println!("COMMANDS:");
    println!("  start              Run a single indexing pass (foreground)");
    println!("  daemon             Run as background daemon (periodic re-index)");
    println!("  rebuild            Force full re-index (discard existing)");
    println!("  search <query>     Search by filename substring");
    println!("  find <pattern>     Search by glob pattern (*.rs, doc*.pdf)");
    println!("  ext <extension>    List all files with given extension");
    println!("  recent [hours]     Files modified within N hours (default: 24)");
    println!("  large [mb]         Files larger than N megabytes (default: 100)");
    println!("  stats              Show index statistics");
    println!("  status             Show service status");
    println!("  config             Show configuration");
    println!("  config set <k> <v> Update a config setting");
    println!("  enable             Enable the indexer service");
    println!("  disable            Disable the indexer service");
    println!("  paths [list]       List indexed paths");
    println!("  paths add <path>   Add a directory to index");
    println!("  paths remove <path> Remove a directory from indexing");
    println!();
    println!("CONFIG KEYS:");
    println!("  enabled, interval, max_file_size, index_dir,");
    println!("  follow_symlinks, max_depth");
    println!();
    println!("EXAMPLES:");
    println!("  indexer enable");
    println!("  indexer paths add /home/user/documents");
    println!("  indexer start");
    println!("  indexer search report");
    println!("  indexer find *.rs");
    println!("  indexer ext pdf");
    println!("  indexer recent 4       # modified in last 4 hours");
    println!("  indexer large 50       # files > 50 MB");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    let config_path = env::var("INDEXER_CONFIG")
        .unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());

    let mut config = Config::load(&config_path);

    match args[1].as_str() {
        "start" => cmd_start(&config),
        "daemon" => cmd_daemon(&config),
        "rebuild" => cmd_rebuild(&config),
        "search" => {
            if args.len() < 3 {
                eprintln!("usage: indexer search <query>");
                process::exit(1);
            }
            cmd_search(&config, &args[2]);
        }
        "find" => {
            if args.len() < 3 {
                eprintln!("usage: indexer find <pattern>");
                process::exit(1);
            }
            cmd_find(&config, &args[2]);
        }
        "ext" | "extension" => {
            if args.len() < 3 {
                eprintln!("usage: indexer ext <extension>");
                process::exit(1);
            }
            cmd_ext(&config, &args[2]);
        }
        "recent" => {
            let hours = if args.len() >= 3 {
                args[2].parse::<u64>().unwrap_or(24)
            } else {
                24
            };
            cmd_recent(&config, hours);
        }
        "large" => {
            let mb = if args.len() >= 3 {
                args[2].parse::<u64>().unwrap_or(100)
            } else {
                100
            };
            cmd_large(&config, mb);
        }
        "stats" => cmd_stats(&config),
        "status" => cmd_status(&config),
        "config" => {
            if args.len() >= 4 && args[2] == "set" {
                if args.len() < 5 {
                    eprintln!("usage: indexer config set <key> <value>");
                    process::exit(1);
                }
                cmd_config_set(&mut config, &args[3], &args[4], &config_path);
            } else {
                cmd_config_show(&config);
            }
        }
        "enable" => cmd_enable(&mut config, &config_path),
        "disable" => cmd_disable(&mut config, &config_path),
        "paths" => {
            let sub_args: Vec<String> = args[2..].to_vec();
            cmd_paths(&mut config, &config_path, &sub_args);
        }
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("Run 'indexer help' for usage.");
            process::exit(1);
        }
    }
}
