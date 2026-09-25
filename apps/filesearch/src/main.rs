//! File search application — instant file search across the filesystem
//!
//! Features:
//! - Real-time search with instant results as you type
//! - Glob pattern matching (wildcards: *, ?, [a-z])
//! - Regex pattern matching
//! - File content search (grep-like): the Content mode reads the files, on a
//!   worker thread, and results arrive while it reads
//! - Search filters (by extension, size, date, type)
//! - File index for instant filename search
//! - Recent searches (the ones something was opened from) and saved searches,
//!   which outlive the window (`filesearch.yaml`)
//! - Result statistics (count, total size)
//! - File type detection and icons
//! - Sort results by name, path, size, modified date -- by key or by pressing a
//!   column heading
//! - Open with the program File Associations names, or open the folder a
//!   result is in with the file manager
//! - Multi-panel UI with search bar, filters sidebar, results list, preview
//!
//! # The pointer
//!
//! Every control is drawn and hit-tested by one walk, [`FileSearchApp::frame`],
//! through [`guitk::frame::Frame`]: the folder button, the query box's save and
//! mode switches, every filter chip and switch, the column headings, the
//! result rows and the preview's actions. The results and the filters panel
//! scroll under the wheel. Until 2026-09-25 none of it took a pointer, and the
//! Open actions, Enter, the saved searches and scrolling past the first page
//! of results were not reachable by any route.

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

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use std::collections::BTreeMap;
use std::fmt;

// ─── Glob Pattern Matching ───────────────────────────────────────────
//
// `globmatch` rather than a copy here. This file used to hold its own matcher,
// and measured against CPython's `fnmatch` over every pattern of length <= 4
// and text of length <= 3 drawn from the metacharacter alphabet -- 730,236
// pairs -- it disagreed on 646. Three families, all silent:
//
//   * it tested literal equality *before* parsing a class, so any `[` in the
//     text short-circuited the class: `[*]`, which is how one searches for a
//     literal asterisk, matched `[]`, `[a]` and `[b]` and not `*`;
//   * a `]` in the first member position ended the class instead of being a
//     literal member, so `[]]` -- the only way to write "a bracket" -- matched
//     nothing;
//   * and consequently `[!]` was a negated *empty* class, which matches
//     everything.
//
// `apps/indexer` held a second copy with a *different* set of bugs, so the two
// search tools in this desktop gave different answers to the same pattern.
// See the `globmatch` module docs.
pub use globmatch::{glob_match, glob_match_chars};
use guitk::dialog::{FileDialog, FilePicker, Picked};

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

impl FileCategory {
    /// Every category, in the order the filter strip draws them.
    ///
    /// The strip used to build its own array of nine, and this enum has
    /// eleven: `Font` and `Database` were categories the matcher understood,
    /// that every file was sorted into, and that no chip ever offered. A
    /// search for fonts could not be narrowed to fonts.
    pub const ALL: [FileCategory; 11] = [
        Self::Document,
        Self::Image,
        Self::Audio,
        Self::Video,
        Self::Archive,
        Self::Code,
        Self::Executable,
        Self::Font,
        Self::Database,
        Self::Config,
        Self::Other,
    ];

    /// The next choice in the strip, treating "no filter" as the first one.
    ///
    /// `None` is part of the cycle rather than a separate key, because the
    /// way out of a filter has to be as reachable as the way in.
    #[must_use]
    pub fn step(current: Option<Self>, forward: bool) -> Option<Self> {
        // Slot 0 is "no filter"; slot n+1 is `ALL[n]`. Saturating rather
        // than modular because this crate denies plain arithmetic, and
        // "past the end goes to the start" reads better than a remainder.
        let at = match current {
            None => 0,
            Some(cat) => Self::ALL
                .iter()
                .position(|c| *c == cat)
                .map_or(0, |i| i.saturating_add(1)),
        };
        let last = Self::ALL.len();
        let next = if forward {
            if at >= last { 0 } else { at.saturating_add(1) }
        } else if at == 0 {
            last
        } else {
            at.saturating_sub(1)
        };
        if next == 0 {
            None
        } else {
            Self::ALL.get(next.saturating_sub(1)).copied()
        }
    }
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
    /// The bands the strip offers, in the order it draws them.
    ///
    /// **Named `CHIPS` and not `ALL` because it is deliberately short.**
    /// `Custom(min, max)` is absent: it carries two numbers that a strip of
    /// chips cannot express, and it is reachable by a dialog that does not
    /// exist yet. A chip that cannot say *which* custom range it means would
    /// be a chip that does nothing.
    ///
    /// `scripts/check-variant-lists.py` reported this one at 7 of 8 the first
    /// time it ran after the strip was wired. It no longer decides what to
    /// check by name -- every list of an enum's variants is checked now, and a
    /// list that is deliberately short is recorded in
    /// `scripts/variant-lists-partial.txt` with its reason, because a gate
    /// whose population is chosen by name fails toward silence for every list
    /// nobody thought to call `ALL`. This doc comment and that record say the
    /// same thing in the two places somebody might look.
    pub const CHIPS: [SizeFilter; 7] = [
        Self::Any,
        Self::Empty,
        Self::Tiny,
        Self::Small,
        Self::Medium,
        Self::Large,
        Self::VeryLarge,
    ];

    /// The next band, wrapping. A `Custom` range steps to the start.
    #[must_use]
    pub fn step(self, forward: bool) -> Self {
        let at = Self::CHIPS.iter().position(|s| *s == self).unwrap_or(0);
        let last = Self::CHIPS.len().saturating_sub(1);
        let next = if forward {
            if at >= last { 0 } else { at.saturating_add(1) }
        } else if at == 0 {
            last
        } else {
            at.saturating_sub(1)
        };
        Self::CHIPS.get(next).copied().unwrap_or(Self::Any)
    }

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
    /// Every date range, in the order the filter strip draws them.
    ///
    /// The strip drew five of these seven: `Yesterday` and `Older` were
    /// understood by `matches` and offered by nothing.
    pub const ALL: [DateFilter; 7] = [
        Self::Any,
        Self::Today,
        Self::Yesterday,
        Self::ThisWeek,
        Self::ThisMonth,
        Self::ThisYear,
        Self::Older,
    ];

    /// The next range in the strip, wrapping.
    #[must_use]
    pub fn step(self, forward: bool) -> Self {
        let at = Self::ALL.iter().position(|d| *d == self).unwrap_or(0);
        let last = Self::ALL.len().saturating_sub(1);
        let next = if forward {
            if at >= last { 0 } else { at.saturating_add(1) }
        } else if at == 0 {
            last
        } else {
            at.saturating_sub(1)
        };
        Self::ALL.get(next).copied().unwrap_or(Self::Any)
    }

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
            current_time: unix_now(),
        }
    }

    /// Check if an entry matches all criteria.
    ///
    /// Content mode reads the file to answer, synchronously. That is correct
    /// and too slow to run over an index on every keystroke, which is why
    /// [`FileSearchApp::execute_search`] hands content searches to a worker
    /// and asks only [`passes_filters`](Self::passes_filters) here.
    #[must_use]
    pub fn matches(&self, entry: &IndexEntry) -> bool {
        if !self.passes_filters(entry) {
            return false;
        }
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
                !entry.is_directory
                    && file_contains(
                        std::path::Path::new(&entry.path),
                        &self.query,
                        self.case_sensitive,
                        MAX_CONTENT_BYTES,
                    ) == Ok(true)
            }
        }
    }

    /// Every criterion but the query: hidden files, folders, kind, extension,
    /// size, date and path.
    #[must_use]
    pub fn passes_filters(&self, entry: &IndexEntry) -> bool {
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
        true
    }
}

// ─── Content Search ──────────────────────────────────────────────────

/// The largest file a content search reads. Bigger ones are skipped and
/// counted, and the count is shown: a search that silently did not look
/// inside a file must not read as one that looked and found nothing.
pub const MAX_CONTENT_BYTES: u64 = 16 * 1024 * 1024;

/// Why a file's contents were not searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentSkip {
    /// Larger than the limit.
    TooLarge,
    /// Could not be read.
    Unreadable,
}

/// Whether the file at `path` contains `query`.
///
/// Case-sensitive, it compares bytes. Otherwise a file that is UTF-8 text is
/// lower-cased as text, so `ÉCOLE` matches `école`; a file that is not is
/// compared with ASCII letters folded and every other byte exact -- it has no
/// characters to fold, and decoding it lossily would compare against
/// replacement characters that are not in the file.
///
/// # Errors
///
/// [`ContentSkip`] when the file was not searched: too large, or unreadable.
pub fn file_contains(
    path: &std::path::Path,
    query: &str,
    case_sensitive: bool,
    limit: u64,
) -> Result<bool, ContentSkip> {
    let size = std::fs::metadata(path)
        .map_err(|_| ContentSkip::Unreadable)?
        .len();
    if size > limit {
        return Err(ContentSkip::TooLarge);
    }
    let bytes = std::fs::read(path).map_err(|_| ContentSkip::Unreadable)?;
    let needle = query.as_bytes();
    if needle.is_empty() {
        return Ok(true);
    }
    if case_sensitive {
        return Ok(bytes.windows(needle.len()).any(|w| w == needle));
    }
    if let Ok(text) = std::str::from_utf8(&bytes) {
        return Ok(text.to_lowercase().contains(&query.to_lowercase()));
    }
    Ok(bytes
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle)))
}

/// What the content worker reports, one message at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentNews {
    /// This index entry's file contains the query.
    Found(usize),
    /// One more file has been read.
    Read,
    /// One more file could not be read, or was too large to.
    Skipped,
    /// Every candidate has been looked at.
    Done,
}

/// A content search running on a worker thread.
///
/// Reading every file under a folder is too slow to do on the window's
/// thread on every keystroke, so the worker reads while the window keeps
/// drawing, and matches arrive as it finds them. Dropping this -- a new
/// query, a changed filter, the window closing -- stops the worker at its
/// next file: a search for what nobody is asking about any more is work for
/// nothing.
struct ContentSearch {
    /// Set to stop the worker.
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// What the worker has found and read.
    news: std::sync::mpsc::Receiver<ContentNews>,
    /// How many files it has to read.
    total: usize,
    /// How many it has read so far.
    read: usize,
    /// How many it could not search.
    skipped: usize,
}

impl ContentSearch {
    /// Start searching `candidates` -- `(index entry, path)` -- for `query`.
    fn start(
        candidates: Vec<(usize, String)>,
        query: &str,
        case_sensitive: bool,
        limit: u64,
    ) -> Self {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tx, news) = std::sync::mpsc::channel();
        let total = candidates.len();
        let stop = std::sync::Arc::clone(&cancel);
        let query = query.to_string();
        std::thread::spawn(move || {
            for (index, path) in candidates {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let told =
                    match file_contains(std::path::Path::new(&path), &query, case_sensitive, limit)
                    {
                        Ok(true) => tx
                            .send(ContentNews::Found(index))
                            .and_then(|()| tx.send(ContentNews::Read)),
                        Ok(false) => tx.send(ContentNews::Read),
                        Err(_) => tx.send(ContentNews::Skipped),
                    };
                // The window has stopped listening: nobody wants the rest.
                if told.is_err() {
                    return;
                }
            }
            // As above: a closed channel means the answer has no reader.
            let _ = tx.send(ContentNews::Done);
        });
        Self {
            cancel,
            news,
            total,
            read: 0,
            skipped: 0,
        }
    }
}

impl Drop for ContentSearch {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Seconds since the epoch, by the system clock.
///
/// "Now", for the date filters and for every "3 hours ago" in the window. It
/// was the constant `1_779_000_000` -- the middle of May 2026 -- so "Today"
/// meant that day forever, and a file saved this morning was listed as
/// modified months in the future. A clock before the epoch reads as the
/// epoch rather than failing: every age is then "just now", which is wrong
/// in the direction that hides nothing.
#[must_use]
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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

// ─── Pointer targets and layout ──────────────────────────────────────

/// Everything in the window a pointer can press, as the renderer records it.
///
/// This app drew a query box, three filter strips, four switches, a sortable
/// table and a pane of action buttons, and handled no pointer event of any
/// kind (`known-issues.md` →
/// `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`). Each
/// variant is recorded by the walk that paints it ([`FileSearchApp::frame`]),
/// so a control and the place a press finds it cannot disagree.
///
/// A result is named by its index into the file index, not by its row: the
/// rows are a sorted, filtered, scrolled view of the index, and the index is
/// the one thing none of those operations renumbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// "Choose folder…", which puts the folder picker up.
    ChooseFolder,
    /// The query box. Typing always reaches it, so a press here only says so.
    SearchBox,
    /// The match-mode switch at the query box's right end.
    SearchMode,
    /// Save, or forget, the search in the query box.
    SaveSearch,
    /// The filters panel itself, which scrolls under the wheel.
    Filters,
    /// "All Types".
    CategoryAll,
    /// One file type.
    Category(FileCategory),
    /// One size band.
    Size(SizeFilter),
    /// One date range.
    Date(DateFilter),
    /// The match-mode row.
    MatchMode,
    /// The case-sensitivity row.
    MatchCase,
    /// The hidden-files row.
    Hidden,
    /// The folders row.
    Folders,
    /// A saved search, which runs it again.
    Saved(u32),
    /// A saved search's ×, which forgets it.
    Unsave(u32),
    /// A recent search, which runs it again.
    Recent(u32),
    /// The results panel itself, which scrolls under the wheel.
    Results,
    /// A column heading, which sorts by it.
    Header(SortColumn),
    /// A result, by its index into [`FileIndex::entries`].
    Result(usize),
    /// Open the selected result.
    Open,
    /// Open the folder the selected result is in.
    OpenLocation,
    /// The shortcut card; a press anywhere puts it away.
    HelpCard,
}

impl Target {
    /// What pressing this does, for the status bar while the pointer is on it,
    /// where a button that has a key can name it.
    #[must_use]
    pub fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::ChooseFolder => "Choose the folder to search (Ctrl+O)",
            Self::SearchMode => "What the query means: name, glob, regex or content (Ctrl+R)",
            Self::SaveSearch => "Save this search, or forget it (Ctrl+D)",
            Self::Header(_) => "Sort by this column; again to reverse it",
            Self::Open => "Open it (Enter)",
            Self::OpenLocation => "Open the folder it is in (Ctrl+L)",
            Self::Unsave(_) => "Forget this saved search",
            _ => return None,
        })
    }
}

/// The table columns a heading press sorts by, as `(column, sort)`.
///
/// Type sorts by extension, which is what the column shows. Category has no
/// column of its own and is sorted from the keyboard (Ctrl+C).
const HEADER_SORTS: [(usize, SortColumn); 5] = [
    (COL_NAME, SortColumn::Name),
    (COL_PATH, SortColumn::Path),
    (COL_SIZE, SortColumn::Size),
    (COL_MODIFIED, SortColumn::Modified),
    (COL_TYPE, SortColumn::Extension),
];

/// Height of the header row above the results.
const RESULTS_HEADER_H: f32 = 24.0;
/// Height of one result row.
const RESULT_ROW_H: f32 = 28.0;
/// How many recent searches the filters panel lists.
const RECENT_SHOWN: usize = 8;
/// The folder button's label.
const FOLDER_BUTTON_LABEL: &str = "Choose folder…  Ctrl+O";

/// Where the window's regions go at a given size.
///
/// One function for the drawing and for everything that needs a region's size
/// between frames -- how many result rows fit, how far the filters can scroll
/// -- so neither has its own copy of the arithmetic.
#[derive(Debug, Clone, Copy)]
struct Layout {
    header: Rect,
    filters: Rect,
    results: Rect,
    preview: Rect,
    status: Rect,
}

impl Layout {
    fn of(app: &FileSearchApp, width: f32, height: f32) -> Self {
        let header_h = 60.0;
        let status_h = 24.0;
        let sidebar_w = if app.show_filters { 200.0 } else { 0.0 };
        let preview_w = if app.show_preview { 280.0 } else { 0.0 };
        let content_y = header_h;
        let content_h = (height - header_h - status_h).max(0.0);
        let results_w = (width - sidebar_w - preview_w).max(0.0);
        Self {
            header: Rect::new(0.0, 0.0, width, header_h),
            filters: Rect::new(0.0, content_y, sidebar_w, content_h),
            results: Rect::new(sidebar_w, content_y, results_w, content_h),
            preview: Rect::new(sidebar_w + results_w, content_y, preview_w, content_h),
            status: Rect::new(0.0, height - status_h, width, status_h),
        }
    }

    /// How many whole result rows the results panel shows.
    fn result_rows(self) -> usize {
        ((self.results.h - RESULTS_HEADER_H) / RESULT_ROW_H).max(0.0) as usize
    }
}

/// The folder button's box, right-aligned on the title row.
fn header_button_rect(width: f32, label: &str) -> Rect {
    let w = guitk::text::width(label, 11.0) + 16.0;
    Rect::new(width - 16.0 - w, 5.0, w, 20.0)
}

/// The query box.
fn search_box_rect(width: f32) -> Rect {
    Rect::new(16.0, 28.0, (width - 32.0).max(0.0), 28.0)
}

/// The save switch and the mode switch inside the query box's right end.
fn search_switch_rects(search: Rect) -> (Rect, Rect) {
    let mode = Rect::new(search.right() - 80.0, 30.0, 68.0, 24.0);
    let save = Rect::new(mode.x - 84.0, 30.0, 78.0, 24.0);
    (save, mode)
}

/// One row of the filters panel.
#[derive(Debug, Clone)]
struct FilterRow {
    rect: Rect,
    label: String,
    kind: FilterRowKind,
}

/// What a filters-panel row is.
#[derive(Debug, Clone, Copy)]
enum FilterRowKind {
    /// A section heading; not a target.
    Heading,
    /// A choice or a switch, drawn lit when `selected`.
    Choice { target: Target, selected: bool },
    /// A saved search: the row runs it, its × forgets it.
    Saved { id: u32 },
}

// ─── Application ─────────────────────────────────────────────────────

use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::table::{Column, Fit, Table};
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

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
/// The keys this program answers, raised by `F1`.
///
/// `?` is not a second way in: the search box takes a typed query, so a `?`
/// has somewhere to go -- the `apps/spreadsheet` case in design-decisions 863.
///
/// The six sort chords are `Ctrl` plus the first letter of the column, which
/// is the only reason they are letters rather than a menu.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Up / Down", "Move through the results"),
    ("PageUp / PageDown", "A page of results"),
    ("Home / End", "First / last result"),
    ("Enter", "Open what is selected"),
    ("Ctrl+L", "Open the folder it is in"),
    ("Ctrl+D", "Save this search, or forget it"),
    ("Backspace", "Rub out a letter of the query"),
    ("Esc", "Clear the query"),
    ("Ctrl+O", "Choose a folder to search"),
    ("Ctrl+N / Ctrl+S / Ctrl+M", "Sort by name / size / modified"),
    (
        "Ctrl+E / Ctrl+C / Ctrl+A",
        "Sort by extension / category / path",
    ),
    ("Ctrl+F", "Show or hide the filters"),
    ("Ctrl+P", "Show or hide the preview"),
    ("Ctrl+1 / Ctrl+2 / Ctrl+3", "Next kind / size / date filter"),
    ("Ctrl+R", "Cycle the match mode"),
    ("Ctrl+U", "Match case"),
    ("Ctrl+H", "Include hidden files"),
    ("F1", "This list"),
];

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
    /// The picker. Holds the dialog and the routing thirteen
    /// applications used to write out by hand.
    pub picker: FilePicker,
    /// How many entries the last index pass could not represent.
    ///
    /// A name on this system is bytes, not text: every byte but `/` and NUL is
    /// legal. `IndexEntry` holds `String`, and `globmatch::glob_match` takes
    /// `&str`, so a name that is not UTF-8 cannot be matched against a pattern
    /// at all. Those entries are **skipped and counted** rather than decoded
    /// lossily -- `guitk`'s own `DirEntry::extension` explains why in the same
    /// words: decoding lossily "could make it match one it should not", which
    /// in a search tool is the whole game. The count is shown, because a file
    /// search that silently omits files is worse than one that says it did.
    pub skipped_unrepresentable: usize,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// Whether the shortcut card is up.
    show_help: bool,
    /// The size the window was last drawn at: what a press is hit-tested
    /// against, and what the picker is laid out in. The picker used to be
    /// placed in the size the window *opened* at, whatever it had been
    /// resized to since.
    window: (f32, f32),
    /// The first result row shown.
    pub results_scroll: usize,
    /// How far the filters panel is scrolled, in pixels.
    pub filters_scroll: f32,
    /// The folder the index was built from, for the header.
    pub root: Option<std::path::PathBuf>,
    /// What the pointer is over, so it can be drawn lit and named.
    hover: Option<Target>,
    /// Every box the last paint recorded; see [`FileSearchApp::target_at`].
    last_hits: Vec<(Target, Rect)>,
    /// The wheel's remainder over the results, so a trackpad's fractions add
    /// up instead of vanishing.
    results_wheel: wheel::Accumulator,
    /// Saved-search entries in the settings file this version cannot read,
    /// kept so they are written back as they were.
    unreadable_saved: Vec<String>,
    /// How a program is started on a path. A field so the tests can see what
    /// would have been started without starting anything.
    launch: fn(&str, &std::path::Path) -> std::io::Result<()>,
    /// The content search in progress, if one is.
    content: Option<ContentSearch>,
    /// The largest file a content search reads; [`MAX_CONTENT_BYTES`] but for
    /// the tests that need a small one.
    content_limit: u64,
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
            show_help: false,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            index: FileIndex::new(),
            picker: FilePicker::new(),
            skipped_unrepresentable: 0,
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
            window: (window_width(), window_height()),
            results_scroll: 0,
            filters_scroll: 0.0,
            root: None,
            hover: None,
            last_hits: Vec::new(),
            results_wheel: wheel::Accumulator::default(),
            unreadable_saved: Vec::new(),
            launch: spawn_program,
            content: None,
            content_limit: MAX_CONTENT_BYTES,
        }
    }

    /// Execute a search with current criteria
    pub fn execute_search(&mut self) {
        // Whatever was being searched for, it is not what is being asked now.
        self.content = None;
        if self.criteria.mode == SearchMode::Content && !self.criteria.query.is_empty() {
            self.start_content_search();
            return;
        }
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

        // A new answer starts at its top.
        self.selected_result = None;
        self.results_scroll = 0;
    }

    /// Start reading the files the filters leave for the query.
    ///
    /// Folders are not candidates: a folder has no contents to search, and
    /// listing one because a file inside it matched would be a different kind
    /// of answer.
    fn start_content_search(&mut self) {
        let candidates: Vec<(usize, String)> = self
            .index
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.is_directory && self.criteria.passes_filters(e))
            .map(|(i, e)| (i, e.path.clone()))
            .collect();
        let total = candidates.len();
        self.results.clear();
        self.selected_result = None;
        self.results_scroll = 0;
        self.content = Some(ContentSearch::start(
            candidates,
            &self.criteria.query,
            self.criteria.case_sensitive,
            self.content_limit,
        ));
        self.status_message = format!("Searching the contents of {total} files...");
    }

    /// Take in whatever the content worker has found since last asked.
    /// Returns whether anything changed on screen.
    pub fn pump_content_search(&mut self) -> bool {
        let Some(search) = self.content.as_mut() else {
            return false;
        };
        let mut found = Vec::new();
        let mut finished = false;
        let mut changed = false;
        loop {
            match search.news.try_recv() {
                Ok(ContentNews::Found(index)) => found.push(index),
                Ok(ContentNews::Read) => search.read = search.read.saturating_add(1),
                Ok(ContentNews::Skipped) => search.skipped = search.skipped.saturating_add(1),
                // A worker that is gone without saying it finished was
                // cancelled or died; either way nothing more is coming.
                Ok(ContentNews::Done) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
            changed = true;
        }
        let (read, skipped, total) = (search.read, search.skipped, search.total);
        if !found.is_empty() {
            // Sorted in, not appended: the table is in the order its heading
            // says, and the selection stays on the file it was on.
            let selected = self
                .selected_result
                .and_then(|i| self.results.get(i).copied());
            self.results.extend(found);
            self.sort_results();
            self.selected_result = selected.and_then(|e| self.results.iter().position(|&r| r == e));
        }
        let count = self.results.len();
        let skipped_note = if skipped == 0 {
            String::new()
        } else {
            format!(" -- {skipped} not searched: too large or unreadable")
        };
        if finished {
            self.content = None;
            self.status_message =
                format!("{count} files contain it, of {total} searched{skipped_note}");
            return true;
        }
        if changed {
            let seen = read.saturating_add(skipped);
            self.status_message = format!(
                "Searching the contents of {total} files... {seen} so far, {count} found{skipped_note}"
            );
        }
        changed
    }

    /// Whether a content search is still reading.
    #[must_use]
    pub fn is_searching_contents(&self) -> bool {
        self.content.is_some()
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

    // ------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------

    /// Route a compositor event into the app.
    /// Put the folder picker up, listing the directory it starts in.
    ///
    /// `select_folder` rather than `open`: this indexes a tree, so the thing
    /// being chosen is a directory. The widget does no I/O -- the host reads
    /// the listing and hands it over, which is the convention `apps/fileassoc`
    /// and `apps/photomanager` follow.
    pub fn open_folder_dialog(&mut self) {
        // `put_up` rather than `open_to_read`: this is the one caller that
        // selects a FOLDER, and a named opener for a single caller would be
        // API invented for symmetry.
        self.picker.put_up(
            FileDialog::select_folder().with_initial_path(FilePicker::default_start()),
            false,
        );
    }

    /// Walk `root` and put what is there into the index.
    ///
    /// Replaces the index rather than adding to it: two folders indexed in
    /// turn would otherwise leave results from the first still matching, and a
    /// search tool reporting a file from a folder you are no longer looking at
    /// is worse than one reporting nothing.
    ///
    /// Bounded at [`MAX_INDEXED`] entries. A walk of a whole disk in a single
    /// frame would hang the window, and an unbounded one would hang it for
    /// however long the disk takes. The bound is reported when it is hit, so a
    /// truncated index cannot be mistaken for a complete one.
    pub fn index_directory(&mut self, root: &std::path::Path) {
        self.index.clear();
        self.skipped_unrepresentable = 0;
        self.root = Some(root.to_path_buf());
        // Ages are shown relative to the moment the index was taken, which is
        // the moment its modification times were read.
        self.criteria.current_time = unix_now();
        let mut queue = vec![root.to_path_buf()];
        let mut indexed = 0_usize;
        let mut truncated = false;

        while let Some(dir) = queue.pop() {
            let Ok(listing) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in listing.flatten() {
                if indexed >= MAX_INDEXED {
                    truncated = true;
                    break;
                }
                let path = entry.path();
                let is_dir = path.is_dir();
                if is_dir {
                    queue.push(path.clone());
                }
                let (Some(path_text), Some(name_text)) =
                    (path.to_str(), path.file_name().and_then(|n| n.to_str()))
                else {
                    // Not text, so no pattern can be matched against it.
                    self.skipped_unrepresentable = self.skipped_unrepresentable.saturating_add(1);
                    continue;
                };
                let meta = entry.metadata().ok();
                let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
                let modified = meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                self.index.add(IndexEntry::new(
                    path_text, name_text, size, modified, modified, is_dir,
                ));
                indexed = indexed.saturating_add(1);
            }
            if truncated {
                break;
            }
        }

        self.status_message = describe_index_pass(
            &root.to_string_lossy(),
            indexed,
            self.skipped_unrepresentable,
            truncated,
        );
    }

    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The picker takes input first while it is up. A tick or a
        // resize comes back as `Ignored` and falls through to its own arm
        // below -- the guard arms this replaced had that property by
        // construction, and it is why neither of these two ever stopped
        // its application's clock.
        match self.picker.handle(event, self.window.0, self.window.1) {
            Picked::Chose(path) => {
                self.index_directory(&path);
                self.execute_search();
                return EventResult::Consumed;
            }
            // Cancelled grouped with Handled: this caller keeps no dialog
            // state of its own that could go stale.
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Mouse(mouse) => {
                // The card is modal: a press anywhere puts it away, and nothing
                // under it hears one.
                if self.show_help {
                    if matches!(mouse.kind, MouseEventKind::Press(_)) {
                        self.show_help = false;
                        return EventResult::Consumed;
                    }
                    return EventResult::Ignored;
                }
                self.handle_mouse(mouse)
            }
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.window = (*width as f32, *height as f32);
                }
                self.keep_selection_visible();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Apply a key press.
    ///
    /// The query box has focus by default, because a search program that needs
    /// a keystroke before it will accept a query is a search program with an
    /// extra step. The shortcuts are therefore on Ctrl, and every plain
    /// printable key is query text.
    pub fn handle_key_help_card(&mut self, key: &KeyEvent) -> Option<EventResult> {
        // Above everything: the search box takes typed text and `F1` is not
        // text, so a reader half-way through a query still gets the keys.
        if key.key == Key::F1 {
            self.show_help = !self.show_help;
            return Some(EventResult::Consumed);
        }
        if self.show_help {
            // Modal. Letting keys through would mean re-running a search the
            // reader cannot see.
            if matches!(key.key, Key::Escape | Key::Enter | Key::F1) {
                self.show_help = false;
            }
            return Some(EventResult::Consumed);
        }
        None
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        if let Some(answered) = self.handle_key_help_card(key) {
            return answered;
        }
        let ctrl = key.modifiers.ctrl;
        match key.key {
            Key::Up => self.step_selection(-1),
            Key::Down => self.step_selection(1),
            Key::PageUp => self.step_selection(self.page_rows().saturating_neg()),
            Key::PageDown => self.step_selection(self.page_rows()),
            Key::Home => self.select_edge(true),
            Key::End => self.select_edge(false),
            Key::Backspace => {
                if self.criteria.query.pop().is_none() {
                    return EventResult::Ignored;
                }
                self.execute_search();
                EventResult::Consumed
            }
            Key::Escape => {
                if self.criteria.query.is_empty() {
                    return EventResult::Ignored;
                }
                self.criteria.query.clear();
                self.execute_search();
                EventResult::Consumed
            }
            // What the F1 card has always said Enter does. It re-ran the
            // search -- which had already run on the last keystroke -- so a
            // search tool could find a file and do nothing with it. With
            // nothing selected, Enter selects the first result, so a second
            // Enter opens it.
            Key::Enter => {
                if self.selected_result.is_some() {
                    self.open_selected()
                } else {
                    self.select_edge(true)
                }
            }
            Key::L if ctrl => self.open_location(),
            Key::D if ctrl => self.toggle_saved(),
            // Sorting, on Ctrl so the bare letters stay available as query
            // text. Pressing the current column again reverses it, which is
            // what a column header does everywhere else.
            Key::O if ctrl => {
                self.open_folder_dialog();
                EventResult::Consumed
            }
            Key::N if ctrl => self.sort_by(SortColumn::Name),
            Key::S if ctrl => self.sort_by(SortColumn::Size),
            Key::M if ctrl => self.sort_by(SortColumn::Modified),
            Key::E if ctrl => self.sort_by(SortColumn::Extension),
            Key::C if ctrl => self.sort_by(SortColumn::Category),
            Key::A if ctrl => self.sort_by(SortColumn::Path),
            Key::F if ctrl => {
                self.show_filters = !self.show_filters;
                EventResult::Consumed
            }
            Key::P if ctrl => {
                self.show_preview = !self.show_preview;
                EventResult::Consumed
            }
            // The three filter strips, by position, because the strips are
            // lists and their headings print these keys. Every one of them
            // was drawn with a selection highlight and nothing could move the
            // selection: a File Type strip that could not filter by type.
            Key::Num1 if ctrl => {
                self.criteria.category_filter =
                    FileCategory::step(self.criteria.category_filter, !key.modifiers.shift);
                self.rerun_with_filters()
            }
            Key::Num2 if ctrl => {
                self.criteria.size_filter = self.criteria.size_filter.step(!key.modifiers.shift);
                self.rerun_with_filters()
            }
            Key::Num3 if ctrl => {
                self.criteria.date_filter = self.criteria.date_filter.step(!key.modifiers.shift);
                self.rerun_with_filters()
            }
            // What the query means, and what the search reaches.
            Key::R if ctrl => {
                self.criteria.mode = self.criteria.mode.next();
                self.rerun_with_filters()
            }
            Key::U if ctrl => {
                self.criteria.case_sensitive = !self.criteria.case_sensitive;
                self.rerun_with_filters()
            }
            Key::H if ctrl => {
                self.criteria.include_hidden = !self.criteria.include_hidden;
                self.rerun_with_filters()
            }
            Key::K if ctrl => {
                self.criteria.include_directories = !self.criteria.include_directories;
                self.rerun_with_filters()
            }
            _ => {
                if key.text.is_empty() || ctrl {
                    return EventResult::Ignored;
                }
                self.criteria.query.push_str(&key.text);
                self.execute_search();
                EventResult::Consumed
            }
        }
    }

    /// Run the search again after a filter moved, and say it was handled.
    ///
    /// Every filter key goes through here rather than setting a field and
    /// stopping: the results on screen are the answer to the *previous*
    /// filter, and leaving them there invites reading them as the new one's.
    fn rerun_with_filters(&mut self) -> EventResult {
        self.execute_search();
        EventResult::Consumed
    }

    /// Sort by a column, reversing it if it is already the sort column.
    fn sort_by(&mut self, column: SortColumn) -> EventResult {
        if self.sort_column == column {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = column;
            self.sort_ascending = true;
        }
        // The results are re-sorted by the search, and the selection is an
        // index into them.
        let selected = self
            .selected_result
            .and_then(|i| self.results.get(i).copied());
        self.execute_search();
        self.selected_result = selected.and_then(|e| self.results.iter().position(|&r| r == e));
        EventResult::Consumed
    }

    /// Move the selection through the results.
    fn step_selection(&mut self, delta: isize) -> EventResult {
        if self.results.is_empty() {
            if self.selected_result.is_none() {
                return EventResult::Ignored;
            }
            self.selected_result = None;
            return EventResult::Consumed;
        }
        let next = match self.selected_result {
            None => 0,
            Some(pos) => {
                let Ok(pos) = isize::try_from(pos) else {
                    return EventResult::Ignored;
                };
                // Clamped to the list, so a page step near either end lands
                // on the end rather than refusing to move; a single step off
                // an end clamps to where it already is, and is ignored below.
                let moved = pos.saturating_add(delta).max(0);
                let last = self.results.len().saturating_sub(1);
                usize::try_from(moved).map_or(last, |m| m.min(last))
            }
        };
        if Some(next) == self.selected_result {
            return EventResult::Ignored;
        }
        self.selected_result = Some(next);
        self.keep_selection_visible();
        EventResult::Consumed
    }

    /// How many rows a page is: the rows the results panel shows, less one so
    /// a page keeps a row of context, and never less than one.
    fn page_rows(&self) -> isize {
        let rows = Layout::of(self, self.window.0, self.window.1).result_rows();
        isize::try_from(rows.saturating_sub(1).max(1)).unwrap_or(1)
    }

    /// Jump to the first or last result.
    fn select_edge(&mut self, first: bool) -> EventResult {
        if self.results.is_empty() {
            return EventResult::Ignored;
        }
        let target = if first {
            Some(0)
        } else {
            self.results.len().checked_sub(1)
        };
        if target == self.selected_result {
            return EventResult::Ignored;
        }
        self.selected_result = target;
        self.keep_selection_visible();
        EventResult::Consumed
    }

    /// Named `render_commands` and not `render`: this takes a width and a
    /// height, exactly as `oswindow::app::App::render` does, and at equal arity
    /// an inherent method silently wins method lookup over the trait's — so an
    /// app that keeps the name draws nothing and reports no error.
    ///
    /// Renders the UI.
    #[must_use]
    pub fn render_commands(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        self.frame(width, height).into_tree().commands
    }

    /// Draw the window at `width` by `height`, recording every control where
    /// it is drawn.
    ///
    /// This is both the renderer and the hit test (see [`Target`]): a press is
    /// resolved by drawing a frame and asking it what is at the point, so a
    /// control and the place a click finds it cannot disagree.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        let l = Layout::of(self, width, height);

        // Background
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        self.draw_header(&mut f, l.header);
        if self.show_filters {
            self.draw_filters(&mut f, l.filters);
        }
        self.draw_results(&mut f, l.results);
        if self.show_preview {
            self.draw_preview(&mut f, l.preview);
        }

        // Status bar
        let sy = l.status.y;
        self.palette
            .push_surface(&mut f, 0.0, sy, width, l.status.h, 0.0, Surface::Card);
        let status = match self.hover.and_then(Target::tip) {
            Some(tip) => tip.to_string(),
            None => format!("{} indexed  |  {}", self.index.count(), self.status_message),
        };
        f.push(RenderCommand::Text {
            x: 12.0,
            y: sy + 6.0,
            text: status,
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
            // A press anywhere puts the card away, and goes no further.
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));
        }
        f
    }

    /// A compact button with a label, lit while the pointer is on it.
    fn draw_button(&self, f: &mut Frame<Target>, rect: Rect, label: &str, target: Target) {
        let surface = if self.hover == Some(target) {
            Surface::Panel
        } else {
            Surface::Card
        };
        self.palette
            .push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, surface);
        f.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + (rect.h - 11.0) / 2.0 - 1.0,
            text: label.to_string(),
            font_size: 11.0,
            color: self.palette.ink(self.palette.teal),
            font_weight: FontWeightHint::Regular,
            max_width: Some((rect.w - 16.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(target, rect);
    }

    /// The header: title, the folder being searched and the button that
    /// changes it, and the query box with its mode and save switches.
    fn draw_header(&self, f: &mut Frame<Target>, header: Rect) {
        let width = header.w;
        self.palette.push_surface(
            f,
            0.0,
            0.0,
            width,
            header.h,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        f.push(RenderCommand::Text {
            x: 16.0,
            y: 8.0,
            text: "File Search".to_string(),
            font_size: 14.0,
            color: self.palette.ink(self.palette.blue),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // The folder button, right-aligned on the title row. There was no
        // way to choose a folder but Ctrl+O -- the status bar said so on
        // launch, and nothing on screen could be pressed to do it.
        let folder = header_button_rect(width, FOLDER_BUTTON_LABEL);
        self.draw_button(f, folder, FOLDER_BUTTON_LABEL, Target::ChooseFolder);

        // Which folder the index holds, between the title and the button.
        let label = self.root_label();
        f.push(RenderCommand::Text {
            x: 110.0,
            y: 10.0,
            text: label,
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((folder.x - 110.0 - 12.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // Search input
        let search = search_box_rect(width);
        self.palette.push_surface(
            f,
            search.x,
            search.y,
            search.w,
            search.h,
            6.0,
            Surface::Card,
        );
        f.hit(Target::SearchBox, search);

        let search_text = if self.criteria.query.is_empty() {
            "Search files...".to_string()
        } else {
            self.criteria.query.clone()
        };
        f.push(RenderCommand::Text {
            x: search.x + 12.0,
            y: 36.0,
            text: search_text,
            font_size: 13.0,
            color: if self.criteria.query.is_empty() {
                self.palette.subtext0
            } else {
                self.palette.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some((search.w - 190.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // Save this search, then the match mode, both inside the box's right
        // end. The mode was a label; it is the switch Ctrl+R turns now.
        let (save, mode) = search_switch_rects(search);
        let saved = self.current_search_is_saved();
        let save_label = if saved { "★ Saved" } else { "☆ Save" };
        self.draw_button(f, save, save_label, Target::SaveSearch);
        self.palette
            .push_surface(f, mode.x, mode.y, mode.w, mode.h, 4.0, Surface::Card);
        f.push(RenderCommand::Text {
            x: mode.x + 8.0,
            y: 36.0,
            text: self.criteria.mode.to_string(),
            font_size: 11.0,
            color: self.palette.ink(self.palette.mauve),
            font_weight: FontWeightHint::Bold,
            max_width: Some(mode.w - 12.0),
            overflow: TextOverflow::Clip,
        });
        f.hit(Target::SearchMode, mode);
    }

    /// The filters sidebar, every row of it a target, scrolled by
    /// `filters_scroll` and clipped to its panel.
    ///
    /// Every chip here was drawn with a selection highlight that only the
    /// keyboard could move, and the four "Search" rows were labels. The panel
    /// was also taller than the window it is drawn in -- the last rows ran
    /// under the status bar -- so it scrolls now.
    fn draw_filters(&self, f: &mut Frame<Target>, panel: Rect) {
        f.push(RenderCommand::FillRect {
            x: panel.x,
            y: panel.y,
            width: panel.w,
            height: panel.h,
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });
        f.hit(Target::Filters, panel);
        f.clip(panel);
        for row in self.filter_rows(panel) {
            let rect = row.rect;
            match row.kind {
                FilterRowKind::Heading => {
                    f.push(RenderCommand::Text {
                        x: rect.x + 12.0,
                        y: rect.y,
                        text: row.label,
                        font_size: 11.0,
                        color: self.palette.subtext0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(rect.w - 24.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
                FilterRowKind::Choice { target, selected } => {
                    if selected || self.hover == Some(target) {
                        let surface = if selected {
                            Surface::Card
                        } else {
                            Surface::Panel
                        };
                        self.palette.push_surface(
                            f,
                            rect.x + 4.0,
                            rect.y,
                            rect.w - 8.0,
                            22.0,
                            4.0,
                            surface,
                        );
                    }
                    f.push(RenderCommand::Text {
                        x: rect.x + 12.0,
                        y: rect.y + 4.0,
                        text: row.label,
                        font_size: 11.0,
                        color: if selected {
                            self.palette.ink(self.palette.blue)
                        } else {
                            self.palette.subtext1
                        },
                        font_weight: if selected {
                            FontWeightHint::Bold
                        } else {
                            FontWeightHint::Regular
                        },
                        max_width: Some(rect.w - 24.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                    f.hit(target, Rect::new(rect.x + 4.0, rect.y, rect.w - 8.0, 22.0));
                }
                FilterRowKind::Saved { id } => {
                    let run = Target::Saved(id);
                    let forget = Target::Unsave(id);
                    if self.hover == Some(run) {
                        self.palette.push_surface(
                            f,
                            rect.x + 4.0,
                            rect.y,
                            rect.w - 8.0,
                            22.0,
                            4.0,
                            Surface::Panel,
                        );
                    }
                    f.push(RenderCommand::Text {
                        x: rect.x + 12.0,
                        y: rect.y + 4.0,
                        text: row.label,
                        font_size: 11.0,
                        color: self.palette.subtext1,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(rect.w - 24.0 - 20.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                    f.hit(run, Rect::new(rect.x + 4.0, rect.y, rect.w - 8.0, 22.0));
                    // The ×, recorded after the row so it wins where they
                    // overlap.
                    let x_box = Rect::new(rect.right() - 26.0, rect.y + 2.0, 18.0, 18.0);
                    if self.hover == Some(forget) {
                        f.push(RenderCommand::FillRect {
                            x: x_box.x,
                            y: x_box.y,
                            width: x_box.w,
                            height: x_box.h,
                            color: self.palette.surface1,
                            corner_radii: CornerRadii::all(4.0),
                        });
                    }
                    f.push(RenderCommand::Text {
                        x: x_box.x + 5.0,
                        y: x_box.y + 2.0,
                        text: "x".to_string(),
                        font_size: 11.0,
                        color: self.palette.subtext0,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                    f.hit(forget, x_box);
                }
            }
        }
        f.unclip();
    }

    /// Every row of the filters panel, laid out from `panel`'s top with the
    /// scroll applied: the drawing and the wheel's limit both read this.
    fn filter_rows(&self, panel: Rect) -> Vec<FilterRow> {
        let x = panel.x;
        let w = panel.w;
        let mut rows = Vec::new();
        let mut fy = panel.y + 8.0 - self.filters_scroll;
        let heading = |rows: &mut Vec<FilterRow>, fy: &mut f32, label: &str| {
            rows.push(FilterRow {
                rect: Rect::new(x, *fy, w, 20.0),
                label: label.to_string(),
                kind: FilterRowKind::Heading,
            });
            *fy += 20.0;
        };
        let choice = |rows: &mut Vec<FilterRow>,
                      fy: &mut f32,
                      label: String,
                      target: Target,
                      selected: bool,
                      step: f32| {
            rows.push(FilterRow {
                rect: Rect::new(x, *fy, w, 22.0),
                label,
                kind: FilterRowKind::Choice { target, selected },
            });
            *fy += step;
        };

        heading(&mut rows, &mut fy, "File Type (Ctrl+1)");
        choice(
            &mut rows,
            &mut fy,
            "All Types".to_string(),
            Target::CategoryAll,
            self.criteria.category_filter.is_none(),
            24.0,
        );
        for cat in FileCategory::ALL {
            choice(
                &mut rows,
                &mut fy,
                format!("{} {cat}", category_icon(cat)),
                Target::Category(cat),
                self.criteria.category_filter == Some(cat),
                24.0,
            );
        }

        fy += 12.0;
        heading(&mut rows, &mut fy, "Size (Ctrl+2)");
        for sf in SizeFilter::CHIPS {
            choice(
                &mut rows,
                &mut fy,
                sf.label().to_string(),
                Target::Size(sf),
                self.criteria.size_filter == sf,
                24.0,
            );
        }

        fy += 12.0;
        heading(&mut rows, &mut fy, "Modified (Ctrl+3)");
        for df in DateFilter::ALL {
            choice(
                &mut rows,
                &mut fy,
                df.label().to_string(),
                Target::Date(df),
                self.criteria.date_filter == df,
                22.0,
            );
        }

        // What the query means and what the walk reaches. These four were
        // read by the matcher and, for a long time, drawn nowhere at all, so a
        // search that silently skipped hidden files looked identical to one
        // that found none. They are switches now as well as labels.
        fy += 12.0;
        heading(&mut rows, &mut fy, "Search");
        let yes_no = |on: bool| if on { "Yes" } else { "No" };
        let switches = [
            (
                format!("Match by (Ctrl+R): {}", self.criteria.mode),
                Target::MatchMode,
            ),
            (
                format!(
                    "Case sensitive (Ctrl+U): {}",
                    yes_no(self.criteria.case_sensitive)
                ),
                Target::MatchCase,
            ),
            (
                format!(
                    "Hidden files (Ctrl+H): {}",
                    yes_no(self.criteria.include_hidden)
                ),
                Target::Hidden,
            ),
            (
                format!(
                    "Folders (Ctrl+K): {}",
                    yes_no(self.criteria.include_directories)
                ),
                Target::Folders,
            ),
        ];
        for (label, target) in switches {
            choice(&mut rows, &mut fy, label, target, false, 22.0);
        }

        // Saved searches, then recent ones: `search_history` was written on
        // every keystroke and read by nothing, and `bookmark_search` had no
        // caller at all.
        let saved: Vec<&SavedSearch> = self
            .search_history
            .iter()
            .filter(|s| s.is_bookmarked)
            .collect();
        if !saved.is_empty() {
            fy += 12.0;
            heading(&mut rows, &mut fy, "Saved searches (Ctrl+D)");
            for s in saved {
                rows.push(FilterRow {
                    rect: Rect::new(x, fy, w, 22.0),
                    label: format!("{} ({})", s.query, s.mode),
                    kind: FilterRowKind::Saved { id: s.id },
                });
                fy += 24.0;
            }
        }
        let recent: Vec<&SavedSearch> = self
            .search_history
            .iter()
            .rev()
            .filter(|s| !s.is_bookmarked)
            .take(RECENT_SHOWN)
            .collect();
        if !recent.is_empty() {
            fy += 12.0;
            heading(&mut rows, &mut fy, "Recent");
            for s in recent {
                choice(
                    &mut rows,
                    &mut fy,
                    format!("{} ({})", s.query, s.mode),
                    Target::Recent(s.id),
                    false,
                    24.0,
                );
            }
        }
        rows
    }

    /// How far the filters panel can scroll: its content's height past the
    /// panel's own.
    fn filters_scroll_limit(&self, panel: Rect) -> f32 {
        let top = panel.y + 8.0 - self.filters_scroll;
        let bottom = self
            .filter_rows(panel)
            .last()
            .map_or(top, |row| row.rect.bottom());
        (bottom - top + 16.0 - panel.h).max(0.0)
    }

    /// Draw the results table, recording each column heading (which sorts)
    /// and each row (which selects).
    ///
    /// Every cell here holds something the filesystem chose, not something this
    /// app authored — a filename, a directory, an extension — and the two that
    /// overflow in practice are Name and Path, which were clipped mid-glyph
    /// with no marker that anything had been dropped. Size and Type were drawn
    /// with no width at all; they happen to fit today only because
    /// [`IndexEntry::new`] refuses extensions of ten characters or more, which
    /// is an incidental property of the parser and not something a table should
    /// be relying on. All five now go through [`Table::cell`].
    ///
    /// The rows start at `results_scroll`. They used to start at the first
    /// result, always, and stop at the panel's bottom edge: a search with more
    /// results than fitted showed the first page and nothing else, and the
    /// keyboard could move the selection onto rows that were never drawn.
    fn draw_results(&self, f: &mut Frame<Target>, area: Rect) {
        let (x, y, w, h) = (area.x, area.y, area.w, area.h);
        f.hit(Target::Results, area);
        let table = Table::new(RESULT_COLUMNS, x);
        f.draw_with(|cmds| table.header(cmds, y + 4.0, self.palette.overlay0, ROW_FONT_SMALL));
        for (col, sort) in HEADER_SORTS {
            let rect = Rect::new(table.left(col), y, table.width(col), RESULTS_HEADER_H);
            if sort == self.sort_column {
                let label_w = RESULT_COLUMNS.get(col).map_or(0.0, |c| {
                    guitk::text::measure(c.label, ROW_FONT_SMALL, FontWeightHint::Bold)
                });
                f.push(RenderCommand::Text {
                    x: rect.x + label_w + 6.0,
                    y: y + 4.0,
                    text: if self.sort_ascending { "▲" } else { "▼" }.to_string(),
                    font_size: 9.0,
                    color: self.palette.overlay0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
            f.hit(Target::Header(sort), rect);
        }

        if self.results.is_empty() {
            let msg = if self.criteria.query.is_empty() {
                "Type to search"
            } else {
                "No results found"
            };
            f.push(RenderCommand::Text {
                x: x + w / 2.0 - 50.0,
                y: y + h / 2.0,
                text: msg.to_string(),
                font_size: 14.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            return;
        }

        let mut ry = y + RESULTS_HEADER_H;
        for (display_idx, &result_idx) in self.results.iter().enumerate().skip(self.results_scroll)
        {
            if ry + RESULT_ROW_H > y + h {
                break;
            }

            let Some(entry) = self.index.entries.get(result_idx) else {
                continue;
            };

            let row = Rect::new(x + 2.0, ry, w - 4.0, RESULT_ROW_H - 2.0);
            let target = Target::Result(result_idx);
            let is_sel = self.selected_result == Some(display_idx);
            if is_sel || self.hover == Some(target) {
                let surface = if is_sel {
                    Surface::Card
                } else {
                    Surface::Panel
                };
                self.palette
                    .push_surface(f, row.x, row.y, row.w, row.h, 4.0, surface);
            }

            let cy = ry + 6.0;

            // Name with icon.
            let icon = if entry.is_directory {
                "📁"
            } else {
                category_icon(entry.category)
            };
            let name_color = if entry.is_directory {
                self.palette.blue
            } else {
                self.palette.text
            };
            let size = if entry.is_directory {
                "—".to_string()
            } else {
                format_size(entry.size)
            };
            let age = self.criteria.current_time.saturating_sub(entry.modified);
            // An extension is whatever follows the last dot in a name the app
            // did not choose, so its length is not ours to assume.
            let kind = if entry.extension.is_empty() {
                "—".to_string()
            } else {
                entry.extension.to_uppercase()
            };
            f.draw_with(|cmds| {
                table.cell(
                    cmds,
                    COL_NAME,
                    cy,
                    &format!("{icon} {}", entry.name),
                    name_color,
                    ROW_FONT,
                    Fit::Start,
                );
                // The directory is cut at the *front*: what distinguishes two
                // results is the deepest directory, not the mount point they
                // share.
                table.cell(
                    cmds,
                    COL_PATH,
                    cy,
                    entry.parent_dir(),
                    self.palette.subtext0,
                    ROW_FONT_SMALL,
                    Fit::End,
                );
                table.cell(
                    cmds,
                    COL_SIZE,
                    cy,
                    &size,
                    self.palette.subtext1,
                    ROW_FONT_SMALL,
                    Fit::Start,
                );
                table.cell(
                    cmds,
                    COL_MODIFIED,
                    cy,
                    &format_relative_time(age),
                    self.palette.subtext0,
                    ROW_FONT_SMALL,
                    Fit::Start,
                );
                table.cell(
                    cmds,
                    COL_TYPE,
                    cy,
                    &kind,
                    self.palette.peach,
                    ROW_FONT_SMALL,
                    Fit::Start,
                );
            });
            f.hit(target, row);

            ry += RESULT_ROW_H;
        }
    }

    fn draw_preview(&self, f: &mut Frame<Target>, area: Rect) {
        let (x, y, w, h) = (area.x, area.y, area.w, area.h);
        // Separator
        f.push(RenderCommand::FillRect {
            x,
            y,
            width: 1.0,
            height: h,
            color: self.palette.surface0,
            corner_radii: CornerRadii::ZERO,
        });

        let Some(entry) = self.selected_entry() else {
            f.push(RenderCommand::Text {
                x: x + w / 2.0 - 50.0,
                y: y + h / 2.0,
                text: "Select a file".to_string(),
                font_size: 13.0,
                color: self.palette.subtext0,
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
        f.push(RenderCommand::Text {
            x: px,
            y: py,
            text: format!("{icon} {}", entry.name),
            font_size: 14.0,
            color: self.palette.text,
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
            f.push(RenderCommand::Text {
                x: px,
                y: py,
                text: label.to_string(),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            f.push(RenderCommand::Text {
                x: px + 80.0,
                y: py,
                text: value.clone(),
                font_size: 11.0,
                color: self.palette.subtext1,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w - 80.0),
                overflow: TextOverflow::Ellipsis,
            });
            py += 18.0;
        }

        // Actions. There were four -- Open, Open Location, Copy Path and
        // Properties -- drawn as buttons and wired to nothing. Two are real
        // now. Copy Path is gone until an application can reach the system
        // clipboard with a path intact (known-issues.md, the explorer's
        // clipboard entry), and Properties is gone because this pane *is* the
        // properties: a button that shows what is already on screen is not an
        // action.
        py += 16.0;
        f.push(RenderCommand::Text {
            x: px,
            y: py,
            text: "Actions".to_string(),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        py += 20.0;

        for (label, target) in [
            ("Open  Enter", Target::Open),
            ("Open the folder it is in  Ctrl+L", Target::OpenLocation),
        ] {
            self.draw_button(f, Rect::new(px, py, max_w, 24.0), label, target);
            py += 28.0;
        }
    }
}

// ─── The pointer, opening, and saved searches ────────────────────────

/// Start `program` on `path`. What [`FileSearchApp::launch`] does unless a
/// test has put something else there.
fn spawn_program(program: &str, path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(drop)
}

/// The program that shows a folder: the file manager, which takes the folder
/// to open as its one argument for exactly this (see its `main`).
const FILE_MANAGER: &str = "/usr/bin/explorer";

/// The settings group saved searches are kept in.
const CONFIG_NAME: &str = "filesearch";
/// The key holding them, as `mode:query` strings.
const SAVED_KEY: [&str; 1] = ["saved"];

impl SearchMode {
    /// The word a saved search records its mode as.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Substring => "name",
            Self::Glob => "glob",
            Self::Regex => "regex",
            Self::Content => "content",
        }
    }

    /// The mode a saved search's word names, if it names one.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Substring, Self::Glob, Self::Regex, Self::Content]
            .into_iter()
            .find(|m| m.key() == key)
    }
}

impl FileSearchApp {
    /// Draw the window at the size it was last given.
    fn current_frame(&self) -> Frame<Target> {
        self.frame(self.window.0, self.window.1)
    }

    /// What is under `(x, y)` in the frame last shown -- for the pointer's
    /// movement and the wheel, which arrive in floods and should not each
    /// draw a frame of their own. A press draws a fresh one instead.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.current_frame().hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    /// Route a pointer event.
    pub fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let Some(target) = self.current_frame().hit_test(event.x, event.y) else {
                    return EventResult::Ignored;
                };
                self.activate(target)
            }
            // A double press on a result opens it, which is what a double
            // press on a file does everywhere else. (Nothing produces this
            // event yet -- `oswindow` does not synthesise it; see
            // known-issues.md -- but it is the one gesture a person will try
            // first, and the day it arrives it should work.)
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                match self.current_frame().hit_test(event.x, event.y) {
                    Some(Target::Result(entry)) => {
                        self.select_entry(entry);
                        self.open_selected()
                    }
                    _ => EventResult::Ignored,
                }
            }
            MouseEventKind::Move => {
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Leave => {
                if self.hover.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Scroll { dy, .. } => {
                let over = self.target_at(event.x, event.y);
                self.scroll(over, dy)
            }
            _ => EventResult::Ignored,
        }
    }

    /// Turn the wheel over `over` by `dy` notches.
    fn scroll(&mut self, over: Option<Target>, dy: f32) -> EventResult {
        let l = Layout::of(self, self.window.0, self.window.1);
        match over {
            Some(Target::Results | Target::Result(_) | Target::Header(_)) => {
                let rows = self.results_wheel.rows(dy);
                let last_top = self.results.len().saturating_sub(l.result_rows());
                let before = self.results_scroll;
                self.results_scroll = self
                    .results_scroll
                    .saturating_add_signed(rows)
                    .min(last_top);
                if self.results_scroll == before {
                    EventResult::Ignored
                } else {
                    EventResult::Consumed
                }
            }
            Some(target) if self.show_filters && target.is_in_filters() => {
                let limit = self.filters_scroll_limit(l.filters);
                let before = self.filters_scroll;
                self.filters_scroll =
                    (self.filters_scroll + wheel::pixels(dy, 24.0)).clamp(0.0, limit);
                if (self.filters_scroll - before).abs() > f32::EPSILON {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Do what pressing `target` means.
    fn activate(&mut self, target: Target) -> EventResult {
        match target {
            Target::ChooseFolder => {
                self.open_folder_dialog();
                EventResult::Consumed
            }
            Target::SearchMode | Target::MatchMode => {
                self.criteria.mode = self.criteria.mode.next();
                self.rerun_with_filters()
            }
            Target::SaveSearch => self.toggle_saved(),
            Target::CategoryAll => {
                self.criteria.category_filter = None;
                self.rerun_with_filters()
            }
            Target::Category(cat) => {
                // Pressing the lit chip again turns it off, which is the way
                // out the lit chip implies.
                self.criteria.category_filter = if self.criteria.category_filter == Some(cat) {
                    None
                } else {
                    Some(cat)
                };
                self.rerun_with_filters()
            }
            Target::Size(sf) => {
                self.criteria.size_filter = sf;
                self.rerun_with_filters()
            }
            Target::Date(df) => {
                self.criteria.date_filter = df;
                self.rerun_with_filters()
            }
            Target::MatchCase => {
                self.criteria.case_sensitive = !self.criteria.case_sensitive;
                self.rerun_with_filters()
            }
            Target::Hidden => {
                self.criteria.include_hidden = !self.criteria.include_hidden;
                self.rerun_with_filters()
            }
            Target::Folders => {
                self.criteria.include_directories = !self.criteria.include_directories;
                self.rerun_with_filters()
            }
            Target::Saved(id) | Target::Recent(id) => self.rerun_saved(id),
            Target::Unsave(id) => {
                self.unbookmark_search(id);
                self.store_saved_searches();
                EventResult::Consumed
            }
            Target::Header(column) => self.sort_by(column),
            Target::Result(entry) => {
                self.select_entry(entry);
                EventResult::Consumed
            }
            Target::Open => self.open_selected(),
            Target::OpenLocation => self.open_location(),
            Target::HelpCard => {
                self.show_help = false;
                EventResult::Consumed
            }
            // Surfaces: the press is theirs, and stops there.
            Target::SearchBox | Target::Filters | Target::Results => EventResult::Consumed,
        }
    }

    /// Select the result that shows index entry `entry`.
    fn select_entry(&mut self, entry: usize) {
        if let Some(pos) = self.results.iter().position(|&r| r == entry) {
            self.selected_result = Some(pos);
            self.keep_selection_visible();
        }
    }

    /// Scroll the results so the selected one is on screen.
    fn keep_selection_visible(&mut self) {
        let Some(selected) = self.selected_result else {
            return;
        };
        let rows = Layout::of(self, self.window.0, self.window.1)
            .result_rows()
            .max(1);
        if selected < self.results_scroll {
            self.results_scroll = selected;
        } else if selected >= self.results_scroll.saturating_add(rows) {
            self.results_scroll = selected.saturating_sub(rows.saturating_sub(1));
        }
    }

    /// Open the selected result: a folder in the file manager, a file with the
    /// program the user has associated with its kind.
    ///
    /// `Enter` was listed on the F1 card as "Open what is selected" and re-ran
    /// the search instead, and the preview's Open button was drawn and wired
    /// to nothing -- so a search tool found files and could do nothing with
    /// them. Opening is also what makes a search worth remembering, so this is
    /// where it joins the recent list.
    pub fn open_selected(&mut self) -> EventResult {
        let Some(entry) = self.selected_entry() else {
            return EventResult::Ignored;
        };
        let path = std::path::PathBuf::from(&entry.path);
        let name = entry.name.clone();
        let program = if entry.is_directory {
            Some(FILE_MANAGER.to_string())
        } else {
            opener_for(&path)
        };
        self.remember_search();
        self.status_message = match program {
            Some(program) => match (self.launch)(&program, &path) {
                Ok(()) => format!("Opening {name} with {program}"),
                Err(e) => format!("Could not start {program}: {e}"),
            },
            None => format!("Nothing is set to open {name} -- File Associations can set one"),
        };
        EventResult::Consumed
    }

    /// Open the folder the selected result is in.
    pub fn open_location(&mut self) -> EventResult {
        let Some(path) = self.selected_entry().map(|e| e.path.clone()) else {
            return EventResult::Ignored;
        };
        let Some(folder) = std::path::Path::new(&path)
            .parent()
            .map(std::path::Path::to_path_buf)
        else {
            return EventResult::Ignored;
        };
        self.remember_search();
        self.status_message = match (self.launch)(FILE_MANAGER, &folder) {
            Ok(()) => format!("Opening {}", entry_folder_label(&folder)),
            Err(e) => format!("Could not start the file manager: {e}"),
        };
        EventResult::Consumed
    }

    /// Record the search in the query box as a recent one.
    ///
    /// On an open, not on every keystroke: `execute_search` used to record
    /// here, so typing "report" left six searches -- "r", "re", "rep" and on --
    /// in a history nothing ever showed. The same search again moves to the
    /// front rather than appearing twice.
    fn remember_search(&mut self) {
        if self.criteria.query.is_empty() {
            return;
        }
        let (query, mode) = (self.criteria.query.clone(), self.criteria.mode);
        let count = self.results.len();
        let now = self.criteria.current_time;
        if let Some(pos) = self
            .search_history
            .iter()
            .position(|s| s.query == query && s.mode == mode)
        {
            let mut again = self.search_history.remove(pos);
            again.result_count = count;
            again.timestamp = now;
            self.search_history.push(again);
            return;
        }
        let id = self.next_search_id;
        self.next_search_id = self.next_search_id.saturating_add(1);
        self.search_history.push(SavedSearch {
            id,
            query,
            mode,
            result_count: count,
            timestamp: now,
            is_bookmarked: false,
            name: None,
        });
        // Keep the last 50 that are not saved.
        while self
            .search_history
            .iter()
            .filter(|s| !s.is_bookmarked)
            .count()
            > 50
        {
            match self.search_history.iter().position(|s| !s.is_bookmarked) {
                Some(oldest) => {
                    self.search_history.remove(oldest);
                }
                None => break,
            }
        }
    }

    /// Whether the search in the query box is a saved one.
    fn current_search_is_saved(&self) -> bool {
        self.search_history.iter().any(|s| {
            s.is_bookmarked && s.query == self.criteria.query && s.mode == self.criteria.mode
        })
    }

    /// Save the search in the query box, or forget it if it is saved.
    ///
    /// Saved searches outlive the window -- `filesearch.yaml` -- because a
    /// saved search that is gone at the next launch was never saved.
    pub fn toggle_saved(&mut self) -> EventResult {
        if self.criteria.query.is_empty() {
            self.status_message = "Type a search to save it".to_string();
            return EventResult::Consumed;
        }
        let (query, mode) = (self.criteria.query.clone(), self.criteria.mode);
        match self
            .search_history
            .iter()
            .position(|s| s.query == query && s.mode == mode)
        {
            Some(pos) => {
                let id = self.search_history.get(pos).map_or(0, |s| s.id);
                if self.current_search_is_saved() {
                    self.unbookmark_search(id);
                } else {
                    self.bookmark_search(id, &query);
                }
            }
            None => {
                self.remember_search();
                let id = self.next_search_id.saturating_sub(1);
                self.bookmark_search(id, &query);
            }
        }
        self.store_saved_searches();
        EventResult::Consumed
    }

    /// Put a saved or recent search back in the query box and run it.
    fn rerun_saved(&mut self, id: u32) -> EventResult {
        let Some(search) = self.search_history.iter().find(|s| s.id == id) else {
            return EventResult::Ignored;
        };
        self.criteria.query = search.query.clone();
        self.criteria.mode = search.mode;
        self.execute_search();
        EventResult::Consumed
    }

    /// Read the saved searches from the user's settings.
    ///
    /// Entries that do not parse -- an unknown mode word, an empty query -- are
    /// not shown, and not invented into something else either. They are kept,
    /// verbatim, and written back with the rest: the file is the user's, and a
    /// search this version cannot run (one saved by a newer version, say) is
    /// still theirs.
    pub fn load_saved_searches(&mut self, doc: &yamldoc::Document) {
        for item in doc.get_seq(&SAVED_KEY).unwrap_or_default() {
            let parsed = item.split_once(':').and_then(|(word, query)| {
                SearchMode::from_key(word)
                    .filter(|_| !query.is_empty())
                    .map(|mode| (mode, query.to_string()))
            });
            let Some((mode, query)) = parsed else {
                self.unreadable_saved.push(item);
                continue;
            };
            let id = self.next_search_id;
            self.next_search_id = self.next_search_id.saturating_add(1);
            self.search_history.push(SavedSearch {
                id,
                name: Some(query.clone()),
                query,
                mode,
                result_count: 0,
                timestamp: 0,
                is_bookmarked: true,
            });
        }
    }

    /// The saved searches as the settings file records them.
    fn saved_search_items(&self) -> Vec<String> {
        self.search_history
            .iter()
            .filter(|s| s.is_bookmarked)
            .map(|s| format!("{}:{}", s.mode.key(), s.query))
            .chain(self.unreadable_saved.iter().cloned())
            .collect()
    }

    /// Write the saved searches to the user's settings, saying so if that
    /// fails: a save that silently did not happen is found out at the next
    /// launch, when the search is gone.
    fn store_saved_searches(&mut self) {
        let items = self.saved_search_items();
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        let mut doc = settingsfile::load(CONFIG_NAME);
        doc.set_seq(&SAVED_KEY, &refs);
        if let Err(e) = settingsfile::store(CONFIG_NAME, &doc) {
            self.status_message = format!("Could not save your saved searches: {e}");
        }
    }

    /// The folder the index holds, for the header.
    fn root_label(&self) -> String {
        match &self.root {
            Some(root) => format!("in {}", entry_folder_label(root)),
            None => "No folder chosen yet".to_string(),
        }
    }
}

impl Target {
    /// Whether this target is part of the filters panel, which the wheel
    /// scrolls as one.
    fn is_in_filters(self) -> bool {
        matches!(
            self,
            Self::Filters
                | Self::CategoryAll
                | Self::Category(_)
                | Self::Size(_)
                | Self::Date(_)
                | Self::MatchMode
                | Self::MatchCase
                | Self::Hidden
                | Self::Folders
                | Self::Saved(_)
                | Self::Unsave(_)
                | Self::Recent(_)
        )
    }
}

impl SearchMode {
    /// The next mode round the ring Ctrl+R steps through.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Substring => Self::Glob,
            Self::Glob => Self::Regex,
            Self::Regex => Self::Content,
            Self::Content => Self::Substring,
        }
    }
}

/// A folder, for a message or a label: its path as the system would print
/// it. Display only -- nothing is ever looked up by this string.
fn entry_folder_label(folder: &std::path::Path) -> String {
    folder.display().to_string()
}

/// The program the user has chosen for this kind of file, read from the File
/// Associations program's configuration -- the same lookup the file manager
/// makes, so a file opens with the same program from either.
///
/// Read on every open rather than cached, for the file manager's reason: an
/// association changed in another window must be the one used next.
fn opener_for(path: &std::path::Path) -> Option<String> {
    // `to_str` rather than bytes: the associations are keys in a YAML
    // document, so they are text by construction and an extension that is not
    // UTF-8 could never match one. `None` here is a refusal, not a lossy
    // conversion.
    let ext = path.extension().and_then(|e| e.to_str())?;
    let doc = settingsfile::load(associations::CONFIG_NAME);
    associations::program_for(&doc, ext)
}

// ─── Formatting Helpers ──────────────────────────────────────────────

#[must_use]
pub fn format_size(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

/// How long ago a file was modified or created, as a label.
///
/// This ladder was the most complete of the five the audit found — it is the
/// one `guitk::duration::relative` was built from — but it capitalised
/// `"Just now"` and had no `"yesterday"`, so one age read differently here
/// than in the clipboard and notification panes.
#[must_use]
pub fn format_relative_time(seconds: u64) -> String {
    guitk::duration::relative(seconds)
}

// ─── Main ────────────────────────────────────────────────────────────

impl App for FileSearchApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        if self.criteria.query.is_empty() {
            "File Search".to_owned()
        } else {
            format!(
                "File Search — {} ({} result{})",
                self.criteria.query,
                self.results.len(),
                if self.results.len() == 1 { "" } else { "s" }
            )
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH, WINDOW_HEIGHT)
    }

    /// A clock only while a content search is reading, to take in what it
    /// finds.
    ///
    /// Nothing else here changes on its own: nothing watches the filesystem,
    /// so an index is a snapshot until the next folder is chosen. When a real
    /// indexer exists -- `userspace/indexer` and `apps/indexer` are both about
    /// this -- a tick would re-run the query against a changed index. See
    /// known-issues.md -> TD-C-SEVERAL-APPS-DISPLAY-DATA-THAT-NOTHING-PRODUCES.
    /// (Polling, because the worker cannot wake the window: see
    /// `requests/e-f-wake-an-application-for-its-own-descriptor.md`.)
    fn tick_interval(&self) -> Option<Duration> {
        self.content
            .as_ref()
            .map(|_| Duration::from_millis(CONTENT_POLL_MS))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        if let Event::Tick { .. } = event {
            return if self.pump_content_search() {
                Response::Redraw
            } else {
                Response::Idle
            };
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.window = (width, height);
        let frame = self.frame(width, height);
        // Kept for the pointer's movement and the wheel; see `target_at`.
        self.last_hits = frame.hits().to_vec();
        let mut commands = frame.into_tree().commands;
        // Last, so it is on top -- the same order in which `handle_event`
        // gives it the click.
        commands.extend(self.picker.render(&self.palette, width, height));
        RenderTree { commands }
    }
}

/// The size the window opens at.
///
/// The layout is computed from whatever size `render` is handed, so these are
/// only the opening request rather than an assumption the drawing depends on.
/// The window size the picker is laid out against.
///
/// The dialog needs the *current* size to place itself, and this app does not
/// track resizes -- `Event::Resize` only recomputes the row count. These two
/// return the size the window opens at, which is right until the user drags a
/// corner; `known-issues.md` carries the rest.
#[must_use]
fn window_width() -> f32 {
    f32::from(u16::try_from(WINDOW_WIDTH).unwrap_or(u16::MAX))
}

#[must_use]
fn window_height() -> f32 {
    f32::from(u16::try_from(WINDOW_HEIGHT).unwrap_or(u16::MAX))
}

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 800;

/// How often, while a content search is reading, the window takes in what it
/// has found: often enough that results appear as they are found, rarely
/// enough that the worker is doing the work and not the window.
const CONTENT_POLL_MS: u64 = 50;

fn main() -> ExitCode {
    // Starts empty. It used to call `populate_sample_index`, under a comment
    // saying "until a real index exists this is what there is to search" --
    // which was true, and the missing piece turned out to be nearer than the
    // `indexer` service that comment pointed at. Ctrl+O reads a real folder.
    let mut app = FileSearchApp::new();
    app.load_saved_searches(&settingsfile::load(CONFIG_NAME));
    app.status_message = String::from("Choose a folder to search: the button above, or Ctrl+O");
    app::launch("filesearch", &mut app)
}

/// The most entries one pass will index.
///
/// A whole disk walked in a single frame would hang the window. The bound is
/// reported when it is reached, so a truncated index is never mistaken for a
/// complete one -- which matters more here than in most places, because "no
/// results" from a search tool reads as "no such file".
/// Representative entries, for tests only.
///
/// `#[cfg(test)]` since 2026-09-15. `main` called it, so the search ran
/// against records written into the source -- and since the application had
/// no way to reach a real file, those records were the only thing it could
/// ever search. A fixture production can reach is a fixture that ships.
#[cfg(test)]
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

pub const MAX_INDEXED: usize = 20_000;

/// What to say about an index pass. Separated so a test can read it.
#[must_use]
pub fn describe_index_pass(root: &str, indexed: usize, skipped: usize, truncated: bool) -> String {
    let mut out = format!("Indexed {indexed} entries from {root}");
    if truncated {
        out.push_str(&format!(" (stopped at the {MAX_INDEXED} limit)"));
    }
    if skipped > 0 {
        out.push_str(&format!(
            " -- {skipped} skipped: their names are not text and no pattern can match them"
        ));
    }
    out
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

    // ------------------------------------------------------------------
    // Events
    //
    // The app had no input handling at all until it was wired to the
    // compositor: the query was set by a caller assigning the field.
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;

    fn indexed() -> FileSearchApp {
        let mut app = FileSearchApp::new();
        populate_sample_index(&mut app.index);
        app.execute_search();
        app
    }

    /// Every chip the three filter strips draw is one the search can be set
    /// to, and every choice the matcher understands has a chip.
    ///
    /// The strips built their own arrays and were *subsets* of the enums they
    /// drew from: nine of eleven file types, five of seven date ranges. So
    /// `Font` and `Database` were categories every file was sorted into and
    /// no chip ever offered, and `Yesterday` and `Older` were ranges
    /// `DateFilter::matches` implemented for nobody.
    #[test]
    fn the_strips_draw_every_choice_the_matcher_understands() {
        let mut app = indexed();
        app.show_filters = true;
        let texts = drawn(&app);

        for cat in FileCategory::ALL {
            assert!(
                texts.iter().any(|t| t.contains(&cat.to_string())),
                "the File Type strip never offers {cat}"
            );
        }
        for df in DateFilter::ALL {
            assert!(
                texts.iter().any(|t| t == df.label()),
                "the Modified strip never offers {}",
                df.label()
            );
        }
        for sf in SizeFilter::CHIPS {
            assert!(
                texts.iter().any(|t| t == sf.label()),
                "the Size strip never offers {}",
                sf.label()
            );
        }
        assert!(
            texts.iter().any(|t| t == "All Types"),
            "nothing offers a way back out of a file-type filter"
        );
    }

    /// Ctrl+1/2/3 step their strip, and stepping right round returns to the
    /// start -- so every chip is reachable and none is a one-way door.
    #[test]
    fn the_filter_keys_reach_every_chip_and_come_back() {
        let mut app = indexed();
        app.show_filters = true;

        let mut seen = vec![app.criteria.category_filter];
        for _ in 0..FileCategory::ALL.len() {
            assert_eq!(
                app.handle_event(&press_ctrl(Key::Num1)),
                EventResult::Consumed,
                "Ctrl+1 was ignored"
            );
            seen.push(app.criteria.category_filter);
        }
        for cat in FileCategory::ALL {
            assert!(
                seen.contains(&Some(cat)),
                "stepping the File Type strip never reached {cat}"
            );
        }
        app.handle_event(&press_ctrl(Key::Num1));
        assert_eq!(
            app.criteria.category_filter, None,
            "the File Type strip does not come back round to no filter"
        );

        let mut dates = vec![app.criteria.date_filter];
        for _ in 1..DateFilter::ALL.len() {
            app.handle_event(&press_ctrl(Key::Num3));
            dates.push(app.criteria.date_filter);
        }
        for df in DateFilter::ALL {
            assert!(
                dates.contains(&df),
                "stepping the Modified strip never reached {}",
                df.label()
            );
        }

        let mut sizes = vec![app.criteria.size_filter];
        for _ in 1..SizeFilter::CHIPS.len() {
            app.handle_event(&press_ctrl(Key::Num2));
            sizes.push(app.criteria.size_filter);
        }
        for sf in SizeFilter::CHIPS {
            assert!(
                sizes.contains(&sf),
                "stepping the Size strip never reached {}",
                sf.label()
            );
        }
    }

    /// Shift steps the other way, or a strip of eleven is a long walk back.
    #[test]
    fn shift_steps_a_strip_backwards() {
        let mut app = indexed();
        app.handle_event(&press_ctrl(Key::Num1));
        let forward = app.criteria.category_filter;
        assert!(forward.is_some(), "control: Ctrl+1 set no filter");
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::Num1,
            pressed: true,
            modifiers: Modifiers {
                ctrl: true,
                shift: true,
                ..Modifiers::NONE
            },
            text: String::new(),
        }));
        assert_eq!(
            app.criteria.category_filter, None,
            "Ctrl+Shift+1 did not step back to where Ctrl+1 came from"
        );
    }

    /// The filter reaches the results, not just the field.
    #[test]
    fn a_file_type_filter_narrows_what_the_search_returns() {
        let mut app = indexed();
        let all = app.results.len();
        assert!(all > 0, "control: the sample index found nothing");

        while app.criteria.category_filter != Some(FileCategory::Image) {
            app.handle_event(&press_ctrl(Key::Num1));
        }
        assert!(
            app.results.len() < all,
            "filtering to Image left all {all} results in place"
        );
        assert!(
            app.results
                .iter()
                .filter_map(|i| app.index.entries.get(*i))
                .all(|e| e.category == FileCategory::Image),
            "an Image filter returned something that is not an image"
        );
    }

    /// The four search options answer their keys and the panel says so.
    #[test]
    fn the_search_options_answer_their_keys() {
        /// A key, the row it changes, and how to read that row's value.
        type OptionCheck = (Key, &'static str, fn(&FileSearchApp) -> String);
        let checks: [OptionCheck; 4] = [
            (Key::R, "Match by (Ctrl+R): ", |a| {
                a.criteria.mode.to_string()
            }),
            (Key::U, "Case sensitive (Ctrl+U): ", |a| {
                a.criteria.case_sensitive.to_string()
            }),
            (Key::H, "Hidden files (Ctrl+H): ", |a| {
                a.criteria.include_hidden.to_string()
            }),
            (Key::K, "Folders (Ctrl+K): ", |a| {
                a.criteria.include_directories.to_string()
            }),
        ];
        for (key, label, read) in checks {
            let mut app = indexed();
            app.show_filters = true;
            let before = read(&app);
            let row_before = drawn(&app)
                .into_iter()
                .find(|t| t.starts_with(label))
                .unwrap_or_else(|| panic!("the panel draws no row starting {label:?}"));

            assert_eq!(
                app.handle_event(&press_ctrl(key)),
                EventResult::Consumed,
                "{label} ignored its key"
            );
            assert_ne!(read(&app), before, "{label} did not change");

            let row_after = drawn(&app)
                .into_iter()
                .find(|t| t.starts_with(label))
                .unwrap_or_else(|| panic!("the row starting {label:?} vanished"));
            assert_ne!(
                row_before, row_after,
                "{label} changed and the panel still reads {row_before:?}"
            );
        }
    }

    /// Hidden files are found only when asked for -- the effect, not the flag.
    #[test]
    fn hidden_files_are_found_only_when_asked_for() {
        let mut app = indexed();
        let hidden_visible = |a: &FileSearchApp| {
            a.results
                .iter()
                .filter_map(|i| a.index.entries.get(*i))
                .any(|e| e.is_hidden)
        };
        assert!(
            app.index.entries.iter().any(|e| e.is_hidden),
            "control: the sample index has no hidden entry to find"
        );
        assert!(
            !hidden_visible(&app),
            "a hidden file was in the results before Ctrl+H"
        );
        app.handle_event(&press_ctrl(Key::H));
        assert!(
            hidden_visible(&app),
            "Ctrl+H did not bring the hidden files into the results"
        );
    }

    fn drawn(app: &FileSearchApp) -> Vec<String> {
        app.render_commands(1280.0, 800.0)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn press_ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        })
    }

    /// Every string the window draws, joined. (The other `drawn` in this
    /// module returns a `Vec`; this one joins for `contains`.)
    fn card_text(app: &FileSearchApp) -> String {
        app.render_commands(1200.0, 800.0)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// **Every key the list advertises is one this program answers.**
    ///
    /// A query typed and results present, because `Up`, `Down`, `Home`, `End`,
    /// `Enter` and `Backspace` all act on one or the other and are correctly
    /// refused without them.
    #[test]
    fn every_advertised_key_does_something() {
        // Scratch settings: Ctrl+D saves a search, which writes them.
        settingsfile::testing::with_scratch_config("fs_advertised", |_| {
            for (label, what) in SHORTCUTS {
                for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                    let mut app = FileSearchApp::new();
                    app.launch = |_, _| Ok(());
                    populate_sample_index(&mut app.index);
                    app.handle_event(&typed('a'));
                    // Results to step through, over real entries: Up, Down,
                    // Home, End and Enter all act on a selection, and Enter
                    // opens what it names.
                    app.results = vec![0, 1, 2];
                    app.selected_result = Some(1);
                    assert_eq!(
                        app.handle_event(&Event::Key(stroke.clone())),
                        EventResult::Consumed,
                        "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                        stroke.key
                    );
                }
            }
        });
    }

    /// **The shortcut list reaches the window, and nothing acts behind it.**
    ///
    /// The control is the half that matters: `Ctrl+F` behind the card must not
    /// toggle the filter panel, and asserting only that it does not would pass
    /// on an app that had lost `Ctrl+F` altogether.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = FileSearchApp::new();
        assert!(
            !card_text(&app).contains("F1 closes this"),
            "the list is up before anybody asked for it"
        );

        app.handle_event(&press(Key::F1));
        let shown = card_text(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        let filters = app.show_filters;
        app.handle_event(&press_ctrl(Key::F));
        assert_eq!(
            app.show_filters, filters,
            "Ctrl+F toggled the filters through the shortcut card"
        );

        app.handle_event(&press(Key::Escape));
        assert!(
            !card_text(&app).contains("F1 closes this"),
            "Escape did not close it"
        );

        app.handle_event(&press_ctrl(Key::F));
        assert_ne!(
            app.show_filters, filters,
            "control: Ctrl+F does nothing even with the card down"
        );
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        })
    }

    /// An open picker takes the keyboard, and the window behind it does not.
    ///
    /// The open test asserts that the KEY HANDLER opened the dialog, which
    /// holds whether or not the picker is ever handed another event. This is
    /// the half routing decides: with a dialog up, a keystroke belongs to the
    /// dialog.
    ///
    /// Found by `scripts/find-unpinned-picker-routing.py`, which cuts the
    /// routing and reports whose tests notice. Sixteen of twenty did not.
    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_results() {
        let mut app = indexed();
        app.selected_result = Some(0);
        let before = app.selected_result;
        assert!(
            app.results.len() > 1,
            "control: the fixture needs more than one result to move between"
        );

        app.handle_event(&press_ctrl(Key::O));
        assert!(app.picker.is_open(), "control: the picker must be up");

        app.handle_event(&press(Key::Down));
        assert_eq!(
            app.selected_result, before,
            "Down at the open dialog moved the selection behind it"
        );
    }

    #[test]
    fn typing_searches_without_a_keystroke_to_get_started() {
        // A search program that needs focus moved to its own query box before
        // it will accept a query is a search program with an extra step.
        let mut app = indexed();
        let all = app.results.len();
        assert!(all > 0, "the sample index should match an empty query");
        for c in "config".chars() {
            assert_eq!(app.handle_event(&typed(c)), EventResult::Consumed);
        }
        assert_eq!(app.criteria.query, "config");
        assert!(
            app.results.len() < all,
            "a query should narrow the results ({} of {all})",
            app.results.len()
        );
        assert!(
            !app.results.is_empty(),
            "the sample index has a config file"
        );
    }

    #[test]
    fn backspace_gives_the_results_back_and_stops_at_an_empty_query() {
        let mut app = indexed();
        let all = app.results.len();
        for c in "config".chars() {
            let _ = app.handle_event(&typed(c));
        }
        for _ in 0.."config".len() {
            let _ = app.handle_event(&press(Key::Backspace));
        }
        assert_eq!(app.criteria.query, "");
        assert_eq!(app.results.len(), all);
        assert_eq!(
            app.handle_event(&press(Key::Backspace)),
            EventResult::Ignored,
            "backspace on an empty query is not a redraw"
        );
    }

    #[test]
    fn escape_clears_the_query_and_does_nothing_when_it_is_already_clear() {
        let mut app = indexed();
        assert_eq!(app.handle_event(&press(Key::Escape)), EventResult::Ignored);
        let _ = app.handle_event(&typed('c'));
        assert_eq!(app.handle_event(&press(Key::Escape)), EventResult::Consumed);
        assert!(app.criteria.query.is_empty());
    }

    #[test]
    fn a_ctrl_shortcut_is_not_typed_into_the_query() {
        // Every bare printable key is query text, so the shortcuts have to be
        // on Ctrl — and Ctrl+S must not put an "s" in the box.
        let mut app = indexed();
        assert_eq!(app.handle_event(&press_ctrl(Key::S)), EventResult::Consumed);
        assert_eq!(app.criteria.query, "", "a shortcut leaked into the query");
        assert_eq!(app.sort_column, SortColumn::Size);
    }

    #[test]
    fn a_ctrl_chord_that_carries_text_is_still_not_query_text() {
        // A `press_ctrl` built here has empty `text`, which is not the only
        // way a Ctrl chord arrives: a keyboard layer that fills in a control
        // character sends one with text, and *that* is the case the `|| ctrl`
        // guard in the fallback arm exists for. Without it this key would be
        // appended to the query.
        let mut app = indexed();
        let chord = Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: "\u{13}".to_owned(), // Ctrl+S as a control character
        });
        assert_eq!(app.handle_event(&chord), EventResult::Ignored);
        assert_eq!(
            app.criteria.query, "",
            "a Ctrl chord control character was typed into the query"
        );
    }

    #[test]
    fn sorting_by_the_same_column_twice_reverses_it() {
        // Which is what a column header does everywhere else.
        let mut app = indexed();
        let _ = app.handle_event(&press_ctrl(Key::N));
        assert_eq!(app.sort_column, SortColumn::Name);
        let first = app.sort_ascending;
        let _ = app.handle_event(&press_ctrl(Key::N));
        assert_eq!(app.sort_column, SortColumn::Name);
        assert_ne!(
            app.sort_ascending, first,
            "the second press did not reverse"
        );
        // A different column starts ascending again rather than inheriting.
        let _ = app.handle_event(&press_ctrl(Key::S));
        assert_eq!(app.sort_column, SortColumn::Size);
        assert!(app.sort_ascending, "a new column should start ascending");
    }

    #[test]
    fn sorting_keeps_the_selection_on_the_same_file() {
        // The selection is an index into the result list, and sorting rebuilds
        // that list — so an index kept across a sort points at a different
        // file, which is the same defect fixed in finance and reminders.
        let mut app = indexed();
        let _ = app.handle_event(&press(Key::Down));
        let _ = app.handle_event(&press(Key::Down));
        let before = app
            .selected_result
            .and_then(|i| app.results.get(i).copied())
            .expect("something is selected");
        let _ = app.handle_event(&press_ctrl(Key::S));
        let after = app
            .selected_result
            .and_then(|i| app.results.get(i).copied());
        assert_eq!(
            after,
            Some(before),
            "sorting moved the selection to another file"
        );
    }

    #[test]
    fn the_arrows_walk_the_results_and_stop_at_the_ends() {
        let mut app = indexed();
        assert!(app.results.len() >= 3);
        assert_eq!(app.selected_result, None);
        // The first press lands on the first row rather than the second.
        assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Consumed);
        assert_eq!(app.selected_result, Some(0));
        assert_eq!(
            app.handle_event(&press(Key::Up)),
            EventResult::Ignored,
            "Up at the top should stay put"
        );
        let _ = app.handle_event(&press(Key::End));
        assert_eq!(app.selected_result, app.results.len().checked_sub(1));
        assert_eq!(
            app.handle_event(&press(Key::Down)),
            EventResult::Ignored,
            "Down at the bottom should stay put"
        );
        let _ = app.handle_event(&press(Key::Home));
        assert_eq!(app.selected_result, Some(0));
    }

    #[test]
    fn a_query_that_matches_nothing_leaves_nothing_selected() {
        let mut app = indexed();
        let _ = app.handle_event(&press(Key::Down));
        assert!(app.selected_result.is_some());
        for c in "zzzz-no-such-file".chars() {
            let _ = app.handle_event(&typed(c));
        }
        assert!(app.results.is_empty(), "that query should match nothing");
        // Set deliberately, because the search may or may not have cleared
        // it and the point here is what the *arrow* does with an empty list.
        app.selected_result = Some(3);
        assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Consumed);
        assert_eq!(
            app.selected_result, None,
            "a selection into an empty result list points at nothing"
        );
        // And once it is gone, moving again is not a redraw.
        assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Ignored);
    }

    #[test]
    fn the_panel_toggles_flip_and_do_not_touch_the_query() {
        let mut app = indexed();
        let (filters, preview) = (app.show_filters, app.show_preview);
        let _ = app.handle_event(&press_ctrl(Key::F));
        assert_ne!(app.show_filters, filters);
        assert_eq!(app.show_preview, preview, "Ctrl+F touched the preview");
        let _ = app.handle_event(&press_ctrl(Key::P));
        assert_ne!(app.show_preview, preview);
        assert_eq!(app.criteria.query, "");
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = indexed();
        let release = Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: false,
            modifiers: Modifiers::NONE,
            text: "x".to_owned(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(app.criteria.query, "");
    }

    #[test]
    fn the_title_reports_the_query_and_how_many_it_found() {
        // A minimised search window is a taskbar entry and nothing else.
        let mut app = indexed();
        assert_eq!(app.title(), "File Search");
        for c in "config".chars() {
            let _ = app.handle_event(&typed(c));
        }
        let title = app.title();
        assert!(title.contains("config"), "title {title:?} omits the query");
        assert!(
            title.contains(&app.results.len().to_string()),
            "title {title:?} omits the result count"
        );
    }

    #[test]
    fn rendering_draws_something_at_an_awkward_size() {
        let mut app = indexed();
        for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
            assert!(
                !app.render(w, h).commands.is_empty(),
                "drew nothing at {w}x{h}"
            );
        }
    }
    use super::*;
    /// The picker is not merely open: it is DRAWN.
    ///
    /// `is_open()` returning true is not the same claim, and assuming it was
    /// is how `apps/flashcards` shipped a dialog that took every keystroke and
    /// painted nothing. Deleting the `picker.render` line in the renderer
    /// leaves `is_open()` true and every other test green; this is the one
    /// that notices.
    #[test]
    fn the_picker_is_drawn_when_it_is_open() {
        // `render`, not `render_commands`: this app draws the picker in the
        // App-trait method, over the top of what `render_commands` produced.
        // The first version of this test called `render_commands` -- written
        // by analogy with the seven other apps, where the picker IS in that
        // function -- and failed against correct code. The render path is
        // per-app and has to be read off the app.
        let mut app = FileSearchApp::new();
        let before = app.render(1024.0, 768.0).commands.len();
        app.open_folder_dialog();
        assert!(app.picker.is_open(), "no picker came up");
        let after = app.render(1024.0, 768.0).commands.len();
        let own = app.picker.render(&app.palette, 1024.0, 768.0).len();
        assert!(
            own > 0,
            "the picker itself draws nothing, so this proves nothing"
        );
        assert!(
            after >= before + own,
            "the frame does not contain the picker's own {own} command(s) ({before} before, {after} after) -- something else grew instead"
        );
    }

    // -- The directory walk --
    //
    // Added with the walk itself rather than after it. The rewrite that gave
    // `apps/sysinfo` its reader deleted three tests and added none, and for one
    // commit that file had fewer tests and more untested code; this is that
    // lesson applied on the same day.

    /// A scratch directory unique to one test, removed when it is done.
    ///
    /// Unique per call because the suite runs in parallel: a fixed name let two
    /// tests delete each other's files in `apps/benchmark` earlier today, and
    /// there the symptom was a measurement that silently came back as nothing.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "slateos-filesearch-{tag}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Self(dir)
        }

        fn file(&self, name: &str, contents: &str) {
            std::fs::write(self.0.join(name), contents).expect("scratch file");
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Best effort: a leaked scratch directory is untidy, and panicking
            // in a `Drop` during an already-failing test would hide the real
            // failure behind an abort.
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// Files on disk become entries that the search can find.
    ///
    /// The whole point of the change: before it, `index.add` was called only by
    /// tests and by `populate_sample_index`, so no file the user had could ever
    /// appear in a result.
    #[test]
    fn a_folder_on_disk_becomes_something_searchable() {
        let scratch = Scratch::new("basic");
        scratch.file("alpha.txt", "one");
        scratch.file("beta.rs", "two");
        scratch.file("gamma.txt", "three");

        let mut app = FileSearchApp::new();
        app.index_directory(&scratch.0);

        assert_eq!(app.index.entries.len(), 3, "{}", app.status_message);

        app.criteria = SearchCriteria::new("alpha");
        app.execute_search();
        assert_eq!(app.results.len(), 1, "searching for alpha found nothing");

        let found = app
            .index
            .entries
            .get(*app.results.first().expect("a result"))
            .expect("an entry");
        assert_eq!(found.name, "alpha.txt");
        assert!(found.size > 0, "the size did not come from the file");
    }

    /// A glob reaches the real names, not just the literal ones.
    #[test]
    fn a_glob_matches_what_is_on_disk() {
        let scratch = Scratch::new("glob");
        scratch.file("alpha.txt", "one");
        scratch.file("beta.rs", "two");
        scratch.file("gamma.txt", "three");

        let mut app = FileSearchApp::new();
        app.index_directory(&scratch.0);
        let hits = app.index.search_glob("*.txt");
        assert_eq!(hits.len(), 2, "glob found {} of 2", hits.len());
    }

    /// Indexing a second folder replaces the first rather than adding to it.
    ///
    /// A search tool reporting a file from a folder you are no longer looking
    /// at is worse than one reporting nothing: the path looks real, because it
    /// is, and nothing on screen says it is from somewhere else.
    #[test]
    fn a_second_folder_replaces_the_first() {
        let first = Scratch::new("first");
        first.file("only-in-first.txt", "x");
        let second = Scratch::new("second");
        second.file("only-in-second.txt", "y");

        let mut app = FileSearchApp::new();
        app.index_directory(&first.0);
        app.index_directory(&second.0);

        let names: Vec<&str> = app.index.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"only-in-second.txt"),
            "the second folder was not indexed: {names:?}"
        );
        assert!(
            !names.contains(&"only-in-first.txt"),
            "the first folder's files survived: {names:?}"
        );
    }

    /// An unreadable folder reports rather than looking like an empty one.
    #[test]
    fn a_folder_that_is_not_there_is_reported_as_zero_rather_than_ignored() {
        let mut app = FileSearchApp::new();
        let missing = std::env::temp_dir().join("slateos-filesearch-no-such-dir");
        app.index_directory(&missing);
        assert!(app.index.entries.is_empty());
        assert!(
            app.status_message.contains("0 entries"),
            "said {:?}",
            app.status_message
        );
    }

    /// The status line says when the index was truncated.
    ///
    /// "No results" from a search tool reads as "no such file", so a cap that
    /// did not announce itself would turn a partial index into a confident
    /// wrong answer.
    #[test]
    fn a_truncated_index_says_so() {
        let quiet = describe_index_pass("/tmp/x", 10, 0, false);
        assert!(!quiet.contains("limit"), "said {quiet:?}");

        let capped = describe_index_pass("/tmp/x", MAX_INDEXED, 0, true);
        assert!(
            capped.contains(&MAX_INDEXED.to_string()),
            "a truncated pass did not name the limit: {capped:?}"
        );
    }

    /// Names that are not text are counted, not hidden.
    ///
    /// They cannot be matched -- `globmatch::glob_match` takes `&str` -- so
    /// they are skipped, and a file search that silently omits files is worse
    /// than one that says it did.
    #[test]
    fn skipped_names_are_reported() {
        let silent = describe_index_pass("/tmp/x", 5, 0, false);
        assert!(!silent.contains("skipped"), "said {silent:?}");

        let noisy = describe_index_pass("/tmp/x", 5, 2, false);
        assert!(
            noisy.contains('2') && noisy.contains("not text"),
            "the skipped count is not explained: {noisy:?}"
        );
    }

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

    /// **A search is remembered when something is opened from it -- not on
    /// every keystroke.** Every search used to be recorded as it ran, so
    /// typing "report" left "r", "re", "rep" and on in the history.
    #[test]
    fn a_search_is_remembered_when_it_is_used_not_as_it_is_typed() {
        let mut app = FileSearchApp::new();
        app.launch = |_, _| Ok(());
        populate_sample_index(&mut app.index);
        for c in "report".chars() {
            app.handle_event(&typed(c));
        }
        assert!(app.search_history.is_empty(), "keystrokes were recorded");
        app.handle_event(&Event::Key(guitk::probe::press(Key::Enter)));
        app.handle_event(&Event::Key(guitk::probe::press(Key::Enter)));
        assert_eq!(
            app.search_history.len(),
            1,
            "opening a result recorded nothing"
        );
        assert_eq!(app.search_history[0].query, "report");
        // The same search again moves; it is not listed twice.
        app.handle_event(&Event::Key(guitk::probe::press(Key::Enter)));
        assert_eq!(app.search_history.len(), 1);
    }

    #[test]
    fn test_app_bookmark() {
        let mut app = FileSearchApp::new();
        populate_sample_index(&mut app.index);
        app.criteria.query = "test".to_string();
        app.execute_search();
        app.remember_search();
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
        // Lowercase now: the clipboard and notification panes both said
        // "just now" for the same age, and this was the only capital.
        assert_eq!(format_relative_time(30), "just now");
        assert_eq!(format_relative_time(3600), "1h ago");
        // Was "1d ago". A day ago is yesterday, which is what every other
        // ladder in the system already called it.
        assert_eq!(format_relative_time(86400), "yesterday");
        assert_eq!(format_relative_time(172_800), "2d ago");
    }

    #[test]
    fn test_render_produces_commands() {
        let mut app = FileSearchApp::new();
        populate_sample_index(&mut app.index);
        let cmds = app.render_commands(1280.0, 800.0);
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

    /// The results panel alone, drawn 900 by 400 at the origin.
    fn results_commands(app: &FileSearchApp) -> Vec<RenderCommand> {
        let mut f = Frame::new(900.0, 400.0);
        app.draw_results(&mut f, Rect::new(0.0, 0.0, 900.0, 400.0));
        f.commands().to_vec()
    }

    #[test]
    fn no_result_cell_escapes_its_column() {
        let app = app_with_a_shouting_result();
        // Render the results panel directly: a whole-app render puts sidebar
        // and search-bar text at x values that fall inside a column's range,
        // and the assertion would then fail on chrome that is not in the table.
        let cmds = results_commands(&app);

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
        let cmds = results_commands(&app);
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
        let cmds = results_commands(&app);
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

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        use guitk::Color;

        fn fills(app: &mut FileSearchApp) -> Vec<Color> {
            app.render(900.0, 650.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = FileSearchApp::new();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // ------------------------------------------------------------------
    // The pointer
    //
    // Driven through `handle_event`, at the point the renderer says it drew
    // the control -- see `guitk::probe`.
    // ------------------------------------------------------------------

    use guitk::probe::{self, Probe};

    impl Probe for FileSearchApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (1280.0, 800.0);

        fn draw(&self, size: (f32, f32)) -> Frame<Target> {
            self.frame(size.0, size.1)
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            size: (f32, f32),
        ) -> EventResult {
            self.window = size;
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
            self.window = size;
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, size: (f32, f32)) -> Option<EventResult> {
            self.window = size;
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    thread_local! {
        /// What the test launcher was asked to start, in order.
        static LAUNCHED: std::cell::RefCell<Vec<(String, std::path::PathBuf)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    /// A launcher that starts nothing and writes down what it was asked.
    ///
    /// Returns a `Result` because it stands in for `FileSearchApp::launch`,
    /// whose type that is; the real launcher can fail.
    #[allow(
        clippy::unnecessary_wraps,
        reason = "the signature is the launch field's, which the real spawn fills"
    )]
    fn record_launch(program: &str, path: &std::path::Path) -> std::io::Result<()> {
        LAUNCHED.with(|l| {
            l.borrow_mut()
                .push((program.to_string(), path.to_path_buf()));
        });
        Ok(())
    }

    /// The sample index, searched for everything, with a launcher that starts
    /// nothing.
    fn wired() -> FileSearchApp {
        let mut app = FileSearchApp::new();
        app.launch = record_launch;
        LAUNCHED.with(|l| l.borrow_mut().clear());
        populate_sample_index(&mut app.index);
        app.criteria.current_time = 1_779_000_000;
        app.execute_search();
        app
    }

    /// Scroll the filters panel with the wheel until `target` is on screen,
    /// as a person would to reach a row below the window's edge.
    fn reveal(app: &mut FileSearchApp, target: Target) {
        for _ in 0..60 {
            if probe::is_visible(app, target) {
                return;
            }
            probe::scroll_at_point(app, Target::Filters, -1.0);
        }
        panic!("{target:?} could not be scrolled into view");
    }

    /// **Every chip and switch in the filters panel answers a press**, and
    /// does what its key does. They were drawn with selection highlights only
    /// the keyboard could move; the four Search rows were labels.
    #[test]
    fn every_filter_answers_the_pointer() {
        let mut app = wired();
        probe::click(&mut app, Target::Category(FileCategory::Image));
        assert_eq!(app.criteria.category_filter, Some(FileCategory::Image));
        assert!(!app.results.is_empty());
        assert!(
            app.results
                .iter()
                .all(|&i| app.index.entries[i].category == FileCategory::Image)
        );
        // The lit chip, pressed again, is the way back out.
        probe::click(&mut app, Target::Category(FileCategory::Image));
        assert_eq!(app.criteria.category_filter, None);
        probe::click(&mut app, Target::Category(FileCategory::Code));
        probe::click(&mut app, Target::CategoryAll);
        assert_eq!(app.criteria.category_filter, None);

        probe::click(&mut app, Target::Size(SizeFilter::Tiny));
        assert_eq!(app.criteria.size_filter, SizeFilter::Tiny);
        probe::click(&mut app, Target::Date(DateFilter::Today));
        assert_eq!(app.criteria.date_filter, DateFilter::Today);

        let mode = app.criteria.mode;
        // The last of the four Search rows; the other three are above it.
        reveal(&mut app, Target::Folders);
        probe::click(&mut app, Target::MatchMode);
        assert_eq!(app.criteria.mode, mode.next());
        probe::click(&mut app, Target::SearchMode);
        assert_eq!(
            app.criteria.mode,
            mode.next().next(),
            "the mode chip is not a switch"
        );
        probe::click(&mut app, Target::MatchCase);
        assert!(app.criteria.case_sensitive);
        probe::click(&mut app, Target::Hidden);
        assert!(app.criteria.include_hidden);
        let folders = app.criteria.include_directories;
        probe::click(&mut app, Target::Folders);
        assert_eq!(app.criteria.include_directories, !folders);
    }

    /// The filters panel is taller than an 800-pixel window, so it scrolls,
    /// and a row scrolled out of it cannot be pressed.
    #[test]
    fn the_filters_panel_scrolls_to_its_last_row() {
        let mut app = wired();
        assert!(
            !probe::is_visible(&app, Target::Folders),
            "the whole panel fits; this test needs a window it overflows"
        );
        for _ in 0..40 {
            probe::scroll_at_point(&mut app, Target::Filters, -1.0);
        }
        assert!(
            probe::is_visible(&app, Target::Folders),
            "the last row is unreachable"
        );
        assert!(
            !probe::is_visible(&app, Target::CategoryAll),
            "a row scrolled away is still pressable"
        );
    }

    /// **A column heading sorts by it, and again reverses it.**
    #[test]
    fn a_column_heading_sorts() {
        let mut app = wired();
        probe::click(&mut app, Target::Header(SortColumn::Size));
        assert_eq!(app.sort_column, SortColumn::Size);
        assert!(app.sort_ascending);
        let sizes: Vec<u64> = app
            .results
            .iter()
            .map(|&i| app.index.entries[i].size)
            .collect();
        assert!(sizes.windows(2).all(|w| w[0] <= w[1]), "{sizes:?}");
        probe::click(&mut app, Target::Header(SortColumn::Size));
        assert!(!app.sort_ascending, "the second press did not reverse it");
    }

    /// **A result is selected by a press, named by its entry and not its
    /// row.**
    #[test]
    fn a_press_selects_the_result_under_it() {
        let mut app = wired();
        let entry = app.results[2];
        probe::click(&mut app, Target::Result(entry));
        assert_eq!(
            app.selected_entry().map(|e| e.path.clone()),
            Some(app.index.entries[entry].path.clone())
        );
        // Sorting moves the row; the target still names the same entry.
        probe::click(&mut app, Target::Header(SortColumn::Size));
        let before = app.selected_entry().map(|e| e.path.clone());
        probe::click(&mut app, Target::Result(entry));
        assert_eq!(app.selected_entry().map(|e| e.path.clone()), before);
    }

    /// Two hundred results: more than any window shows at once.
    fn many_results() -> FileSearchApp {
        let mut app = FileSearchApp::new();
        app.launch = record_launch;
        for i in 0..200 {
            app.index.add(IndexEntry::new(
                &format!("/data/file{i:03}.txt"),
                &format!("file{i:03}.txt"),
                10,
                0,
                0,
                false,
            ));
        }
        app.execute_search();
        app
    }

    /// **The results scroll: with the wheel, and to follow the selection.**
    /// They were cut off at the panel's bottom edge, and the keyboard could
    /// move the selection onto rows that were never drawn.
    #[test]
    fn results_past_the_panel_can_be_reached() {
        let mut app = many_results();
        assert_eq!(app.results.len(), 200);
        let last = *app.results.last().unwrap();
        assert!(!probe::is_visible(&app, Target::Result(last)));

        probe::key(&mut app, &probe::press(Key::End));
        assert!(
            probe::is_visible(&app, Target::Result(last)),
            "the selection left the screen"
        );
        probe::key(&mut app, &probe::press(Key::Home));
        assert!(probe::is_visible(&app, Target::Result(app.results[0])));

        probe::scroll_at_point(&mut app, Target::Results, -5.0);
        assert!(
            app.results_scroll > 0,
            "the wheel over the results scrolled nothing"
        );
        assert!(!probe::is_visible(&app, Target::Result(app.results[0])));

        // A new search starts at the top of its own answer, not wherever the
        // last one had been scrolled to.
        app.execute_search();
        assert_eq!(app.results_scroll, 0, "a new search kept the old scroll");
        assert!(probe::is_visible(&app, Target::Result(app.results[0])));

        // Page Down moves a page, and lands on the end rather than refusing.
        let mut app2 = many_results();
        probe::key(&mut app2, &probe::press(Key::Down));
        probe::key(&mut app2, &probe::press(Key::PageDown));
        assert!(app2.selected_result.unwrap() > 1, "Page Down moved one row");
        for _ in 0..20 {
            probe::key(&mut app2, &probe::press(Key::PageDown));
        }
        assert_eq!(app2.selected_result, Some(199));
    }

    /// **Enter opens what is selected**, as the F1 card always said, and the
    /// preview's two actions do what they say. Open used to re-run the search
    /// and the action buttons were wired to nothing.
    #[test]
    fn enter_and_the_actions_open_what_is_selected() {
        settingsfile::testing::with_scratch_config("fs_open", |root| {
            // A program for PDFs, as File Associations would record it.
            let mut doc = yamldoc::Document::new();
            doc.set_str(&[associations::ASSOCIATIONS, "pdf"], "/usr/bin/pdfviewer");
            settingsfile::store(associations::CONFIG_NAME, &doc).unwrap();
            let _ = root;

            let mut app = wired();
            app.criteria.query = "report".to_string();
            app.execute_search();
            probe::key(&mut app, &probe::press(Key::Enter));
            assert!(
                app.selected_result.is_some(),
                "Enter with nothing selected selected nothing"
            );
            probe::key(&mut app, &probe::press(Key::Enter));
            let launched = LAUNCHED.with(|l| l.borrow().clone());
            assert_eq!(
                launched.len(),
                1,
                "Enter started nothing: {}",
                app.status_message
            );
            assert_eq!(launched[0].0, "/usr/bin/pdfviewer");
            assert!(launched[0].1.ends_with("report.pdf"));

            probe::click(&mut app, Target::OpenLocation);
            let launched = LAUNCHED.with(|l| l.borrow().clone());
            assert_eq!(launched.len(), 2);
            assert_eq!(launched[1].0, FILE_MANAGER);
            assert!(launched[1].1.ends_with("Documents"), "{:?}", launched[1].1);

            probe::click(&mut app, Target::Open);
            assert_eq!(
                LAUNCHED.with(|l| l.borrow().len()),
                3,
                "the Open button started nothing"
            );
        });
    }

    /// A folder among the results opens in the file manager, whatever the
    /// associations say about anything.
    #[test]
    fn a_folder_result_opens_in_the_file_manager() {
        settingsfile::testing::with_scratch_config("fs_open_folder", |_| {
            let mut app = wired();
            app.index.add(IndexEntry::new(
                "/home/user/Projects",
                "Projects",
                0,
                1_779_000_000,
                1_779_000_000,
                true,
            ));
            app.criteria.query = "Projects".to_string();
            app.execute_search();
            let folder = app
                .results
                .iter()
                .position(|&i| app.index.entries[i].is_directory)
                .expect("the folder is a result");
            app.selected_result = Some(folder);
            probe::key(&mut app, &probe::press(Key::Enter));
            let launched = LAUNCHED.with(|l| l.borrow().clone());
            assert_eq!(launched.len(), 1, "{}", app.status_message);
            assert_eq!(launched[0].0, FILE_MANAGER);
            assert!(launched[0].1.ends_with("Projects"));
        });
    }

    /// A kind with no program says so, and starts nothing.
    #[test]
    fn a_file_nothing_opens_says_so() {
        settingsfile::testing::with_scratch_config("fs_no_opener", |_| {
            let mut app = wired();
            app.criteria.query = "report".to_string();
            app.execute_search();
            app.selected_result = Some(0);
            probe::key(&mut app, &probe::press(Key::Enter));
            assert!(LAUNCHED.with(|l| l.borrow().is_empty()));
            assert!(
                app.status_message.contains("Nothing is set to open"),
                "{}",
                app.status_message
            );
        });
    }

    /// **A search can be saved, survives the window, and can be forgotten.**
    /// `bookmark_search` had no caller; a saved search that is gone at the
    /// next launch was never saved.
    #[test]
    fn a_saved_search_outlives_the_window() {
        settingsfile::testing::with_scratch_config("fs_saved", |_| {
            let mut app = wired();
            app.criteria.query = "*.rs".to_string();
            app.criteria.mode = SearchMode::Glob;
            app.execute_search();
            probe::click(&mut app, Target::SaveSearch);
            assert!(app.current_search_is_saved());

            // The next window finds it, and pressing it runs it.
            let mut next = wired();
            next.load_saved_searches(&settingsfile::load(CONFIG_NAME));
            let id = next
                .search_history
                .iter()
                .find(|s| s.is_bookmarked)
                .map(|s| s.id)
                .expect("the saved search did not survive");
            reveal(&mut next, Target::Saved(id));
            probe::click(&mut next, Target::Saved(id));
            assert_eq!(next.criteria.query, "*.rs");
            assert_eq!(next.criteria.mode, SearchMode::Glob);
            assert_eq!(next.results.len(), 2);

            // Its × forgets it, there and on disk.
            reveal(&mut next, Target::Unsave(id));
            probe::click(&mut next, Target::Unsave(id));
            assert!(!next.current_search_is_saved());
            let mut third = wired();
            third.load_saved_searches(&settingsfile::load(CONFIG_NAME));
            assert!(third.search_history.iter().all(|s| !s.is_bookmarked));
        });
    }

    /// A saved search this version cannot read is kept, not rewritten away.
    #[test]
    fn an_unreadable_saved_search_is_written_back_as_it_was() {
        settingsfile::testing::with_scratch_config("fs_saved_foreign", |_| {
            let mut doc = yamldoc::Document::new();
            doc.set_seq(&SAVED_KEY, &["fuzzy:repor", "glob:*.md"]);
            settingsfile::store(CONFIG_NAME, &doc).unwrap();

            let mut app = wired();
            app.load_saved_searches(&settingsfile::load(CONFIG_NAME));
            assert_eq!(
                app.search_history
                    .iter()
                    .filter(|s| s.is_bookmarked)
                    .count(),
                1
            );
            app.criteria.query = "notes".to_string();
            app.toggle_saved();
            let kept = settingsfile::load(CONFIG_NAME).get_seq(&SAVED_KEY).unwrap();
            assert!(kept.contains(&"fuzzy:repor".to_string()), "{kept:?}");
            assert!(kept.contains(&"glob:*.md".to_string()), "{kept:?}");
            assert!(kept.contains(&"name:notes".to_string()), "{kept:?}");
        });
    }

    /// A recent search is offered back, and pressing it runs it again.
    #[test]
    fn a_recent_search_can_be_run_again() {
        let mut app = wired();
        app.criteria.query = "budget".to_string();
        app.execute_search();
        app.selected_result = Some(0);
        app.open_selected();
        app.criteria.query.clear();
        app.execute_search();
        let id = app.search_history[0].id;
        reveal(&mut app, Target::Recent(id));
        probe::click(&mut app, Target::Recent(id));
        assert_eq!(app.criteria.query, "budget");
        assert_eq!(app.results.len(), 1);
    }

    /// The folder button puts the picker up; the header names the folder.
    #[test]
    fn the_folder_button_asks_for_a_folder() {
        let mut app = FileSearchApp::new();
        probe::click(&mut app, Target::ChooseFolder);
        assert!(app.picker.is_open(), "the button put nothing up");
        let mut app = FileSearchApp::new();
        let text = drawn(&app).join(" | ");
        assert!(text.contains("No folder chosen yet"), "{text}");
        app.root = Some(std::path::PathBuf::from("/home/user/docs"));
        let text = drawn(&app).join(" | ");
        assert!(text.contains("/home/user/docs"), "{text}");
    }

    /// **"Now" is the clock's now.** It was the constant `1_779_000_000`, so
    /// "Today" meant one day in May 2026 forever.
    #[test]
    fn now_is_read_from_the_clock() {
        let before = unix_now();
        let app = FileSearchApp::new();
        assert!(app.criteria.current_time >= before);
        assert!(
            app.criteria.current_time > 1_779_000_000,
            "still the frozen date"
        );

        // And an index pass reads it again, so ages are measured from when the
        // modification times were read.
        let dir = std::env::temp_dir().join(format!("fs-now-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fresh.txt"), b"x").unwrap();
        let mut app = FileSearchApp::new();
        app.criteria.current_time = 5;
        app.index_directory(&dir);
        assert!(
            app.criteria.current_time >= before,
            "indexing kept a stale clock"
        );
        app.execute_search();
        app.criteria.date_filter = DateFilter::Today;
        app.execute_search();
        assert!(
            app.results
                .iter()
                .any(|&i| app.index.entries[i].name == "fresh.txt"),
            "a file written a moment ago is not \"Today\""
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Hovering an action names its key in the status bar.
    #[test]
    fn hovering_an_action_names_its_key() {
        let mut app = wired();
        app.selected_result = Some(0);
        let (x, y) = probe::rect_of(&app, Target::OpenLocation).unwrap().centre();
        app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Move,
        }));
        assert!(drawn(&app).join(" | ").contains("(Ctrl+L)"));
    }

    /// A press while the shortcut card is up puts it away and reaches nothing
    /// under it.
    #[test]
    fn a_press_puts_the_card_away() {
        let mut app = wired();
        app.handle_event(&Event::Key(probe::press(Key::F1)));
        let (x, y) = probe::rect_of(&app, Target::CategoryAll).unwrap().centre();
        app.criteria.category_filter = Some(FileCategory::Code);
        app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert!(!app.show_help);
        assert_eq!(
            app.criteria.category_filter,
            Some(FileCategory::Code),
            "the press went through"
        );
    }

    // ------------------------------------------------------------------
    // Content search
    // ------------------------------------------------------------------

    /// A scratch folder of files with known contents, removed on drop.
    struct Contents {
        dir: std::path::PathBuf,
    }

    impl Contents {
        fn new(tag: &str, files: &[(&str, &[u8])]) -> Self {
            let dir = std::env::temp_dir().join(format!("fs-content-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir); // a leftover from a killed run
            std::fs::create_dir_all(&dir).unwrap();
            for (name, bytes) in files {
                std::fs::write(dir.join(name), bytes).unwrap();
            }
            Self { dir }
        }
    }

    impl Drop for Contents {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir); // best effort; temp dir
        }
    }

    /// Run the worker to the end, as the window's clock would.
    fn finish_content_search(app: &mut FileSearchApp) {
        let started = std::time::Instant::now();
        while app.is_searching_contents() {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(20),
                "the content search never finished"
            );
            app.pump_content_search();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    fn result_names(app: &FileSearchApp) -> Vec<String> {
        let mut names: Vec<String> = app
            .results
            .iter()
            .map(|&i| app.index.entries[i].name.clone())
            .collect();
        names.sort();
        names
    }

    fn content_app(files: &Contents) -> FileSearchApp {
        let mut app = FileSearchApp::new();
        app.index_directory(&files.dir);
        app.criteria.mode = SearchMode::Content;
        app
    }

    /// **Content mode finds files by what is in them.** It matched names --
    /// "Content search would need actual file reading. For now, match against
    /// name as fallback" -- so a phrase in a document was reported as in none.
    #[test]
    fn content_mode_searches_contents_not_names() {
        let files = Contents::new(
            "basic",
            &[
                ("letter.txt", b"Dear Sam, hello from the coast."),
                ("hello.txt", b"nothing to see"),
                ("notes.md", b"HELLO in capitals"),
            ],
        );
        let mut app = content_app(&files);
        app.criteria.query = "hello".to_string();
        app.execute_search();
        assert!(
            app.is_searching_contents(),
            "the search did not go to the worker"
        );
        finish_content_search(&mut app);
        assert_eq!(result_names(&app), vec!["letter.txt", "notes.md"]);
        assert!(
            app.status_message.contains("2 files contain it"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn match_case_holds_in_content_mode() {
        let files = Contents::new("case", &[("a.txt", b"hello"), ("b.txt", b"HELLO")]);
        let mut app = content_app(&files);
        app.criteria.case_sensitive = true;
        app.criteria.query = "hello".to_string();
        app.execute_search();
        finish_content_search(&mut app);
        assert_eq!(result_names(&app), vec!["a.txt"]);
    }

    /// A UTF-8 file is folded as text; a file that is not is folded only in
    /// its ASCII letters, and never decoded lossily.
    #[test]
    fn content_folding_is_textual_for_text_and_exact_for_bytes() {
        let files = Contents::new(
            "fold",
            &[
                ("french.txt", "ÉCOLE PRIMAIRE".as_bytes()),
                ("binary.bin", b"\xff\xfeSECRET\x00"),
            ],
        );
        let mut app = content_app(&files);
        app.criteria.query = "école".to_string();
        app.execute_search();
        finish_content_search(&mut app);
        assert_eq!(result_names(&app), vec!["french.txt"]);

        app.criteria.query = "secret".to_string();
        app.execute_search();
        finish_content_search(&mut app);
        assert_eq!(result_names(&app), vec!["binary.bin"]);
    }

    /// **A new query stops the old search.** Its matches must not arrive into
    /// the new one's results.
    #[test]
    fn a_new_query_replaces_the_search_in_progress() {
        let many: Vec<(String, Vec<u8>)> = (0..200)
            .map(|i| (format!("f{i:03}.txt"), b"alpha".to_vec()))
            .collect();
        let refs: Vec<(&str, &[u8])> = many
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        let mut with_beta = refs.clone();
        with_beta.push(("only.txt", b"beta"));
        let files = Contents::new("cancel", &with_beta);
        let mut app = content_app(&files);
        app.criteria.query = "alpha".to_string();
        app.execute_search();
        app.criteria.query = "beta".to_string();
        app.execute_search();
        finish_content_search(&mut app);
        assert_eq!(
            result_names(&app),
            vec!["only.txt"],
            "the first search's matches leaked in"
        );
    }

    /// Leaving content mode stops its search: the worker's matches must not
    /// arrive into a name search's results.
    #[test]
    fn leaving_content_mode_stops_its_search() {
        let files = Contents::new(
            "leave",
            &[("inside.txt", b"target"), ("target.txt", b"nothing")],
        );
        let mut app = content_app(&files);
        app.criteria.query = "target".to_string();
        app.execute_search();
        app.criteria.mode = SearchMode::Substring;
        app.execute_search();
        assert!(!app.is_searching_contents());
        std::thread::sleep(std::time::Duration::from_millis(50));
        app.pump_content_search();
        assert_eq!(result_names(&app), vec!["target.txt"]);
    }

    /// A file over the limit is skipped, and the skip is said.
    #[test]
    fn a_file_too_large_to_read_is_counted_not_hidden() {
        let files = Contents::new(
            "large",
            &[("small.txt", b"needle"), ("large.txt", b"needle but long")],
        );
        let mut app = content_app(&files);
        app.content_limit = 8;
        app.criteria.query = "needle".to_string();
        app.execute_search();
        finish_content_search(&mut app);
        assert_eq!(result_names(&app), vec!["small.txt"]);
        assert!(
            app.status_message.contains("1 not searched"),
            "{}",
            app.status_message
        );
    }

    /// The clock runs while the worker reads, and stops when it is done: a
    /// window that ticks with nothing to take in holds the machine awake.
    #[test]
    fn the_clock_runs_only_while_contents_are_read() {
        let files = Contents::new("clock", &[("a.txt", b"x")]);
        let mut app = content_app(&files);
        assert!(app.tick_interval().is_none());
        app.criteria.query = "x".to_string();
        app.execute_search();
        assert!(app.tick_interval().is_some());
        finish_content_search(&mut app);
        assert!(
            app.tick_interval().is_none(),
            "the clock outlived the search"
        );
    }

    /// Folders are not candidates, and the other filters still apply.
    #[test]
    fn content_mode_keeps_the_other_filters() {
        let files = Contents::new(
            "filters",
            &[
                ("a.txt", b"word"),
                ("b.md", b"word"),
                (".hidden.txt", b"word"),
            ],
        );
        std::fs::create_dir_all(files.dir.join("word")).unwrap();
        let mut app = content_app(&files);
        app.criteria.query = "word".to_string();
        app.criteria.extension_filter = Some("txt".to_string());
        app.execute_search();
        finish_content_search(&mut app);
        assert_eq!(result_names(&app), vec!["a.txt"]);
    }
}
