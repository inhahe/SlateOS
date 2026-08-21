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
///
/// Both engines in this module work on `&[char]`, not `&[u8]`. They used to
/// step a byte at a time, which made `?` match one *byte*: `?.txt` did not
/// match `\u{65e5}.txt`, and `[\u{e9}]` matched either half of the two-byte
/// `\u{e9}` and so also matched part of the unrelated `\u{e8}`. Since both
/// inputs here are `&str` — filenames already validated as UTF-8 — character
/// semantics is both what these doc comments promise and what the caller can
/// supply. (Contrast `apps/backup`, whose matcher runs on raw path bytes that
/// need not be UTF-8, so there only the `?` advance could be fixed.)
#[must_use]
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_chars(&pat, &txt)
}

/// `glob_match` for callers that already hold decoded characters. Searching an
/// index calls this once per entry, so the pattern is decoded once by the
/// caller instead of once per candidate.
#[must_use]
pub fn glob_match_chars(pattern: &[char], text: &[char]) -> bool {
    glob_match_impl(pattern, text)
}

fn glob_match_impl(pattern: &[char], text: &[char]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && (pattern.get(pi) == Some(&'?') || pattern.get(pi) == text.get(ti))
        {
            pi = pi.saturating_add(1);
            ti = ti.saturating_add(1);
        } else if pi < pattern.len() && pattern.get(pi) == Some(&'*') {
            star_pi = pi;
            star_ti = ti;
            pi = pi.saturating_add(1);
        } else if pi < pattern.len() && pattern.get(pi) == Some(&'[') {
            // Character class
            let class_end = pattern
                .get(pi..)
                .and_then(|s| s.iter().position(|&b| b == ']'));
            if let Some(end_offset) = class_end {
                let class_start = pi.saturating_add(1);
                let class_end_pos = pi.saturating_add(end_offset);
                let ch = text.get(ti).copied().unwrap_or('\0');
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
    while pi < pattern.len() && pattern.get(pi) == Some(&'*') {
        pi = pi.saturating_add(1);
    }

    pi == pattern.len()
}

/// Check if a character matches a character class like [a-z] or [abc]
fn char_class_matches(class: &[char], ch: char) -> bool {
    let negated = class.first() == Some(&'!') || class.first() == Some(&'^');
    let class = if negated {
        class.get(1..).unwrap_or_default()
    } else {
        class
    };

    let mut matches = false;
    let mut i = 0;
    while i < class.len() {
        if i.saturating_add(2) < class.len() && class.get(i.saturating_add(1)) == Some(&'-') {
            let lo = class.get(i).copied().unwrap_or('\0');
            let hi = class.get(i.saturating_add(2)).copied().unwrap_or('\0');
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
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    regex_match_chars(&pat, &txt)
}

/// `regex_match` for callers that already hold decoded characters. See
/// `glob_match_chars`.
#[must_use]
pub fn regex_match_chars(pattern: &[char], text: &[char]) -> bool {
    let pat_bytes = pattern;
    let text_bytes = text;

    // Check if anchored at start
    let (pat, anchored_start) = if pat_bytes.first() == Some(&'^') {
        (pat_bytes.get(1..).unwrap_or_default(), true)
    } else {
        (pat_bytes, false)
    };

    // Check if anchored at end
    let (pat, anchored_end) = if pat.last() == Some(&'$') {
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

fn regex_match_at(pattern: &[char], text: &[char], start: usize, anchored_end: bool) -> bool {
    let mut pi = 0;
    let mut ti = start;

    while pi < pattern.len() {
        // Parse current atom
        let (matcher, atom_len) = parse_regex_atom(pattern, pi);

        // Check for quantifier
        let next = pattern.get(pi.saturating_add(atom_len)).copied();
        match next {
            Some('*') => {
                // Greedy: match as many as possible, then backtrack
                let mut count = 0;
                while ti.saturating_add(count) < text.len()
                    && matcher.matches(text.get(ti.saturating_add(count)).copied().unwrap_or('\0'))
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
            Some('+') => {
                // One or more
                if ti >= text.len() || !matcher.matches(text.get(ti).copied().unwrap_or('\0')) {
                    return false;
                }
                ti = ti.saturating_add(1);
                let mut count = 0;
                while ti.saturating_add(count) < text.len()
                    && matcher.matches(text.get(ti.saturating_add(count)).copied().unwrap_or('\0'))
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
            Some('?') => {
                // Zero or one
                let rest = pattern
                    .get(pi.saturating_add(atom_len).saturating_add(1)..)
                    .unwrap_or_default();
                // Try with match
                if ti < text.len()
                    && matcher.matches(text.get(ti).copied().unwrap_or('\0'))
                    && regex_match_at(rest, text, ti.saturating_add(1), anchored_end)
                {
                    return true;
                }
                // Try without match
                return regex_match_at(rest, text, ti, anchored_end);
            }
            _ => {
                // Exactly one match required
                if ti >= text.len() || !matcher.matches(text.get(ti).copied().unwrap_or('\0')) {
                    return false;
                }
                ti = ti.saturating_add(1);
                pi = pi.saturating_add(atom_len);
            }
        }
    }

    if anchored_end { ti == text.len() } else { true }
}

/// A single matchable regex atom. Unlike a bare `fn(char) -> bool`, this enum
/// can carry the specific literal character to match and a borrowed
/// character-class body, so literal characters match by value (e.g. `world`
/// matches only "world", not "any five lowercase letters").
#[derive(Clone, Copy)]
enum Matcher<'a> {
    Any,
    Literal(char),
    Digit,
    NotDigit,
    Word,
    NotWord,
    Space,
    NotSpace,
    /// Character class `[...]`. `body` is the characters between the brackets;
    /// if the class began with `^` the sense is negated and `^` is excluded
    /// from `body`.
    Class {
        body: &'a [char],
        negated: bool,
    },
    /// Matches nothing (end of pattern / malformed).
    Never,
}

impl Matcher<'_> {
    fn matches(self, c: char) -> bool {
        match self {
            Matcher::Any => true,
            Matcher::Literal(b) => c == b,
            Matcher::Digit => c.is_ascii_digit(),
            Matcher::NotDigit => !c.is_ascii_digit(),
            Matcher::Word => c.is_ascii_alphanumeric() || c == '_',
            Matcher::NotWord => !c.is_ascii_alphanumeric() && c != '_',
            Matcher::Space => c.is_ascii_whitespace(),
            Matcher::NotSpace => !c.is_ascii_whitespace(),
            Matcher::Class { body, negated } => class_matches(body, c) != negated,
            Matcher::Never => false,
        }
    }
}

/// Test whether character `c` is a member of a character-class body (the text
/// between `[` and `]`, with any leading `^` already stripped). Supports
/// literal characters and `a-z` style ranges. Because the body is `&[char]`,
/// a range compares scalar values, so a non-ASCII range such as
/// `[\u{430}-\u{44f}]` (Cyrillic) means what it looks like instead of
/// comparing the first byte of each endpoint's encoding.
fn class_matches(body: &[char], c: char) -> bool {
    let mut i = 0;
    while let Some(&lo) = body.get(i) {
        // Range "x-y": a '-' with a character on either side (not the final one).
        if body.get(i.saturating_add(1)) == Some(&'-')
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

/// Returns (matcher, characters consumed from pattern).
fn parse_regex_atom(pattern: &[char], pos: usize) -> (Matcher<'_>, usize) {
    match pattern.get(pos) {
        Some('.') => (Matcher::Any, 1),
        Some('\\') => match pattern.get(pos.saturating_add(1)) {
            Some('d') => (Matcher::Digit, 2),
            Some('w') => (Matcher::Word, 2),
            Some('s') => (Matcher::Space, 2),
            Some('D') => (Matcher::NotDigit, 2),
            Some('W') => (Matcher::NotWord, 2),
            Some('S') => (Matcher::NotSpace, 2),
            // Escaped literal: \. \$ \\ etc. match the literal following char.
            Some(&ch) => (Matcher::Literal(ch), 2),
            // Trailing backslash: match a literal backslash.
            None => (Matcher::Literal('\\'), 1),
        },
        Some('[') => {
            // Find the closing ']' relative to the '['.
            let rest = pattern.get(pos.saturating_add(1)..).unwrap_or_default();
            if let Some(close) = rest.iter().position(|&b| b == ']') {
                let inner = rest.get(..close).unwrap_or_default();
                let (negated, body) = match inner.first() {
                    Some('^') => (true, inner.get(1..).unwrap_or_default()),
                    _ => (false, inner),
                };
                // Total consumed = '[' + body/negation + ']'.
                let len = close.saturating_add(2);
                (Matcher::Class { body, negated }, len)
            } else {
                // No closing bracket: treat '[' as a literal.
                (Matcher::Literal('['), 1)
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

/// Longest trailing segment still treated as a file extension.
///
/// Past this it is almost certainly part of the name — a timestamp or a version
/// suffix — rather than a format. The previous limit was nine characters, which
/// was really standing in for "reject the whole name when it has no dot"; now
/// that the dotless case is rejected directly, the limit can be generous enough
/// for the real long ones (`.properties`, `.compressed`, `.appxbundle`).
const MAX_EXTENSION_LEN: usize = 24;

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
        // An extension is what follows the *last* dot, and only if there is a
        // dot with something before it. `rsplit('.').next()` does not say that:
        // on a name with no dot it yields the whole name, so `readme` was
        // indexed with extension `readme`, shown as type "README", and
        // categorised as if `readme` were a format. A leading dot is likewise
        // not an extension — `.bashrc` is a name — so an empty stem is
        // rejected too.
        let ext = name
            .rsplit_once('.')
            .filter(|(stem, _)| !stem.is_empty())
            .map(|(_, ext)| ext)
            .filter(|e| e.len() <= MAX_EXTENSION_LEN && !e.contains('/'))
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
        // Decode the pattern once, not once per entry: this runs over the
        // whole index.
        let pat: Vec<char> = pattern.to_lowercase().chars().collect();
        self.entries
            .iter()
            .filter(|e| {
                let name: Vec<char> = e.name_lower.chars().collect();
                glob_match_chars(&pat, &name)
            })
            .collect()
    }

    /// Search by regex pattern
    #[must_use]
    pub fn search_regex(&self, pattern: &str) -> Vec<&IndexEntry> {
        let pat: Vec<char> = pattern.chars().collect();
        self.entries
            .iter()
            .filter(|e| {
                let name: Vec<char> = e.name.chars().collect();
                regex_match_chars(&pat, &name)
            })
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

use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::table::{Column, Fit, Table};

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

/// Columns of the results table.
///
/// One definition that the header row and the body rows both read, so they
/// cannot drift apart — and so a test can ask whether a cell fits its column.
const RESULT_COLUMNS: &[Column] = &[
    Column {
        label: "Name",
        width: 260.0,
    },
    Column {
        label: "Path",
        width: 200.0,
    },
    Column {
        label: "Size",
        width: 80.0,
    },
    Column {
        label: "Modified",
        width: 120.0,
    },
    Column {
        label: "Type",
        width: 80.0,
    },
];

const COL_NAME: usize = 0;
const COL_PATH: usize = 1;
const COL_SIZE: usize = 2;
const COL_MODIFIED: usize = 3;
const COL_TYPE: usize = 4;

/// Font size of the results table's name cell.
const ROW_FONT: f32 = 12.0;
/// Font size of the results table's header and its remaining cells.
const ROW_FONT_SMALL: f32 = 11.0;

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
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Ellipsis,
            });
            fy += 22.0;
        }
    }

    /// Draw the results table.
    ///
    /// Every cell here holds something the filesystem chose, not something this
    /// app authored — a filename, a directory, an extension — and the two that
    /// overflow in practice are Name and Path, which were clipped mid-glyph
    /// with no marker that anything had been dropped. Size and Type were drawn
    /// with no width at all; they happen to fit today only because
    /// [`IndexEntry::new`] refuses extensions of ten characters or more, which
    /// is an incidental property of the parser and not something a table should
    /// be relying on. All five now go through [`Table::cell`].
    fn render_results(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        let table = Table::new(RESULT_COLUMNS, x);
        table.header(cmds, y + 4.0, colors::OVERLAY0, ROW_FONT_SMALL);

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
                overflow: TextOverflow::Clip,
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

            let cy = ry + 6.0;

            // Name with icon.
            let icon = if entry.is_directory {
                "📁"
            } else {
                category_icon(entry.category)
            };
            table.cell(
                cmds,
                COL_NAME,
                cy,
                &format!("{icon} {}", entry.name),
                if entry.is_directory {
                    colors::BLUE
                } else {
                    colors::TEXT
                },
                ROW_FONT,
                Fit::Start,
            );

            // The directory is cut at the *front*: what distinguishes two
            // results is the deepest directory, not the mount point they share.
            table.cell(
                cmds,
                COL_PATH,
                cy,
                entry.parent_dir(),
                colors::SUBTEXT0,
                ROW_FONT_SMALL,
                Fit::End,
            );

            table.cell(
                cmds,
                COL_SIZE,
                cy,
                &if entry.is_directory {
                    "—".to_string()
                } else {
                    format_size(entry.size)
                },
                colors::SUBTEXT1,
                ROW_FONT_SMALL,
                Fit::Start,
            );

            let age = self.criteria.current_time.saturating_sub(entry.modified);
            table.cell(
                cmds,
                COL_MODIFIED,
                cy,
                &format_relative_time(age),
                colors::SUBTEXT0,
                ROW_FONT_SMALL,
                Fit::Start,
            );

            // An extension is whatever follows the last dot in a name the app
            // did not choose, so its length is not ours to assume.
            table.cell(
                cmds,
                COL_TYPE,
                cy,
                &if entry.extension.is_empty() {
                    "—".to_string()
                } else {
                    entry.extension.to_uppercase()
                },
                colors::PEACH,
                ROW_FONT_SMALL,
                Fit::Start,
            );

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
                overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: px + 80.0,
                y: py,
                text: value.clone(),
                font_size: 11.0,
                color: colors::SUBTEXT1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w - 80.0),
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Clip,
            });
            py += 28.0;
        }
    }
}

// ─── Formatting Helpers ──────────────────────────────────────────────

#[must_use]
pub fn format_size(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
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

    // ─── Byte/character confusion in the two matchers ────────────────
    //
    // Both engines used to step one byte at a time, so every
    // single-character construct (`?`, `.`, a class, `\D`/`\W`/`\S`)
    // consumed a byte. Each test below is written so that it fails against
    // that old behaviour, in both directions: a character that should match
    // and did not, and a run of bytes that should not match and did.

    /// Non-ASCII names of known character length, one per encoded width.
    fn wide_names() -> [(&'static str, usize); 4] {
        [("é", 2), ("日", 3), ("р", 2), ("😀", 4)]
    }

    #[test]
    fn a_glob_question_mark_matches_one_character_not_one_byte() {
        let mut checked = 0;
        for (ch, width) in wide_names() {
            let name = format!("{ch}.txt");
            assert!(
                glob_match("?.txt", &name),
                "`?.txt` should match {name:?} — `?` is one character"
            );
            // The old engine needed one `?` per byte. Make sure we did not
            // simply move the off-by-N: `width` question marks must NOT match.
            let many = "?".repeat(width);
            assert!(
                !glob_match(&format!("{many}.txt"), &name),
                "{width} `?`s should no longer match the {width} bytes of {name:?}"
            );
            checked += 1;
        }
        assert!(checked >= 4, "only {checked} names checked");

        assert!(glob_match("??.txt", "日本.txt"));
        assert!(!glob_match("?.txt", "ab.txt"));
    }

    #[test]
    fn a_glob_class_compares_whole_characters() {
        // Old: the class matched the shared *first byte* of é (C3 A9) and
        // è (C3 A8), so this was a false positive.
        assert!(!glob_match("[é]*", "èb"));
        assert!(glob_match("[é]*", "éb"));
        // Old: the class consumed one byte of a two-byte character, leaving
        // the pattern exhausted with text remaining, so this was a false
        // negative.
        assert!(glob_match("[é]", "é"));
        assert!(!glob_match("[é]", "è"));
    }

    #[test]
    fn a_glob_range_over_non_ascii_compares_scalar_values() {
        // Cyrillic а-я. Byte-wise this range was meaningless.
        assert!(glob_match("[а-я]", "р"));
        assert!(!glob_match("[а-я]", "z"));
    }

    #[test]
    fn a_regex_dot_matches_one_character_not_one_byte() {
        let mut checked = 0;
        for (ch, width) in wide_names() {
            assert!(regex_match("^.$", ch), "`.` should match {ch:?} whole");
            // The decisive direction: `.` per *byte* used to match here.
            let dots = ".".repeat(width);
            assert!(
                !regex_match(&format!("^{dots}$"), ch),
                "{width} dots must not match the {width} bytes of {ch:?}"
            );
            checked += 1;
        }
        assert!(checked >= 4, "only {checked} characters checked");

        assert!(regex_match("a.c", "a日c"));
        assert!(regex_match("^..$", "日本"));
    }

    #[test]
    fn a_regex_negated_class_does_not_match_part_of_a_character() {
        // \W, \D and \S are defined by ASCII predicates, so every byte of a
        // multi-byte character satisfied them. Three of them used to match
        // one kanji exactly.
        assert!(!regex_match("^\\W\\W\\W$", "日"));
        assert!(regex_match("^\\W$", "日"));
        assert!(!regex_match("^\\D\\D$", "é"));
        assert!(regex_match("^\\D$", "é"));
    }

    #[test]
    fn a_non_ascii_name_is_found_through_the_index() {
        // Covers the bulk-search paths, which decode the pattern once and
        // each candidate separately.
        let mut index = FileIndex::new();
        index.add(IndexEntry::new("/t/日本.rs", "日本.rs", 100, 0, 0, false));
        index.add(IndexEntry::new("/t/ab.rs", "ab.rs", 100, 0, 0, false));

        let by_glob = index.search_glob("??.rs");
        assert_eq!(by_glob.len(), 2, "both two-character stems should match");

        let by_regex = index.search_regex("^..\\.rs$");
        assert_eq!(by_regex.len(), 2);

        assert_eq!(index.search_glob("?.rs").len(), 0);
    }

    #[test]
    fn an_ascii_pattern_matches_exactly_as_before() {
        // For ASCII input a character index and a byte index are the same
        // number, so none of the above may have changed anything here.
        let globs = [
            ("*.rs", "main.rs", true),
            ("*.rs", "main.py", false),
            ("?.txt", "a.txt", true),
            ("?.txt", "ab.txt", false),
            ("[abc].txt", "a.txt", true),
            ("[abc].txt", "d.txt", false),
            ("[a-z]*", "hello", true),
            ("[^0-9]*", "hello", true),
            ("src/*/mod.rs", "src/net/mod.rs", true),
        ];
        let mut checked = 0;
        for (pat, text, want) in globs {
            assert_eq!(glob_match(pat, text), want, "glob {pat:?} vs {text:?}");
            checked += 1;
        }

        let regexes = [
            ("hello", "hello world", true),
            ("^hello", "hello world", true),
            ("^world", "hello world", false),
            ("world$", "hello world", true),
            ("h.llo", "hello", true),
            ("\\d", "abc123", true),
            ("^\\d$", "abc", false),
            ("^a+b$", "aaab", true),
            ("^a*b$", "b", true),
            ("^ab?c$", "ac", true),
            ("\\.rs$", "main.rs", true),
        ];
        for (pat, text, want) in regexes {
            assert_eq!(regex_match(pat, text), want, "regex {pat:?} vs {text:?}");
            checked += 1;
        }
        assert!(checked >= 20, "only {checked} cases checked");
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
        assert_eq!(format_size(1_073_741_824), "1.0 GiB");
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

    // ─── Extension parsing ───────────────────────────────────────────

    fn ext_of(name: &str) -> String {
        IndexEntry::new(&format!("/tmp/{name}"), name, 0, 0, 0, false).extension
    }

    #[test]
    fn a_name_with_no_dot_has_no_extension() {
        // `rsplit('.').next()` yields the whole string when there is no dot, so
        // `readme` used to be indexed with extension `readme` and displayed in
        // the Type column as "README".
        assert_eq!(ext_of("readme"), "");
        assert_eq!(ext_of("Makefile"), "");
        assert_eq!(ext_of("LICENSE"), "");
    }

    #[test]
    fn a_leading_dot_is_a_name_not_an_extension() {
        assert_eq!(ext_of(".bashrc"), "");
        assert_eq!(ext_of(".gitignore"), "");
    }

    #[test]
    fn a_dotfile_with_a_real_extension_keeps_it() {
        assert_eq!(ext_of(".eslintrc.json"), "json");
    }

    #[test]
    fn the_last_dot_wins() {
        assert_eq!(ext_of("archive.tar.gz"), "gz");
        assert_eq!(ext_of("v1.2.3.zip"), "zip");
    }

    #[test]
    fn a_long_but_real_extension_is_kept() {
        // The old nine-character limit dropped these on the floor, because it
        // was really compensating for the dotless-name bug above.
        assert_eq!(ext_of("app.properties"), "properties");
        assert_eq!(ext_of("bundle.appxbundle"), "appxbundle");
    }

    #[test]
    fn an_absurd_trailing_segment_is_not_an_extension() {
        let name = format!("backup.{}", "z".repeat(MAX_EXTENSION_LEN + 1));
        assert_eq!(ext_of(&name), "");
    }

    #[test]
    fn an_extension_is_indexed_lowercase() {
        assert_eq!(ext_of("PHOTO.JPEG"), "jpeg");
    }

    // ─── Results table column fitting ────────────────────────────────
    //
    // Every cell in this table holds a string the filesystem chose. These tests
    // hold the table to the rule that no cell draws past its column's right
    // edge, and that a value too long to show is visibly cut rather than
    // silently clipped.

    /// An index holding one entry whose every field is far too long for its
    /// column, and one that comfortably fits.
    fn app_with_a_shouting_result() -> FileSearchApp {
        let mut app = FileSearchApp::new();
        app.index.add(IndexEntry::new(
            "/home/user/archive/2024/quarterly/very/deeply/nested/reports/\
             An Extremely Long Report Filename That Will Not Fit In The Column.pdf",
            "An Extremely Long Report Filename That Will Not Fit In The Column.pdf",
            123_456_789,
            1_000,
            0,
            false,
        ));
        app.index
            .add(IndexEntry::new("/tmp/a.txt", "a.txt", 12, 1_000, 0, false));
        app.criteria = SearchCriteria::new("");
        app.criteria.current_time = 2_000;
        app.results = vec![0, 1];
        app
    }

    #[test]
    fn no_result_cell_escapes_its_column() {
        let app = app_with_a_shouting_result();
        let mut cmds = Vec::new();
        // Render the results panel directly: a whole-app render puts sidebar
        // and search-bar text at x values that fall inside a column's range,
        // and the assertion would then fail on chrome that is not in the table.
        app.render_results(&mut cmds, 0.0, 0.0, 900.0, 400.0);

        let table = Table::new(RESULT_COLUMNS, 0.0);
        let spans = table.spans();
        let mut checked = 0usize;
        for cmd in &cmds {
            let RenderCommand::Text {
                x,
                text,
                font_size,
                font_weight,
                max_width: Some(_),
                overflow: TextOverflow::Ellipsis,
                ..
            } = cmd
            else {
                continue;
            };
            let Some(&(_, right)) = spans.iter().find(|(l, _)| (l - x).abs() < 0.01) else {
                continue;
            };
            let drawn = x + guitk::text::measure(text, *font_size, *font_weight);
            assert!(
                drawn <= right + 0.5,
                "cell {text:?} starting at {x} runs to {drawn}, \
                 past its column's right edge {right}"
            );
            checked += 1;
        }
        // 5 header labels + 2 rows x 5 cells.
        assert!(checked >= 15, "only {checked} cells checked");
    }

    /// The texts drawn in one column of the results table, header excluded.
    fn result_column_cells(cmds: &[RenderCommand], index: usize) -> Vec<String> {
        let left = Table::new(RESULT_COLUMNS, 0.0).left(index);
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x,
                    text,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(_),
                    overflow: TextOverflow::Ellipsis,
                    ..
                } if (x - left).abs() < 0.01 => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_overlong_filename_is_marked_as_cut() {
        let app = app_with_a_shouting_result();
        let mut cmds = Vec::new();
        app.render_results(&mut cmds, 0.0, 0.0, 900.0, 400.0);
        let names = result_column_cells(&cmds, COL_NAME);
        assert!(
            names.iter().any(|n| n.ends_with('…')),
            "a filename too long for its column must be visibly cut: {names:?}"
        );
        assert!(
            names.iter().any(|n| n.ends_with("a.txt")),
            "a filename that fits must be drawn verbatim: {names:?}"
        );
    }

    #[test]
    fn an_overlong_directory_keeps_its_deepest_component() {
        let app = app_with_a_shouting_result();
        let mut cmds = Vec::new();
        app.render_results(&mut cmds, 0.0, 0.0, 900.0, 400.0);
        let paths = result_column_cells(&cmds, COL_PATH);
        let deep = paths
            .iter()
            .find(|p| p.starts_with('…'))
            .expect("the deep path should be cut at the front");
        assert!(
            deep.ends_with("reports"),
            "what distinguishes two results is the deepest directory, \
             which must survive the cut: {deep:?}"
        );
        assert!(
            paths.iter().any(|p| p == "/tmp"),
            "a path that fits must be drawn verbatim: {paths:?}"
        );
    }
}
