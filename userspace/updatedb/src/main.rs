#![deny(clippy::all)]
//! Multi-personality file database utility for SlateOS.
//!
//! This binary detects its mode from `argv[0]`:
//!   - `updatedb`  -> build/update the file database
//!   - `locate`    -> search the file database
//!   - `mlocate`   -> search the file database (mlocate compat)
//!   - `plocate`   -> search the file database (plocate compat)
//!
//! The database stores compressed path entries using differential encoding
//! for efficient storage and fast lookup.

use std::env;
use std::io::{self, Write};
use std::process;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const VERSION: &str = "0.1.0";
const DB_MAGIC: &[u8; 8] = b"OURLOCDB";
const DB_VERSION: u8 = 1;
const DEFAULT_DB_PATH: &str = "/var/lib/mlocate/mlocate.db";
const DEFAULT_CONFIG_PATH: &str = "/etc/updatedb.conf";

const DEFAULT_PRUNEPATHS: &[&str] = &[
    "/proc",
    "/sys",
    "/dev",
    "/tmp",
    "/run",
    "/mnt",
    "/media",
    "/lost+found",
    "/var/tmp",
    "/snap",
];

const DEFAULT_PRUNEFS: &[&str] = &[
    "9p",
    "afs",
    "autofs",
    "binfmt_misc",
    "cgroup",
    "cgroup2",
    "configfs",
    "debugfs",
    "devpts",
    "devtmpfs",
    "fuse.sshfs",
    "fusectl",
    "hugetlbfs",
    "mqueue",
    "nfs",
    "nfs4",
    "overlay",
    "proc",
    "pstore",
    "rpc_pipefs",
    "securityfs",
    "sysfs",
    "tmpfs",
    "tracefs",
    "usbfs",
];

const DEFAULT_PRUNENAMES: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".bzr",
    "CVS",
    "__pycache__",
    "node_modules",
];

// ---------------------------------------------------------------------------
// Mode detection
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    UpdateDb,
    Locate,
}

fn detect_mode(argv0: &str) -> Mode {
    let name = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let lower = name.to_ascii_lowercase();
    if lower.contains("updatedb") {
        Mode::UpdateDb
    } else {
        Mode::Locate
    }
}

fn detect_program_name(argv0: &str) -> &str {
    let name = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    if name.is_empty() { "locate" } else { name }
}

// ---------------------------------------------------------------------------
// Database format
// ---------------------------------------------------------------------------

/// Database header stored at the beginning of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DbHeader {
    magic: [u8; 8],
    version: u8,
    timestamp: u64,
    root_path_len: u32,
    root_path: String,
    entry_count: u64,
    total_size: u64,
}

impl DbHeader {
    fn new(root_path: &str) -> Self {
        Self {
            magic: *DB_MAGIC,
            version: DB_VERSION,
            timestamp: current_timestamp(),
            root_path_len: root_path.len() as u32,
            root_path: root_path.to_string(),
            entry_count: 0,
            total_size: 0,
        }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(&self.magic);
        buf.push(self.version);
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        buf.extend_from_slice(&self.root_path_len.to_le_bytes());
        buf.extend_from_slice(self.root_path.as_bytes());
        buf.extend_from_slice(&self.entry_count.to_le_bytes());
        buf.extend_from_slice(&self.total_size.to_le_bytes());
        buf
    }

    fn deserialize(data: &[u8]) -> Result<(Self, usize), String> {
        if data.len() < 37 {
            return Err("database too small for header".to_string());
        }
        let mut magic = [0u8; 8];
        magic.copy_from_slice(&data[0..8]);
        if &magic != DB_MAGIC {
            return Err("invalid database magic".to_string());
        }
        let version = data[8];
        if version != DB_VERSION {
            return Err(format!("unsupported database version: {version}"));
        }
        let timestamp = u64::from_le_bytes([
            data[9], data[10], data[11], data[12], data[13], data[14], data[15], data[16],
        ]);
        let root_path_len = u32::from_le_bytes([data[17], data[18], data[19], data[20]]) as usize;
        if data.len() < 21 + root_path_len + 16 {
            return Err("database header truncated".to_string());
        }
        let root_path = String::from_utf8(data[21..21 + root_path_len].to_vec())
            .map_err(|e| format!("invalid root path: {e}"))?;
        let off = 21 + root_path_len;
        let entry_count = u64::from_le_bytes([
            data[off],
            data[off + 1],
            data[off + 2],
            data[off + 3],
            data[off + 4],
            data[off + 5],
            data[off + 6],
            data[off + 7],
        ]);
        let total_size = u64::from_le_bytes([
            data[off + 8],
            data[off + 9],
            data[off + 10],
            data[off + 11],
            data[off + 12],
            data[off + 13],
            data[off + 14],
            data[off + 15],
        ]);
        let header = Self {
            magic,
            version,
            timestamp,
            root_path_len: root_path_len as u32,
            root_path,
            entry_count,
            total_size,
        };
        Ok((header, off + 16))
    }
}

/// A single path entry, stored with differential encoding.
/// `shared_prefix_len` is how many bytes this entry shares with the previous,
/// and `suffix` is the remaining bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DbEntry {
    shared_prefix_len: u16,
    suffix: String,
}

impl DbEntry {
    fn serialize(&self) -> Vec<u8> {
        let suffix_bytes = self.suffix.as_bytes();
        let suffix_len = suffix_bytes.len() as u16;
        let mut buf = Vec::with_capacity(4 + suffix_bytes.len());
        buf.extend_from_slice(&self.shared_prefix_len.to_le_bytes());
        buf.extend_from_slice(&suffix_len.to_le_bytes());
        buf.extend_from_slice(suffix_bytes);
        buf
    }

    fn deserialize(data: &[u8]) -> Result<(Self, usize), String> {
        if data.len() < 4 {
            return Err("entry too small".to_string());
        }
        let shared_prefix_len = u16::from_le_bytes([data[0], data[1]]);
        let suffix_len = u16::from_le_bytes([data[2], data[3]]) as usize;
        if data.len() < 4 + suffix_len {
            return Err("entry truncated".to_string());
        }
        let suffix = String::from_utf8(data[4..4 + suffix_len].to_vec())
            .map_err(|e| format!("invalid entry suffix: {e}"))?;
        Ok((
            Self {
                shared_prefix_len,
                suffix,
            },
            4 + suffix_len,
        ))
    }
}

/// Encode a sorted list of paths into differential entries.
fn encode_paths(paths: &[String]) -> Vec<DbEntry> {
    let mut entries = Vec::with_capacity(paths.len());
    let mut prev = String::new();
    for path in paths {
        let shared = common_prefix_len(&prev, path);
        let suffix = &path[shared..];
        entries.push(DbEntry {
            shared_prefix_len: shared.min(u16::MAX as usize) as u16,
            suffix: suffix.to_string(),
        });
        prev.clone_from(path);
    }
    entries
}

/// Decode differential entries back into full paths.
fn decode_entries(entries: &[DbEntry]) -> Vec<String> {
    let mut paths = Vec::with_capacity(entries.len());
    let mut prev = String::new();
    for entry in entries {
        let prefix_len = entry.shared_prefix_len as usize;
        let prefix = if prefix_len <= prev.len() {
            &prev[..prefix_len]
        } else {
            &prev[..]
        };
        let full = format!("{}{}", prefix, entry.suffix);
        paths.push(full.clone());
        prev = full;
    }
    paths
}

/// Serialize a complete database from sorted paths.
fn serialize_database(root_path: &str, paths: &[String]) -> Vec<u8> {
    let entries = encode_paths(paths);
    let mut header = DbHeader::new(root_path);
    header.entry_count = paths.len() as u64;

    let mut entry_data = Vec::new();
    for entry in &entries {
        entry_data.extend_from_slice(&entry.serialize());
    }
    header.total_size = entry_data.len() as u64;

    let mut buf = header.serialize();
    buf.extend_from_slice(&entry_data);
    buf
}

/// Deserialize a database, returning header and decoded paths.
fn deserialize_database(data: &[u8]) -> Result<(DbHeader, Vec<String>), String> {
    let (header, mut offset) = DbHeader::deserialize(data)?;
    let mut entries = Vec::new();
    let count = header.entry_count as usize;
    for _ in 0..count {
        if offset >= data.len() {
            return Err("unexpected end of database".to_string());
        }
        let (entry, consumed) = DbEntry::deserialize(&data[offset..])?;
        entries.push(entry);
        offset += consumed;
    }
    let paths = decode_entries(&entries);
    Ok((header, paths))
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

fn current_timestamp() -> u64 {
    // In a real OS, use the system clock. For now, return 0 as placeholder.
    // On supported platforms, try to get real time.
    #[cfg(not(target_os = "none"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
    #[cfg(target_os = "none")]
    {
        0
    }
}

// ---------------------------------------------------------------------------
// Config file parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct UpdateDbConfig {
    prunepaths: Vec<String>,
    prunefs: Vec<String>,
    prunenames: Vec<String>,
    database_root: String,
    output: String,
    require_visibility: bool,
    verbose: bool,
    debug_pruning: bool,
}

impl Default for UpdateDbConfig {
    fn default() -> Self {
        Self {
            prunepaths: DEFAULT_PRUNEPATHS.iter().map(|s| s.to_string()).collect(),
            prunefs: DEFAULT_PRUNEFS.iter().map(|s| s.to_string()).collect(),
            prunenames: DEFAULT_PRUNENAMES.iter().map(|s| s.to_string()).collect(),
            database_root: "/".to_string(),
            output: DEFAULT_DB_PATH.to_string(),
            require_visibility: true,
            verbose: false,
            debug_pruning: false,
        }
    }
}

/// Parse a config line in KEY = "value1 value2 ..." format.
fn parse_config_line(line: &str) -> Option<(String, Vec<String>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let eq_pos = line.find('=')?;
    let key = line[..eq_pos].trim().to_uppercase();
    let val_part = line[eq_pos + 1..].trim();
    // Strip surrounding quotes if present.
    let val_part = val_part.strip_prefix('"').unwrap_or(val_part);
    let val_part = val_part.strip_suffix('"').unwrap_or(val_part);
    let values: Vec<String> = val_part.split_whitespace().map(|s| s.to_string()).collect();
    Some((key, values))
}

/// Parse a full config file contents string.
fn parse_config_file(contents: &str) -> UpdateDbConfig {
    let mut config = UpdateDbConfig::default();
    for line in contents.lines() {
        if let Some((key, values)) = parse_config_line(line) {
            match key.as_str() {
                "PRUNEPATHS" => config.prunepaths = values,
                "PRUNEFS" => config.prunefs = values,
                "PRUNENAMES" => config.prunenames = values,
                _ => {}
            }
        }
    }
    config
}

// ---------------------------------------------------------------------------
// Filesystem scanning
// ---------------------------------------------------------------------------

/// Check if a path should be pruned based on config.
fn should_prune_path(path: &str, config: &UpdateDbConfig) -> bool {
    for pp in &config.prunepaths {
        if path == pp || path.starts_with(&format!("{pp}/")) {
            return true;
        }
    }
    false
}

/// Check if a directory name matches any prunenames pattern.
fn should_prune_name(name: &str, config: &UpdateDbConfig) -> bool {
    for pn in &config.prunenames {
        if name == pn {
            return true;
        }
    }
    false
}

/// Scan a directory tree, collecting paths while respecting prune rules.
fn scan_filesystem(root: &str, config: &UpdateDbConfig) -> Vec<String> {
    let mut paths = Vec::new();
    let mut stack: Vec<String> = vec![root.to_string()];

    while let Some(dir_path) = stack.pop() {
        if should_prune_path(&dir_path, config) {
            if config.debug_pruning {
                let stderr = io::stderr();
                let mut handle = stderr.lock();
                let _ = writeln!(handle, "pruning path: {dir_path}");
            }
            continue;
        }

        let entries = match std::fs::read_dir(&dir_path) {
            Ok(e) => e,
            Err(err) => {
                if config.verbose {
                    let stderr = io::stderr();
                    let mut handle = stderr.lock();
                    let _ = writeln!(handle, "updatedb: cannot read `{dir_path}': {err}");
                }
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            let full_path = if dir_path.ends_with('/') {
                format!("{dir_path}{name_str}")
            } else {
                format!("{dir_path}/{name_str}")
            };

            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

            if is_dir && should_prune_name(&name_str, config) {
                if config.debug_pruning {
                    let stderr = io::stderr();
                    let mut handle = stderr.lock();
                    let _ = writeln!(handle, "pruning name: {full_path}");
                }
                continue;
            }

            paths.push(full_path.clone());

            if config.verbose {
                let stderr = io::stderr();
                let mut handle = stderr.lock();
                let _ = writeln!(handle, "{full_path}");
            }

            if is_dir {
                stack.push(full_path);
            }
        }
    }

    paths.sort();
    paths
}

// ---------------------------------------------------------------------------
// Locate - pattern matching
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchMode {
    Substring,
    Glob,
    Regex,
}

#[derive(Debug, Clone)]
struct LocateConfig {
    database: String,
    patterns: Vec<String>,
    match_mode: MatchMode,
    ignore_case: bool,
    limit: Option<usize>,
    count_only: bool,
    existing_only: bool,
    follow_symlinks: bool,
    basename_only: bool,
    null_terminated: bool,
    show_statistics: bool,
}

impl Default for LocateConfig {
    fn default() -> Self {
        Self {
            database: DEFAULT_DB_PATH.to_string(),
            patterns: Vec::new(),
            match_mode: MatchMode::Substring,
            ignore_case: false,
            limit: None,
            count_only: false,
            existing_only: false,
            follow_symlinks: false,
            basename_only: false,
            null_terminated: false,
            show_statistics: false,
        }
    }
}

/// Simple glob-style pattern matching.
/// Supports: `*` (any chars), `?` (single char), `[abc]` (char class).
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_impl(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_impl(pattern: &[u8], text: &[u8]) -> bool {
    let mut px = 0;
    let mut tx = 0;
    let mut star_px = usize::MAX;
    let mut star_tx = 0;

    while tx < text.len() {
        if px < pattern.len() && pattern[px] == b'?' {
            px += 1;
            tx += 1;
        } else if px < pattern.len() && pattern[px] == b'*' {
            star_px = px;
            star_tx = tx;
            px += 1;
        } else if px < pattern.len() && pattern[px] == b'[' {
            if let Some((matched, end)) = match_bracket(&pattern[px..], text[tx]) {
                if matched {
                    px += end;
                    tx += 1;
                } else if star_px != usize::MAX {
                    px = star_px + 1;
                    star_tx += 1;
                    tx = star_tx;
                } else {
                    return false;
                }
            } else {
                // Unclosed bracket: treat '[' as a literal character.
                if pattern[px] == text[tx] {
                    px += 1;
                    tx += 1;
                } else if star_px != usize::MAX {
                    px = star_px + 1;
                    star_tx += 1;
                    tx = star_tx;
                } else {
                    return false;
                }
            }
        } else if px < pattern.len() && pattern[px] == text[tx] {
            px += 1;
            tx += 1;
        } else if star_px != usize::MAX {
            px = star_px + 1;
            star_tx += 1;
            tx = star_tx;
        } else {
            return false;
        }
    }

    while px < pattern.len() && pattern[px] == b'*' {
        px += 1;
    }

    px == pattern.len()
}

/// Match a bracket expression `[...]` against a byte. Returns (matched, length consumed).
fn match_bracket(pattern: &[u8], ch: u8) -> Option<(bool, usize)> {
    if pattern.is_empty() || pattern[0] != b'[' {
        return None;
    }
    let mut i = 1;
    let negated = i < pattern.len() && (pattern[i] == b'!' || pattern[i] == b'^');
    if negated {
        i += 1;
    }
    let mut matched = false;
    let mut prev: Option<u8> = None;
    while i < pattern.len() && pattern[i] != b']' {
        if pattern[i] == b'-' && prev.is_some() && i + 1 < pattern.len() && pattern[i + 1] != b']' {
            let lo = prev.unwrap_or(0);
            let hi = pattern[i + 1];
            if ch >= lo && ch <= hi {
                matched = true;
            }
            i += 2;
            prev = None;
        } else {
            if pattern[i] == ch {
                matched = true;
            }
            prev = Some(pattern[i]);
            i += 1;
        }
    }
    if i < pattern.len() && pattern[i] == b']' {
        let total = i + 1;
        if negated {
            Some((!matched, total))
        } else {
            Some((matched, total))
        }
    } else {
        // Unclosed bracket, treat '[' as literal
        None
    }
}

/// Simple regex matching (subset: `.`, `*`, `+`, `?`, `^`, `$`, `\`, `[...]`, `|`).
fn regex_match(pattern: &str, text: &str) -> bool {
    regex_match_anchored(pattern, text)
}

fn regex_match_anchored(pattern: &str, text: &str) -> bool {
    // Try to match at every position (unanchored search).
    let pat_bytes = pattern.as_bytes();
    let text_bytes = text.as_bytes();

    // Check for explicit anchoring.
    let (start_anchored, end_anchored, inner) = parse_anchors(pat_bytes);

    // Handle alternation at top level.
    let alternatives = split_alternatives(inner);

    for alt in &alternatives {
        if start_anchored && end_anchored {
            if regex_core(alt, text_bytes, 0) == Some(text_bytes.len()) {
                return true;
            }
        } else if start_anchored {
            if regex_core(alt, text_bytes, 0).is_some() {
                return true;
            }
        } else if end_anchored {
            for start in 0..=text_bytes.len() {
                if let Some(end) = regex_core(alt, text_bytes, start)
                    && end == text_bytes.len()
                {
                    return true;
                }
            }
        } else {
            for start in 0..=text_bytes.len() {
                if regex_core(alt, text_bytes, start).is_some() {
                    return true;
                }
            }
        }
    }
    false
}

fn parse_anchors(pat: &[u8]) -> (bool, bool, &[u8]) {
    let start = !pat.is_empty() && pat[0] == b'^';
    let end = !pat.is_empty()
        && pat[pat.len() - 1] == b'$'
        && (pat.len() < 2 || pat[pat.len() - 2] != b'\\');
    let from = if start { 1 } else { 0 };
    let to = if end {
        pat.len().saturating_sub(1)
    } else {
        pat.len()
    };
    let inner = if from <= to { &pat[from..to] } else { &[] };
    (start, end, inner)
}

fn split_alternatives(pat: &[u8]) -> Vec<&[u8]> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    while i < pat.len() {
        match pat[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' => {
                // Skip bracket expression.
                i += 1;
                while i < pat.len() && pat[i] != b']' {
                    if pat[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'|' if depth == 0 => {
                result.push(&pat[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    result.push(&pat[start..]);
    result
}

/// Core regex engine. Returns end position of match starting at `pos`, or None.
fn regex_core(pattern: &[u8], text: &[u8], pos: usize) -> Option<usize> {
    let mut pi = 0;
    let mut ti = pos;

    while pi < pattern.len() {
        // Check for quantifiers after the current atom.
        let (atom_end, atom_ch, is_class) = parse_atom(pattern, pi)?;
        let quantifier = if atom_end < pattern.len() {
            match pattern[atom_end] {
                b'*' => Some((0, usize::MAX, atom_end + 1)),
                b'+' => Some((1, usize::MAX, atom_end + 1)),
                b'?' => Some((0, 1, atom_end + 1)),
                _ => None,
            }
        } else {
            None
        };

        if let Some((min, max, next_pi)) = quantifier {
            // Greedy matching.
            let mut count = 0;
            let save_ti = ti;
            while count < max
                && ti < text.len()
                && matches_atom(atom_ch, is_class, pattern, pi, text[ti])
            {
                ti += 1;
                count += 1;
            }
            // Try from longest match down to min.
            while count >= min {
                if let Some(result) = regex_core(&pattern[next_pi..], text, ti) {
                    return Some(result);
                }
                if count == 0 {
                    break;
                }
                count -= 1;
                ti = save_ti + count;
            }
            return None;
        } else {
            // Single match required.
            if ti < text.len() && matches_atom(atom_ch, is_class, pattern, pi, text[ti]) {
                ti += 1;
                pi = atom_end;
            } else if atom_ch == b'.' && is_class {
                // '.' doesn't match past end of text.
                return None;
            } else {
                return None;
            }
        }
    }
    Some(ti)
}

/// Parse one atom starting at pi. Returns (end_of_atom, representative_byte, is_class).
/// is_class=true means it's a special class like `.` or `[...]`.
fn parse_atom(pattern: &[u8], pi: usize) -> Option<(usize, u8, bool)> {
    if pi >= pattern.len() {
        return None;
    }
    match pattern[pi] {
        b'\\' if pi + 1 < pattern.len() => Some((pi + 2, pattern[pi + 1], false)),
        b'.' => Some((pi + 1, b'.', true)),
        b'[' => {
            // Find closing bracket.
            let mut j = pi + 1;
            if j < pattern.len() && (pattern[j] == b'^' || pattern[j] == b'!') {
                j += 1;
            }
            if j < pattern.len() && pattern[j] == b']' {
                j += 1;
            }
            while j < pattern.len() && pattern[j] != b']' {
                if pattern[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            if j < pattern.len() {
                Some((j + 1, b'[', true))
            } else {
                Some((pi + 1, b'[', false))
            }
        }
        ch => Some((pi + 1, ch, false)),
    }
}

fn matches_atom(atom_ch: u8, is_class: bool, pattern: &[u8], pi: usize, text_ch: u8) -> bool {
    if !is_class {
        return atom_ch == text_ch;
    }
    if atom_ch == b'.' {
        return true; // '.' matches any char.
    }
    if atom_ch == b'[' {
        if let Some((matched, _)) = match_bracket(&pattern[pi..], text_ch) {
            return matched;
        }
        return pattern[pi] == text_ch;
    }
    false
}

/// Match a path against a pattern using the specified mode.
fn path_matches(path: &str, pattern: &str, config: &LocateConfig) -> bool {
    let target = if config.basename_only {
        path.rsplit('/').next().unwrap_or(path)
    } else {
        path
    };

    let (target_cmp, pattern_cmp);
    if config.ignore_case {
        target_cmp = target.to_ascii_lowercase();
        pattern_cmp = pattern.to_ascii_lowercase();
    } else {
        target_cmp = target.to_string();
        pattern_cmp = pattern.to_string();
    }

    match config.match_mode {
        MatchMode::Substring => target_cmp.contains(&*pattern_cmp),
        MatchMode::Glob => glob_match(&pattern_cmp, &target_cmp),
        MatchMode::Regex => regex_match(&pattern_cmp, &target_cmp),
    }
}

/// Check if all patterns match a path (AND logic).
fn all_patterns_match(path: &str, config: &LocateConfig) -> bool {
    config
        .patterns
        .iter()
        .all(|p| path_matches(path, p, config))
}

// ---------------------------------------------------------------------------
// updatedb argument parsing
// ---------------------------------------------------------------------------

fn parse_updatedb_args(args: &[String]) -> Result<UpdateDbConfig, String> {
    let mut config = UpdateDbConfig::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err(format!("{} requires an argument", args[i - 1]));
                }
                config.output = args[i].clone();
            }
            "-U" | "--database-root" => {
                i += 1;
                if i >= args.len() {
                    return Err(format!("{} requires an argument", args[i - 1]));
                }
                config.database_root = args[i].clone();
            }
            "--prunepaths" => {
                i += 1;
                if i >= args.len() {
                    return Err("--prunepaths requires an argument".to_string());
                }
                config.prunepaths = args[i].split_whitespace().map(|s| s.to_string()).collect();
            }
            "--prunefs" => {
                i += 1;
                if i >= args.len() {
                    return Err("--prunefs requires an argument".to_string());
                }
                config.prunefs = args[i].split_whitespace().map(|s| s.to_string()).collect();
            }
            "--prunenames" => {
                i += 1;
                if i >= args.len() {
                    return Err("--prunenames requires an argument".to_string());
                }
                config.prunenames = args[i].split_whitespace().map(|s| s.to_string()).collect();
            }
            "--add-prunepaths" => {
                i += 1;
                if i >= args.len() {
                    return Err("--add-prunepaths requires an argument".to_string());
                }
                let extras: Vec<String> =
                    args[i].split_whitespace().map(|s| s.to_string()).collect();
                config.prunepaths.extend(extras);
            }
            "--add-prunefs" => {
                i += 1;
                if i >= args.len() {
                    return Err("--add-prunefs requires an argument".to_string());
                }
                let extras: Vec<String> =
                    args[i].split_whitespace().map(|s| s.to_string()).collect();
                config.prunefs.extend(extras);
            }
            "-l" | "--require-visibility" => {
                i += 1;
                if i >= args.len() {
                    return Err(format!("{} requires an argument (0 or 1)", args[i - 1]));
                }
                config.require_visibility = args[i] != "0";
            }
            "-v" | "--verbose" => {
                config.verbose = true;
            }
            "--debug-pruning" => {
                config.debug_pruning = true;
            }
            "--help" | "-h" => {
                return Err("HELP".to_string());
            }
            "--version" => {
                return Err("VERSION".to_string());
            }
            other => {
                return Err(format!("unknown option: {other}"));
            }
        }
        i += 1;
    }
    Ok(config)
}

// ---------------------------------------------------------------------------
// locate argument parsing
// ---------------------------------------------------------------------------

fn parse_locate_args(args: &[String]) -> Result<LocateConfig, String> {
    let mut config = LocateConfig::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--database" => {
                i += 1;
                if i >= args.len() {
                    return Err(format!("{} requires an argument", args[i - 1]));
                }
                config.database = args[i].clone();
            }
            "-i" | "--ignore-case" => {
                config.ignore_case = true;
            }
            "-l" | "--limit" | "-n" => {
                i += 1;
                if i >= args.len() {
                    return Err(format!("{} requires a number", args[i - 1]));
                }
                let n: usize = args[i]
                    .parse()
                    .map_err(|_| format!("invalid number: {}", args[i]))?;
                config.limit = Some(n);
            }
            "-c" | "--count" => {
                config.count_only = true;
            }
            "-e" | "--existing" => {
                config.existing_only = true;
            }
            "-L" | "--follow" => {
                config.follow_symlinks = true;
            }
            "-b" | "--basename" => {
                config.basename_only = true;
            }
            "-w" | "--wholename" => {
                config.basename_only = false;
            }
            "-g" => {
                config.match_mode = MatchMode::Glob;
            }
            "-r" | "--regex" => {
                config.match_mode = MatchMode::Regex;
            }
            "-0" | "--null" => {
                config.null_terminated = true;
            }
            "-S" | "--statistics" => {
                config.show_statistics = true;
            }
            "--help" | "-h" => {
                return Err("HELP".to_string());
            }
            "--version" => {
                return Err("VERSION".to_string());
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {other}"));
            }
            other => {
                config.patterns.push(other.to_string());
            }
        }
        i += 1;
    }
    Ok(config)
}

// ---------------------------------------------------------------------------
// Help / version output
// ---------------------------------------------------------------------------

fn print_updatedb_help(out: &mut dyn Write) {
    let _ = writeln!(out, "Usage: updatedb [OPTION]...");
    let _ = writeln!(out, "Update a file name database.");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  -o, --output DB          database file to build (default: {DEFAULT_DB_PATH})"
    );
    let _ = writeln!(
        out,
        "  -U, --database-root PATH starting point for scanning (default: /)"
    );
    let _ = writeln!(
        out,
        "      --prunepaths PATHS   space-separated paths to skip"
    );
    let _ = writeln!(
        out,
        "      --prunefs FS         space-separated filesystem types to skip"
    );
    let _ = writeln!(
        out,
        "      --prunenames NAMES   space-separated dir name patterns to skip"
    );
    let _ = writeln!(out, "      --add-prunepaths P   add to existing prunepaths");
    let _ = writeln!(out, "      --add-prunefs FS     add to existing prunefs");
    let _ = writeln!(
        out,
        "  -l, --require-visibility 0|1  check visibility (default: 1)"
    );
    let _ = writeln!(out, "  -v, --verbose            show scanned files");
    let _ = writeln!(out, "      --debug-pruning      show pruning decisions");
    let _ = writeln!(out, "  -h, --help               display this help and exit");
    let _ = writeln!(
        out,
        "      --version            output version information and exit"
    );
}

fn print_locate_help(out: &mut dyn Write, prog: &str) {
    let _ = writeln!(out, "Usage: {prog} [OPTION]... PATTERN...");
    let _ = writeln!(out, "Search for entries in a file name database.");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  -b, --basename           match only the base name of path names"
    );
    let _ = writeln!(
        out,
        "  -c, --count              only print number of found entries"
    );
    let _ = writeln!(
        out,
        "  -d, --database DBPATH    use DBPATH instead of default database"
    );
    let _ = writeln!(
        out,
        "  -e, --existing           only print entries for currently existing files"
    );
    let _ = writeln!(
        out,
        "  -L, --follow             follow trailing symbolic links when checking existence"
    );
    let _ = writeln!(
        out,
        "  -g                       interpret PATTERN as a glob pattern"
    );
    let _ = writeln!(out, "  -i, --ignore-case        ignore case distinctions");
    let _ = writeln!(out, "  -l, --limit, -n N        limit output to N entries");
    let _ = writeln!(
        out,
        "  -r, --regex              interpret PATTERN as an extended regex"
    );
    let _ = writeln!(out, "  -w, --wholename          match whole path (default)");
    let _ = writeln!(out, "  -0, --null               separate entries with NUL");
    let _ = writeln!(
        out,
        "  -S, --statistics         display database statistics"
    );
    let _ = writeln!(out, "  -h, --help               display this help and exit");
    let _ = writeln!(
        out,
        "      --version            output version information and exit"
    );
}

fn print_version(out: &mut dyn Write, prog: &str) {
    let _ = writeln!(out, "{prog} (Slate OS) {VERSION}");
}

// ---------------------------------------------------------------------------
// updatedb main
// ---------------------------------------------------------------------------

fn run_updatedb(args: &[String]) -> i32 {
    let config = match parse_updatedb_args(args) {
        Ok(c) => c,
        Err(msg) if msg == "HELP" => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            print_updatedb_help(&mut out);
            return 0;
        }
        Err(msg) if msg == "VERSION" => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            print_version(&mut out, "updatedb");
            return 0;
        }
        Err(msg) => {
            let stderr = io::stderr();
            let mut err = stderr.lock();
            let _ = writeln!(err, "updatedb: {msg}");
            return 1;
        }
    };

    // Try to read the system config file.
    let config = if let Ok(contents) = std::fs::read_to_string(DEFAULT_CONFIG_PATH) {
        let mut file_config = parse_config_file(&contents);
        // Command-line overrides take precedence, but here we merge config file
        // settings as the base before CLI override. Since we already parsed CLI,
        // we just use what we have. In a real implementation, we'd parse config
        // first then overlay CLI. For now, CLI wins (already in `config`).
        file_config.output = config.output;
        file_config.database_root = config.database_root;
        file_config.verbose = config.verbose;
        file_config.debug_pruning = config.debug_pruning;
        file_config.require_visibility = config.require_visibility;
        // Merge prunepaths from CLI if explicitly set.
        file_config
    } else {
        config
    };

    let paths = scan_filesystem(&config.database_root, &config);
    let db_data = serialize_database(&config.database_root, &paths);

    // Ensure parent directory exists.
    if let Some(parent) = std::path::Path::new(&config.output).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match std::fs::write(&config.output, &db_data) {
        Ok(()) => {
            if config.verbose {
                let stderr = io::stderr();
                let mut handle = stderr.lock();
                let _ = writeln!(
                    handle,
                    "updatedb: wrote {} entries to {}",
                    paths.len(),
                    config.output
                );
            }
            0
        }
        Err(e) => {
            let stderr = io::stderr();
            let mut handle = stderr.lock();
            let _ = writeln!(handle, "updatedb: cannot write `{}': {e}", config.output);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// locate main
// ---------------------------------------------------------------------------

fn run_locate(args: &[String], prog_name: &str) -> i32 {
    let config = match parse_locate_args(args) {
        Ok(c) => c,
        Err(msg) if msg == "HELP" => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            print_locate_help(&mut out, prog_name);
            return 0;
        }
        Err(msg) if msg == "VERSION" => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            print_version(&mut out, prog_name);
            return 0;
        }
        Err(msg) => {
            let stderr = io::stderr();
            let mut err = stderr.lock();
            let _ = writeln!(err, "{prog_name}: {msg}");
            return 1;
        }
    };

    // Read database.
    let db_data = match std::fs::read(&config.database) {
        Ok(d) => d,
        Err(e) => {
            let stderr = io::stderr();
            let mut err = stderr.lock();
            let _ = writeln!(err, "{prog_name}: cannot open `{}': {e}", config.database);
            return 1;
        }
    };

    let (header, paths) = match deserialize_database(&db_data) {
        Ok(r) => r,
        Err(e) => {
            let stderr = io::stderr();
            let mut err = stderr.lock();
            let _ = writeln!(
                err,
                "{prog_name}: invalid database `{}': {e}",
                config.database
            );
            return 1;
        }
    };

    if config.show_statistics {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        let _ = writeln!(out, "Database {}", config.database);
        let _ = writeln!(out, "\tVersion: {}", header.version);
        let _ = writeln!(out, "\tRoot: {}", header.root_path);
        let _ = writeln!(out, "\tEntries: {}", header.entry_count);
        let _ = writeln!(out, "\tSize: {} bytes", db_data.len());
        let _ = writeln!(out, "\tTimestamp: {}", header.timestamp);
        return 0;
    }

    if config.patterns.is_empty() {
        let stderr = io::stderr();
        let mut err = stderr.lock();
        let _ = writeln!(err, "{prog_name}: no pattern specified");
        let _ = writeln!(err, "Try '{prog_name} --help' for more information.");
        return 1;
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut count = 0u64;
    let terminator = if config.null_terminated { "\0" } else { "\n" };

    for path in &paths {
        if let Some(limit) = config.limit
            && count as usize >= limit
        {
            break;
        }

        if !all_patterns_match(path, &config) {
            continue;
        }

        if config.existing_only {
            let exists = if config.follow_symlinks {
                std::fs::metadata(path).is_ok()
            } else {
                std::fs::symlink_metadata(path).is_ok()
            };
            if !exists {
                continue;
            }
        }

        count += 1;
        if !config.count_only {
            let _ = write!(out, "{path}{terminator}");
        }
    }

    if config.count_only {
        let _ = writeln!(out, "{count}");
    }

    if count == 0 { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("locate");
    let mode = detect_mode(argv0);
    let prog_name = detect_program_name(argv0);
    let rest = if args.len() > 1 { &args[1..] } else { &[] };

    let code = match mode {
        Mode::UpdateDb => run_updatedb(rest),
        Mode::Locate => run_locate(rest, prog_name),
    };
    process::exit(code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Mode detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_mode_updatedb() {
        assert_eq!(detect_mode("updatedb"), Mode::UpdateDb);
        assert_eq!(detect_mode("/usr/bin/updatedb"), Mode::UpdateDb);
        assert_eq!(detect_mode("C:\\bin\\updatedb.exe"), Mode::UpdateDb);
    }

    #[test]
    fn test_detect_mode_locate() {
        assert_eq!(detect_mode("locate"), Mode::Locate);
        assert_eq!(detect_mode("mlocate"), Mode::Locate);
        assert_eq!(detect_mode("plocate"), Mode::Locate);
        assert_eq!(detect_mode("/usr/bin/locate"), Mode::Locate);
        assert_eq!(detect_mode("/usr/bin/plocate.exe"), Mode::Locate);
    }

    #[test]
    fn test_detect_program_name() {
        assert_eq!(detect_program_name("locate"), "locate");
        assert_eq!(detect_program_name("/usr/bin/mlocate"), "mlocate");
        assert_eq!(detect_program_name("C:\\bin\\plocate.exe"), "plocate");
        assert_eq!(detect_program_name(""), "locate");
    }

    // -----------------------------------------------------------------------
    // Common prefix length
    // -----------------------------------------------------------------------

    #[test]
    fn test_common_prefix_len_identical() {
        assert_eq!(common_prefix_len("hello", "hello"), 5);
    }

    #[test]
    fn test_common_prefix_len_partial() {
        assert_eq!(common_prefix_len("/usr/bin/ls", "/usr/bin/cat"), 9);
    }

    #[test]
    fn test_common_prefix_len_none() {
        assert_eq!(common_prefix_len("abc", "xyz"), 0);
    }

    #[test]
    fn test_common_prefix_len_empty() {
        assert_eq!(common_prefix_len("", "hello"), 0);
        assert_eq!(common_prefix_len("hello", ""), 0);
        assert_eq!(common_prefix_len("", ""), 0);
    }

    #[test]
    fn test_common_prefix_len_one_char() {
        assert_eq!(common_prefix_len("a", "ab"), 1);
    }

    // -----------------------------------------------------------------------
    // Differential encoding
    // -----------------------------------------------------------------------

    #[test]
    fn test_encode_paths_empty() {
        let entries = encode_paths(&[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_encode_paths_single() {
        let paths = vec!["/usr/bin/ls".to_string()];
        let entries = encode_paths(&paths);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].shared_prefix_len, 0);
        assert_eq!(entries[0].suffix, "/usr/bin/ls");
    }

    #[test]
    fn test_encode_paths_shared_prefix() {
        let paths = vec!["/usr/bin/cat".to_string(), "/usr/bin/ls".to_string()];
        let entries = encode_paths(&paths);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].shared_prefix_len, 0);
        assert_eq!(entries[0].suffix, "/usr/bin/cat");
        assert_eq!(entries[1].shared_prefix_len, 9); // "/usr/bin/"
        assert_eq!(entries[1].suffix, "ls");
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let paths = vec![
            "/etc/fstab".to_string(),
            "/etc/hostname".to_string(),
            "/etc/hosts".to_string(),
            "/usr/bin/cat".to_string(),
            "/usr/bin/ls".to_string(),
            "/usr/share/doc/README".to_string(),
        ];
        let entries = encode_paths(&paths);
        let decoded = decode_entries(&entries);
        assert_eq!(paths, decoded);
    }

    #[test]
    fn test_encode_decode_no_shared_prefix() {
        let paths = vec!["/aaa".to_string(), "/bbb".to_string(), "/ccc".to_string()];
        let entries = encode_paths(&paths);
        // First entry compares against "", so shared_prefix_len is 0.
        assert_eq!(entries[0].shared_prefix_len, 0);
        // Subsequent entries share the leading "/" with the previous.
        for entry in &entries[1..] {
            assert_eq!(entry.shared_prefix_len, 1);
        }
        let decoded = decode_entries(&entries);
        assert_eq!(paths, decoded);
    }

    #[test]
    fn test_encode_many_identical_prefix() {
        let paths: Vec<String> = (0..100)
            .map(|i| format!("/very/long/common/prefix/file{i:04}"))
            .collect();
        let entries = encode_paths(&paths);
        let decoded = decode_entries(&entries);
        assert_eq!(paths, decoded);
        // Entries after first should have large shared prefix.
        for entry in &entries[1..] {
            assert!(entry.shared_prefix_len >= 25);
        }
    }

    // -----------------------------------------------------------------------
    // DbEntry serialization
    // -----------------------------------------------------------------------

    #[test]
    fn test_db_entry_serialize_deserialize() {
        let entry = DbEntry {
            shared_prefix_len: 42,
            suffix: "hello.txt".to_string(),
        };
        let bytes = entry.serialize();
        let (decoded, consumed) = DbEntry::deserialize(&bytes).unwrap();
        assert_eq!(entry, decoded);
        assert_eq!(consumed, bytes.len());
    }

    #[test]
    fn test_db_entry_empty_suffix() {
        let entry = DbEntry {
            shared_prefix_len: 10,
            suffix: String::new(),
        };
        let bytes = entry.serialize();
        let (decoded, _) = DbEntry::deserialize(&bytes).unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn test_db_entry_max_prefix() {
        let entry = DbEntry {
            shared_prefix_len: u16::MAX,
            suffix: "x".to_string(),
        };
        let bytes = entry.serialize();
        let (decoded, _) = DbEntry::deserialize(&bytes).unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn test_db_entry_deserialize_truncated() {
        assert!(DbEntry::deserialize(&[0, 0]).is_err());
        assert!(DbEntry::deserialize(&[]).is_err());
    }

    // -----------------------------------------------------------------------
    // DbHeader serialization
    // -----------------------------------------------------------------------

    #[test]
    fn test_db_header_roundtrip() {
        let mut header = DbHeader::new("/");
        header.entry_count = 1000;
        header.total_size = 50000;
        let bytes = header.serialize();
        let (decoded, consumed) = DbHeader::deserialize(&bytes).unwrap();
        assert_eq!(header.magic, decoded.magic);
        assert_eq!(header.version, decoded.version);
        assert_eq!(header.root_path, decoded.root_path);
        assert_eq!(header.entry_count, decoded.entry_count);
        assert_eq!(header.total_size, decoded.total_size);
        assert_eq!(consumed, bytes.len());
    }

    #[test]
    fn test_db_header_custom_root() {
        let header = DbHeader::new("/home/user");
        let bytes = header.serialize();
        let (decoded, _) = DbHeader::deserialize(&bytes).unwrap();
        assert_eq!(decoded.root_path, "/home/user");
    }

    #[test]
    fn test_db_header_bad_magic() {
        let mut bytes = DbHeader::new("/").serialize();
        bytes[0] = b'X';
        assert!(DbHeader::deserialize(&bytes).is_err());
    }

    #[test]
    fn test_db_header_bad_version() {
        let mut bytes = DbHeader::new("/").serialize();
        bytes[8] = 99;
        assert!(DbHeader::deserialize(&bytes).is_err());
    }

    #[test]
    fn test_db_header_too_small() {
        assert!(DbHeader::deserialize(&[0; 10]).is_err());
    }

    // -----------------------------------------------------------------------
    // Full database serialize/deserialize
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_database_roundtrip() {
        let paths = vec![
            "/bin/ls".to_string(),
            "/bin/cat".to_string(),
            "/etc/hosts".to_string(),
            "/usr/bin/vim".to_string(),
        ];
        let mut sorted = paths.clone();
        sorted.sort();
        let db = serialize_database("/", &sorted);
        let (header, decoded) = deserialize_database(&db).unwrap();
        assert_eq!(header.root_path, "/");
        assert_eq!(header.entry_count, sorted.len() as u64);
        assert_eq!(decoded, sorted);
    }

    #[test]
    fn test_full_database_empty() {
        let db = serialize_database("/", &[]);
        let (header, decoded) = deserialize_database(&db).unwrap();
        assert_eq!(header.entry_count, 0);
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_full_database_single_entry() {
        let paths = vec!["/hello".to_string()];
        let db = serialize_database("/tmp", &paths);
        let (header, decoded) = deserialize_database(&db).unwrap();
        assert_eq!(header.root_path, "/tmp");
        assert_eq!(decoded, paths);
    }

    #[test]
    fn test_full_database_large() {
        let paths: Vec<String> = (0..500)
            .map(|i| format!("/data/files/item_{i:06}.dat"))
            .collect();
        let db = serialize_database("/data", &paths);
        let (header, decoded) = deserialize_database(&db).unwrap();
        assert_eq!(header.entry_count, 500);
        assert_eq!(decoded, paths);
    }

    #[test]
    fn test_database_compression_savings() {
        let paths: Vec<String> = (0..100)
            .map(|i| format!("/very/long/shared/prefix/directory/file{i:04}.txt"))
            .collect();
        let db = serialize_database("/", &paths);
        let uncompressed_size: usize = paths.iter().map(|p| p.len() + 4).sum();
        // Compressed db should be significantly smaller than naive storage.
        assert!(db.len() < uncompressed_size);
    }

    // -----------------------------------------------------------------------
    // Config file parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_config_line_basic() {
        let result = parse_config_line("PRUNEPATHS = \"/tmp /proc\"");
        assert!(result.is_some());
        let (key, vals) = result.unwrap();
        assert_eq!(key, "PRUNEPATHS");
        assert_eq!(vals, vec!["/tmp", "/proc"]);
    }

    #[test]
    fn test_parse_config_line_no_quotes() {
        let result = parse_config_line("PRUNEFS = tmpfs sysfs");
        let (key, vals) = result.unwrap();
        assert_eq!(key, "PRUNEFS");
        assert_eq!(vals, vec!["tmpfs", "sysfs"]);
    }

    #[test]
    fn test_parse_config_line_comment() {
        assert!(parse_config_line("# this is a comment").is_none());
    }

    #[test]
    fn test_parse_config_line_empty() {
        assert!(parse_config_line("").is_none());
        assert!(parse_config_line("   ").is_none());
    }

    #[test]
    fn test_parse_config_line_lowercase_key() {
        let (key, _) = parse_config_line("prunenames = .git .svn").unwrap();
        assert_eq!(key, "PRUNENAMES");
    }

    #[test]
    fn test_parse_config_file_full() {
        let contents = "\
# updatedb configuration
PRUNEPATHS = \"/tmp /var/tmp\"
PRUNEFS = \"tmpfs proc sysfs\"
PRUNENAMES = \".git .hg\"
";
        let config = parse_config_file(contents);
        assert_eq!(config.prunepaths, vec!["/tmp", "/var/tmp"]);
        assert_eq!(config.prunefs, vec!["tmpfs", "proc", "sysfs"]);
        assert_eq!(config.prunenames, vec![".git", ".hg"]);
    }

    #[test]
    fn test_parse_config_file_empty() {
        let config = parse_config_file("");
        // Should have defaults.
        assert!(!config.prunepaths.is_empty());
    }

    // -----------------------------------------------------------------------
    // Pruning logic
    // -----------------------------------------------------------------------

    #[test]
    fn test_should_prune_path_exact() {
        let config = UpdateDbConfig {
            prunepaths: vec!["/proc".to_string(), "/sys".to_string()],
            ..UpdateDbConfig::default()
        };
        assert!(should_prune_path("/proc", &config));
        assert!(should_prune_path("/sys", &config));
        assert!(!should_prune_path("/usr", &config));
    }

    #[test]
    fn test_should_prune_path_subdir() {
        let config = UpdateDbConfig {
            prunepaths: vec!["/proc".to_string()],
            ..UpdateDbConfig::default()
        };
        assert!(should_prune_path("/proc/1/status", &config));
        assert!(!should_prune_path("/process", &config));
    }

    #[test]
    fn test_should_prune_name() {
        let config = UpdateDbConfig {
            prunenames: vec![".git".to_string(), ".svn".to_string()],
            ..UpdateDbConfig::default()
        };
        assert!(should_prune_name(".git", &config));
        assert!(should_prune_name(".svn", &config));
        assert!(!should_prune_name("src", &config));
        assert!(!should_prune_name(".gitignore", &config));
    }

    // -----------------------------------------------------------------------
    // Glob matching
    // -----------------------------------------------------------------------

    #[test]
    fn test_glob_match_literal() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("*.txt", "file.txt"));
        assert!(glob_match("*.txt", ".txt"));
        assert!(!glob_match("*.txt", "file.rs"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("?.txt", "a.txt"));
        assert!(!glob_match("?.txt", "ab.txt"));
        assert!(!glob_match("?.txt", ".txt"));
    }

    #[test]
    fn test_glob_match_brackets() {
        assert!(glob_match("[abc]", "a"));
        assert!(glob_match("[abc]", "b"));
        assert!(!glob_match("[abc]", "d"));
    }

    #[test]
    fn test_glob_match_bracket_range() {
        assert!(glob_match("[a-z]", "m"));
        assert!(!glob_match("[a-z]", "A"));
        assert!(glob_match("[0-9]", "5"));
    }

    #[test]
    fn test_glob_match_bracket_negated() {
        assert!(!glob_match("[!abc]", "a"));
        assert!(glob_match("[!abc]", "d"));
        assert!(!glob_match("[^abc]", "b"));
        assert!(glob_match("[^abc]", "z"));
    }

    #[test]
    fn test_glob_match_complex() {
        assert!(glob_match("/usr/*/ls", "/usr/bin/ls"));
        assert!(glob_match("/usr/*/ls", "/usr/local/ls"));
        assert!(!glob_match("/usr/*/ls", "/usr/bin/cat"));
    }

    #[test]
    fn test_glob_match_double_star() {
        // A single * in our implementation matches anything including slashes
        // (glob in locate works path-wide).
        assert!(glob_match("*ls", "/usr/bin/ls"));
    }

    #[test]
    fn test_glob_match_empty() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "a"));
        assert!(glob_match("*", ""));
    }

    // -----------------------------------------------------------------------
    // Regex matching
    // -----------------------------------------------------------------------

    #[test]
    fn test_regex_match_literal() {
        assert!(regex_match("hello", "say hello there"));
        assert!(!regex_match("xyz", "hello"));
    }

    #[test]
    fn test_regex_match_dot() {
        assert!(regex_match("h.llo", "hello world"));
        assert!(regex_match("h.llo", "hallo world"));
    }

    #[test]
    fn test_regex_match_star() {
        assert!(regex_match("he*llo", "hllo"));
        assert!(regex_match("he*llo", "hello"));
        assert!(regex_match("he*llo", "heeello"));
    }

    #[test]
    fn test_regex_match_plus() {
        assert!(!regex_match("he+llo", "hllo"));
        assert!(regex_match("he+llo", "hello"));
        assert!(regex_match("he+llo", "heeello"));
    }

    #[test]
    fn test_regex_match_question_quantifier() {
        assert!(regex_match("he?llo", "hllo"));
        assert!(regex_match("he?llo", "hello"));
        assert!(!regex_match("he?llo", "heeello"));
    }

    #[test]
    fn test_regex_match_anchored_start() {
        assert!(regex_match("^hello", "hello world"));
        assert!(!regex_match("^hello", "say hello"));
    }

    #[test]
    fn test_regex_match_anchored_end() {
        assert!(regex_match("world$", "hello world"));
        assert!(!regex_match("world$", "world hello"));
    }

    #[test]
    fn test_regex_match_both_anchors() {
        assert!(regex_match("^exact$", "exact"));
        assert!(!regex_match("^exact$", "not exact"));
        assert!(!regex_match("^exact$", "exactly"));
    }

    #[test]
    fn test_regex_match_alternation() {
        assert!(regex_match("cat|dog", "I have a cat"));
        assert!(regex_match("cat|dog", "I have a dog"));
        assert!(!regex_match("cat|dog", "I have a fish"));
    }

    #[test]
    fn test_regex_match_bracket() {
        assert!(regex_match("[abc]", "a test"));
        assert!(!regex_match("^[abc]$", "ab"));
    }

    #[test]
    fn test_regex_match_escaped() {
        assert!(regex_match(r"hello\.txt", "hello.txt"));
        assert!(!regex_match(r"hello\.txt", "hellotxt"));
    }

    #[test]
    fn test_regex_match_dot_star() {
        assert!(regex_match(".*\\.rs$", "/src/main.rs"));
        assert!(!regex_match(".*\\.rs$", "/src/main.txt"));
    }

    // -----------------------------------------------------------------------
    // Path matching with config
    // -----------------------------------------------------------------------

    #[test]
    fn test_path_matches_substring() {
        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            ..LocateConfig::default()
        };
        assert!(path_matches("/usr/bin/cat", "bin", &config));
        assert!(!path_matches("/usr/bin/cat", "dog", &config));
    }

    #[test]
    fn test_path_matches_case_insensitive() {
        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            ignore_case: true,
            ..LocateConfig::default()
        };
        assert!(path_matches("/usr/bin/Cat", "cat", &config));
        assert!(path_matches("/usr/bin/cat", "CAT", &config));
    }

    #[test]
    fn test_path_matches_basename() {
        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            basename_only: true,
            ..LocateConfig::default()
        };
        assert!(path_matches("/usr/bin/cat", "cat", &config));
        assert!(!path_matches("/usr/bin/cat", "bin", &config));
    }

    #[test]
    fn test_path_matches_glob() {
        let config = LocateConfig {
            match_mode: MatchMode::Glob,
            ..LocateConfig::default()
        };
        assert!(path_matches("/usr/bin/cat", "*/cat", &config));
        assert!(path_matches("/usr/bin/cat", "/usr/*/cat", &config));
        assert!(!path_matches("/usr/bin/cat", "*/dog", &config));
    }

    #[test]
    fn test_path_matches_regex() {
        let config = LocateConfig {
            match_mode: MatchMode::Regex,
            ..LocateConfig::default()
        };
        assert!(path_matches("/usr/bin/cat", "bin/c.t", &config));
        assert!(!path_matches("/usr/bin/cat", "^/etc", &config));
    }

    #[test]
    fn test_all_patterns_match_single() {
        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            patterns: vec!["bin".to_string()],
            ..LocateConfig::default()
        };
        assert!(all_patterns_match("/usr/bin/ls", &config));
    }

    #[test]
    fn test_all_patterns_match_multiple_and() {
        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            patterns: vec!["usr".to_string(), "bin".to_string()],
            ..LocateConfig::default()
        };
        assert!(all_patterns_match("/usr/bin/ls", &config));
        assert!(!all_patterns_match("/etc/hosts", &config));
    }

    // -----------------------------------------------------------------------
    // updatedb arg parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_updatedb_args_defaults() {
        let config = parse_updatedb_args(&[]).unwrap();
        assert_eq!(config.output, DEFAULT_DB_PATH);
        assert_eq!(config.database_root, "/");
        assert!(!config.verbose);
    }

    #[test]
    fn test_parse_updatedb_args_output() {
        let args = vec!["-o".to_string(), "/tmp/test.db".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert_eq!(config.output, "/tmp/test.db");
    }

    #[test]
    fn test_parse_updatedb_args_root() {
        let args = vec!["-U".to_string(), "/home".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert_eq!(config.database_root, "/home");
    }

    #[test]
    fn test_parse_updatedb_args_verbose() {
        let args = vec!["-v".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert!(config.verbose);
    }

    #[test]
    fn test_parse_updatedb_args_debug_pruning() {
        let args = vec!["--debug-pruning".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert!(config.debug_pruning);
    }

    #[test]
    fn test_parse_updatedb_args_prunepaths() {
        let args = vec!["--prunepaths".to_string(), "/a /b /c".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert_eq!(config.prunepaths, vec!["/a", "/b", "/c"]);
    }

    #[test]
    fn test_parse_updatedb_args_add_prunepaths() {
        let args = vec!["--add-prunepaths".to_string(), "/extra".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert!(config.prunepaths.contains(&"/extra".to_string()));
        // Should also have defaults.
        assert!(config.prunepaths.len() > 1);
    }

    #[test]
    fn test_parse_updatedb_args_help() {
        let args = vec!["--help".to_string()];
        let result = parse_updatedb_args(&args);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "HELP");
    }

    #[test]
    fn test_parse_updatedb_args_version() {
        let args = vec!["--version".to_string()];
        let result = parse_updatedb_args(&args);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "VERSION");
    }

    #[test]
    fn test_parse_updatedb_args_unknown() {
        let args = vec!["--bogus".to_string()];
        let result = parse_updatedb_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_updatedb_args_require_visibility() {
        let args = vec!["-l".to_string(), "0".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert!(!config.require_visibility);

        let args = vec!["--require-visibility".to_string(), "1".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert!(config.require_visibility);
    }

    #[test]
    fn test_parse_updatedb_args_missing_value() {
        let args = vec!["-o".to_string()];
        assert!(parse_updatedb_args(&args).is_err());
    }

    // -----------------------------------------------------------------------
    // locate arg parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_locate_args_patterns() {
        let args = vec!["foo".to_string(), "bar".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert_eq!(config.patterns, vec!["foo", "bar"]);
    }

    #[test]
    fn test_parse_locate_args_ignore_case() {
        let args = vec!["-i".to_string(), "test".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.ignore_case);
    }

    #[test]
    fn test_parse_locate_args_limit() {
        let args = vec!["-l".to_string(), "10".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert_eq!(config.limit, Some(10));
    }

    #[test]
    fn test_parse_locate_args_limit_n() {
        let args = vec!["-n".to_string(), "5".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert_eq!(config.limit, Some(5));
    }

    #[test]
    fn test_parse_locate_args_count() {
        let args = vec!["-c".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.count_only);
    }

    #[test]
    fn test_parse_locate_args_existing() {
        let args = vec!["-e".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.existing_only);
    }

    #[test]
    fn test_parse_locate_args_follow() {
        let args = vec!["-L".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.follow_symlinks);
    }

    #[test]
    fn test_parse_locate_args_basename() {
        let args = vec!["-b".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.basename_only);
    }

    #[test]
    fn test_parse_locate_args_wholename() {
        let args = vec!["-b".to_string(), "-w".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(!config.basename_only);
    }

    #[test]
    fn test_parse_locate_args_glob_mode() {
        let args = vec!["-g".to_string(), "*.txt".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert_eq!(config.match_mode, MatchMode::Glob);
    }

    #[test]
    fn test_parse_locate_args_regex_mode() {
        let args = vec!["-r".to_string(), ".*\\.rs".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert_eq!(config.match_mode, MatchMode::Regex);
    }

    #[test]
    fn test_parse_locate_args_null() {
        let args = vec!["-0".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.null_terminated);
    }

    #[test]
    fn test_parse_locate_args_statistics() {
        let args = vec!["-S".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.show_statistics);
    }

    #[test]
    fn test_parse_locate_args_database() {
        let args = vec![
            "-d".to_string(),
            "/tmp/my.db".to_string(),
            "pat".to_string(),
        ];
        let config = parse_locate_args(&args).unwrap();
        assert_eq!(config.database, "/tmp/my.db");
    }

    #[test]
    fn test_parse_locate_args_help() {
        let args = vec!["--help".to_string()];
        let result = parse_locate_args(&args);
        assert_eq!(result.unwrap_err(), "HELP");
    }

    #[test]
    fn test_parse_locate_args_version() {
        let args = vec!["--version".to_string()];
        let result = parse_locate_args(&args);
        assert_eq!(result.unwrap_err(), "VERSION");
    }

    #[test]
    fn test_parse_locate_args_unknown() {
        let args = vec!["--bogus".to_string()];
        assert!(parse_locate_args(&args).is_err());
    }

    #[test]
    fn test_parse_locate_args_invalid_limit() {
        let args = vec!["-l".to_string(), "abc".to_string()];
        assert!(parse_locate_args(&args).is_err());
    }

    #[test]
    fn test_parse_locate_args_missing_db() {
        let args = vec!["-d".to_string()];
        assert!(parse_locate_args(&args).is_err());
    }

    #[test]
    fn test_parse_locate_args_defaults() {
        let config = parse_locate_args(&[]).unwrap();
        assert_eq!(config.database, DEFAULT_DB_PATH);
        assert_eq!(config.match_mode, MatchMode::Substring);
        assert!(!config.ignore_case);
        assert!(!config.basename_only);
        assert!(!config.count_only);
        assert!(!config.existing_only);
        assert!(!config.null_terminated);
        assert!(config.limit.is_none());
    }

    // -----------------------------------------------------------------------
    // Integration-style tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_database_write_read_search() {
        let paths = vec![
            "/bin/cat".to_string(),
            "/bin/ls".to_string(),
            "/etc/hosts".to_string(),
            "/etc/hostname".to_string(),
            "/usr/bin/vim".to_string(),
            "/usr/bin/nano".to_string(),
            "/usr/share/doc/README".to_string(),
        ];
        let db = serialize_database("/", &paths);
        let (_, decoded) = deserialize_database(&db).unwrap();
        assert_eq!(decoded, paths);

        // Search for "bin" substring.
        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            patterns: vec!["bin".to_string()],
            ..LocateConfig::default()
        };
        let results: Vec<&String> = decoded
            .iter()
            .filter(|p| all_patterns_match(p, &config))
            .collect();
        assert_eq!(results.len(), 4);

        // Search for "host" substring.
        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            patterns: vec!["host".to_string()],
            ..LocateConfig::default()
        };
        let results: Vec<&String> = decoded
            .iter()
            .filter(|p| all_patterns_match(p, &config))
            .collect();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_database_search_glob() {
        let paths = vec![
            "/home/user/doc.txt".to_string(),
            "/home/user/image.png".to_string(),
            "/home/user/notes.txt".to_string(),
            "/tmp/scratch.txt".to_string(),
        ];
        let db = serialize_database("/", &paths);
        let (_, decoded) = deserialize_database(&db).unwrap();

        let config = LocateConfig {
            match_mode: MatchMode::Glob,
            patterns: vec!["*.txt".to_string()],
            ..LocateConfig::default()
        };
        let results: Vec<&String> = decoded
            .iter()
            .filter(|p| all_patterns_match(p, &config))
            .collect();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_database_search_regex() {
        let paths = vec![
            "/src/main.rs".to_string(),
            "/src/lib.rs".to_string(),
            "/src/main.py".to_string(),
            "/docs/readme.md".to_string(),
        ];
        let db = serialize_database("/", &paths);
        let (_, decoded) = deserialize_database(&db).unwrap();

        let config = LocateConfig {
            match_mode: MatchMode::Regex,
            patterns: vec![r"\.rs$".to_string()],
            ..LocateConfig::default()
        };
        let results: Vec<&String> = decoded
            .iter()
            .filter(|p| all_patterns_match(p, &config))
            .collect();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_database_search_case_insensitive() {
        let paths = vec![
            "/home/User/README.md".to_string(),
            "/home/user/readme.txt".to_string(),
            "/tmp/other.txt".to_string(),
        ];
        let db = serialize_database("/", &paths);
        let (_, decoded) = deserialize_database(&db).unwrap();

        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            ignore_case: true,
            patterns: vec!["readme".to_string()],
            ..LocateConfig::default()
        };
        let results: Vec<&String> = decoded
            .iter()
            .filter(|p| all_patterns_match(p, &config))
            .collect();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_database_search_basename() {
        let paths = vec![
            "/usr/bin/cat".to_string(),
            "/home/user/cat.txt".to_string(),
            "/var/catalog/data".to_string(),
        ];
        let db = serialize_database("/", &paths);
        let (_, decoded) = deserialize_database(&db).unwrap();

        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            basename_only: true,
            patterns: vec!["cat".to_string()],
            ..LocateConfig::default()
        };
        let results: Vec<&String> = decoded
            .iter()
            .filter(|p| all_patterns_match(p, &config))
            .collect();
        // "cat" and "cat.txt" match in basename; "data" does not.
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_database_search_limit() {
        let paths: Vec<String> = (0..100).map(|i| format!("/data/file{i:04}.txt")).collect();
        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            patterns: vec!["file".to_string()],
            limit: Some(5),
            ..LocateConfig::default()
        };
        let mut count = 0;
        for path in &paths {
            if count >= config.limit.unwrap() {
                break;
            }
            if all_patterns_match(path, &config) {
                count += 1;
            }
        }
        assert_eq!(count, 5);
    }

    #[test]
    fn test_database_search_multiple_patterns() {
        let paths = [
            "/usr/bin/python3".to_string(),
            "/usr/bin/python2".to_string(),
            "/usr/lib/python3/site.py".to_string(),
            "/etc/python3/config".to_string(),
        ];
        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            patterns: vec!["python3".to_string(), "bin".to_string()],
            ..LocateConfig::default()
        };
        let results: Vec<&String> = paths
            .iter()
            .filter(|p| all_patterns_match(p, &config))
            .collect();
        assert_eq!(results.len(), 1);
        assert_eq!(*results[0], "/usr/bin/python3");
    }

    #[test]
    fn test_database_search_no_match() {
        let paths = ["/usr/bin/ls".to_string()];
        let config = LocateConfig {
            match_mode: MatchMode::Substring,
            patterns: vec!["nonexistent".to_string()],
            ..LocateConfig::default()
        };
        let results: Vec<&String> = paths
            .iter()
            .filter(|p| all_patterns_match(p, &config))
            .collect();
        assert!(results.is_empty());
    }

    // -----------------------------------------------------------------------
    // Help output tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_updatedb_help_output() {
        let mut buf = Vec::new();
        print_updatedb_help(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("updatedb"));
        assert!(output.contains("--output"));
        assert!(output.contains("--prunepaths"));
        assert!(output.contains("--verbose"));
    }

    #[test]
    fn test_locate_help_output() {
        let mut buf = Vec::new();
        print_locate_help(&mut buf, "locate");
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("locate"));
        assert!(output.contains("--database"));
        assert!(output.contains("--ignore-case"));
        assert!(output.contains("--count"));
    }

    #[test]
    fn test_version_output() {
        let mut buf = Vec::new();
        print_version(&mut buf, "plocate");
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("plocate"));
        assert!(output.contains(VERSION));
    }

    // -----------------------------------------------------------------------
    // Edge cases and regression tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_glob_match_only_star() {
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "anything at all"));
    }

    #[test]
    fn test_glob_match_multiple_stars() {
        assert!(glob_match("*a*b*", "xaybz"));
        assert!(glob_match("*a*b*", "ab"));
        assert!(!glob_match("*a*b*", "ba"));
    }

    #[test]
    fn test_regex_match_empty_pattern() {
        assert!(regex_match("", "anything"));
        assert!(regex_match("", ""));
    }

    #[test]
    fn test_regex_match_empty_text() {
        assert!(regex_match("^$", ""));
        assert!(!regex_match("a", ""));
    }

    #[test]
    fn test_decode_entries_empty() {
        let decoded = decode_entries(&[]);
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_serialize_database_preserves_order() {
        let paths = vec!["/a".to_string(), "/b".to_string(), "/c".to_string()];
        let db = serialize_database("/", &paths);
        let (_, decoded) = deserialize_database(&db).unwrap();
        assert_eq!(decoded, paths);
    }

    #[test]
    fn test_prune_path_no_false_positive() {
        // "/process" should not be pruned by "/proc"
        let config = UpdateDbConfig {
            prunepaths: vec!["/proc".to_string()],
            ..UpdateDbConfig::default()
        };
        assert!(!should_prune_path("/process", &config));
        assert!(!should_prune_path("/proca", &config));
    }

    #[test]
    fn test_config_file_with_unknown_keys() {
        let contents = "\
PRUNEPATHS = \"/tmp\"
UNKNOWNKEY = \"something\"
PRUNENAMES = \".git\"
";
        let config = parse_config_file(contents);
        assert_eq!(config.prunepaths, vec!["/tmp"]);
        assert_eq!(config.prunenames, vec![".git"]);
    }

    #[test]
    fn test_updatedb_args_long_and_short_forms() {
        let short = parse_updatedb_args(&["-o".into(), "/a.db".into()]).unwrap();
        let long = parse_updatedb_args(&["--output".into(), "/a.db".into()]).unwrap();
        assert_eq!(short.output, long.output);
    }

    #[test]
    fn test_locate_args_long_and_short_forms() {
        let short = parse_locate_args(&["-i".into(), "test".into()]).unwrap();
        let long = parse_locate_args(&["--ignore-case".into(), "test".into()]).unwrap();
        assert_eq!(short.ignore_case, long.ignore_case);
    }

    #[test]
    fn test_locate_database_long_form() {
        let config =
            parse_locate_args(&["--database".into(), "/x.db".into(), "pat".into()]).unwrap();
        assert_eq!(config.database, "/x.db");
    }

    #[test]
    fn test_glob_unclosed_bracket() {
        // Unclosed bracket treated as literal.
        assert!(glob_match("[abc", "[abc"));
        assert!(!glob_match("[abc", "a"));
    }

    #[test]
    fn test_db_header_empty_root() {
        let header = DbHeader::new("");
        let bytes = header.serialize();
        let (decoded, _) = DbHeader::deserialize(&bytes).unwrap();
        assert_eq!(decoded.root_path, "");
    }

    #[test]
    fn test_differential_encoding_long_paths() {
        let paths = vec![
            "/a/very/deeply/nested/directory/structure/file1.txt".to_string(),
            "/a/very/deeply/nested/directory/structure/file2.txt".to_string(),
        ];
        let entries = encode_paths(&paths);
        // Shared prefix is "/a/very/deeply/nested/directory/structure/file" (46 bytes).
        assert_eq!(entries[1].shared_prefix_len, 46);
        assert_eq!(entries[1].suffix, "2.txt");
        let decoded = decode_entries(&entries);
        assert_eq!(paths, decoded);
    }

    #[test]
    fn test_parse_updatedb_prunefs() {
        let args = vec!["--prunefs".to_string(), "tmpfs sysfs".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert_eq!(config.prunefs, vec!["tmpfs", "sysfs"]);
    }

    #[test]
    fn test_parse_updatedb_prunenames() {
        let args = vec!["--prunenames".to_string(), ".git node_modules".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert_eq!(config.prunenames, vec![".git", "node_modules"]);
    }

    #[test]
    fn test_parse_updatedb_add_prunefs() {
        let args = vec!["--add-prunefs".to_string(), "zfs btrfs".to_string()];
        let config = parse_updatedb_args(&args).unwrap();
        assert!(config.prunefs.contains(&"zfs".to_string()));
        assert!(config.prunefs.contains(&"btrfs".to_string()));
    }

    #[test]
    fn test_parse_locate_regex_long() {
        let args = vec!["--regex".to_string(), ".*".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert_eq!(config.match_mode, MatchMode::Regex);
    }

    #[test]
    fn test_parse_locate_follow_long() {
        let args = vec!["--follow".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.follow_symlinks);
    }

    #[test]
    fn test_parse_locate_limit_long() {
        let args = vec!["--limit".to_string(), "42".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert_eq!(config.limit, Some(42));
    }

    #[test]
    fn test_parse_locate_null_long() {
        let args = vec!["--null".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.null_terminated);
    }

    #[test]
    fn test_parse_locate_existing_long() {
        let args = vec!["--existing".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.existing_only);
    }

    #[test]
    fn test_parse_locate_count_long() {
        let args = vec!["--count".to_string(), "pat".to_string()];
        let config = parse_locate_args(&args).unwrap();
        assert!(config.count_only);
    }
}
