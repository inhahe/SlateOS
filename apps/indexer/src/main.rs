//! indexer — Slate OS background file indexer service.
//!
//! Builds and maintains a searchable index of the filesystem, enabling
//! near-instant file search by name, glob pattern, path, or content.
//!
//! Usage:
//!   indexer start                     Start the indexing service
//!   indexer stop                      Stop the service gracefully
//!   indexer status                    Show indexing status
//!   indexer search <QUERY>            Search the index
//!   indexer reindex [PATH]            Force re-scan of specified path or all
//!   indexer config                    Show current configuration
//!   indexer config set <KEY> <VALUE>  Set a config option

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use textfmt::kv;

// ============================================================================
// Constants
// ============================================================================

const CONFIG_PATH: &str = "/etc/indexer.conf";
const INDEX_PATH: &str = "/var/indexer/index.db";
const PID_FILE: &str = "/var/indexer/indexer.pid";
const INDEX_MAGIC: &[u8; 4] = b"OIDX";
/// Version 2 stores entry paths as their exact bytes. Version 1 wrote them
/// through `to_string_lossy`, so any path that was not UTF-8 came back with
/// U+FFFD where its bytes had been and no longer named a real file. The index
/// is a derived cache, so a version bump needs no migration: `deserialize`
/// rejects the old file and the caller reindexes.
const INDEX_VERSION: u32 = 2;
/// Bytes before the first entry: magic(4) + version(4) + count(8) +
/// last_indexed(8) + dirs_scanned(8).
const INDEX_HEADER_LEN: usize = 32;
const DEFAULT_MAX_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MB
const DEFAULT_SCAN_INTERVAL: u64 = 3600; // 1 hour
const DEFAULT_RESULT_LIMIT: usize = 50;
const TRIGRAM_SIZE: usize = 3;
const SCAN_BATCH_SIZE: usize = 500;
const SCAN_BATCH_PAUSE_MS: u64 = 10;
const FUZZY_MAX_DISTANCE: u32 = 2;

// ============================================================================
// Configuration
// ============================================================================

/// Which repeated-key list a config key feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListKey {
    IndexPath,
    ExcludePath,
    IncludeExtension,
    ExcludeExtension,
}

/// Write a list as one `key = value` line per entry, or as the empty plural
/// form when there is nothing to write.
///
/// The escaping is [`textfmt::kv`]'s, not this file's: a path here may contain
/// any byte but `/` and NUL, so it can hold a newline, a tab or an edge space,
/// and the two ways of getting that wrong (a `replace`-chain decoder; escaping
/// a trailing space as backslash-space, which the parser's trim then eats) had
/// each already been written more than once in this tree.
///
/// No `extra` separators are passed. The keys here are fixed identifiers, so
/// the `=` on the key's side is never user data and the first one on a line is
/// always the real separator.
fn push_list(out: &mut String, key: &str, plural_key: &str, values: &[String]) {
    if values.is_empty() {
        out.push_str(&format!("{plural_key} = \n"));
        return;
    }
    for value in values {
        out.push_str(&format!("{key} = {}\n", kv::escape(value, &[])));
    }
}

#[derive(Debug, Clone)]
struct Config {
    index_paths: Vec<String>,
    exclude_paths: Vec<String>,
    include_extensions: Option<Vec<String>>,
    exclude_extensions: Vec<String>,
    max_file_size: u64,
    scan_interval_secs: u64,
    index_contents: bool,
    enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            index_paths: vec!["/home".to_string()],
            exclude_paths: vec![
                "/tmp".to_string(),
                "/var/cache".to_string(),
                "/.git".to_string(),
                "/node_modules".to_string(),
            ],
            include_extensions: None,
            exclude_extensions: vec![
                ".o".to_string(),
                ".so".to_string(),
                ".tmp".to_string(),
                ".lock".to_string(),
            ],
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            scan_interval_secs: DEFAULT_SCAN_INTERVAL,
            index_contents: false,
            enabled: false,
        }
    }
}

impl Config {
    /// Load configuration from the config file. Falls back to defaults if missing.
    fn load() -> Self {
        Self::load_from_path(Path::new(CONFIG_PATH))
    }

    fn load_from_path(path: &Path) -> Self {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        Self::parse(&content)
    }

    fn parse(content: &str) -> Self {
        let mut cfg = Self::default();
        // Which repeated keys have already replaced their default. The first
        // `index_path` line in a file replaces the built-in list; the rest add
        // to it. Without this a config could only ever grow the defaults.
        let mut replaced: Vec<&str> = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                if Self::list_for_key(key).is_some() {
                    let first = !replaced.contains(&key);
                    if first {
                        replaced.push(key);
                    }
                    cfg.push_field(key, value, first);
                } else {
                    cfg.set_field(key, value);
                }
            }
        }

        cfg
    }

    /// Whether `key` is one of the repeated single-value keys, and which list
    /// it feeds. Returning the discriminant rather than a bool keeps the set of
    /// repeated keys defined in exactly one place.
    fn list_for_key(key: &str) -> Option<ListKey> {
        match key {
            "index_path" => Some(ListKey::IndexPath),
            "exclude_path" => Some(ListKey::ExcludePath),
            "include_extension" => Some(ListKey::IncludeExtension),
            "exclude_extension" => Some(ListKey::ExcludeExtension),
            _ => None,
        }
    }

    /// Append one value to a repeated-key list, clearing the defaults first
    /// time round.
    fn push_field(&mut self, key: &str, value: &str, first: bool) {
        let Some(which) = Self::list_for_key(key) else {
            return;
        };
        let decoded = kv::unescape(value);
        let list = match which {
            ListKey::IndexPath => &mut self.index_paths,
            ListKey::ExcludePath => &mut self.exclude_paths,
            ListKey::ExcludeExtension => &mut self.exclude_extensions,
            ListKey::IncludeExtension => self.include_extensions.get_or_insert_with(Vec::new),
        };
        if first {
            list.clear();
        }
        list.push(decoded);
    }

    fn set_field(&mut self, key: &str, value: &str) {
        match key {
            "index_paths" => {
                self.index_paths = Self::parse_list(value);
            }
            "exclude_paths" => {
                self.exclude_paths = Self::parse_list(value);
            }
            "include_extensions" => {
                let list = Self::parse_list(value);
                self.include_extensions = if list.is_empty() { None } else { Some(list) };
            }
            "exclude_extensions" => {
                self.exclude_extensions = Self::parse_list(value);
            }
            "max_file_size" => {
                if let Ok(v) = value.parse::<u64>() {
                    self.max_file_size = v;
                }
            }
            "scan_interval_secs" => {
                if let Ok(v) = value.parse::<u64>() {
                    self.scan_interval_secs = v;
                }
            }
            "index_contents" => {
                self.index_contents = value == "true";
            }
            "enabled" => {
                self.enabled = value == "true";
            }
            _ => {} // Ignore unknown keys
        }
    }

    /// Parse a legacy comma-separated list, as hand-written configs still use.
    ///
    /// Retained for the plural keys only. It cannot represent a path
    /// containing a comma, which is why the writer no longer emits this form
    /// -- see [`Config::serialize`].
    fn parse_list(value: &str) -> Vec<String> {
        value
            .split(',')
            .map(|s| kv::unescape(s.trim()))
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Serialize configuration to the config file format.
    ///
    /// Paths and extensions get **one line each**, escaped, rather than being
    /// joined with commas. On this system a path may contain any byte except
    /// `/` and NUL, so the comma that used to separate them is an ordinary
    /// filename character — and the failure it caused was not cosmetic:
    ///
    /// * `exclude_paths` decides what the indexer does *not* read. Excluding
    ///   `/home/u/Private, Ltd` used to serialise as two entries,
    ///   `/home/u/Private` and `Ltd`, neither of which matches the directory
    ///   the user actually named. With `index_contents` on, the contents of a
    ///   directory the user explicitly excluded then went into the index.
    /// * `index_paths` decides what it does read, so the same split silently
    ///   pointed the scanner at directories nobody chose.
    ///
    /// A newline in a path would have been worse still — it could write a
    /// whole config line, including `enabled = false` — so line breaks are
    /// escaped rather than rejected: every path is representable and nothing
    /// has to be silently dropped.
    ///
    /// An empty list is written in the legacy plural form with no value, which
    /// is the one thing repeated keys cannot express: no lines at all is
    /// indistinguishable from "key absent", which means "keep the defaults".
    fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str("# Slate OS File Indexer Configuration\n");
        out.push_str("#\n");
        out.push_str("# Paths and extensions take one line each. Values are escaped:\n");
        out.push_str("#   \\\\  backslash    \\n  newline    \\r  return\n");
        out.push_str("#   \\t  tab          \\s  a leading or trailing space\n");
        out.push_str("# A path may contain any byte but `/` and NUL, commas included,\n");
        out.push_str("# so the list is not comma-separated.\n\n");
        out.push_str(&format!("enabled = {}\n", self.enabled));
        push_list(&mut out, "index_path", "index_paths", &self.index_paths);
        push_list(
            &mut out,
            "exclude_path",
            "exclude_paths",
            &self.exclude_paths,
        );
        match self.include_extensions {
            // Absent entirely means "no filter"; an empty list means the same
            // after parsing, so both round-trip to `None`.
            Some(ref exts) => push_list(
                &mut out,
                "include_extension",
                "include_extensions",
                exts.as_slice(),
            ),
            None => out.push_str("include_extensions = \n"),
        }
        push_list(
            &mut out,
            "exclude_extension",
            "exclude_extensions",
            &self.exclude_extensions,
        );
        out.push_str(&format!("max_file_size = {}\n", self.max_file_size));
        out.push_str(&format!(
            "scan_interval_secs = {}\n",
            self.scan_interval_secs
        ));
        out.push_str(&format!("index_contents = {}\n", self.index_contents));
        out
    }

    /// Save configuration to the config file.
    fn save(&self) -> io::Result<()> {
        self.save_to_path(Path::new(CONFIG_PATH))
    }

    fn save_to_path(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.serialize())
    }
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "File Indexer Configuration:")?;
        writeln!(f, "  enabled:            {}", self.enabled)?;
        writeln!(f, "  index_paths:        {:?}", self.index_paths)?;
        writeln!(f, "  exclude_paths:      {:?}", self.exclude_paths)?;
        writeln!(f, "  include_extensions: {:?}", self.include_extensions)?;
        writeln!(f, "  exclude_extensions: {:?}", self.exclude_extensions)?;
        writeln!(
            f,
            "  max_file_size:      {} bytes ({} MB)",
            self.max_file_size,
            self.max_file_size / (1024 * 1024)
        )?;
        writeln!(f, "  scan_interval_secs: {}", self.scan_interval_secs)?;
        writeln!(f, "  index_contents:     {}", self.index_contents)?;
        Ok(())
    }
}

// ============================================================================
// Index Entry
// ============================================================================

/// A single indexed file entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexEntry {
    /// Full path to the file. Authoritative: this is what identifies the file
    /// and what a caller opens, so it holds the path's exact bytes.
    path: PathBuf,
    /// Filename component, as a **search key only** — never as an identity and
    /// never displayed.
    ///
    /// A query is UTF-8 text the user typed, so matching it against a lossy
    /// rendering of the name is the only thing that can be meant; the bytes a
    /// query could never have named become U+FFFD, which no query matches. Use
    /// [`IndexEntry::path`] for anything that has to name the file.
    filename: String,
    /// File size in bytes.
    size: u64,
    /// Modification time as seconds since UNIX epoch.
    mtime: u64,
    /// Detected file type category.
    file_type: FileType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileType {
    Regular,
    Directory,
    Symlink,
    Other,
}

impl FileType {
    fn as_byte(self) -> u8 {
        match self {
            Self::Regular => 0,
            Self::Directory => 1,
            Self::Symlink => 2,
            Self::Other => 3,
        }
    }

    fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::Regular,
            1 => Self::Directory,
            2 => Self::Symlink,
            _ => Self::Other,
        }
    }
}

/// Derive the search key in [`IndexEntry::filename`] from a path.
///
/// One function so the scanner and the index loader cannot disagree about what
/// a given path's key is — they did not, but nothing stopped them.
fn filename_key(path: &Path) -> String {
    path.file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Build an `OsString` from the raw bytes of a path.
///
/// Split per platform rather than papered over with
/// `OsStr::from_encoded_bytes_unchecked`, whose contract is that the bytes are
/// valid for the platform's `OsStr` encoding — true for arbitrary bytes on
/// Unix, but not on Windows, where `OsStr` is WTF-8. Our target is
/// `target-family = ["unix"]`, so the safe total conversion is the one that
/// runs; Windows appears only as a test host.
#[cfg(unix)]
fn os_string_from_bytes(bytes: Vec<u8>) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(bytes)
}

/// Test-host fallback: a Windows `OsString` cannot hold a byte string that is
/// not WTF-8. Only reachable on a non-Unix host reading an index written on
/// the target.
#[cfg(not(unix))]
fn os_string_from_bytes(bytes: Vec<u8>) -> std::ffi::OsString {
    match String::from_utf8(bytes) {
        Ok(s) => std::ffi::OsString::from(s),
        Err(e) => std::ffi::OsString::from(String::from_utf8_lossy(e.as_bytes()).into_owned()),
    }
}

impl IndexEntry {
    fn filename_lower(&self) -> String {
        self.filename.to_ascii_lowercase()
    }
}

// ============================================================================
// Index
// ============================================================================

/// The main file index structure.
#[derive(Debug, Clone)]
struct FileIndex {
    /// All indexed entries, sorted by filename for binary search.
    entries: Vec<IndexEntry>,
    /// Filename (lowercase) -> list of entry indices, sorted.
    name_lookup: BTreeMap<String, Vec<usize>>,
    /// Trigram -> list of entry indices (for content search).
    trigram_index: BTreeMap<[u8; 3], Vec<usize>>,
    /// Timestamp when the index was last built/updated.
    last_indexed: u64,
    /// Total number of directories scanned.
    dirs_scanned: u64,
}

impl FileIndex {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            name_lookup: BTreeMap::new(),
            trigram_index: BTreeMap::new(),
            last_indexed: 0,
            dirs_scanned: 0,
        }
    }

    /// Build the index from a set of file entries.
    fn build_from_entries(entries: Vec<IndexEntry>) -> Self {
        let mut index = Self {
            entries,
            name_lookup: BTreeMap::new(),
            trigram_index: BTreeMap::new(),
            last_indexed: current_timestamp(),
            dirs_scanned: 0,
        };
        index.rebuild_lookup();
        index
    }

    /// Rebuild the name lookup table from entries.
    fn rebuild_lookup(&mut self) {
        self.name_lookup.clear();
        for (i, entry) in self.entries.iter().enumerate() {
            let key = entry.filename_lower();
            self.name_lookup.entry(key).or_default().push(i);
        }
    }

    /// Add a single entry to the index.
    fn add_entry(&mut self, entry: IndexEntry) {
        let key = entry.filename_lower();
        let idx = self.entries.len();
        self.entries.push(entry);
        self.name_lookup.entry(key).or_default().push(idx);
    }

    /// Remove entries whose paths start with the given prefix (for incremental rescan).
    fn remove_path_prefix(&mut self, prefix: &Path) {
        self.entries.retain(|e| !e.path.starts_with(prefix));
        self.rebuild_lookup();
    }

    /// Total number of indexed files.
    fn file_count(&self) -> usize {
        self.entries.len()
    }

    /// Approximate index size in memory.
    fn approx_size_bytes(&self) -> usize {
        let entries_size = self.entries.len() * std::mem::size_of::<IndexEntry>();
        let paths_size: usize = self.entries.iter().map(|e| e.path.as_os_str().len()).sum();
        let names_size: usize = self.entries.iter().map(|e| e.filename.len()).sum();
        entries_size + paths_size + names_size
    }

    // ---- Serialization ----

    /// Serialize the index to a binary format.
    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Header: magic + version + entry count + timestamp
        buf.extend_from_slice(INDEX_MAGIC);
        buf.extend_from_slice(&INDEX_VERSION.to_le_bytes());
        buf.extend_from_slice(&(self.entries.len() as u64).to_le_bytes());
        buf.extend_from_slice(&self.last_indexed.to_le_bytes());
        buf.extend_from_slice(&self.dirs_scanned.to_le_bytes());

        // Entries
        for entry in &self.entries {
            // The exact bytes, not `to_string_lossy`: a path is a byte string
            // here, and an index entry whose path has been through U+FFFD
            // substitution no longer names the file it was built from.
            let path_bytes = entry.path.as_os_str().as_encoded_bytes();
            buf.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(path_bytes);
            buf.extend_from_slice(&entry.size.to_le_bytes());
            buf.extend_from_slice(&entry.mtime.to_le_bytes());
            buf.push(entry.file_type.as_byte());
        }

        buf
    }

    /// Deserialize the index from binary data.
    fn deserialize(data: &[u8]) -> Result<Self, IndexError> {
        // 32, not 28: `dirs_scanned` is read from bytes 24..32, so a file of
        // 28..=31 bytes passed this check and then panicked on the read.
        if data.len() < INDEX_HEADER_LEN {
            return Err(IndexError::CorruptIndex("header too short".into()));
        }

        let magic = &data[0..4];
        if magic != INDEX_MAGIC {
            return Err(IndexError::CorruptIndex("invalid magic".into()));
        }

        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != INDEX_VERSION {
            return Err(IndexError::CorruptIndex(format!(
                "unsupported version: {}",
                version
            )));
        }

        let entry_count = u64::from_le_bytes([
            data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
        ]) as usize;

        let last_indexed = u64::from_le_bytes([
            data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
        ]);

        let dirs_scanned = u64::from_le_bytes([
            data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
        ]);

        let mut offset = 32;
        let mut entries = Vec::with_capacity(entry_count.min(1_000_000));

        for _ in 0..entry_count {
            if offset + 4 > data.len() {
                return Err(IndexError::CorruptIndex(
                    "truncated entry path length".into(),
                ));
            }
            let path_len = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            offset += 4;

            if offset + path_len > data.len() {
                return Err(IndexError::CorruptIndex("truncated entry path".into()));
            }
            let path = PathBuf::from(os_string_from_bytes(
                data[offset..offset + path_len].to_vec(),
            ));
            offset += path_len;

            if offset + 17 > data.len() {
                return Err(IndexError::CorruptIndex("truncated entry metadata".into()));
            }
            let size = u64::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            offset += 8;

            let mtime = u64::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            offset += 8;

            let file_type = FileType::from_byte(data[offset]);
            offset += 1;

            let filename = filename_key(&path);

            entries.push(IndexEntry {
                path,
                filename,
                size,
                mtime,
                file_type,
            });
        }

        let mut index = Self::build_from_entries(entries);
        index.last_indexed = last_indexed;
        index.dirs_scanned = dirs_scanned;
        Ok(index)
    }

    /// Load the index from the default path.
    fn load() -> Result<Self, IndexError> {
        Self::load_from_path(Path::new(INDEX_PATH))
    }

    fn load_from_path(path: &Path) -> Result<Self, IndexError> {
        let data = fs::read(path).map_err(|e| IndexError::Io(e.to_string()))?;
        Self::deserialize(&data)
    }

    /// Save the index to the default path.
    fn save(&self) -> Result<(), IndexError> {
        self.save_to_path(Path::new(INDEX_PATH))
    }

    fn save_to_path(&self, path: &Path) -> Result<(), IndexError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| IndexError::Io(e.to_string()))?;
        }
        let data = self.serialize();
        fs::write(path, &data).map_err(|e| IndexError::Io(e.to_string()))?;
        Ok(())
    }
}

// ============================================================================
// Trigram Index (for content search)
// ============================================================================

/// Extract trigrams from text content.
fn extract_trigrams(text: &str) -> Vec<[u8; 3]> {
    let bytes = text.as_bytes();
    if bytes.len() < TRIGRAM_SIZE {
        return Vec::new();
    }
    let mut trigrams = Vec::with_capacity(bytes.len() - TRIGRAM_SIZE + 1);
    for window in bytes.windows(TRIGRAM_SIZE) {
        let trigram = [
            window[0].to_ascii_lowercase(),
            window[1].to_ascii_lowercase(),
            window[2].to_ascii_lowercase(),
        ];
        trigrams.push(trigram);
    }
    trigrams.sort_unstable();
    trigrams.dedup();
    trigrams
}

/// Add content trigrams for a file to the index.
fn index_file_content(index: &mut FileIndex, entry_idx: usize, content: &str) {
    let trigrams = extract_trigrams(content);
    for trigram in trigrams {
        index
            .trigram_index
            .entry(trigram)
            .or_default()
            .push(entry_idx);
    }
}

// ============================================================================
// Search
// ============================================================================

/// A search result with ranking information.
#[derive(Debug, Clone)]
struct SearchResult {
    entry: IndexEntry,
    rank: SearchRank,
    score: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SearchRank {
    /// Exact filename match (highest priority).
    Exact = 0,
    /// Filename starts with query.
    Prefix = 1,
    /// Filename contains query as substring.
    Substring = 2,
    /// Glob pattern match.
    Glob = 3,
    /// Fuzzy match (Levenshtein distance).
    Fuzzy = 4,
    /// Content match (lowest priority for filename searches).
    Content = 5,
}

impl fmt::Display for SearchResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_char = match self.entry.file_type {
            FileType::Regular => '-',
            FileType::Directory => 'd',
            FileType::Symlink => 'l',
            FileType::Other => '?',
        };
        write!(
            f,
            "{} {:>10}  {}",
            type_char,
            format_size(self.entry.size),
            self.entry.path.display()
        )
    }
}

/// Search the index with the given query.
fn search(index: &FileIndex, query: &str, limit: usize) -> Vec<SearchResult> {
    let query_lower = query.to_ascii_lowercase();
    let mut results: Vec<SearchResult> = Vec::new();

    // Determine if query is a glob pattern.
    let is_glob = query.contains('*') || query.contains('?') || query.contains('[');
    // Determine if query looks like a path search.
    let is_path_search = query.contains('/');

    for entry in &index.entries {
        let name_lower = entry.filename_lower();

        if is_path_search {
            // Match against full path.
            // Matching a query against a lossy rendering is a selection
            // heuristic, not an identity check: the result carries `entry`,
            // whose path is exact, so what gets opened is still the right file.
            let path_str = entry.path.to_string_lossy().to_ascii_lowercase();
            if is_glob {
                if glob_match(&query_lower, &path_str) {
                    results.push(SearchResult {
                        entry: entry.clone(),
                        rank: SearchRank::Glob,
                        score: 0,
                    });
                }
            } else if path_str.contains(&query_lower) {
                results.push(SearchResult {
                    entry: entry.clone(),
                    rank: SearchRank::Substring,
                    score: path_str.len() as u32,
                });
            }
        } else if is_glob {
            if glob_match(&query_lower, &name_lower) {
                results.push(SearchResult {
                    entry: entry.clone(),
                    rank: SearchRank::Glob,
                    score: 0,
                });
            }
        } else {
            // Filename matching with ranking.
            if name_lower == query_lower {
                results.push(SearchResult {
                    entry: entry.clone(),
                    rank: SearchRank::Exact,
                    score: 0,
                });
            } else if name_lower.starts_with(&query_lower) {
                results.push(SearchResult {
                    entry: entry.clone(),
                    rank: SearchRank::Prefix,
                    score: name_lower.len() as u32,
                });
            } else if name_lower.contains(&query_lower) {
                results.push(SearchResult {
                    entry: entry.clone(),
                    rank: SearchRank::Substring,
                    score: name_lower.len() as u32,
                });
            } else {
                // Fuzzy match.
                let distance = levenshtein(&query_lower, &name_lower);
                if distance <= FUZZY_MAX_DISTANCE {
                    results.push(SearchResult {
                        entry: entry.clone(),
                        rank: SearchRank::Fuzzy,
                        score: distance,
                    });
                }
            }
        }
    }

    // Sort by rank (best first), then by score (lower = better).
    results.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.score.cmp(&b.score)));

    results.truncate(limit);
    results
}

/// Search for files containing specific text (trigram-based).
fn search_content(index: &FileIndex, query: &str, limit: usize) -> Vec<SearchResult> {
    let query_trigrams = extract_trigrams(query);
    if query_trigrams.is_empty() {
        return Vec::new();
    }

    // Find entries that contain ALL query trigrams (intersection).
    let mut candidate_sets: Vec<&Vec<usize>> = Vec::new();
    for trigram in &query_trigrams {
        if let Some(entries) = index.trigram_index.get(trigram) {
            candidate_sets.push(entries);
        } else {
            // If any trigram has no matches, no files can match.
            return Vec::new();
        }
    }

    if candidate_sets.is_empty() {
        return Vec::new();
    }

    // Intersect all candidate sets.
    let mut candidates: Vec<usize> = candidate_sets[0].clone();
    for set in &candidate_sets[1..] {
        candidates.retain(|idx| set.contains(idx));
    }

    let mut results: Vec<SearchResult> = candidates
        .into_iter()
        .filter_map(|idx| {
            index.entries.get(idx).map(|entry| SearchResult {
                entry: entry.clone(),
                rank: SearchRank::Content,
                score: 0,
            })
        })
        .collect();

    results.truncate(limit);
    results
}

// ============================================================================
// Glob Pattern Matching
// ============================================================================

/// Simple glob pattern matching supporting *, ?, and character classes [abc].
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_chars(&pat, &txt)
}

fn glob_match_chars(pattern: &[char], text: &[char]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() {
            match pattern[pi] {
                '*' => {
                    star_pi = pi;
                    star_ti = ti;
                    pi += 1;
                    continue;
                }
                '?' => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                '[' => {
                    // Character class.
                    if let Some((matched, end)) = match_char_class(&pattern[pi..], text[ti])
                        && matched
                    {
                        pi += end;
                        ti += 1;
                        continue;
                    }
                    // Fall through to star backtrack.
                }
                ch => {
                    if ch == text[ti] {
                        pi += 1;
                        ti += 1;
                        continue;
                    }
                    // Fall through to star backtrack.
                }
            }
        }

        // Backtrack to last star.
        if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    // Consume remaining stars.
    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// Match a character class like [abc] or [a-z]. Returns (matched,
/// characters consumed).
fn match_char_class(pattern: &[char], ch: char) -> Option<(bool, usize)> {
    if pattern.is_empty() || pattern[0] != '[' {
        return None;
    }

    let mut i = 1;
    let negate = if i < pattern.len() && pattern[i] == '!' {
        i += 1;
        true
    } else {
        false
    };

    let mut matched = false;
    while i < pattern.len() && pattern[i] != ']' {
        if i + 2 < pattern.len() && pattern[i + 1] == '-' {
            // Range.
            let lo = pattern[i];
            let hi = pattern[i + 2];
            if ch >= lo && ch <= hi {
                matched = true;
            }
            i += 3;
        } else {
            if pattern[i] == ch {
                matched = true;
            }
            i += 1;
        }
    }

    if i < pattern.len() && pattern[i] == ']' {
        let consumed = i + 1; // Include the ']'.
        if negate {
            Some((!matched, consumed))
        } else {
            Some((matched, consumed))
        }
    } else {
        // Malformed — no closing bracket.
        None
    }
}

// ============================================================================
// Levenshtein Distance (for fuzzy matching)
// ============================================================================

/// Compute the Levenshtein edit distance between two strings.
/// Uses bounded computation: returns early if distance exceeds max_distance.
///
/// The distance is counted in *characters*. Counting it in bytes made fuzzy
/// matching useless for non-ASCII names: one substituted kanji costs up to 3
/// against a `FUZZY_MAX_DISTANCE` budget the user thinks of as "a couple of
/// typos", so a near-exact CJK match was rejected while a much worse ASCII one
/// was accepted. The early-out on `abs_diff` of the lengths had the same
/// problem, rejecting candidates before the DP ever ran.
fn levenshtein(a: &str, b: &str) -> u32 {
    levenshtein_bounded(a, b, FUZZY_MAX_DISTANCE + 1)
}

fn levenshtein_bounded(a: &str, b: &str, max: u32) -> u32 {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    // Quick length check.
    let len_diff = m.abs_diff(n);
    if len_diff as u32 > max {
        return max;
    }

    if m == 0 {
        return n as u32;
    }
    if n == 0 {
        return m as u32;
    }

    // Standard DP with single row optimization.
    let mut prev_row: Vec<u32> = (0..=n as u32).collect();
    let mut curr_row: Vec<u32> = vec![0; n + 1];

    for i in 1..=m {
        curr_row[0] = i as u32;
        let mut row_min = curr_row[0];

        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr_row[j] = (prev_row[j] + 1)
                .min(curr_row[j - 1] + 1)
                .min(prev_row[j - 1] + cost);
            row_min = row_min.min(curr_row[j]);
        }

        if row_min > max {
            return max;
        }

        std::mem::swap(&mut prev_row, &mut curr_row);
    }

    prev_row[n]
}

// ============================================================================
// Scanner
// ============================================================================

/// Scan statistics.
#[derive(Debug, Default, Clone)]
struct ScanStats {
    files_found: u64,
    dirs_scanned: u64,
    files_skipped: u64,
    errors: u64,
}

impl fmt::Display for ScanStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Files: {}, Dirs: {}, Skipped: {}, Errors: {}",
            self.files_found, self.dirs_scanned, self.files_skipped, self.errors
        )
    }
}

/// Scan directories and build an index.
fn scan(config: &Config) -> (FileIndex, ScanStats) {
    let mut entries: Vec<IndexEntry> = Vec::new();
    let mut stats = ScanStats::default();

    for root in &config.index_paths {
        let root_path = Path::new(root);
        if !root_path.exists() {
            continue;
        }
        scan_directory(root_path, config, &mut entries, &mut stats);
    }

    let mut index = FileIndex::build_from_entries(entries);
    index.dirs_scanned = stats.dirs_scanned;
    (index, stats)
}

/// Whether `dir` is covered by one of the configured exclusions.
///
/// One function rather than the copy in each of the two scanners: they were
/// identical, which is exactly how they stay identical only until one of them
/// is edited.
///
/// The exclusions are UTF-8 text from the config file, so this matches against
/// a lossy rendering of the directory — a selection heuristic, not an identity
/// check. A directory whose name is not UTF-8 can still be excluded by naming
/// one of its ASCII ancestors.
///
/// `contains` rather than a component-wise comparison is deliberately loose and
/// is the documented behaviour: excluding `node_modules` excludes it at any
/// depth. (The previous `ends_with(excl) || contains(excl)` was the same test
/// written twice — `contains` is true whenever `ends_with` is.)
fn is_excluded_dir(dir: &Path, config: &Config) -> bool {
    let dir_str = dir.to_string_lossy();
    config.exclude_paths.iter().any(|e| dir_str.contains(e))
}

/// Recursively scan a directory.
fn scan_directory(
    dir: &Path,
    config: &Config,
    entries: &mut Vec<IndexEntry>,
    stats: &mut ScanStats,
) {
    if is_excluded_dir(dir, config) {
        return;
    }

    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => {
            stats.errors += 1;
            return;
        }
    };

    stats.dirs_scanned += 1;

    for dir_entry in read_dir {
        let dir_entry = match dir_entry {
            Ok(de) => de,
            Err(_) => {
                stats.errors += 1;
                continue;
            }
        };

        let path = dir_entry.path();
        let metadata = match dir_entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                stats.errors += 1;
                continue;
            }
        };

        let file_type = if metadata.is_dir() {
            FileType::Directory
        } else if metadata.is_symlink() {
            FileType::Symlink
        } else {
            FileType::Regular
        };

        // Get the filename search key. A path with no final component is not a
        // file we can index.
        if path.file_name().is_none() {
            continue;
        }
        let filename = filename_key(&path);

        // Check extension filters.
        if file_type == FileType::Regular {
            if let Some(ext) = path.extension() {
                let ext_str = format!(".{}", ext.to_string_lossy());
                let ext_lower = ext_str.to_ascii_lowercase();

                // Check exclude extensions.
                if config
                    .exclude_extensions
                    .iter()
                    .any(|e| e.to_ascii_lowercase() == ext_lower)
                {
                    stats.files_skipped += 1;
                    continue;
                }

                // Check include extensions (if set).
                if let Some(ref includes) = config.include_extensions
                    && !includes.iter().any(|e| e.to_ascii_lowercase() == ext_lower)
                {
                    stats.files_skipped += 1;
                    continue;
                }
            } else if config.include_extensions.is_some() {
                // No extension and include filter is set — skip.
                stats.files_skipped += 1;
                continue;
            }

            // Check file size.
            if metadata.len() > config.max_file_size {
                stats.files_skipped += 1;
                continue;
            }
        }

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        entries.push(IndexEntry {
            path: path.clone(),
            filename,
            size: metadata.len(),
            mtime,
            file_type,
        });
        stats.files_found += 1;

        // Recurse into subdirectories.
        if metadata.is_dir() {
            scan_directory(&path, config, entries, stats);
        }
    }
}

/// Incremental scan: only re-scan directories whose mtime has changed.
fn scan_incremental(config: &Config, existing: &mut FileIndex) -> ScanStats {
    let mut stats = ScanStats::default();

    for root in &config.index_paths {
        let root_path = Path::new(root);
        if !root_path.exists() {
            continue;
        }
        scan_directory_incremental(root_path, config, existing, &mut stats);
    }

    existing.last_indexed = current_timestamp();
    existing.rebuild_lookup();
    stats
}

/// Incrementally scan a directory: only rescan if mtime differs.
fn scan_directory_incremental(
    dir: &Path,
    config: &Config,
    index: &mut FileIndex,
    stats: &mut ScanStats,
) {
    if is_excluded_dir(dir, config) {
        return;
    }

    let dir_meta = match fs::metadata(dir) {
        Ok(m) => m,
        Err(_) => {
            stats.errors += 1;
            return;
        }
    };

    let dir_mtime = dir_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Check if we already have an entry for this directory with same mtime.
    let needs_rescan = !index
        .entries
        .iter()
        .any(|e| e.path == dir && e.file_type == FileType::Directory && e.mtime == dir_mtime);

    if !needs_rescan {
        // Still recurse to check subdirectories.
        if let Ok(rd) = fs::read_dir(dir) {
            for entry in rd.flatten() {
                if let Ok(m) = entry.metadata()
                    && m.is_dir()
                {
                    scan_directory_incremental(&entry.path(), config, index, stats);
                }
            }
        }
        return;
    }

    // Directory has changed — remove old entries under it and rescan.
    index.remove_path_prefix(dir);
    stats.dirs_scanned += 1;

    let mut new_entries = Vec::new();
    scan_directory(dir, config, &mut new_entries, stats);
    for entry in new_entries {
        index.add_entry(entry);
    }
}

// ============================================================================
// Service Management
// ============================================================================

/// Start the indexer service (runs in foreground, for service manager to daemonize).
fn cmd_start(config: &Config) {
    if !config.enabled {
        eprintln!("error: indexer is disabled. Enable with: indexer config set enabled true");
        process::exit(1);
    }

    println!("Starting file indexer service...");
    println!("  Indexing paths: {:?}", config.index_paths);
    println!("  Scan interval: {} seconds", config.scan_interval_secs);

    // Write PID file.
    if let Err(e) = write_pid_file() {
        eprintln!("warning: could not write PID file: {}", e);
    }

    // Initial full scan.
    println!("Performing initial scan...");
    let (mut index, stats) = scan(config);
    println!("Initial scan complete. {}", stats);

    if let Err(e) = index.save() {
        eprintln!("error: failed to save index: {}", e);
    } else {
        println!(
            "Index saved: {} files, ~{} bytes",
            index.file_count(),
            format_size(index.approx_size_bytes() as u64)
        );
    }

    // Main service loop: periodic rescan.
    loop {
        std::thread::sleep(Duration::from_secs(config.scan_interval_secs));

        println!("Starting incremental rescan...");
        let stats = scan_incremental(config, &mut index);
        println!("Incremental scan complete. {}", stats);

        if let Err(e) = index.save() {
            eprintln!("error: failed to save index: {}", e);
        }
    }
}

/// Stop the indexer service by signaling the PID.
fn cmd_stop() {
    match fs::read_to_string(PID_FILE) {
        Ok(pid_str) => {
            let pid = pid_str.trim();
            println!("Stopping indexer service (PID {})...", pid);
            // On Slate OS, we'd send an IPC shutdown message. For now, remove PID file.
            if let Err(e) = fs::remove_file(PID_FILE) {
                eprintln!("warning: could not remove PID file: {}", e);
            }
            println!("Stop signal sent.");
        }
        Err(_) => {
            eprintln!("error: indexer does not appear to be running (no PID file).");
            process::exit(1);
        }
    }
}

/// Show service status.
fn cmd_status() {
    let running = Path::new(PID_FILE).exists();
    println!(
        "Indexer service: {}",
        if running { "running" } else { "stopped" }
    );

    match FileIndex::load() {
        Ok(index) => {
            println!("  Files indexed:      {}", index.file_count());
            println!("  Directories scanned: {}", index.dirs_scanned);
            println!(
                "  Last indexed:       {}",
                format_timestamp(index.last_indexed)
            );
            println!(
                "  Index size (approx): {}",
                format_size(index.approx_size_bytes() as u64)
            );
            if !index.trigram_index.is_empty() {
                println!("  Content trigrams:   {}", index.trigram_index.len());
            }
        }
        Err(e) => {
            println!("  Index: not available ({})", e);
        }
    }
}

/// Search the index.
fn cmd_search(query: &str, config: &Config) {
    let index = match FileIndex::load() {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("error: could not load index: {}", e);
            eprintln!("hint: run 'indexer reindex' to build the index first.");
            process::exit(1);
        }
    };

    let results = search(&index, query, DEFAULT_RESULT_LIMIT);

    if results.is_empty() && config.index_contents {
        // Try content search as fallback.
        let content_results = search_content(&index, query, DEFAULT_RESULT_LIMIT);
        if content_results.is_empty() {
            println!("No results found for: {}", query);
        } else {
            println!("Content matches for '{}':", query);
            for result in &content_results {
                println!("  {}", result);
            }
            println!("\n({} results)", content_results.len());
        }
    } else if results.is_empty() {
        println!("No results found for: {}", query);
    } else {
        println!("Results for '{}':", query);
        for result in &results {
            println!("  {}", result);
        }
        println!("\n({} results)", results.len());
    }
}

/// Force a reindex.
fn cmd_reindex(path: Option<&str>, config: &Config) {
    if let Some(target) = path {
        println!("Reindexing: {}", target);
        let mut index = FileIndex::load().unwrap_or_else(|_| FileIndex::new());
        let target_path = Path::new(target);
        index.remove_path_prefix(target_path);

        let mut entries = Vec::new();
        let mut stats = ScanStats::default();
        scan_directory(target_path, config, &mut entries, &mut stats);
        for entry in entries {
            index.add_entry(entry);
        }
        index.last_indexed = current_timestamp();
        println!("Reindex complete. {}", stats);

        if let Err(e) = index.save() {
            eprintln!("error: failed to save index: {}", e);
            process::exit(1);
        }
    } else {
        println!("Performing full reindex...");
        let (index, stats) = scan(config);
        println!("Full reindex complete. {}", stats);
        println!(
            "Index: {} files, ~{}",
            index.file_count(),
            format_size(index.approx_size_bytes() as u64)
        );
        if let Err(e) = index.save() {
            eprintln!("error: failed to save index: {}", e);
            process::exit(1);
        }
    }
    println!("Index saved to {}", INDEX_PATH);
}

/// Show or modify configuration.
fn cmd_config(args: &[String]) {
    let mut config = Config::load();

    if args.is_empty() {
        print!("{}", config);
        return;
    }

    if args.len() >= 3 && args[0] == "set" {
        let key = &args[1];
        let value = &args[2..].join(" ");
        config.set_field(key, value);
        match config.save() {
            Ok(()) => println!("Configuration updated: {} = {}", key, value),
            Err(e) => {
                eprintln!("error: failed to save configuration: {}", e);
                process::exit(1);
            }
        }
    } else {
        eprintln!("usage: indexer config set <KEY> <VALUE>");
        process::exit(1);
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return "never".to_string();
    }
    // Simple timestamp format: seconds since epoch (full datetime requires OS time APIs).
    let age_secs = current_timestamp().saturating_sub(ts);
    if age_secs < 60 {
        format!("{} seconds ago", age_secs)
    } else if age_secs < 3600 {
        format!("{} minutes ago", age_secs / 60)
    } else if age_secs < 86400 {
        format!("{} hours ago", age_secs / 3600)
    } else {
        format!("{} days ago", age_secs / 86400)
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn write_pid_file() -> io::Result<()> {
    if let Some(parent) = Path::new(PID_FILE).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(PID_FILE, format!("{}", process::id()))
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug)]
enum IndexError {
    Io(String),
    CorruptIndex(String),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "I/O error: {}", msg),
            Self::CorruptIndex(msg) => write!(f, "corrupt index: {}", msg),
        }
    }
}

impl std::error::Error for IndexError {}

// ============================================================================
// Main / CLI Dispatch
// ============================================================================

fn print_usage() {
    println!("Usage: indexer <COMMAND> [ARGS...]");
    println!();
    println!("Commands:");
    println!("  start               Start the indexing service");
    println!("  stop                Stop the service gracefully");
    println!("  status              Show indexing status");
    println!("  search <QUERY>      Search the index");
    println!("  reindex [PATH]      Force re-scan of specified path or all");
    println!("  config              Show current configuration");
    println!("  config set <K> <V>  Set a config option");
    println!();
    println!("Configuration file: {}", CONFIG_PATH);
    println!("Index file: {}", INDEX_PATH);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    let config = Config::load();

    match args[1].as_str() {
        "start" => cmd_start(&config),
        "stop" => cmd_stop(),
        "status" => cmd_status(),
        "search" => {
            if args.len() < 3 {
                eprintln!("usage: indexer search <QUERY>");
                process::exit(1);
            }
            let query = args[2..].join(" ");
            cmd_search(&query, &config);
        }
        "reindex" => {
            let path = if args.len() >= 3 {
                Some(args[2].as_str())
            } else {
                None
            };
            cmd_reindex(path, &config);
        }
        "config" => {
            cmd_config(&args[2..]);
        }
        "help" | "--help" | "-h" => {
            print_usage();
        }
        other => {
            eprintln!("error: unknown command '{}'", other);
            eprintln!("Run 'indexer help' for usage information.");
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Configuration Tests ----

    #[test]
    fn test_config_default() {
        let cfg = Config::default();
        assert_eq!(cfg.index_paths, vec!["/home"]);
        assert!(!cfg.enabled);
        assert!(!cfg.index_contents);
        assert_eq!(cfg.max_file_size, 50 * 1024 * 1024);
        assert_eq!(cfg.scan_interval_secs, 3600);
    }

    #[test]
    fn test_config_parse() {
        let content = r#"
enabled = true
index_paths = /home, /data
exclude_paths = /tmp, /.git
max_file_size = 10485760
scan_interval_secs = 1800
index_contents = true
exclude_extensions = .o, .tmp
"#;
        let cfg = Config::parse(content);
        assert!(cfg.enabled);
        assert_eq!(cfg.index_paths, vec!["/home", "/data"]);
        assert_eq!(cfg.exclude_paths, vec!["/tmp", "/.git"]);
        assert_eq!(cfg.max_file_size, 10_485_760);
        assert_eq!(cfg.scan_interval_secs, 1800);
        assert!(cfg.index_contents);
        assert_eq!(cfg.exclude_extensions, vec![".o", ".tmp"]);
    }

    #[test]
    fn test_config_parse_empty() {
        let cfg = Config::parse("");
        // Should be all defaults.
        assert_eq!(cfg.index_paths, vec!["/home"]);
        assert!(!cfg.enabled);
    }

    #[test]
    fn test_config_parse_comments() {
        let content = "# This is a comment\nenabled = true\n# Another comment\n";
        let cfg = Config::parse(content);
        assert!(cfg.enabled);
    }

    /// Paths that are legal on this system -- any byte but `/` and NUL -- and
    /// that the comma-joined format could not carry.
    const HOSTILE_PATHS: &[&str] = &[
        "/home/u/Private, Ltd",
        "/home/u/a,b,c",
        "/home/u/trailing space ",
        "/home/u/ leading space",
        "/home/u/tab\there",
        "/home/u/new\nline",
        "/home/u/enabled = false",
        r"/home/u/back\slash",
        r"/home/u/\n",
        r"/home/u/\s",
        "/home/u/# not a comment",
        "/home/u/has = equals",
    ];

    #[test]
    fn a_path_containing_a_comma_stays_one_path() {
        // The security-relevant case: `exclude_paths` decides what is *not*
        // read, so a path that splits leaves the directory the user named
        // unexcluded and its contents indexed.
        for path in HOSTILE_PATHS {
            let cfg = Config {
                index_paths: vec![(*path).to_string()],
                exclude_paths: vec![(*path).to_string()],
                ..Config::default()
            };
            let parsed = Config::parse(&cfg.serialize());
            assert_eq!(
                parsed.exclude_paths,
                vec![(*path).to_string()],
                "exclusion was rewritten: {path:?}"
            );
            assert_eq!(
                parsed.index_paths,
                vec![(*path).to_string()],
                "index path was rewritten: {path:?}"
            );
        }
    }

    #[test]
    fn a_path_cannot_forge_a_config_line() {
        // A newline in a path could otherwise write any setting it liked --
        // `enabled = false` silently turns the indexer off.
        let cfg = Config {
            enabled: true,
            index_contents: true,
            index_paths: vec![
                "/a\nenabled = false".to_string(),
                "/b\nindex_contents = false\nmax_file_size = 1".to_string(),
            ],
            max_file_size: 999,
            ..Config::default()
        };
        let parsed = Config::parse(&cfg.serialize());
        assert!(parsed.enabled, "a path turned the indexer off");
        assert!(parsed.index_contents, "a path changed a setting");
        assert_eq!(parsed.max_file_size, 999, "a path changed a setting");
        assert_eq!(parsed.index_paths.len(), 2, "a path forged an entry");
    }

    #[test]
    fn every_list_round_trips_exactly() {
        let cfg = Config {
            index_paths: HOSTILE_PATHS.iter().map(|s| (*s).to_string()).collect(),
            exclude_paths: HOSTILE_PATHS.iter().map(|s| (*s).to_string()).collect(),
            include_extensions: Some(vec![".a,b".to_string(), " .c ".to_string()]),
            exclude_extensions: vec![".x\ny".to_string()],
            ..Config::default()
        };
        let parsed = Config::parse(&cfg.serialize());
        assert_eq!(parsed.index_paths, cfg.index_paths);
        assert_eq!(parsed.exclude_paths, cfg.exclude_paths);
        assert_eq!(parsed.include_extensions, cfg.include_extensions);
        assert_eq!(parsed.exclude_extensions, cfg.exclude_extensions);
        // Serialising the reparsed config must give the same file back, or the
        // format has a state it cannot express.
        assert_eq!(parsed.serialize(), cfg.serialize());
    }

    #[test]
    fn an_empty_list_stays_empty_rather_than_reverting_to_defaults() {
        // The one thing repeated keys cannot say: no lines is indistinguishable
        // from "key absent", which means keep the defaults.
        let cfg = Config {
            index_paths: Vec::new(),
            exclude_paths: Vec::new(),
            exclude_extensions: Vec::new(),
            ..Config::default()
        };
        let parsed = Config::parse(&cfg.serialize());
        assert!(parsed.index_paths.is_empty());
        assert!(parsed.exclude_paths.is_empty());
        assert!(parsed.exclude_extensions.is_empty());
    }

    #[test]
    fn a_repeated_key_replaces_the_defaults_once_then_appends() {
        let cfg = Config::parse("index_path = /one\nindex_path = /two\nindex_path = /three\n");
        assert_eq!(cfg.index_paths, vec!["/one", "/two", "/three"]);
        // The built-in default must be gone, not appended to.
        assert!(!cfg.index_paths.iter().any(|p| p == "/home"));
    }

    #[test]
    fn a_backslash_before_an_n_survives_the_round_trip() {
        // The replace-chain decoder's signature failure, and `\n` is a legal
        // directory name here.
        for text in [r"\n", r"\\n", r"\s", r"\\", "real\nnewline", "  pad  "] {
            assert_eq!(kv::unescape(&kv::escape(text, &[])), text, "{text:?}");
        }
    }

    #[test]
    fn test_config_roundtrip() {
        let cfg = Config {
            enabled: true,
            index_paths: vec!["/home".into(), "/srv".into()],
            max_file_size: 100_000_000,
            ..Config::default()
        };

        let serialized = cfg.serialize();
        let parsed = Config::parse(&serialized);
        assert_eq!(parsed.enabled, cfg.enabled);
        assert_eq!(parsed.index_paths, cfg.index_paths);
        assert_eq!(parsed.max_file_size, cfg.max_file_size);
    }

    #[test]
    fn test_config_set_field() {
        let mut cfg = Config::default();
        cfg.set_field("enabled", "true");
        assert!(cfg.enabled);
        cfg.set_field("max_file_size", "999");
        assert_eq!(cfg.max_file_size, 999);
        cfg.set_field("include_extensions", ".rs, .py, .txt");
        assert_eq!(
            cfg.include_extensions,
            Some(vec![".rs".into(), ".py".into(), ".txt".into()])
        );
    }

    // ---- Index Building Tests ----

    #[test]
    fn test_index_build_from_entries() {
        let entries = vec![
            IndexEntry {
                path: PathBuf::from("/home/user/hello.txt"),
                filename: "hello.txt".into(),
                size: 100,
                mtime: 1000,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/home/user/world.rs"),
                filename: "world.rs".into(),
                size: 200,
                mtime: 2000,
                file_type: FileType::Regular,
            },
        ];

        let index = FileIndex::build_from_entries(entries);
        assert_eq!(index.file_count(), 2);
        assert!(index.name_lookup.contains_key("hello.txt"));
        assert!(index.name_lookup.contains_key("world.rs"));
    }

    #[test]
    fn test_index_add_entry() {
        let mut index = FileIndex::new();
        index.add_entry(IndexEntry {
            path: PathBuf::from("/foo/bar.txt"),
            filename: "bar.txt".into(),
            size: 42,
            mtime: 500,
            file_type: FileType::Regular,
        });
        assert_eq!(index.file_count(), 1);
        assert!(index.name_lookup.contains_key("bar.txt"));
    }

    #[test]
    fn test_index_remove_path_prefix() {
        let entries = vec![
            IndexEntry {
                path: PathBuf::from("/home/a/file1.txt"),
                filename: "file1.txt".into(),
                size: 10,
                mtime: 100,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/home/b/file2.txt"),
                filename: "file2.txt".into(),
                size: 20,
                mtime: 200,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/data/file3.txt"),
                filename: "file3.txt".into(),
                size: 30,
                mtime: 300,
                file_type: FileType::Regular,
            },
        ];

        let mut index = FileIndex::build_from_entries(entries);
        index.remove_path_prefix(Path::new("/home"));
        assert_eq!(index.file_count(), 1);
        assert_eq!(index.entries[0].filename, "file3.txt");
    }

    // ---- Serialization Tests ----

    #[test]
    fn test_index_serialize_deserialize() {
        let entries = vec![
            IndexEntry {
                path: PathBuf::from("/home/user/doc.txt"),
                filename: "doc.txt".into(),
                size: 1024,
                mtime: 1700000000,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/home/user/src"),
                filename: "src".into(),
                size: 0,
                mtime: 1700000100,
                file_type: FileType::Directory,
            },
        ];

        let index = FileIndex::build_from_entries(entries);
        let data = index.serialize();
        let loaded = FileIndex::deserialize(&data).expect("deserialize should succeed");

        assert_eq!(loaded.file_count(), 2);
        assert_eq!(loaded.entries[0].filename, "doc.txt");
        assert_eq!(loaded.entries[0].size, 1024);
        assert_eq!(loaded.entries[1].filename, "src");
        assert_eq!(loaded.entries[1].file_type, FileType::Directory);
    }

    #[test]
    fn test_index_deserialize_bad_magic() {
        let data = b"BAAD\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let result = FileIndex::deserialize(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_index_deserialize_too_short() {
        let data = b"OIDX";
        let result = FileIndex::deserialize(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_index_deserialize_wrong_version() {
        let mut data = vec![0u8; 32];
        data[0..4].copy_from_slice(INDEX_MAGIC);
        data[4..8].copy_from_slice(&99u32.to_le_bytes()); // Bad version
        let result = FileIndex::deserialize(&data);
        assert!(result.is_err());
    }

    /// A header of 28..=31 bytes passed the old length check (`< 28`) and then
    /// panicked reading `dirs_scanned` out of bytes 24..32. A corrupt or
    /// truncated index file must be an error, never a crash.
    #[test]
    fn a_header_that_stops_mid_field_is_an_error_not_a_panic() {
        for len in 0..INDEX_HEADER_LEN {
            let mut data = vec![0u8; len];
            if len >= 4 {
                data[0..4].copy_from_slice(INDEX_MAGIC);
            }
            if len >= 8 {
                data[4..8].copy_from_slice(&INDEX_VERSION.to_le_bytes());
            }
            assert!(
                FileIndex::deserialize(&data).is_err(),
                "a {len}-byte index is not a valid index"
            );
        }
    }

    /// An entry count in the header that the file does not actually contain
    /// must not be trusted into an out-of-bounds read.
    #[test]
    fn an_entry_count_larger_than_the_file_is_an_error_not_a_panic() {
        let mut data = vec![0u8; INDEX_HEADER_LEN];
        data[0..4].copy_from_slice(INDEX_MAGIC);
        data[4..8].copy_from_slice(&INDEX_VERSION.to_le_bytes());
        data[8..16].copy_from_slice(&1_000u64.to_le_bytes());
        assert!(FileIndex::deserialize(&data).is_err());
    }

    /// The index stores the path it will later hand back to whoever opens the
    /// file, so it must store bytes. Written through `to_string_lossy`, a name
    /// that is not UTF-8 came back with U+FFFD in place of its bytes and no
    /// longer named anything.
    ///
    /// Unix-only: a Windows `OsString` cannot hold such a path at all, and our
    /// target is `target-family = ["unix"]`.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_path_survives_the_index_file() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let raw = b"/home/u/caf\xE9.txt".to_vec();
        let path = PathBuf::from(std::ffi::OsString::from_vec(raw.clone()));
        let entries = vec![IndexEntry {
            filename: filename_key(&path),
            path,
            size: 3,
            mtime: 7,
            file_type: FileType::Regular,
        }];
        let data = FileIndex::build_from_entries(entries).serialize();
        let loaded = FileIndex::deserialize(&data).expect("round trip");
        assert_eq!(
            loaded.entries[0].path.as_os_str().as_bytes(),
            raw.as_slice(),
            "the 0xE9 must come back as 0xE9, not as U+FFFD"
        );
    }

    /// Paths that *are* UTF-8 must survive byte-for-byte too — including the
    /// multi-byte ones, which is the case a lossy round trip happens to pass.
    #[test]
    fn a_multibyte_path_survives_the_index_file() {
        let path = PathBuf::from("/home/u/写真/Ωμέγα.txt");
        let entries = vec![IndexEntry {
            filename: filename_key(&path),
            path: path.clone(),
            size: 3,
            mtime: 7,
            file_type: FileType::Regular,
        }];
        let data = FileIndex::build_from_entries(entries).serialize();
        let loaded = FileIndex::deserialize(&data).expect("round trip");
        assert_eq!(loaded.entries[0].path, path);
        assert_eq!(loaded.entries[0].filename, "Ωμέγα.txt");
    }

    /// The two scanners each carried a copy of this test; they must agree, and
    /// the `contains` match is deliberately depth-independent.
    #[test]
    fn an_exclusion_matches_at_any_depth() {
        let config = Config {
            exclude_paths: vec!["node_modules".to_string()],
            ..Config::default()
        };
        assert!(is_excluded_dir(Path::new("/srv/app/node_modules"), &config));
        assert!(is_excluded_dir(
            Path::new("/srv/app/node_modules/pkg/dist"),
            &config
        ));
        assert!(!is_excluded_dir(Path::new("/srv/app/src"), &config));
    }

    // ---- Search Tests ----

    fn make_test_index() -> FileIndex {
        let entries = vec![
            IndexEntry {
                path: PathBuf::from("/home/user/main.rs"),
                filename: "main.rs".into(),
                size: 500,
                mtime: 1000,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/home/user/main_test.rs"),
                filename: "main_test.rs".into(),
                size: 300,
                mtime: 1100,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/home/user/readme.md"),
                filename: "readme.md".into(),
                size: 1000,
                mtime: 900,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/home/user/lib.rs"),
                filename: "lib.rs".into(),
                size: 800,
                mtime: 1200,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/data/config.yaml"),
                filename: "config.yaml".into(),
                size: 200,
                mtime: 500,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/home/user/documents"),
                filename: "documents".into(),
                size: 0,
                mtime: 800,
                file_type: FileType::Directory,
            },
            IndexEntry {
                path: PathBuf::from("/home/user/main_helper.rs"),
                filename: "main_helper.rs".into(),
                size: 400,
                mtime: 1050,
                file_type: FileType::Regular,
            },
        ];
        FileIndex::build_from_entries(entries)
    }

    #[test]
    fn test_search_exact_match() {
        let index = make_test_index();
        let results = search(&index, "main.rs", 50);
        assert!(!results.is_empty());
        assert_eq!(results[0].rank, SearchRank::Exact);
        assert_eq!(results[0].entry.filename, "main.rs");
    }

    #[test]
    fn test_search_prefix_match() {
        let index = make_test_index();
        let results = search(&index, "main", 50);
        // "main.rs" should be prefix, "main_test.rs" should be prefix, "main_helper.rs" should be prefix
        assert!(results.len() >= 3);
        // First result should be prefix matches.
        for r in &results {
            assert!(
                r.rank == SearchRank::Prefix || r.rank == SearchRank::Fuzzy,
                "unexpected rank: {:?} for {}",
                r.rank,
                r.entry.filename
            );
        }
    }

    #[test]
    fn test_search_substring_match() {
        let index = make_test_index();
        let results = search(&index, "test", 50);
        assert!(!results.is_empty());
        // "main_test.rs" contains "test" as substring.
        assert!(results.iter().any(|r| r.entry.filename == "main_test.rs"));
    }

    #[test]
    fn test_search_case_insensitive() {
        let index = make_test_index();
        let results = search(&index, "MAIN.RS", 50);
        assert!(!results.is_empty());
        assert_eq!(results[0].entry.filename, "main.rs");
    }

    #[test]
    fn test_search_glob_star() {
        let index = make_test_index();
        let results = search(&index, "*.rs", 50);
        assert!(results.len() >= 3);
        for r in &results {
            assert!(r.entry.filename.ends_with(".rs"));
        }
    }

    #[test]
    fn test_search_glob_question() {
        let index = make_test_index();
        let results = search(&index, "lib.??", 50);
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.entry.filename == "lib.rs"));
    }

    #[test]
    fn test_search_glob_prefix() {
        let index = make_test_index();
        let results = search(&index, "main_*.rs", 50);
        assert!(results.len() >= 2);
        for r in &results {
            assert!(r.entry.filename.starts_with("main_"));
            assert!(r.entry.filename.ends_with(".rs"));
        }
    }

    #[test]
    fn test_search_path_match() {
        let index = make_test_index();
        let results = search(&index, "/data/", 50);
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.entry.filename == "config.yaml"));
    }

    #[test]
    fn test_search_fuzzy_match() {
        let index = make_test_index();
        // "reaadme.md" is edit distance 1 from "readme.md".
        let results = search(&index, "reaadme.md", 50);
        assert!(results.iter().any(|r| r.entry.filename == "readme.md"));
    }

    #[test]
    fn test_search_no_results() {
        let index = make_test_index();
        let results = search(&index, "nonexistent_file_xyz_123456", 50);
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_result_limit() {
        let index = make_test_index();
        let results = search(&index, "*.rs", 2);
        assert!(results.len() <= 2);
    }

    #[test]
    fn test_search_ranking_order() {
        let index = make_test_index();
        let results = search(&index, "main.rs", 50);
        // Exact match should come first.
        assert_eq!(results[0].rank, SearchRank::Exact);
        // Subsequent results should not have a better rank.
        for i in 1..results.len() {
            assert!(results[i].rank >= results[0].rank);
        }
    }

    // ---- Glob Pattern Tests ----

    #[test]
    fn test_glob_star() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(!glob_match("*.rs", "main.py"));
    }

    #[test]
    fn test_glob_question() {
        assert!(glob_match("?.rs", "a.rs"));
        assert!(!glob_match("?.rs", "ab.rs"));
    }

    #[test]
    fn test_glob_char_class() {
        assert!(glob_match("[abc].txt", "a.txt"));
        assert!(glob_match("[abc].txt", "b.txt"));
        assert!(!glob_match("[abc].txt", "d.txt"));
    }

    #[test]
    fn test_glob_char_range() {
        assert!(glob_match("[a-z].txt", "m.txt"));
        assert!(!glob_match("[a-z].txt", "5.txt"));
    }

    #[test]
    fn test_glob_negated_class() {
        assert!(!glob_match("[!abc].txt", "a.txt"));
        assert!(glob_match("[!abc].txt", "d.txt"));
    }

    #[test]
    fn test_glob_complex() {
        assert!(glob_match("test_*.py", "test_main.py"));
        assert!(glob_match("**/main.*", "src/main.rs"));
        assert!(glob_match("*.??", "file.rs"));
    }

    #[test]
    fn test_glob_empty() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "a"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
    }

    // ---- Levenshtein Distance Tests ----

    #[test]
    fn test_levenshtein_identical() {
        assert_eq!(levenshtein("hello", "hello"), 0);
    }

    #[test]
    fn test_levenshtein_insertion() {
        assert_eq!(levenshtein("helo", "hello"), 1);
    }

    #[test]
    fn test_levenshtein_deletion() {
        assert_eq!(levenshtein("hello", "helo"), 1);
    }

    #[test]
    fn test_levenshtein_substitution() {
        assert_eq!(levenshtein("hello", "hallo"), 1);
    }

    // ---- Byte/character confusion ----
    //
    // The glob matcher and the edit distance both used to walk bytes.

    #[test]
    fn a_glob_question_mark_matches_one_character_not_one_byte() {
        let mut checked = 0;
        for (ch, width) in [("é", 2), ("日", 3), ("😀", 4)] {
            let name = format!("{ch}.txt");
            assert!(glob_match("?.txt", &name), "`?` should match {name:?}");
            let many = "?".repeat(width);
            assert!(
                !glob_match(&format!("{many}.txt"), &name),
                "{width} `?`s must no longer match the {width} bytes of {name:?}"
            );
            checked += 1;
        }
        assert!(checked >= 3, "only {checked} checked");
        assert!(glob_match("??.txt", "日本.txt"));
    }

    #[test]
    fn a_glob_class_compares_whole_characters() {
        // é and è share a lead byte, so a byte-wise class confused them.
        assert!(glob_match("[é]", "é"));
        assert!(!glob_match("[é]*", "èb"));
        // A non-ASCII range compares scalar values now, not encoding bytes.
        assert!(glob_match("[а-я]", "р"));
        assert!(!glob_match("[а-я]", "z"));
    }

    #[test]
    fn edit_distance_counts_characters_not_bytes() {
        // One substituted kanji is one edit, not three.
        assert_eq!(levenshtein("日本", "日水"), 1);
        // One inserted kanji is one edit, not three.
        assert_eq!(levenshtein("日", "日本"), 1);
        assert_eq!(levenshtein("café", "cafe"), 1);
        // Identical non-ASCII strings are distance 0 either way; the point is
        // that a near miss stays inside the fuzzy budget.
        assert!(levenshtein("日本語", "日本誤") <= FUZZY_MAX_DISTANCE);
    }

    #[test]
    fn an_ascii_pattern_and_distance_are_unchanged() {
        let mut checked = 0;
        for (pat, text, want) in [
            ("*.rs", "main.rs", true),
            ("*.rs", "main.py", false),
            ("?.txt", "a.txt", true),
            ("?.txt", "ab.txt", false),
            ("[abc]*", "batch", true),
            ("[!abc]*", "batch", false),
            ("src/*/mod.rs", "src/net/mod.rs", true),
        ] {
            assert_eq!(glob_match(pat, text), want, "{pat:?} vs {text:?}");
            checked += 1;
        }
        for (a, b, want) in [
            ("hello", "hello", 0),
            ("helo", "hello", 1),
            ("hello", "hallo", 1),
        ] {
            assert_eq!(levenshtein(a, b), want, "{a:?} vs {b:?}");
            checked += 1;
        }
        assert!(checked >= 10, "only {checked} checked");
    }

    #[test]
    fn test_levenshtein_two_edits() {
        assert_eq!(levenshtein("hello", "hlelo"), 2);
    }

    #[test]
    fn test_levenshtein_empty() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn test_levenshtein_bounded_cutoff() {
        // These are far apart — bounded version should return early.
        let d = levenshtein_bounded("abcdef", "xyz", 3);
        assert!(d >= 3);
    }

    // ---- Trigram Tests ----

    #[test]
    fn test_extract_trigrams() {
        let trigrams = extract_trigrams("hello");
        // "hel", "ell", "llo"
        assert_eq!(trigrams.len(), 3);
        assert!(trigrams.contains(b"hel"));
        assert!(trigrams.contains(b"ell"));
        assert!(trigrams.contains(b"llo"));
    }

    #[test]
    fn test_extract_trigrams_short() {
        let trigrams = extract_trigrams("hi");
        assert!(trigrams.is_empty());
    }

    #[test]
    fn test_extract_trigrams_case_insensitive() {
        let t1 = extract_trigrams("Hello");
        let t2 = extract_trigrams("hello");
        assert_eq!(t1, t2);
    }

    #[test]
    fn test_content_search_basic() {
        let mut index = FileIndex::new();
        index.add_entry(IndexEntry {
            path: PathBuf::from("/file1.txt"),
            filename: "file1.txt".into(),
            size: 100,
            mtime: 1000,
            file_type: FileType::Regular,
        });
        index.add_entry(IndexEntry {
            path: PathBuf::from("/file2.txt"),
            filename: "file2.txt".into(),
            size: 200,
            mtime: 2000,
            file_type: FileType::Regular,
        });

        // Index content for file 0.
        index_file_content(&mut index, 0, "hello world");
        // Index content for file 1.
        index_file_content(&mut index, 1, "goodbye world");

        let results = search_content(&index, "hello", 50);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.filename, "file1.txt");

        let results = search_content(&index, "world", 50);
        assert_eq!(results.len(), 2);
    }

    // ---- Exclusion Pattern Tests ----

    #[test]
    fn test_extension_excluded() {
        let config = Config {
            exclude_extensions: vec![".o".into(), ".tmp".into()],
            ..Config::default()
        };
        // Simulate what the scanner checks.
        let ext = ".o";
        let excluded = config
            .exclude_extensions
            .iter()
            .any(|e| e.eq_ignore_ascii_case(ext));
        assert!(excluded);
    }

    #[test]
    fn test_extension_included() {
        let config = Config {
            include_extensions: Some(vec![".rs".into(), ".py".into()]),
            ..Config::default()
        };
        let ext = ".rs";
        let included = config
            .include_extensions
            .as_ref()
            .map(|exts| exts.iter().any(|e| e.eq_ignore_ascii_case(ext)))
            .unwrap_or(true);
        assert!(included);

        let ext2 = ".txt";
        let included2 = config
            .include_extensions
            .as_ref()
            .map(|exts| exts.iter().any(|e| e.eq_ignore_ascii_case(ext2)))
            .unwrap_or(true);
        assert!(!included2);
    }

    // ---- Utility Tests ----

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1073741824), "1.00 GB");
    }

    #[test]
    fn test_file_type_roundtrip() {
        for ft in [
            FileType::Regular,
            FileType::Directory,
            FileType::Symlink,
            FileType::Other,
        ] {
            assert_eq!(FileType::from_byte(ft.as_byte()), ft);
        }
    }

    // ---- Index Incremental Update Tests ----

    #[test]
    fn test_index_incremental_add() {
        let entries = vec![IndexEntry {
            path: PathBuf::from("/home/old.txt"),
            filename: "old.txt".into(),
            size: 50,
            mtime: 100,
            file_type: FileType::Regular,
        }];
        let mut index = FileIndex::build_from_entries(entries);
        assert_eq!(index.file_count(), 1);

        index.add_entry(IndexEntry {
            path: PathBuf::from("/home/new.txt"),
            filename: "new.txt".into(),
            size: 75,
            mtime: 200,
            file_type: FileType::Regular,
        });
        assert_eq!(index.file_count(), 2);
        assert!(index.name_lookup.contains_key("new.txt"));
    }

    #[test]
    fn test_index_incremental_remove_and_add() {
        let entries = vec![
            IndexEntry {
                path: PathBuf::from("/home/a/one.txt"),
                filename: "one.txt".into(),
                size: 10,
                mtime: 100,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/home/a/two.txt"),
                filename: "two.txt".into(),
                size: 20,
                mtime: 200,
                file_type: FileType::Regular,
            },
            IndexEntry {
                path: PathBuf::from("/home/b/three.txt"),
                filename: "three.txt".into(),
                size: 30,
                mtime: 300,
                file_type: FileType::Regular,
            },
        ];
        let mut index = FileIndex::build_from_entries(entries);

        // Simulate rescan of /home/a: remove old, add new.
        index.remove_path_prefix(Path::new("/home/a"));
        assert_eq!(index.file_count(), 1);
        assert_eq!(index.entries[0].filename, "three.txt");

        index.add_entry(IndexEntry {
            path: PathBuf::from("/home/a/updated.txt"),
            filename: "updated.txt".into(),
            size: 99,
            mtime: 999,
            file_type: FileType::Regular,
        });
        assert_eq!(index.file_count(), 2);
        assert!(index.name_lookup.contains_key("updated.txt"));
    }
}
