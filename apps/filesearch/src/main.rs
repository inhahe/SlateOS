//! File search application — instant file search across the filesystem
//!
//! Features:
//! - Real-time search with instant results as you type
//! - Glob pattern matching (wildcards: *, ?, [a-z])
//! - Regex pattern matching
//! - File content search (grep-like)
//! - Search filters (by extension, size, date, type)
//! - File index for instant filename search
//! - Recent searches history
//! - Bookmarked searches (saved queries)
//! - Result statistics (count, total size)
//! - File type detection and icons
//! - Sort results by name, path, size, modified date
//! - Open file location / open with default app
//! - Multi-panel UI with search bar, filters sidebar, results list, preview

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]

use std::collections::BTreeMap;
use std::fmt;

// ─── Glob Pattern Matching ───────────────────────────────────────────

/// Match a string against a glob pattern
/// Supports: * (any chars), ? (single char), [a-z] (char class)
#[must_use]
pub fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_impl(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_impl(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && (pattern.get(pi) == Some(&b'?') || pattern.get(pi) == text.get(ti))
        {
            pi = pi.saturating_add(1);
            ti = ti.saturating_add(1);
        } else if pi < pattern.len() && pattern.get(pi) == Some(&b'*') {
            star_pi = pi;
            star_ti = ti;
            pi = pi.saturating_add(1);
        } else if pi < pattern.len() && pattern.get(pi) == Some(&b'[') {
            // Character class
            let class_end = pattern
                .get(pi..)
                .and_then(|s| s.iter().position(|&b| b == b']'));
            if let Some(end_offset) = class_end {
                let class_start = pi.saturating_add(1);
                let class_end_pos = pi.saturating_add(end_offset);
                let ch = text.get(ti).copied().unwrap_or(0);
                let class_bytes = pattern.get(class_start..class_end_pos).unwrap_or_default();

                if char_class_matches(class_bytes, ch) {
                    pi = class_end_pos.saturating_add(1);
                    ti = ti.saturating_add(1);
                } else if star_pi != usize::MAX {
                    pi = star_pi.saturating_add(1);
                    star_ti = star_ti.saturating_add(1);
                    ti = star_ti;
                } else {
                    return false;
                }
            } else {
                // Malformed class, treat as literal
                if star_pi == usize::MAX {
                    return false;
                }
                pi = star_pi.saturating_add(1);
                star_ti = star_ti.saturating_add(1);
                ti = star_ti;
            }
        } else if star_pi != usize::MAX {
            pi = star_pi.saturating_add(1);
            star_ti = star_ti.saturating_add(1);
            ti = star_ti;
        } else {
            return false;
        }
    }

    // Skip trailing stars
    while pi < pattern.len() && pattern.get(pi) == Some(&b'*') {
        pi = pi.saturating_add(1);
    }

    pi == pattern.len()
}

/// Check if a character matches a character class like [a-z] or [abc]
fn char_class_matches(class: &[u8], ch: u8) -> bool {
    let negated = class.first() == Some(&b'!') || class.first() == Some(&b'^');
    let class = if negated {
        class.get(1..).unwrap_or_default()
    } else {
        class
    };

    let mut matches = false;
    let mut i = 0;
    while i < class.len() {
        if i.saturating_add(2) < class.len() && class.get(i.saturating_add(1)) == Some(&b'-') {
            let lo = class.get(i).copied().unwrap_or(0);
            let hi = class.get(i.saturating_add(2)).copied().unwrap_or(0);
            if ch >= lo && ch <= hi {
                matches = true;
            }
            i = i.saturating_add(3);
        } else {
            if class.get(i) == Some(&ch) {
                matches = true;
            }
            i = i.saturating_add(1);
        }
    }

    if negated { !matches } else { matches }
}

// ─── Simple Regex Engine ─────────────────────────────────────────────

/// A very simple regex matcher supporting:
/// . (any char), * (zero or more), + (one or more), ? (zero or one),
/// ^ (start), $ (end), \d (digit), \w (word char), \s (whitespace),
/// character classes [abc], [a-z]
#[must_use]
pub fn regex_match(pattern: &str, text: &str) -> bool {
    let pat_bytes = pattern.as_bytes();
    let text_bytes = text.as_bytes();

    // Check if anchored at start
    let (pat, anchored_start) = if pat_bytes.first() == Some(&b'^') {
        (pat_bytes.get(1..).unwrap_or_default(), true)
    } else {
        (pat_bytes, false)
    };

    // Check if anchored at end
    let (pat, anchored_end) = if pat.last() == Some(&b'$') {
        (
            pat.get(..pat.len().saturating_sub(1)).unwrap_or_default(),
            true,
        )
    } else {
        (pat, false)
    };

    if anchored_start {
        regex_match_at(pat, text_bytes, 0, anchored_end)
    } else {
        // Try matching at every position
        for start in 0..=text_bytes.len() {
            if regex_match_at(pat, text_bytes, start, anchored_end) {
                return true;
            }
        }
        false
    }
}

fn regex_match_at(pattern: &[u8], text: &[u8], start: usize, anchored_end: bool) -> bool {
    let mut pi = 0;
    let mut ti = start;

    while pi < pattern.len() {
        // Parse current atom
        let (matcher, atom_len) = parse_regex_atom(pattern, pi);

        // Check for quantifier
        let next = pattern.get(pi.saturating_add(atom_len)).copied();
        match next {
            Some(b'*') => {
                // Greedy: match as many as possible, then backtrack
                let mut count = 0;
                while ti.saturating_add(count) < text.len()
                    && matcher.matches(text.get(ti.saturating_add(count)).copied().unwrap_or(0))
                {
                    count = count.saturating_add(1);
                }
                // Try from max down to 0
                loop {
                    if regex_match_at(
                        pattern
                            .get(pi.saturating_add(atom_len).saturating_add(1)..)
                            .unwrap_or_default(),
                        text,
                        ti.saturating_add(count),
                        anchored_end,
                    ) {
                        return true;
                    }
                    if count == 0 {
                        break;
                    }
                    count = count.saturating_sub(1);
                }
                return false;
            }
            Some(b'+') => {
                // One or more
                if ti >= text.len() || !matcher.matches(text.get(ti).copied().unwrap_or(0)) {
                    return false;
                }
                ti = ti.saturating_add(1);
                let mut count = 0;
                while ti.saturating_add(count) < text.len()
                    && matcher.matches(text.get(ti.saturating_add(count)).copied().unwrap_or(0))
                {
                    count = count.saturating_add(1);
                }
                loop {
                    if regex_match_at(
                        pattern
                            .get(pi.saturating_add(atom_len).saturating_add(1)..)
                            .unwrap_or_default(),
                        text,
                        ti.saturating_add(count),
                        anchored_end,
                    ) {
                        return true;
                    }
                    if count == 0 {
                        break;
                    }
                    count = count.saturating_sub(1);
                }
                return false;
            }
            Some(b'?') => {
                // Zero or one
                let rest = pattern
                    .get(pi.saturating_add(atom_len).saturating_add(1)..)
                    .unwrap_or_default();
                // Try with match
                if ti < text.len()
                    && matcher.matches(text.get(ti).copied().unwrap_or(0))
                    && regex_match_at(rest, text, ti.saturating_add(1), anchored_end)
                {
                    return true;
                }
                // Try without match
                return regex_match_at(rest, text, ti, anchored_end);
            }
            _ => {
                // Exactly one match required
                if ti >= text.len() || !matcher.matches(text.get(ti).copied().unwrap_or(0)) {
                    return false;
                }
                ti = ti.saturating_add(1);
                pi = pi.saturating_add(atom_len);
            }
        }
    }

    if anchored_end { ti == text.len() } else { true }
}

/// A single matchable regex atom. Unlike a bare `fn(u8) -> bool`, this enum can
/// carry the specific literal byte to match and a borrowed character-class body,
/// so literal characters match by value (e.g. `world` matches only "world", not
/// "any five lowercase letters").
#[derive(Clone, Copy)]
enum Matcher<'a> {
    Any,
    Literal(u8),
    Digit,
    NotDigit,
    Word,
    NotWord,
    Space,
    NotSpace,
    /// Character class `[...]`. `body` is the bytes between the brackets; if the
    /// class began with `^` the sense is negated and `^` is excluded from `body`.
    Class {
        body: &'a [u8],
        negated: bool,
    },
    /// Matches nothing (end of pattern / malformed).
    Never,
}

impl Matcher<'_> {
    fn matches(self, c: u8) -> bool {
        match self {
            Matcher::Any => true,
            Matcher::Literal(b) => c == b,
            Matcher::Digit => c.is_ascii_digit(),
            Matcher::NotDigit => !c.is_ascii_digit(),
            Matcher::Word => c.is_ascii_alphanumeric() || c == b'_',
            Matcher::NotWord => !c.is_ascii_alphanumeric() && c != b'_',
            Matcher::Space => c.is_ascii_whitespace(),
            Matcher::NotSpace => !c.is_ascii_whitespace(),
            Matcher::Class { body, negated } => class_matches(body, c) != negated,
            Matcher::Never => false,
        }
    }
}

/// Test whether byte `c` is a member of a character-class body (the text between
/// `[` and `]`, with any leading `^` already stripped). Supports literal bytes
/// and `a-z` style ranges.
fn class_matches(body: &[u8], c: u8) -> bool {
    let mut i = 0;
    while let Some(&lo) = body.get(i) {
        // Range "x-y": a '-' with a byte on either side (and not the final char).
        if body.get(i.saturating_add(1)) == Some(&b'-')
            && let Some(&hi) = body.get(i.saturating_add(2))
        {
            if lo <= c && c <= hi {
                return true;
            }
            i = i.saturating_add(3);
            continue;
        }
        if lo == c {
            return true;
        }
        i = i.saturating_add(1);
    }
    false
}

/// Returns (matcher, bytes consumed from pattern).
fn parse_regex_atom(pattern: &[u8], pos: usize) -> (Matcher<'_>, usize) {
    match pattern.get(pos) {
        Some(b'.') => (Matcher::Any, 1),
        Some(b'\\') => match pattern.get(pos.saturating_add(1)) {
            Some(b'd') => (Matcher::Digit, 2),
            Some(b'w') => (Matcher::Word, 2),
            Some(b's') => (Matcher::Space, 2),
            Some(b'D') => (Matcher::NotDigit, 2),
            Some(b'W') => (Matcher::NotWord, 2),
            Some(b'S') => (Matcher::NotSpace, 2),
            // Escaped literal: \. \$ \\ etc. match the literal following byte.
            Some(&ch) => (Matcher::Literal(ch), 2),
            // Trailing backslash: match a literal backslash.
            None => (Matcher::Literal(b'\\'), 1),
        },
        Some(b'[') => {
            // Find the closing ']' relative to the '['.
            let rest = pattern.get(pos.saturating_add(1)..).unwrap_or_default();
            if let Some(close) = rest.iter().position(|&b| b == b']') {
                let inner = rest.get(..close).unwrap_or_default();
                let (negated, body) = match inner.first() {
                    Some(b'^') => (true, inner.get(1..).unwrap_or_default()),
                    _ => (false, inner),
                };
                // Total consumed = '[' + body/negation + ']'.
                let len = close.saturating_add(2);
                (Matcher::Class { body, negated }, len)
            } else {
                // No closing bracket: treat '[' as a literal.
                (Matcher::Literal(b'['), 1)
            }
        }
        Some(&ch) => (Matcher::Literal(ch), 1),
        None => (Matcher::Never, 0),
    }
}

// ─── File Types ──────────────────────────────────────────────────────

/// File type categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileCategory {
    Document,
    Image,
    Audio,
    Video,
    Archive,
    Code,
    Executable,
    Font,
    Database,
    Config,
    Other,
}

impl fmt::Display for FileCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document => write!(f, "Document"),
            Self::Image => write!(f, "Image"),
            Self::Audio => write!(f, "Audio"),
            Self::Video => write!(f, "Video"),
            Self::Archive => write!(f, "Archive"),
            Self::Code => write!(f, "Code"),
            Self::Executable => write!(f, "Executable"),
            Self::Font => write!(f, "Font"),
            Self::Database => write!(f, "Database"),
            Self::Config => write!(f, "Config"),
            Self::Other => write!(f, "Other"),
        }
    }
}

/// Detect file category from extension
#[must_use]
pub fn categorize_extension(ext: &str) -> FileCategory {
    match ext.to_lowercase().as_str() {
        "txt" | "doc" | "docx" | "pdf" | "odt" | "rtf" | "md" | "tex" | "csv" | "xls" | "xlsx"
        | "pptx" => FileCategory::Document,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" | "ico" | "webp" | "tiff" | "psd"
        | "raw" => FileCategory::Image,
        "mp3" | "wav" | "flac" | "ogg" | "aac" | "wma" | "opus" | "m4a" | "mid" | "midi" => {
            FileCategory::Audio
        }
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "vob" | "mpg" | "mpeg" => {
            FileCategory::Video
        }
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" | "lz4" | "cab" => {
            FileCategory::Archive
        }
        "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "java" | "go" | "rb" | "php" | "cs"
        | "swift" | "kt" | "lua" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "html" | "css"
        | "scss" | "json" | "xml" | "yaml" | "yml" | "toml" | "sql" => FileCategory::Code,
        "exe" | "msi" | "app" | "bin" | "elf" | "so" | "dll" | "dylib" | "wasm" => {
            FileCategory::Executable
        }
        "ttf" | "otf" | "woff" | "woff2" | "eot" => FileCategory::Font,
        "db" | "sqlite" | "sqlite3" | "mdb" | "accdb" => FileCategory::Database,
        "conf" | "cfg" | "ini" | "env" | "properties" => FileCategory::Config,
        _ => FileCategory::Other,
    }
}

/// Get an icon character for a file category
#[must_use]
pub fn category_icon(cat: FileCategory) -> &'static str {
    match cat {
        FileCategory::Document => "📄",
        FileCategory::Image => "🖼",
        FileCategory::Audio => "🎵",
        FileCategory::Video => "🎬",
        FileCategory::Archive => "📦",
        FileCategory::Code => "💻",
        FileCategory::Executable => "⚙",
        FileCategory::Font => "🔤",
        FileCategory::Database => "🗃",
        FileCategory::Config => "🔧",
        FileCategory::Other => "📁",
    }
}

// ─── Search Index ────────────────────────────────────────────────────

/// An indexed file entry
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub path: String,
    pub name: String,
    pub name_lower: String,
    pub extension: String,
    pub size: u64,
    pub modified: u64, // Unix timestamp
    pub created: u64,
    pub is_directory: bool,
    pub is_hidden: bool,
    pub category: FileCategory,
}

impl IndexEntry {
    #[must_use]
    pub fn new(
        path: &str,
        name: &str,
        size: u64,
        modified: u64,
        created: u64,
        is_dir: bool,
    ) -> Self {
        let ext = name
            .rsplit('.')
            .next()
            .filter(|e| e.len() < 10 && !e.contains('/'))
            .unwrap_or("")
            .to_string();
        let category = if is_dir {
            FileCategory::Other
        } else {
            categorize_extension(&ext)
        };
        let is_hidden = name.starts_with('.');

        Self {
            path: path.to_string(),
            name: name.to_string(),
            name_lower: name.to_lowercase(),
            extension: ext.to_lowercase(),
            size,
            modified,
            created,
            is_directory: is_dir,
            is_hidden,
            category,
        }
    }

    /// Get parent directory path
    #[must_use]
    pub fn parent_dir(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(parent, _)| parent)
    }
}

/// File index for fast searching
pub struct FileIndex {
    entries: Vec<IndexEntry>,
    total_size: u64,
    #[allow(dead_code)]
    last_updated: u64,
}

impl Default for FileIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl FileIndex {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            total_size: 0,
            last_updated: 0,
        }
    }

    /// Add an entry to the index
    pub fn add(&mut self, entry: IndexEntry) {
        self.total_size = self.total_size.saturating_add(entry.size);
        self.entries.push(entry);
    }

    /// Clear the index
    pub fn clear(&mut self) {
        self.entries.clear();
        self.total_size = 0;
    }

    /// Total number of indexed entries
    #[must_use]
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Total indexed size
    #[must_use]
    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Search by filename substring (case-insensitive)
    #[must_use]
    pub fn search_name(&self, query: &str) -> Vec<&IndexEntry> {
        let q = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| e.name_lower.contains(&q))
            .collect()
    }

    /// Search by glob pattern
    #[must_use]
    pub fn search_glob(&self, pattern: &str) -> Vec<&IndexEntry> {
        let lower_pat = pattern.to_lowercase();
        self.entries
            .iter()
            .filter(|e| glob_match(&lower_pat, &e.name_lower))
            .collect()
    }

    /// Search by regex pattern
    #[must_use]
    pub fn search_regex(&self, pattern: &str) -> Vec<&IndexEntry> {
        self.entries
            .iter()
            .filter(|e| regex_match(pattern, &e.name))
            .collect()
    }

    /// Search with full filter criteria
    #[must_use]
    pub fn search(&self, criteria: &SearchCriteria) -> Vec<&IndexEntry> {
        self.entries
            .iter()
            .filter(|e| criteria.matches(e))
            .collect()
    }

    /// Get entries by extension
    #[must_use]
    pub fn by_extension(&self, ext: &str) -> Vec<&IndexEntry> {
        let lower = ext.to_lowercase();
        self.entries
            .iter()
            .filter(|e| e.extension == lower)
            .collect()
    }

    /// Get entries by category
    #[must_use]
    pub fn by_category(&self, cat: FileCategory) -> Vec<&IndexEntry> {
        self.entries.iter().filter(|e| e.category == cat).collect()
    }

    /// Get all unique extensions with counts
    #[must_use]
    pub fn extension_stats(&self) -> BTreeMap<String, usize> {
        let mut stats = BTreeMap::new();
        for entry in &self.entries {
            if !entry.extension.is_empty() {
                let slot = stats.entry(entry.extension.clone()).or_insert(0usize);
                *slot = slot.saturating_add(1);
            }
        }
        stats
    }

    /// Get category counts
    #[must_use]
    pub fn category_stats(&self) -> BTreeMap<FileCategory, (usize, u64)> {
        let mut stats: BTreeMap<FileCategory, (usize, u64)> = BTreeMap::new();
        for entry in &self.entries {
            let stat = stats.entry(entry.category).or_insert((0, 0));
            stat.0 = stat.0.saturating_add(1);
            stat.1 = stat.1.saturating_add(entry.size);
        }
        stats
    }

    /// Get the N largest files
    #[must_use]
    pub fn largest_files(&self, n: usize) -> Vec<&IndexEntry> {
        let mut sorted: Vec<&IndexEntry> = self.entries.iter().collect();
        sorted.sort_by_key(|e| core::cmp::Reverse(e.size));
        sorted.truncate(n);
        sorted
    }

    /// Get recently modified files
    #[must_use]
    pub fn recently_modified(&self, n: usize) -> Vec<&IndexEntry> {
        let mut sorted: Vec<&IndexEntry> = self.entries.iter().collect();
        sorted.sort_by_key(|e| core::cmp::Reverse(e.modified));
        sorted.truncate(n);
        sorted
    }

    /// Get duplicate filenames (same name, different paths)
    #[must_use]
    pub fn find_duplicates(&self) -> BTreeMap<String, Vec<&IndexEntry>> {
        let mut by_name: BTreeMap<String, Vec<&IndexEntry>> = BTreeMap::new();
        for entry in &self.entries {
            by_name
                .entry(entry.name_lower.clone())
                .or_default()
                .push(entry);
        }
        by_name
            .into_iter()
            .filter(|(_, entries)| entries.len() > 1)
            .collect()
    }
}

// ─── Search Criteria ─────────────────────────────────────────────────

/// Search mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchMode {
    Substring,
    Glob,
    Regex,
    Content,
}

impl fmt::Display for SearchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Substring => write!(f, "Name"),
            Self::Glob => write!(f, "Glob"),
            Self::Regex => write!(f, "Regex"),
            Self::Content => write!(f, "Content"),
        }
    }
}

/// Size filter
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeFilter {
    Any,
    Empty,
    Tiny,             // < 10 KB
    Small,            // 10 KB - 1 MB
    Medium,           // 1 MB - 100 MB
    Large,            // 100 MB - 1 GB
    VeryLarge,        // > 1 GB
    Custom(u64, u64), // min, max bytes
}

impl SizeFilter {
    #[must_use]
    pub fn matches(self, size: u64) -> bool {
        match self {
            Self::Any => true,
            Self::Empty => size == 0,
            Self::Tiny => size < 10_240,
            Self::Small => (10_240..1_048_576).contains(&size),
            Self::Medium => (1_048_576..104_857_600).contains(&size),
            Self::Large => (104_857_600..1_073_741_824).contains(&size),
            Self::VeryLarge => size >= 1_073_741_824,
            Self::Custom(min, max) => size >= min && size <= max,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any Size",
            Self::Empty => "Empty (0 B)",
            Self::Tiny => "Tiny (< 10 KB)",
            Self::Small => "Small (10 KB - 1 MB)",
            Self::Medium => "Medium (1 - 100 MB)",
            Self::Large => "Large (100 MB - 1 GB)",
            Self::VeryLarge => "Huge (> 1 GB)",
            Self::Custom(_, _) => "Custom",
        }
    }
}

/// Date filter
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DateFilter {
    Any,
    Today,
    Yesterday,
    ThisWeek,
    ThisMonth,
    ThisYear,
    Older,
}

impl DateFilter {
    /// Check if a timestamp matches (relative to `now`)
    #[must_use]
    pub fn matches(self, timestamp: u64, now: u64) -> bool {
        let age = now.saturating_sub(timestamp);
        match self {
            Self::Any => true,
            Self::Today => age < 86400,
            Self::Yesterday => (86400..172_800).contains(&age),
            Self::ThisWeek => age < 604_800,
            Self::ThisMonth => age < 2_592_000,
            Self::ThisYear => age < 31_536_000,
            Self::Older => age >= 31_536_000,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any Date",
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::ThisWeek => "This Week",
            Self::ThisMonth => "This Month",
            Self::ThisYear => "This Year",
            Self::Older => "Older",
        }
    }
}

/// Full search criteria
#[derive(Debug, Clone)]
pub struct SearchCriteria {
    pub query: String,
    pub mode: SearchMode,
    pub case_sensitive: bool,
    pub include_hidden: bool,
    pub include_directories: bool,
    pub category_filter: Option<FileCategory>,
    pub extension_filter: Option<String>,
    pub size_filter: SizeFilter,
    pub date_filter: DateFilter,
    pub path_contains: Option<String>,
    pub current_time: u64,
}

impl SearchCriteria {
    #[must_use]
    pub fn new(query: &str) -> Self {
        Self {
            query: query.to_string(),
            mode: SearchMode::Substring,
            case_sensitive: false,
            include_hidden: false,
            include_directories: true,
            category_filter: None,
            extension_filter: None,
            size_filter: SizeFilter::Any,
            date_filter: DateFilter::Any,
            path_contains: None,
            current_time: 1_779_000_000,
        }
    }

    /// Check if an entry matches all criteria
    #[must_use]
    pub fn matches(&self, entry: &IndexEntry) -> bool {
        // Hidden file filter
        if !self.include_hidden && entry.is_hidden {
            return false;
        }

        // Directory filter
        if !self.include_directories && entry.is_directory {
            return false;
        }

        // Category filter
        if let Some(cat) = self.category_filter
            && entry.category != cat
        {
            return false;
        }

        // Extension filter
        if let Some(ref ext) = self.extension_filter
            && entry.extension != ext.to_lowercase()
        {
            return false;
        }

        // Size filter
        if !self.size_filter.matches(entry.size) {
            return false;
        }

        // Date filter
        if !self.date_filter.matches(entry.modified, self.current_time) {
            return false;
        }

        // Path filter
        if let Some(ref path_filter) = self.path_contains
            && !entry
                .path
                .to_lowercase()
                .contains(&path_filter.to_lowercase())
        {
            return false;
        }

        // Query match
        if self.query.is_empty() {
            return true;
        }

        match self.mode {
            SearchMode::Substring => {
                if self.case_sensitive {
                    entry.name.contains(&self.query)
                } else {
                    entry.name_lower.contains(&self.query.to_lowercase())
                }
            }
            SearchMode::Glob => {
                if self.case_sensitive {
                    glob_match(&self.query, &entry.name)
                } else {
                    glob_match(&self.query.to_lowercase(), &entry.name_lower)
                }
            }
            SearchMode::Regex => regex_match(&self.query, &entry.name),
            SearchMode::Content => {
                // Content search would need actual file reading
                // For now, match against name as fallback
                entry.name_lower.contains(&self.query.to_lowercase())
            }
        }
    }
}

// ─── Search History ──────────────────────────────────────────────────

/// A saved/recent search
#[derive(Debug, Clone)]
pub struct SavedSearch {
    pub id: u32,
    pub query: String,
    pub mode: SearchMode,
    pub result_count: usize,
    pub timestamp: u64,
    pub is_bookmarked: bool,
    pub name: Option<String>, // Custom name for bookmarked searches
}

// ─── Sort Options ────────────────────────────────────────────────────

/// Result sort column
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortColumn {
    Name,
    Path,
    Size,
    Modified,
    Extension,
    Category,
}

impl fmt::Display for SortColumn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name => write!(f, "Name"),
            Self::Path => write!(f, "Path"),
            Self::Size => write!(f, "Size"),
            Self::Modified => write!(f, "Modified"),
            Self::Extension => write!(f, "Extension"),
            Self::Category => write!(f, "Category"),
        }
    }
}

// ─── Application ─────────────────────────────────────────────────────

use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

mod colors {
    use guitk::Color;
    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    #[allow(dead_code)]
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const _RED: Color = Color::from_hex(0xF38BA8);
    #[allow(dead_code)]
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);
    pub const _LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
}

/// Main file search application
pub struct FileSearchApp {
    pub index: FileIndex,
    pub criteria: SearchCriteria,
    pub results: Vec<usize>, // Indices into index.entries
    pub selected_result: Option<usize>,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    pub search_history: Vec<SavedSearch>,
    pub next_search_id: u32,
    pub show_filters: bool,
    pub show_preview: bool,
    pub status_message: String,
    pub is_searching: bool,
    pub search_time_ms: u64,
}

impl Default for FileSearchApp {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSearchApp {
    #[must_use]
    pub fn new() -> Self {
        Self {
            index: FileIndex::new(),
            criteria: SearchCriteria::new(""),
            results: Vec::new(),
            selected_result: None,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            search_history: Vec::new(),
            next_search_id: 1,
            show_filters: true,
            show_preview: true,
            status_message: "Ready — type to search".to_string(),
            is_searching: false,
            search_time_ms: 0,
        }
    }

    /// Execute a search with current criteria
    pub fn execute_search(&mut self) {
        self.is_searching = true;
        let start = std::time::Instant::now();

        let matching: Vec<usize> = self
            .index
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| self.criteria.matches(e))
            .map(|(i, _)| i)
            .collect();

        self.search_time_ms = start.elapsed().as_millis() as u64;
        let count = matching.len();
        self.results = matching;
        self.sort_results();
        self.is_searching = false;

        // Calculate total size of results
        let total_size: u64 = self
            .results
            .iter()
            .filter_map(|&i| self.index.entries.get(i))
            .map(|e| e.size)
            .sum();

        self.status_message = format!(
            "{count} results ({}) in {}ms",
            format_size(total_size),
            self.search_time_ms
        );

        // Add to history
        self.add_to_history(count);

        self.selected_result = None;
    }

    /// Sort results according to current sort settings
    fn sort_results(&mut self) {
        let entries = &self.index.entries;
        let col = self.sort_column;
        let asc = self.sort_ascending;

        self.results.sort_by(|&a, &b| {
            let ea = entries.get(a);
            let eb = entries.get(b);
            let cmp = match (ea, eb) {
                (Some(ea), Some(eb)) => match col {
                    SortColumn::Name => ea.name_lower.cmp(&eb.name_lower),
                    SortColumn::Path => ea.path.cmp(&eb.path),
                    SortColumn::Size => ea.size.cmp(&eb.size),
                    SortColumn::Modified => ea.modified.cmp(&eb.modified),
                    SortColumn::Extension => ea.extension.cmp(&eb.extension),
                    SortColumn::Category => (ea.category as u8).cmp(&(eb.category as u8)),
                },
                _ => std::cmp::Ordering::Equal,
            };
            if asc { cmp } else { cmp.reverse() }
        });
    }

    /// Add current search to history
    fn add_to_history(&mut self, result_count: usize) {
        if self.criteria.query.is_empty() {
            return;
        }

        let id = self.next_search_id;
        self.next_search_id = self.next_search_id.saturating_add(1);

        self.search_history.push(SavedSearch {
            id,
            query: self.criteria.query.clone(),
            mode: self.criteria.mode,
            result_count,
            timestamp: self.criteria.current_time,
            is_bookmarked: false,
            name: None,
        });

        // Keep last 50 non-bookmarked
        let bookmarked_count = self
            .search_history
            .iter()
            .filter(|s| s.is_bookmarked)
            .count();
        while self.search_history.len().saturating_sub(bookmarked_count) > 50 {
            if let Some(pos) = self.search_history.iter().position(|s| !s.is_bookmarked) {
                self.search_history.remove(pos);
            } else {
                break;
            }
        }
    }

    /// Bookmark a search
    pub fn bookmark_search(&mut self, id: u32, name: &str) {
        if let Some(search) = self.search_history.iter_mut().find(|s| s.id == id) {
            search.is_bookmarked = true;
            search.name = Some(name.to_string());
        }
    }

    /// Remove a bookmark
    pub fn unbookmark_search(&mut self, id: u32) {
        if let Some(search) = self.search_history.iter_mut().find(|s| s.id == id) {
            search.is_bookmarked = false;
            search.name = None;
        }
    }

    /// Get selected entry
    #[must_use]
    pub fn selected_entry(&self) -> Option<&IndexEntry> {
        self.selected_result
            .and_then(|i| self.results.get(i))
            .and_then(|&idx| self.index.entries.get(idx))
    }

    /// Render the UI
    #[must_use]
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let header_h = 60.0;
        let sidebar_w = if self.show_filters { 200.0 } else { 0.0 };
        let preview_w = if self.show_preview { 280.0 } else { 0.0 };
        let status_h = 24.0;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: colors::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header with search bar
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: header_h,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // App title
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: 8.0,
            text: "File Search".to_string(),
            font_size: 14.0,
            color: colors::BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Search input
        let search_x = 16.0;
        let search_w = width - 32.0;
        cmds.push(RenderCommand::FillRect {
            x: search_x,
            y: 28.0,
            width: search_w,
            height: 28.0,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        let search_text = if self.criteria.query.is_empty() {
            "Search files...".to_string()
        } else {
            self.criteria.query.clone()
        };
        cmds.push(RenderCommand::Text {
            x: search_x + 12.0,
            y: 36.0,
            text: search_text,
            font_size: 13.0,
            color: if self.criteria.query.is_empty() {
                colors::OVERLAY0
            } else {
                colors::TEXT
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(search_w - 120.0),
        });

        // Search mode indicator
        cmds.push(RenderCommand::FillRect {
            x: search_x + search_w - 80.0,
            y: 30.0,
            width: 68.0,
            height: 24.0,
            color: colors::SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: search_x + search_w - 72.0,
            y: 36.0,
            text: self.criteria.mode.to_string(),
            font_size: 11.0,
            color: colors::MAUVE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let content_y = header_h;
        let content_h = height - header_h - status_h;

        // Filters sidebar
        if self.show_filters {
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: content_y,
                width: sidebar_w,
                height: content_h,
                color: colors::MANTLE,
                corner_radii: CornerRadii::ZERO,
            });

            self.render_filters(&mut cmds, 0.0, content_y, sidebar_w, content_h);
        }

        // Results area
        let results_x = sidebar_w;
        let results_w = width - sidebar_w - preview_w;
        self.render_results(&mut cmds, results_x, content_y, results_w, content_h);

        // Preview pane
        if self.show_preview {
            let preview_x = results_x + results_w;
            self.render_preview(&mut cmds, preview_x, content_y, preview_w, content_h);
        }

        // Status bar
        let sy = height - status_h;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: sy,
            width,
            height: status_h,
            color: colors::CRUST,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: sy + 6.0,
            text: format!("{} indexed  |  {}", self.index.count(), self.status_message),
            font_size: 11.0,
            color: colors::SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });

        cmds
    }

    fn render_filters(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, _h: f32) {
        let mut fy = y + 8.0;

        // Categories section
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: fy,
            text: "File Type".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        fy += 20.0;

        let categories = [
            FileCategory::Document,
            FileCategory::Image,
            FileCategory::Audio,
            FileCategory::Video,
            FileCategory::Archive,
            FileCategory::Code,
            FileCategory::Executable,
            FileCategory::Config,
            FileCategory::Other,
        ];

        for cat in &categories {
            let is_sel = self.criteria.category_filter == Some(*cat);
            if is_sel {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: fy,
                    width: w - 8.0,
                    height: 22.0,
                    color: colors::SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: fy + 4.0,
                text: format!("{} {cat}", category_icon(*cat)),
                font_size: 11.0,
                color: if is_sel {
                    colors::BLUE
                } else {
                    colors::SUBTEXT1
                },
                font_weight: if is_sel {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(w - 24.0),
            });
            fy += 24.0;
        }

        // Size filter section
        fy += 12.0;
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: fy,
            text: "Size".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        fy += 20.0;

        let sizes = [
            SizeFilter::Any,
            SizeFilter::Empty,
            SizeFilter::Tiny,
            SizeFilter::Small,
            SizeFilter::Medium,
            SizeFilter::Large,
            SizeFilter::VeryLarge,
        ];
        for sf in &sizes {
            let is_sel = self.criteria.size_filter == *sf;
            if is_sel {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: fy,
                    width: w - 8.0,
                    height: 22.0,
                    color: colors::SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: fy + 4.0,
                text: sf.label().to_string(),
                font_size: 11.0,
                color: if is_sel {
                    colors::BLUE
                } else {
                    colors::SUBTEXT1
                },
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 24.0),
            });
            fy += 24.0;
        }

        // Date filter section
        fy += 12.0;
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: fy,
            text: "Modified".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        fy += 20.0;

        let dates = [
            DateFilter::Any,
            DateFilter::Today,
            DateFilter::ThisWeek,
            DateFilter::ThisMonth,
            DateFilter::ThisYear,
        ];
        for df in &dates {
            let is_sel = self.criteria.date_filter == *df;
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: fy + 4.0,
                text: df.label().to_string(),
                font_size: 11.0,
                color: if is_sel {
                    colors::BLUE
                } else {
                    colors::SUBTEXT1
                },
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 24.0),
            });
            fy += 22.0;
        }
    }

    fn render_results(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        // Column headers
        let cols = [
            ("Name", 260.0),
            ("Path", 200.0),
            ("Size", 80.0),
            ("Modified", 120.0),
            ("Type", 80.0),
        ];
        let mut cx = x + 8.0;
        for (label, cw) in &cols {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: y + 4.0,
                text: label.to_string(),
                font_size: 11.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(*cw),
            });
            cx += cw + 8.0;
        }

        let row_h = 28.0;
        let mut ry = y + 24.0;

        if self.results.is_empty() {
            let msg = if self.criteria.query.is_empty() {
                "Type to search"
            } else {
                "No results found"
            };
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 50.0,
                y: y + h / 2.0,
                text: msg.to_string(),
                font_size: 14.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        }

        for (display_idx, &result_idx) in self.results.iter().enumerate() {
            if ry + row_h > y + h {
                break;
            }

            let entry = match self.index.entries.get(result_idx) {
                Some(e) => e,
                None => continue,
            };

            let is_sel = self.selected_result == Some(display_idx);
            if is_sel {
                cmds.push(RenderCommand::FillRect {
                    x: x + 2.0,
                    y: ry,
                    width: w - 4.0,
                    height: row_h - 2.0,
                    color: colors::SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let mut cx = x + 8.0;

            // Name with icon
            let icon = if entry.is_directory {
                "📁"
            } else {
                category_icon(entry.category)
            };
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: format!("{icon} {}", entry.name),
                font_size: 12.0,
                color: if entry.is_directory {
                    colors::BLUE
                } else {
                    colors::TEXT
                },
                font_weight: FontWeightHint::Regular,
                max_width: Some(260.0),
            });
            cx += 268.0;

            // Path
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: entry.parent_dir().to_string(),
                font_size: 11.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
            cx += 208.0;

            // Size
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: if entry.is_directory {
                    "—".to_string()
                } else {
                    format_size(entry.size)
                },
                font_size: 11.0,
                color: colors::SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cx += 88.0;

            // Modified date (just show relative)
            let age = self.criteria.current_time.saturating_sub(entry.modified);
            let date_str = format_relative_time(age);
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: date_str,
                font_size: 11.0,
                color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
            });
            cx += 128.0;

            // Type
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 6.0,
                text: if entry.extension.is_empty() {
                    "—".to_string()
                } else {
                    entry.extension.to_uppercase()
                },
                font_size: 11.0,
                color: colors::PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            ry += row_h;
        }
    }

    fn render_preview(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        // Separator
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 1.0,
            height: h,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let entry = if let Some(e) = self.selected_entry() {
            e
        } else {
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 50.0,
                y: y + h / 2.0,
                text: "Select a file".to_string(),
                font_size: 13.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        };

        let px = x + 12.0;
        let max_w = w - 24.0;
        let mut py = y + 12.0;

        // Icon and name
        let icon = if entry.is_directory {
            "📁"
        } else {
            category_icon(entry.category)
        };
        cmds.push(RenderCommand::Text {
            x: px,
            y: py,
            text: format!("{icon} {}", entry.name),
            font_size: 14.0,
            color: colors::TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        py += 24.0;

        // Details
        let fields: Vec<(&str, String)> = vec![
            ("Path:", entry.path.clone()),
            ("Size:", format_size(entry.size)),
            (
                "Type:",
                format!("{} (.{})", entry.category, entry.extension),
            ),
            (
                "Modified:",
                format_relative_time(self.criteria.current_time.saturating_sub(entry.modified)),
            ),
            (
                "Created:",
                format_relative_time(self.criteria.current_time.saturating_sub(entry.created)),
            ),
            (
                "Hidden:",
                if entry.is_hidden { "Yes" } else { "No" }.to_string(),
            ),
            (
                "Directory:",
                if entry.is_directory { "Yes" } else { "No" }.to_string(),
            ),
        ];

        for (label, value) in &fields {
            cmds.push(RenderCommand::Text {
                x: px,
                y: py,
                text: label.to_string(),
                font_size: 11.0,
                color: colors::OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: px + 80.0,
                y: py,
                text: value.clone(),
                font_size: 11.0,
                color: colors::SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w - 80.0),
            });
            py += 18.0;
        }

        // Quick actions
        py += 16.0;
        cmds.push(RenderCommand::Text {
            x: px,
            y: py,
            text: "Actions".to_string(),
            font_size: 11.0,
            color: colors::OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        py += 20.0;

        let actions = ["Open", "Open Location", "Copy Path", "Properties"];
        for action in &actions {
            cmds.push(RenderCommand::FillRect {
                x: px,
                y: py,
                width: max_w,
                height: 24.0,
                color: colors::SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: px + 10.0,
                y: py + 5.0,
                text: action.to_string(),
                font_size: 11.0,
                color: colors::TEAL,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            py += 28.0;
        }
    }
}

// ─── Formatting Helpers ──────────────────────────────────────────────

#[must_use]
pub fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        format!("{:.2} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[must_use]
pub fn format_relative_time(seconds: u64) -> String {
    if seconds < 60 {
        "Just now".to_string()
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else if seconds < 604_800 {
        format!("{}d ago", seconds / 86400)
    } else if seconds < 2_592_000 {
        format!("{}w ago", seconds / 604_800)
    } else if seconds < 31_536_000 {
        format!("{}mo ago", seconds / 2_592_000)
    } else {
        format!("{}y ago", seconds / 31_536_000)
    }
}

// ─── Main ────────────────────────────────────────────────────────────

fn main() {
    let mut app = FileSearchApp::new();

    // Populate index with sample files
    populate_sample_index(&mut app.index);

    // Execute a sample search
    app.criteria.query = "config".to_string();
    app.execute_search();

    let cmds = app.render(1280.0, 800.0);
    let _ = cmds;
}

fn populate_sample_index(index: &mut FileIndex) {
    let now: u64 = 1_779_000_000;
    let files = [
        (
            "/home/user/Documents/report.pdf",
            "report.pdf",
            2_500_000,
            now - 3600,
        ),
        (
            "/home/user/Documents/budget.xlsx",
            "budget.xlsx",
            150_000,
            now - 86400,
        ),
        (
            "/home/user/Documents/notes.md",
            "notes.md",
            5_000,
            now - 7200,
        ),
        (
            "/home/user/Pictures/vacation.jpg",
            "vacation.jpg",
            4_200_000,
            now - 604_800,
        ),
        (
            "/home/user/Pictures/screenshot.png",
            "screenshot.png",
            350_000,
            now - 172_800,
        ),
        (
            "/home/user/Music/song.mp3",
            "song.mp3",
            8_500_000,
            now - 86400,
        ),
        (
            "/home/user/Music/album.flac",
            "album.flac",
            45_000_000,
            now - 2_592_000,
        ),
        (
            "/home/user/Videos/recording.mp4",
            "recording.mp4",
            250_000_000,
            now - 604_800,
        ),
        (
            "/home/user/Projects/app/src/main.rs",
            "main.rs",
            12_000,
            now - 1800,
        ),
        (
            "/home/user/Projects/app/src/lib.rs",
            "lib.rs",
            8_000,
            now - 1800,
        ),
        (
            "/home/user/Projects/app/Cargo.toml",
            "Cargo.toml",
            500,
            now - 3600,
        ),
        (
            "/home/user/Projects/config.yaml",
            "config.yaml",
            2_000,
            now - 7200,
        ),
        (
            "/home/user/Projects/app/.gitignore",
            ".gitignore",
            200,
            now - 86400,
        ),
        (
            "/home/user/.config/editor/config.toml",
            "config.toml",
            1_500,
            now - 259_200,
        ),
        (
            "/home/user/.config/shell/config.sh",
            "config.sh",
            3_000,
            now - 604_800,
        ),
        (
            "/home/user/Downloads/installer.exe",
            "installer.exe",
            50_000_000,
            now - 172_800,
        ),
        (
            "/home/user/Downloads/archive.tar.gz",
            "archive.tar.gz",
            25_000_000,
            now - 259_200,
        ),
        (
            "/home/user/Downloads/font.ttf",
            "font.ttf",
            500_000,
            now - 86400,
        ),
        (
            "/home/user/backup.db",
            "backup.db",
            100_000_000,
            now - 43_200,
        ),
        (
            "/home/user/readme.txt",
            "readme.txt",
            4_000,
            now - 31_536_000,
        ),
    ];

    for (path, name, size, modified) in &files {
        index.add(IndexEntry::new(
            path,
            name,
            *size,
            *modified,
            modified.saturating_sub(86400),
            false,
        ));
    }

    // Add some directories
    let dirs = [
        ("/home/user/Documents", "Documents"),
        ("/home/user/Pictures", "Pictures"),
        ("/home/user/Music", "Music"),
        ("/home/user/Videos", "Videos"),
        ("/home/user/Projects", "Projects"),
        ("/home/user/Downloads", "Downloads"),
    ];
    for (path, name) in &dirs {
        index.add(IndexEntry::new(path, name, 0, now, now - 2_592_000, true));
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // Glob matching tests
    #[test]
    fn test_glob_star() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(!glob_match("*.rs", "main.py"));
    }

    #[test]
    fn test_glob_question() {
        assert!(glob_match("?.txt", "a.txt"));
        assert!(!glob_match("?.txt", "ab.txt"));
    }

    #[test]
    fn test_glob_char_class() {
        assert!(glob_match("[abc].txt", "a.txt"));
        assert!(glob_match("[abc].txt", "b.txt"));
        assert!(!glob_match("[abc].txt", "d.txt"));
    }

    #[test]
    fn test_glob_range() {
        assert!(glob_match("[a-z].txt", "m.txt"));
        assert!(!glob_match("[a-z].txt", "5.txt"));
    }

    #[test]
    fn test_glob_negated_class() {
        assert!(!glob_match("[!a-z].txt", "m.txt"));
        assert!(glob_match("[!a-z].txt", "5.txt"));
    }

    #[test]
    fn test_glob_complex() {
        assert!(glob_match("src/*.rs", "src/main.rs"));
        assert!(glob_match("*.tar.gz", "archive.tar.gz"));
    }

    #[test]
    fn test_glob_exact() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
    }

    // Regex tests
    #[test]
    fn test_regex_literal() {
        assert!(regex_match("hello", "hello world"));
    }

    #[test]
    fn test_regex_anchored() {
        assert!(regex_match("^hello", "hello world"));
        assert!(!regex_match("^world", "hello world"));
    }

    #[test]
    fn test_regex_end_anchor() {
        assert!(regex_match("world$", "hello world"));
        assert!(!regex_match("hello$", "hello world"));
    }

    #[test]
    fn test_regex_dot() {
        assert!(regex_match("h.llo", "hello"));
    }

    #[test]
    fn test_regex_digit() {
        assert!(regex_match("\\d", "abc123"));
        assert!(!regex_match("^\\d$", "abc"));
    }

    // File category tests
    #[test]
    fn test_categorize_document() {
        assert_eq!(categorize_extension("pdf"), FileCategory::Document);
        assert_eq!(categorize_extension("txt"), FileCategory::Document);
    }

    #[test]
    fn test_categorize_image() {
        assert_eq!(categorize_extension("png"), FileCategory::Image);
        assert_eq!(categorize_extension("jpg"), FileCategory::Image);
    }

    #[test]
    fn test_categorize_code() {
        assert_eq!(categorize_extension("rs"), FileCategory::Code);
        assert_eq!(categorize_extension("py"), FileCategory::Code);
    }

    #[test]
    fn test_categorize_unknown() {
        assert_eq!(categorize_extension("xyz"), FileCategory::Other);
    }

    // Index tests
    #[test]
    fn test_index_add_search() {
        let mut index = FileIndex::new();
        index.add(IndexEntry::new(
            "/test/hello.txt",
            "hello.txt",
            100,
            0,
            0,
            false,
        ));
        index.add(IndexEntry::new(
            "/test/world.rs",
            "world.rs",
            200,
            0,
            0,
            false,
        ));
        assert_eq!(index.count(), 2);
        assert_eq!(index.total_size(), 300);

        let results = index.search_name("hello");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "hello.txt");
    }

    #[test]
    fn test_index_search_glob() {
        let mut index = FileIndex::new();
        index.add(IndexEntry::new("/test/a.rs", "a.rs", 100, 0, 0, false));
        index.add(IndexEntry::new("/test/b.py", "b.py", 100, 0, 0, false));
        index.add(IndexEntry::new("/test/c.rs", "c.rs", 100, 0, 0, false));

        let results = index.search_glob("*.rs");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_index_by_extension() {
        let mut index = FileIndex::new();
        index.add(IndexEntry::new("/a.txt", "a.txt", 100, 0, 0, false));
        index.add(IndexEntry::new("/b.txt", "b.txt", 100, 0, 0, false));
        index.add(IndexEntry::new("/c.md", "c.md", 100, 0, 0, false));

        let results = index.by_extension("txt");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_index_by_category() {
        let mut index = FileIndex::new();
        index.add(IndexEntry::new("/a.jpg", "a.jpg", 100, 0, 0, false));
        index.add(IndexEntry::new("/b.png", "b.png", 100, 0, 0, false));
        index.add(IndexEntry::new("/c.rs", "c.rs", 100, 0, 0, false));

        let results = index.by_category(FileCategory::Image);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_index_largest_files() {
        let mut index = FileIndex::new();
        index.add(IndexEntry::new("/a", "a", 100, 0, 0, false));
        index.add(IndexEntry::new("/b", "b", 500, 0, 0, false));
        index.add(IndexEntry::new("/c", "c", 300, 0, 0, false));

        let largest = index.largest_files(2);
        assert_eq!(largest.len(), 2);
        assert_eq!(largest[0].size, 500);
    }

    #[test]
    fn test_index_extension_stats() {
        let mut index = FileIndex::new();
        index.add(IndexEntry::new("/a.txt", "a.txt", 100, 0, 0, false));
        index.add(IndexEntry::new("/b.txt", "b.txt", 100, 0, 0, false));
        index.add(IndexEntry::new("/c.rs", "c.rs", 100, 0, 0, false));

        let stats = index.extension_stats();
        assert_eq!(stats.get("txt"), Some(&2));
        assert_eq!(stats.get("rs"), Some(&1));
    }

    #[test]
    fn test_index_duplicates() {
        let mut index = FileIndex::new();
        index.add(IndexEntry::new("/a/file.txt", "file.txt", 100, 0, 0, false));
        index.add(IndexEntry::new("/b/file.txt", "file.txt", 200, 0, 0, false));
        index.add(IndexEntry::new(
            "/c/other.txt",
            "other.txt",
            100,
            0,
            0,
            false,
        ));

        let dupes = index.find_duplicates();
        assert_eq!(dupes.len(), 1);
        assert!(dupes.contains_key("file.txt"));
    }

    // Search criteria tests
    #[test]
    fn test_criteria_substring() {
        let criteria = SearchCriteria::new("hello");
        let entry = IndexEntry::new("/test/hello.txt", "hello.txt", 100, 0, 0, false);
        assert!(criteria.matches(&entry));
    }

    #[test]
    fn test_criteria_hidden_filter() {
        let mut criteria = SearchCriteria::new("");
        criteria.include_hidden = false;
        let entry = IndexEntry::new("/test/.hidden", ".hidden", 100, 0, 0, false);
        assert!(!criteria.matches(&entry));
    }

    #[test]
    fn test_criteria_category_filter() {
        let mut criteria = SearchCriteria::new("");
        criteria.category_filter = Some(FileCategory::Image);
        let img = IndexEntry::new("/a.jpg", "a.jpg", 100, 0, 0, false);
        let code = IndexEntry::new("/a.rs", "a.rs", 100, 0, 0, false);
        assert!(criteria.matches(&img));
        assert!(!criteria.matches(&code));
    }

    #[test]
    fn test_criteria_size_filter() {
        let mut criteria = SearchCriteria::new("");
        criteria.size_filter = SizeFilter::Large;
        let large = IndexEntry::new("/big", "big", 500_000_000, 0, 0, false);
        let small = IndexEntry::new("/small", "small", 100, 0, 0, false);
        assert!(criteria.matches(&large));
        assert!(!criteria.matches(&small));
    }

    #[test]
    fn test_size_filter_ranges() {
        assert!(SizeFilter::Empty.matches(0));
        assert!(!SizeFilter::Empty.matches(1));
        assert!(SizeFilter::Tiny.matches(5000));
        assert!(SizeFilter::Small.matches(100_000));
        assert!(SizeFilter::Medium.matches(50_000_000));
        assert!(SizeFilter::Large.matches(500_000_000));
        assert!(SizeFilter::VeryLarge.matches(2_000_000_000));
    }

    #[test]
    fn test_date_filter() {
        let now = 1_779_000_000u64;
        assert!(DateFilter::Today.matches(now - 3600, now));
        assert!(!DateFilter::Today.matches(now - 100_000, now));
        assert!(DateFilter::ThisWeek.matches(now - 86400, now));
    }

    // App tests
    #[test]
    fn test_app_search() {
        let mut app = FileSearchApp::new();
        populate_sample_index(&mut app.index);
        app.criteria.query = "config".to_string();
        app.execute_search();
        assert!(app.results.len() >= 2); // config.yaml, config.toml, config.sh
    }

    #[test]
    fn test_app_glob_search() {
        let mut app = FileSearchApp::new();
        populate_sample_index(&mut app.index);
        app.criteria.query = "*.rs".to_string();
        app.criteria.mode = SearchMode::Glob;
        app.execute_search();
        assert_eq!(app.results.len(), 2); // main.rs, lib.rs
    }

    #[test]
    fn test_app_search_history() {
        let mut app = FileSearchApp::new();
        populate_sample_index(&mut app.index);
        app.criteria.query = "test".to_string();
        app.execute_search();
        assert_eq!(app.search_history.len(), 1);
    }

    #[test]
    fn test_app_bookmark() {
        let mut app = FileSearchApp::new();
        populate_sample_index(&mut app.index);
        app.criteria.query = "test".to_string();
        app.execute_search();
        let id = app.search_history[0].id;
        app.bookmark_search(id, "My Search");
        assert!(app.search_history[0].is_bookmarked);
        assert_eq!(app.search_history[0].name.as_deref(), Some("My Search"));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1_073_741_824), "1.00 GiB");
    }

    #[test]
    fn test_format_relative() {
        assert_eq!(format_relative_time(30), "Just now");
        assert_eq!(format_relative_time(3600), "1h ago");
        assert_eq!(format_relative_time(86400), "1d ago");
    }

    #[test]
    fn test_render_produces_commands() {
        let mut app = FileSearchApp::new();
        populate_sample_index(&mut app.index);
        let cmds = app.render(1280.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_entry_parent_dir() {
        let entry = IndexEntry::new("/home/user/test.txt", "test.txt", 100, 0, 0, false);
        assert_eq!(entry.parent_dir(), "/home/user");
    }
}
