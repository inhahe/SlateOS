//! `Slate OS` Batch File Renamer
//!
//! A powerful batch file renaming tool with:
//! - Multiple rename operations (find/replace, insert, remove, case change,
//!   numbering, date stamp, regex)
//! - Live preview showing old → new names before committing
//! - Undo/redo for rename operations
//! - Operation chaining (apply multiple transforms in sequence)
//! - Name conflict detection and resolution
//! - File type filtering
//! - Drag-and-drop file addition
//! - History of past rename sessions
//! - Template-based renaming with variables
//! - Extension handling (rename, add, remove, change)
//!
//! Uses the guitk library for UI rendering.

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
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::cognitive_complexity)]
#![allow(dead_code)]

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::table::{Column, Fit, Table};
use guitk::text;
use std::collections::{BTreeMap, BTreeSet};

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 750.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const SIDEBAR_WIDTH: f32 = 280.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const PADDING: f32 = 8.0;
const LINE_HEIGHT: f32 = 22.0;
const CHAR_WIDTH: f32 = 7.5;
const SMALL_TEXT: f32 = 11.0;
const NORMAL_TEXT: f32 = 13.0;
const HEADER_TEXT: f32 = 15.0;
const TITLE_TEXT: f32 = 17.0;
const BUTTON_HEIGHT: f32 = 28.0;
const INPUT_HEIGHT: f32 = 26.0;

/// Columns of the rename-preview file list.
///
/// One definition read by both the header row and the body rows. Previously
/// each width lived in the `headers` array *and* as a literal in the row
/// cursor's increment (250.0 and `cx += 258.0`), which agreed only by hand.
const FILE_COLUMNS: &[Column] = &[
    Column {
        label: "",
        width: 30.0,
    },
    Column {
        label: "Original Name",
        width: 250.0,
    },
    Column {
        label: "→",
        width: 20.0,
    },
    Column {
        label: "New Name",
        width: 250.0,
    },
    Column {
        label: "Size",
        width: 80.0,
    },
    Column {
        label: "Status",
        width: 80.0,
    },
];

const COL_CHECK: usize = 0;
const COL_ORIGINAL: usize = 1;
const COL_ARROW: usize = 2;
const COL_NEW: usize = 3;
const COL_SIZE: usize = 4;
const COL_STATUS: usize = 5;

/// Width available to an operation row's detail line in the operation list.
const OP_DETAIL_WIDTH: f32 = SIDEBAR_WIDTH - 40.0;
/// Font size of that detail line.
const OP_DETAIL_SIZE: f32 = SMALL_TEXT - 1.0;

/// Frame a user-typed string in developer-authored text, fitting `width`.
///
/// The frame is what tells the user *which* operation this is — "at start"
/// distinguishes an insert from every other insert — so the frame always
/// survives and the user's string is elided to whatever is left. Eliding the
/// whole line instead would cut the frame off the end and leave two operations
/// looking identical.
fn framed_detail(prefix: &str, user: &str, suffix: &str, width: f32) -> String {
    let frame_width = text::measure(
        &format!("{prefix}{suffix}"),
        OP_DETAIL_SIZE,
        FontWeightHint::Regular,
    );
    let room = (width - frame_width).max(0.0);
    let fitted = text::elide(user, room, "…", OP_DETAIL_SIZE, FontWeightHint::Regular);
    format!("{prefix}{fitted}{suffix}")
}

/// Summarise a find→replace pair for a fixed-width row.
///
/// Both halves matter — the row exists so the user can tell one operation from
/// the next — so each is elided against its own share of the row. Eliding the
/// line as a whole would let a long search string push the replacement off the
/// end entirely, which is the half that says what the rename will *do*.
fn find_replace_detail(find: &str, replace: &str, width: f32) -> String {
    let frame_width = text::measure("\"\" → \"\"", OP_DETAIL_SIZE, FontWeightHint::Regular);
    let half = (width - frame_width).max(0.0) / 2.0;
    let fit = |s: &str| text::elide(s, half, "…", OP_DETAIL_SIZE, FontWeightHint::Regular);
    format!("\"{}\" → \"{}\"", fit(find), fit(replace))
}

const MAX_FILES: usize = 10_000;
const MAX_OPERATIONS: usize = 50;
const MAX_UNDO: usize = 100;
const MAX_HISTORY: usize = 50;

// ============================================================================
// Rename operation types
// ============================================================================

/// A single rename operation that transforms a filename.
#[derive(Debug, Clone)]
enum RenameOp {
    /// Find and replace text in the filename.
    FindReplace {
        find: String,
        replace: String,
        case_sensitive: bool,
        replace_all: bool,
    },
    /// Insert text at a position.
    Insert {
        text: String,
        position: InsertPosition,
    },
    /// Remove characters from the filename.
    Remove { from: usize, count: usize },
    /// Change the case of the filename.
    ChangeCase(CaseMode),
    /// Add sequential numbering.
    Number {
        start: usize,
        step: usize,
        padding: usize,
        position: InsertPosition,
        separator: String,
    },
    /// Add a date/time stamp.
    DateStamp {
        format: DateFormat,
        position: InsertPosition,
        separator: String,
    },
    /// Regex find and replace.
    Regex {
        pattern: String,
        replacement: String,
    },
    /// Trim whitespace or specific characters.
    Trim { chars: String, mode: TrimMode },
    /// Change the file extension.
    Extension(ExtensionOp),
    /// Apply a template with variables.
    Template { template: String },
}

impl RenameOp {
    fn label(&self) -> &str {
        match self {
            Self::FindReplace { .. } => "Find & Replace",
            Self::Insert { .. } => "Insert Text",
            Self::Remove { .. } => "Remove Characters",
            Self::ChangeCase(_) => "Change Case",
            Self::Number { .. } => "Add Numbering",
            Self::DateStamp { .. } => "Date Stamp",
            Self::Regex { .. } => "Regex Replace",
            Self::Trim { .. } => "Trim",
            Self::Extension(_) => "Extension",
            Self::Template { .. } => "Template",
        }
    }

    fn color(&self) -> Color {
        match self {
            Self::FindReplace { .. } => BLUE,
            Self::Insert { .. } => GREEN,
            Self::Remove { .. } => RED,
            Self::ChangeCase(_) => MAUVE,
            Self::Number { .. } => PEACH,
            Self::DateStamp { .. } => TEAL,
            Self::Regex { .. } => YELLOW,
            Self::Trim { .. } => LAVENDER,
            Self::Extension(_) => OVERLAY0,
            Self::Template { .. } => SUBTEXT1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsertPosition {
    /// Insert at the beginning of the name (before extension).
    Start,
    /// Insert at the end of the name (before extension).
    End,
    /// Insert at a specific character index.
    At(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaseMode {
    Upper,
    Lower,
    Title,
    Sentence,
    Toggle,
    CamelCase,
    SnakeCase,
    KebabCase,
}

impl CaseMode {
    fn label(self) -> &'static str {
        match self {
            Self::Upper => "UPPERCASE",
            Self::Lower => "lowercase",
            Self::Title => "Title Case",
            Self::Sentence => "Sentence case",
            Self::Toggle => "tOGGLE cASE",
            Self::CamelCase => "camelCase",
            Self::SnakeCase => "snake_case",
            Self::KebabCase => "kebab-case",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateFormat {
    YmdHyphen,  // 2024-01-15
    YmdSlash,   // 2024/01/15
    DmyHyphen,  // 15-01-2024
    YmdCompact, // 20240115
    Timestamp,  // 20240115_143022
}

impl DateFormat {
    fn label(self) -> &'static str {
        match self {
            Self::YmdHyphen => "YYYY-MM-DD",
            Self::YmdSlash => "YYYY/MM/DD",
            Self::DmyHyphen => "DD-MM-YYYY",
            Self::YmdCompact => "YYYYMMDD",
            Self::Timestamp => "YYYYMMDD_HHMMSS",
        }
    }

    fn format(self, year: u16, month: u8, day: u8, hour: u8, min: u8, sec: u8) -> String {
        match self {
            Self::YmdHyphen => format!("{year:04}-{month:02}-{day:02}"),
            Self::YmdSlash => format!("{year:04}/{month:02}/{day:02}"),
            Self::DmyHyphen => format!("{day:02}-{month:02}-{year:04}"),
            Self::YmdCompact => format!("{year:04}{month:02}{day:02}"),
            Self::Timestamp => format!("{year:04}{month:02}{day:02}_{hour:02}{min:02}{sec:02}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrimMode {
    Both,
    Start,
    End,
}

#[derive(Debug, Clone)]
enum ExtensionOp {
    /// Replace extension with a new one.
    Replace(String),
    /// Add an extension.
    Add(String),
    /// Remove the extension.
    Remove,
    /// Make extension lowercase.
    Lower,
    /// Make extension uppercase.
    Upper,
}

// ============================================================================
// File entry
// ============================================================================

/// A file entry in the rename list.
#[derive(Debug, Clone)]
struct FileEntry {
    /// Original full path.
    original_path: String,
    /// Original filename (without path).
    original_name: String,
    /// New filename after all operations.
    new_name: String,
    /// File size in bytes.
    size: u64,
    /// Whether this file is selected for renaming.
    selected: bool,
    /// Whether there's a naming conflict.
    conflict: bool,
    /// File type/extension.
    extension: String,
    /// Last modified timestamp (mock).
    modified_ms: u64,
}

impl FileEntry {
    fn new(path: &str, name: &str, size: u64, modified_ms: u64) -> Self {
        // An extension is what follows the *last* dot, and only when there is
        // something before that dot. `rsplit('.').next()` does not say that:
        // the `len() < name.len()` guard it was paired with rejects a dotless
        // name but still accepts a leading dot, so `.bashrc` was indexed with
        // extension `bashrc` -- shown in the details pane as its type, and
        // swept up by a `bashrc` extension filter alongside real `x.bashrc`
        // files.
        let extension = name
            .rsplit_once('.')
            .filter(|(stem, _)| !stem.is_empty())
            .map_or("", |(_, ext)| ext)
            .to_string();
        Self {
            original_path: path.to_string(),
            original_name: name.to_string(),
            new_name: name.to_string(),
            size,
            selected: true,
            conflict: false,
            extension,
            modified_ms,
        }
    }

    /// Split into (stem, extension) parts.
    fn split_name(name: &str) -> (&str, &str) {
        match name.rfind('.') {
            Some(pos) if pos > 0 => (&name[..pos], &name[pos..]),
            _ => (name, ""),
        }
    }
}

/// The byte offset of character `chars`, or the end of `s` if it is shorter.
///
/// Every position a rename rule takes — "insert at 3", "remove 2 from 5" — is a
/// position in **characters**, because characters are what the user can see in
/// the name they are typing a rule against. `InsertPosition::At`'s own doc
/// comment has always said "character index".
///
/// The code did not: it clamped with `.min(stem.len())`, a *byte* length, and
/// then sliced. For any name that is not pure ASCII that is a different
/// position from the one the user asked for, and not necessarily a character
/// boundary at all — "insert at 1" into `"\u{65e5}\u{672c}\u{8a9e}"` sliced
/// inside the first kanji and aborted the renamer partway through a batch.
///
/// Going through here makes the position mean what it says, and makes the
/// slices sound as a side effect. For ASCII names the two are the same number,
/// so no existing rule changes behaviour.
fn char_offset(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(i, _)| i)
}

/// One filesystem rename, in the order it must be performed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RenameStep {
    from: String,
    to: String,
}

/// Order a set of renames so that no step overwrites a file a later step still
/// needs, inserting temporary names where a cycle makes that impossible.
///
/// `existing` is every filename present in the directory right now;
/// `renames` is the `(from, to)` set the user asked for, in any order.
///
/// The problem this solves is the ordinary one for a bulk renamer and the
/// reason the old conflict check refused so much: renaming `1.jpg` → `2.jpg`
/// and `2.jpg` → `3.jpg` is perfectly valid, but doing them in that order
/// destroys `2.jpg` before it has been moved to `3.jpg`. Done in the reverse
/// order it is fine. Swapping two names — `a` → `b`, `b` → `a` — has no valid
/// order at all, and needs one of them parked under a temporary name first.
///
/// The algorithm is the obvious one: repeatedly emit every rename whose
/// destination is currently free; when a full pass emits nothing, everything
/// left is part of a cycle, so break one link by renaming its source to an
/// unused temporary name and re-queue the rest of that rename. Breaking a link
/// always frees the name some other queued rename wants — that is what being a
/// cycle means — so each break makes progress and the loop terminates.
///
/// Note it is `fs::rename`'s *overwriting* that makes this necessary. Slate OS
/// paths are case-sensitive, so `a.txt` → `A.txt` is a real rename between two
/// distinct names and needs no special handling here.
fn rename_plan(existing: &[String], renames: &[(String, String)]) -> Vec<RenameStep> {
    let mut occupied: BTreeSet<String> = existing.iter().cloned().collect();
    let mut pending: Vec<(String, String)> = renames
        .iter()
        .filter(|(from, to)| from != to)
        .cloned()
        .collect();
    let mut steps = Vec::new();
    let mut temp_counter: usize = 0;

    while !pending.is_empty() {
        let mut blocked = Vec::new();
        let mut progressed = false;
        for (from, to) in pending.drain(..) {
            if occupied.contains(&to) {
                blocked.push((from, to));
            } else {
                occupied.remove(&from);
                occupied.insert(to.clone());
                steps.push(RenameStep { from, to });
                progressed = true;
            }
        }
        pending = blocked;

        if progressed || pending.is_empty() {
            continue;
        }

        // Everything left is in a cycle. Park one source under a name nothing
        // uses, which frees its old name for whichever rename was waiting on
        // it, and re-queue the second half of the move.
        let (from, to) = pending.remove(0);
        let temp = unused_temp_name(&occupied, &mut temp_counter);
        occupied.remove(&from);
        occupied.insert(temp.clone());
        steps.push(RenameStep {
            from,
            to: temp.clone(),
        });
        pending.push((temp, to));
    }

    steps
}

/// A name that no file in `occupied` has, for parking one side of a rename
/// cycle. The leading dot keeps it out of the way of ordinary names, and the
/// loop means even a directory that already contains one is handled rather
/// than trusted.
fn unused_temp_name(occupied: &BTreeSet<String>, counter: &mut usize) -> String {
    loop {
        let name = format!(".renamer-tmp-{counter}");
        *counter = counter.saturating_add(1);
        if !occupied.contains(&name) {
            return name;
        }
    }
}

/// `path` with its last component replaced by `new_name`.
///
/// Structural, not textual: the directory part is whatever precedes the final
/// separator, and only that final component is replaced. A substring
/// `replace` would rewrite matching *directory* names too, which is how
/// renaming `photos/photos.jpg` turns into `holiday.jpg/holiday.jpg`.
///
/// Slate OS uses `/`; `\` is accepted as well so a path picked up from a host
/// filesystem during development does not silently become a single filename
/// with backslashes in it.
fn replace_file_name(path: &str, new_name: &str) -> String {
    match path.rfind(['/', '\\']) {
        // `sep + 1` is a byte index just past an ASCII separator, so it is
        // always a char boundary.
        Some(sep) => {
            let mut out =
                String::with_capacity(sep.saturating_add(1).saturating_add(new_name.len()));
            out.push_str(&path[..=sep]);
            out.push_str(new_name);
            out
        }
        // A bare filename with no directory part is entirely the name.
        None => new_name.to_string(),
    }
}

// ============================================================================
// Rename engine
// ============================================================================

/// The core rename engine that applies operations to filenames.
struct RenameEngine;

impl RenameEngine {
    /// Apply a single operation to a filename, with an index (for numbering).
    fn apply(op: &RenameOp, name: &str, index: usize) -> String {
        let (stem, ext) = FileEntry::split_name(name);

        match op {
            RenameOp::FindReplace {
                find,
                replace,
                case_sensitive,
                replace_all,
            } => {
                let new_stem = if *case_sensitive {
                    if *replace_all {
                        stem.replace(find.as_str(), replace.as_str())
                    } else {
                        stem.replacen(find.as_str(), replace.as_str(), 1)
                    }
                } else {
                    Self::case_insensitive_replace(stem, find, replace, *replace_all)
                };
                format!("{new_stem}{ext}")
            }
            RenameOp::Insert { text, position } => {
                let insert_pos = match position {
                    InsertPosition::Start => 0,
                    InsertPosition::End => stem.len(),
                    InsertPosition::At(pos) => char_offset(stem, *pos),
                };
                let mut new_stem = String::with_capacity(stem.len().saturating_add(text.len()));
                new_stem.push_str(&stem[..insert_pos]);
                new_stem.push_str(text);
                new_stem.push_str(&stem[insert_pos..]);
                format!("{new_stem}{ext}")
            }
            RenameOp::Remove { from, count } => {
                // `from` and `count` are both counts of characters.
                let from_clamped = char_offset(stem, *from);
                let end = char_offset(stem, from.saturating_add(*count));
                let mut new_stem = String::with_capacity(stem.len());
                new_stem.push_str(&stem[..from_clamped]);
                new_stem.push_str(&stem[end..]);
                format!("{new_stem}{ext}")
            }
            RenameOp::ChangeCase(mode) => {
                let new_stem = Self::apply_case(stem, *mode);
                format!("{new_stem}{ext}")
            }
            RenameOp::Number {
                start,
                step,
                padding,
                position,
                separator,
            } => {
                let num = start.saturating_add(index.saturating_mul(*step));
                let num_str = format!("{num:0>width$}", width = *padding);
                let insert_str = match position {
                    InsertPosition::Start => format!("{num_str}{separator}"),
                    InsertPosition::End => format!("{separator}{num_str}"),
                    InsertPosition::At(_) => format!("{separator}{num_str}{separator}"),
                };
                match position {
                    InsertPosition::Start => format!("{insert_str}{stem}{ext}"),
                    InsertPosition::End => format!("{stem}{insert_str}{ext}"),
                    InsertPosition::At(pos) => {
                        let pos = char_offset(stem, *pos);
                        let mut s = String::new();
                        s.push_str(&stem[..pos]);
                        s.push_str(&insert_str);
                        s.push_str(&stem[pos..]);
                        format!("{s}{ext}")
                    }
                }
            }
            RenameOp::DateStamp {
                format,
                position,
                separator,
            } => {
                // Mock date (in real OS, would use system time)
                let date_str = format.format(2026, 5, 18, 14, 30, 0);
                match position {
                    InsertPosition::Start => format!("{date_str}{separator}{stem}{ext}"),
                    InsertPosition::End => format!("{stem}{separator}{date_str}{ext}"),
                    InsertPosition::At(pos) => {
                        let pos = char_offset(stem, *pos);
                        let mut s = String::new();
                        s.push_str(&stem[..pos]);
                        s.push_str(separator);
                        s.push_str(&date_str);
                        s.push_str(separator);
                        s.push_str(&stem[pos..]);
                        format!("{s}{ext}")
                    }
                }
            }
            RenameOp::Regex {
                pattern,
                replacement,
            } => {
                // Simple regex: only support literal patterns for now
                // (real implementation would use our NFA regex engine)

                name.replace(pattern.as_str(), replacement.as_str())
            }
            RenameOp::Trim { chars, mode } => {
                let new_stem = if chars.is_empty() {
                    match mode {
                        TrimMode::Both => stem.trim().to_string(),
                        TrimMode::Start => stem.trim_start().to_string(),
                        TrimMode::End => stem.trim_end().to_string(),
                    }
                } else {
                    let chars_arr: Vec<char> = chars.chars().collect();
                    match mode {
                        TrimMode::Both => stem
                            .trim_matches(|c: char| chars_arr.contains(&c))
                            .to_string(),
                        TrimMode::Start => stem
                            .trim_start_matches(|c: char| chars_arr.contains(&c))
                            .to_string(),
                        TrimMode::End => stem
                            .trim_end_matches(|c: char| chars_arr.contains(&c))
                            .to_string(),
                    }
                };
                format!("{new_stem}{ext}")
            }
            RenameOp::Extension(ext_op) => match ext_op {
                ExtensionOp::Replace(new_ext) => {
                    if new_ext.starts_with('.') {
                        format!("{stem}{new_ext}")
                    } else {
                        format!("{stem}.{new_ext}")
                    }
                }
                ExtensionOp::Add(new_ext) => {
                    if new_ext.starts_with('.') {
                        format!("{name}{new_ext}")
                    } else {
                        format!("{name}.{new_ext}")
                    }
                }
                ExtensionOp::Remove => stem.to_string(),
                ExtensionOp::Lower => format!("{stem}{}", ext.to_ascii_lowercase()),
                ExtensionOp::Upper => format!("{stem}{}", ext.to_ascii_uppercase()),
            },
            RenameOp::Template { template } => {
                let (stem_part, ext_part) = FileEntry::split_name(name);

                template
                    .replace("{name}", stem_part)
                    .replace("{ext}", ext_part.trim_start_matches('.'))
                    .replace("{n}", &format!("{index}"))
                    .replace("{N}", &format!("{index:03}"))
                    .replace("{original}", name)
            }
        }
    }

    fn case_insensitive_replace(s: &str, find: &str, replace: &str, all: bool) -> String {
        if find.is_empty() {
            return s.to_string();
        }
        let lower = s.to_ascii_lowercase();
        let find_lower = find.to_ascii_lowercase();
        let mut result = String::with_capacity(s.len());
        let mut start: usize = 0;

        while let Some(pos) = lower[start..].find(&find_lower) {
            let abs_pos = start.saturating_add(pos);
            result.push_str(&s[start..abs_pos]);
            result.push_str(replace);
            start = abs_pos.saturating_add(find.len());
            if !all {
                break;
            }
        }
        result.push_str(&s[start..]);
        result
    }

    fn apply_case(s: &str, mode: CaseMode) -> String {
        match mode {
            CaseMode::Upper => s.to_ascii_uppercase(),
            CaseMode::Lower => s.to_ascii_lowercase(),
            CaseMode::Title => {
                let mut result = String::with_capacity(s.len());
                let mut capitalize = true;
                for ch in s.chars() {
                    if ch == ' ' || ch == '_' || ch == '-' {
                        result.push(ch);
                        capitalize = true;
                    } else if capitalize {
                        result.extend(ch.to_uppercase());
                        capitalize = false;
                    } else {
                        result.extend(ch.to_lowercase());
                    }
                }
                result
            }
            CaseMode::Sentence => {
                let mut result = String::with_capacity(s.len());
                let mut first = true;
                for ch in s.chars() {
                    if first && ch.is_alphabetic() {
                        result.extend(ch.to_uppercase());
                        first = false;
                    } else {
                        result.extend(ch.to_lowercase());
                    }
                }
                result
            }
            CaseMode::Toggle => s
                .chars()
                .map(|c| {
                    if c.is_uppercase() {
                        c.to_ascii_lowercase()
                    } else {
                        c.to_ascii_uppercase()
                    }
                })
                .collect(),
            CaseMode::CamelCase => {
                let mut result = String::with_capacity(s.len());
                let mut capitalize = false;
                for ch in s.chars() {
                    if ch == ' ' || ch == '_' || ch == '-' {
                        capitalize = true;
                    } else if capitalize {
                        result.extend(ch.to_uppercase());
                        capitalize = false;
                    } else {
                        result.push(ch);
                    }
                }
                result
            }
            CaseMode::SnakeCase => {
                let mut result = String::with_capacity(s.len());
                for (i, ch) in s.chars().enumerate() {
                    if ch.is_uppercase() && i > 0 {
                        result.push('_');
                    }
                    result.extend(ch.to_lowercase());
                }
                result.replace([' ', '-'], "_")
            }
            CaseMode::KebabCase => {
                let mut result = String::with_capacity(s.len());
                for (i, ch) in s.chars().enumerate() {
                    if ch.is_uppercase() && i > 0 {
                        result.push('-');
                    }
                    result.extend(ch.to_lowercase());
                }
                result.replace([' ', '_'], "-")
            }
        }
    }
}

// ============================================================================
// Rename history entry
// ============================================================================

/// A record of a completed rename operation batch.
#[derive(Debug, Clone)]
struct RenameRecord {
    /// Pairs of (`old_name`, `new_name`).
    renames: Vec<(String, String)>,
    /// The operations that were applied.
    operations: Vec<String>,
    /// When the rename was performed (mock timestamp).
    timestamp_ms: u64,
}

// ============================================================================
// App state
// ============================================================================

/// The batch file renamer application state.
struct RenamerApp {
    /// Files to rename.
    files: Vec<FileEntry>,
    /// Active rename operations (applied in order).
    operations: Vec<RenameOp>,
    /// Undo stack of rename records.
    undo_stack: Vec<RenameRecord>,
    /// Redo stack.
    redo_stack: Vec<RenameRecord>,
    /// Scroll offset in the file list.
    scroll_offset: f32,
    /// Selected file index.
    selected_file: usize,
    /// Selected operation index in the sidebar.
    selected_op: usize,
    /// Which sidebar panel is active.
    sidebar_panel: SidebarPanel,
    /// Current time (mock).
    current_time_ms: u64,
    /// Status message.
    status_message: String,
    /// Filter: file extension (empty = all).
    filter_extension: String,
    /// Whether to show only conflicting files.
    filter_conflicts: bool,
    /// History of past rename sessions.
    history: Vec<RenameRecord>,
    /// Search/filter text for the file list.
    search_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarPanel {
    Operations,
    Preview,
    History,
}

impl RenamerApp {
    fn new() -> Self {
        Self {
            files: Vec::new(),
            operations: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            scroll_offset: 0.0,
            selected_file: 0,
            selected_op: 0,
            sidebar_panel: SidebarPanel::Operations,
            current_time_ms: 0,
            status_message: String::new(),
            filter_extension: String::new(),
            filter_conflicts: false,
            history: Vec::new(),
            search_text: String::new(),
        }
    }

    /// Add a file to the rename list.
    fn add_file(&mut self, path: &str, name: &str, size: u64, modified: u64) {
        if self.files.len() >= MAX_FILES {
            return;
        }
        self.files.push(FileEntry::new(path, name, size, modified));
        self.apply_operations();
    }

    /// Add a rename operation and recompute previews.
    fn add_operation(&mut self, op: RenameOp) {
        if self.operations.len() >= MAX_OPERATIONS {
            return;
        }
        self.operations.push(op);
        self.apply_operations();
    }

    /// Remove an operation by index and recompute.
    fn remove_operation(&mut self, index: usize) {
        if index < self.operations.len() {
            self.operations.remove(index);
            self.apply_operations();
        }
    }

    /// Move an operation up in the chain.
    fn move_operation_up(&mut self, index: usize) {
        if index > 0 && index < self.operations.len() {
            self.operations.swap(index, index.saturating_sub(1));
            self.apply_operations();
        }
    }

    /// Move an operation down in the chain.
    fn move_operation_down(&mut self, index: usize) {
        if index.saturating_add(1) < self.operations.len() {
            self.operations.swap(index, index.saturating_add(1));
            self.apply_operations();
        }
    }

    /// Apply all operations to all files and update previews.
    fn apply_operations(&mut self) {
        for (i, file) in self.files.iter_mut().enumerate() {
            let mut name = file.original_name.clone();
            for op in &self.operations {
                name = RenameEngine::apply(op, &name, i);
            }
            file.new_name = name;
        }
        self.detect_conflicts();
    }

    /// Flag the files whose new name would collide with another file's *final*
    /// name once the whole batch has been applied.
    ///
    /// Two things this deliberately does not do, both of which it used to:
    ///
    /// - **It does not compare case-insensitively.** Slate OS has a
    ///   case-sensitive filesystem (`design.txt`), so `A.txt` and `a.txt` are
    ///   two different files and renaming one to the other's *case variant* is
    ///   a legitimate rename, not a collision. The old
    ///   `to_ascii_lowercase`/`eq_ignore_ascii_case` comparison refused those,
    ///   and — being ASCII-only — was not even consistently case-insensitive
    ///   for the names where a user might expect it to be.
    /// - **It does not treat a name another file is vacating as taken.** The
    ///   old check compared each new name against every other file's
    ///   *original* name, so shifting a numbered sequence — `1.jpg` → `2.jpg`,
    ///   `2.jpg` → `3.jpg`, the single most common bulk rename there is — was
    ///   flagged as one conflict per file and refused outright. What that
    ///   really is is an *ordering* constraint, and [`rename_plan`] is what
    ///   satisfies it.
    fn detect_conflicts(&mut self) {
        // The name each file ends up with: its new name if it is in the batch,
        // otherwise the name it already has.
        let finals: Vec<String> = self
            .files
            .iter()
            .map(|f| {
                if f.selected {
                    f.new_name.clone()
                } else {
                    f.original_name.clone()
                }
            })
            .collect();

        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for name in &finals {
            *counts.entry(name.as_str()).or_insert(0) = counts
                .get(name.as_str())
                .copied()
                .unwrap_or(0)
                .saturating_add(1);
        }

        for (file, name) in self.files.iter_mut().zip(finals.iter()) {
            // Only a file being renamed can be *told* to fix anything, so only
            // that one is flagged; an unselected file is the victim, not the
            // cause.
            file.conflict = file.selected && counts.get(name.as_str()).copied().unwrap_or(0) > 1;
        }
    }

    /// Execute the rename (commit changes).
    fn execute_rename(&mut self) {
        let record = RenameRecord {
            renames: self
                .files
                .iter()
                .filter(|f| f.selected && f.original_name != f.new_name && !f.conflict)
                .map(|f| (f.original_name.clone(), f.new_name.clone()))
                .collect(),
            operations: self
                .operations
                .iter()
                .map(|o| o.label().to_string())
                .collect(),
            timestamp_ms: self.current_time_ms,
        };

        if record.renames.is_empty() {
            self.status_message = "No files to rename".into();
            return;
        }

        let count = record.renames.len();

        // Ordered, so that when this is wired to `fs::rename` the steps can be
        // performed straight through without one clobbering another. Applying
        // them here in the same order keeps the preview honest about what the
        // filesystem will actually do.
        let plan = rename_plan(&self.current_names(), &record.renames);
        self.apply_plan(&plan);

        self.undo_stack.push(record.clone());
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();

        self.history.push(record);
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }

        self.status_message = format!("Renamed {count} files");
    }

    /// Undo the last rename operation.
    ///
    /// Goes through [`rename_plan`] for the same reason [`Self::execute_rename`]
    /// does: undoing a batch is itself a batch, and reversing a swap or a
    /// shifted sequence by walking the pairs in stored order clobbers exactly
    /// as the forward direction would. The old implementation also never
    /// touched `original_path`, so an undone rename left the path naming the
    /// file it had just been renamed *away* from.
    fn undo(&mut self) {
        if let Some(record) = self.undo_stack.pop() {
            let reversed: Vec<(String, String)> = record
                .renames
                .iter()
                .map(|(old, new)| (new.clone(), old.clone()))
                .collect();
            let plan = rename_plan(&self.current_names(), &reversed);
            self.apply_plan(&plan);

            let count = record.renames.len();
            self.redo_stack.push(record);
            self.status_message = format!("Undid rename of {count} files");
            self.apply_operations();
        }
    }

    /// Redo the last undone rename.
    fn redo(&mut self) {
        if let Some(record) = self.redo_stack.pop() {
            let plan = rename_plan(&self.current_names(), &record.renames);
            self.apply_plan(&plan);

            let count = record.renames.len();
            self.undo_stack.push(record);
            self.status_message = format!("Redid rename of {count} files");
            self.apply_operations();
        }
    }

    /// The name every file in the list currently has on disk.
    fn current_names(&self) -> Vec<String> {
        self.files.iter().map(|f| f.original_name.clone()).collect()
    }

    /// Perform an ordered plan against the in-memory list.
    ///
    /// A step whose `from` names no file is skipped rather than ignored
    /// silently at a distance: it means the list and the plan have diverged,
    /// which cannot happen for a plan built from `current_names()` in the same
    /// call, and is a bug if it ever does.
    fn apply_plan(&mut self, plan: &[RenameStep]) {
        for step in plan {
            if let Some(file) = self.files.iter_mut().find(|f| f.original_name == step.from) {
                file.original_path = replace_file_name(&file.original_path, &step.to);
                file.original_name = step.to.clone();
            }
        }
        // Every file's preview should now show the name it actually has, or
        // the next `detect_conflicts` compares against stale targets.
        for file in &mut self.files {
            file.new_name = file.original_name.clone();
        }
    }

    /// Get filtered files.
    fn filtered_files(&self) -> Vec<(usize, &FileEntry)> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                if self.filter_conflicts && !f.conflict {
                    return false;
                }
                if !self.filter_extension.is_empty()
                    && !f.extension.eq_ignore_ascii_case(&self.filter_extension)
                {
                    return false;
                }
                if !self.search_text.is_empty() {
                    let lower = self.search_text.to_ascii_lowercase();
                    if !f.original_name.to_ascii_lowercase().contains(&lower)
                        && !f.new_name.to_ascii_lowercase().contains(&lower)
                    {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Count files that will be renamed (selected, changed, no conflict).
    fn rename_count(&self) -> usize {
        self.files
            .iter()
            .filter(|f| f.selected && f.original_name != f.new_name && !f.conflict)
            .count()
    }

    /// Count files with conflicts.
    fn conflict_count(&self) -> usize {
        self.files.iter().filter(|f| f.conflict).count()
    }

    /// Select or deselect all files.
    fn select_all(&mut self, selected: bool) {
        for file in &mut self.files {
            file.selected = selected;
        }
    }

    /// Clear all files from the list.
    fn clear_files(&mut self) {
        self.files.clear();
        self.selected_file = 0;
    }

    /// Clear all operations.
    fn clear_operations(&mut self) {
        self.operations.clear();
        self.apply_operations();
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(256);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Toolbar
        self.render_toolbar(&mut cmds);

        // Sidebar (operations list)
        self.render_sidebar(&mut cmds);

        // Main area (file list with old → new preview)
        self.render_file_list(&mut cmds);

        // Status bar
        self.render_status_bar(&mut cmds);

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: TOOLBAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 10.0,
            text: "Batch File Renamer".into(),
            font_size: TITLE_TEXT,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Toolbar buttons. Pair label with color so we never index out of bounds.
        let buttons = [
            ("Add Files", BLUE),
            ("Rename", GREEN),
            ("Undo", PEACH),
            ("Redo", PEACH),
            ("Clear", RED),
        ];
        let mut bx = 220.0;
        for (label, color) in buttons {
            let bw = text::padded_width(label, 10.0, 12.0, FontWeightHint::Regular);
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: 6.0,
                width: bw,
                height: BUTTON_HEIGHT,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 10.0,
                y: 12.0,
                text: label.into(),
                font_size: SMALL_TEXT,
                color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(bw - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            bx += bw + 6.0;
        }

        // File count
        let count_text = format!(
            "{} files | {} to rename | {} conflicts",
            self.files.len(),
            self.rename_count(),
            self.conflict_count()
        );
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 300.0,
            y: 14.0,
            text: count_text,
            font_size: SMALL_TEXT,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(290.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>) {
        let x = 0.0;
        let y = TOOLBAR_HEIGHT;
        let h = WINDOW_HEIGHT - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: SIDEBAR_WIDTH,
            height: h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Sidebar tabs
        let tabs = ["Operations", "Preview", "History"];
        let tab_w = SIDEBAR_WIDTH / 3.0;
        for (i, tab) in tabs.iter().enumerate() {
            let tx = x + i as f32 * tab_w;
            let is_active = match self.sidebar_panel {
                SidebarPanel::Operations => i == 0,
                SidebarPanel::Preview => i == 1,
                SidebarPanel::History => i == 2,
            };
            let bg = if is_active { SURFACE0 } else { MANTLE };
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y,
                width: tab_w,
                height: 28.0,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });
            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tx,
                    y,
                    width: tab_w,
                    height: 2.0,
                    color: BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }
            cmds.push(RenderCommand::Text {
                x: tx + 6.0,
                y: y + 8.0,
                text: (*tab).into(),
                font_size: SMALL_TEXT,
                color: if is_active { TEXT } else { SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_w - 12.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        let content_y = y + 32.0;

        match self.sidebar_panel {
            SidebarPanel::Operations => {
                self.render_operations_panel(cmds, x, content_y);
            }
            SidebarPanel::Preview => {
                self.render_preview_panel(cmds, x, content_y);
            }
            SidebarPanel::History => {
                self.render_history_panel(cmds, x, content_y);
            }
        }
    }

    fn render_operations_panel(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        if self.operations.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + PADDING,
                text: "No operations added yet.".into(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + PADDING + 18.0,
                text: "Add operations to see a".into(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + PADDING + 34.0,
                text: "live rename preview.".into(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        let mut oy = y + 4.0;
        for (i, op) in self.operations.iter().enumerate() {
            let is_selected = i == self.selected_op;
            let bg = if is_selected { SURFACE0 } else { MANTLE };

            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: oy,
                width: SIDEBAR_WIDTH - 8.0,
                height: 30.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });

            // Color indicator
            cmds.push(RenderCommand::FillRect {
                x: x + 8.0,
                y: oy + 6.0,
                width: 4.0,
                height: 18.0,
                color: op.color(),
                corner_radii: CornerRadii::all(2.0),
            });

            // Operation index and label
            cmds.push(RenderCommand::Text {
                x: x + 18.0,
                y: oy + 4.0,
                text: format!("{}. {}", i.saturating_add(1), op.label()),
                font_size: SMALL_TEXT,
                color: if is_selected { TEXT } else { SUBTEXT0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(SIDEBAR_WIDTH - 40.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Operation details
            // The user-typed halves of these summaries are elided against the
            // row's real width; the row is a fixed 34px, so wrapping is not an
            // option and a silent cut would leave two operations looking alike.
            let detail = match op {
                RenameOp::FindReplace { find, replace, .. } => {
                    find_replace_detail(find, replace, OP_DETAIL_WIDTH)
                }
                RenameOp::Insert { text, position } => {
                    let where_ = match position {
                        InsertPosition::Start => "start".to_string(),
                        InsertPosition::End => "end".to_string(),
                        InsertPosition::At(n) => format!("pos {n}"),
                    };
                    framed_detail("\"", text, &format!("\" at {where_}"), OP_DETAIL_WIDTH)
                }
                RenameOp::ChangeCase(mode) => mode.label().into(),
                RenameOp::Number {
                    start,
                    step,
                    padding,
                    ..
                } => format!("from {start} step {step} pad {padding}"),
                RenameOp::Extension(ext_op) => match ext_op {
                    ExtensionOp::Replace(e) => framed_detail("→ .", e, "", OP_DETAIL_WIDTH),
                    ExtensionOp::Add(e) => framed_detail("+ .", e, "", OP_DETAIL_WIDTH),
                    ExtensionOp::Remove => "remove".into(),
                    ExtensionOp::Lower => "lowercase".into(),
                    ExtensionOp::Upper => "UPPERCASE".into(),
                },
                _ => String::new(),
            };
            if !detail.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + 18.0,
                    y: oy + 17.0,
                    text: detail,
                    font_size: OP_DETAIL_SIZE,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(OP_DETAIL_WIDTH),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            oy += 34.0;
        }
    }

    fn render_preview_panel(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        if let Some(file) = self.files.get(self.selected_file) {
            let mut py = y + PADDING;

            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: py,
                text: "Selected File".into(),
                font_size: HEADER_TEXT,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
            py += 22.0;

            // Original name
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: py,
                text: "Original:".into(),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
            py += 16.0;
            cmds.push(RenderCommand::Text {
                x: x + PADDING + 8.0,
                y: py,
                text: file.original_name.clone(),
                font_size: NORMAL_TEXT,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 3.0),
                overflow: TextOverflow::Ellipsis,
            });
            py += 22.0;

            // New name
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: py,
                text: "New:".into(),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
            py += 16.0;
            let name_color = if file.conflict {
                RED
            } else if file.new_name != file.original_name {
                GREEN
            } else {
                TEXT
            };
            cmds.push(RenderCommand::Text {
                x: x + PADDING + 8.0,
                y: py,
                text: file.new_name.clone(),
                font_size: NORMAL_TEXT,
                color: name_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 3.0),
                overflow: TextOverflow::Ellipsis,
            });
            py += 22.0;

            if file.conflict {
                cmds.push(RenderCommand::Text {
                    x: x + PADDING,
                    y: py,
                    text: "⚠ Name conflict detected!".into(),
                    font_size: SMALL_TEXT,
                    color: RED,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
                    overflow: TextOverflow::Ellipsis,
                });
                py += 18.0;
            }

            // Metadata
            py += 8.0;
            cmds.push(RenderCommand::FillRect {
                x: x + PADDING,
                y: py,
                width: SIDEBAR_WIDTH - PADDING * 2.0,
                height: 1.0,
                color: SURFACE1,
                corner_radii: CornerRadii::ZERO,
            });
            py += 8.0;

            let size_str = format_size(file.size);
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: py,
                text: format!("Size: {size_str}"),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
            py += 16.0;

            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: py,
                text: format!(
                    "Extension: {}",
                    if file.extension.is_empty() {
                        "(none)"
                    } else {
                        &file.extension
                    }
                ),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + PADDING,
                text: "No file selected".into(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_history_panel(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        if self.history.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + PADDING,
                text: "No rename history yet.".into(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        let mut hy = y + 4.0;
        for (i, record) in self.history.iter().rev().enumerate().take(20) {
            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: hy,
                width: SIDEBAR_WIDTH - 8.0,
                height: 28.0,
                color: if i % 2 == 0 { MANTLE } else { SURFACE0 },
                corner_radii: CornerRadii::all(3.0),
            });

            let label = format!(
                "{} files — {}",
                record.renames.len(),
                record.operations.join(", ")
            );
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: hy + 7.0,
                text: label,
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 20.0),
                overflow: TextOverflow::Ellipsis,
            });

            hy += 30.0;
        }
    }

    fn render_file_list(&self, cmds: &mut Vec<RenderCommand>) {
        let x = SIDEBAR_WIDTH;
        let y = TOOLBAR_HEIGHT;
        let w = WINDOW_WIDTH - SIDEBAR_WIDTH;
        let h = WINDOW_HEIGHT - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Column headers
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let table = Table::new(FILE_COLUMNS, x);
        table.header(cmds, y + 5.0, SUBTEXT1, SMALL_TEXT);

        // File rows
        let filtered = self.filtered_files();
        let visible_rows = ((h - 24.0) / LINE_HEIGHT) as usize;
        let start = (self.scroll_offset / LINE_HEIGHT) as usize;

        let mut ry = y + 24.0;
        for (display_idx, (file_idx, file)) in
            filtered.iter().enumerate().skip(start).take(visible_rows)
        {
            let is_selected = *file_idx == self.selected_file;
            let bg = if is_selected {
                SURFACE0
            } else if display_idx % 2 == 0 {
                BASE
            } else {
                Color::from_hex(0x1F1F30) // Slightly lighter than base
            };

            cmds.push(RenderCommand::FillRect {
                x,
                y: ry,
                width: w,
                height: LINE_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });

            // The checkbox is a graphic, not a text cell, so it is placed
            // against its column's left edge by hand.
            let cx = table.left(COL_CHECK);

            let check_color = if file.selected { GREEN } else { SURFACE2 };
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: ry + 4.0,
                width: 14.0,
                height: 14.0,
                color: check_color,
                corner_radii: CornerRadii::all(2.0),
            });
            if file.selected {
                cmds.push(RenderCommand::Text {
                    x: cx + 2.0,
                    y: ry + 4.0,
                    text: "✓".into(),
                    font_size: 10.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(12.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            let changed = file.original_name != file.new_name;

            // Both names are cut at the *end* (`Fit::End`). This list is a
            // rename preview, and its whole job is to let the user check what
            // is about to happen to their files before committing. A name cut
            // the usual way loses the extension and any numeric suffix --
            // exactly the parts a rename usually changes -- so a batch of long
            // names would all read identically and the user would be
            // confirming a rename they cannot actually see.
            table.cell(
                cmds,
                COL_ORIGINAL,
                ry + 4.0,
                &file.original_name,
                TEXT,
                SMALL_TEXT,
                Fit::End,
            );

            table.cell_weighted(
                cmds,
                COL_ARROW,
                ry + 4.0,
                if changed { "→" } else { "=" },
                if changed { GREEN } else { OVERLAY0 },
                SMALL_TEXT,
                Fit::Start,
                FontWeightHint::Bold,
            );

            let new_color = if file.conflict {
                RED
            } else if changed {
                GREEN
            } else {
                SUBTEXT0
            };
            table.cell_weighted(
                cmds,
                COL_NEW,
                ry + 4.0,
                &file.new_name,
                new_color,
                SMALL_TEXT,
                Fit::End,
                if changed {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            );

            table.cell(
                cmds,
                COL_SIZE,
                ry + 4.0,
                &format_size(file.size),
                SUBTEXT0,
                SMALL_TEXT,
                Fit::Start,
            );

            let status = if file.conflict {
                ("Conflict", RED)
            } else if changed {
                ("Changed", GREEN)
            } else {
                ("", OVERLAY0)
            };
            if !status.0.is_empty() {
                table.cell_weighted(
                    cmds,
                    COL_STATUS,
                    ry + 4.0,
                    status.0,
                    status.1,
                    SMALL_TEXT,
                    Fit::Start,
                    FontWeightHint::Bold,
                );
            }

            ry += LINE_HEIGHT;
        }
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: WINDOW_WIDTH,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let msg = if self.status_message.is_empty() {
            format!(
                "Ready | {} files | {} selected | {} operations",
                self.files.len(),
                self.files.iter().filter(|f| f.selected).count(),
                self.operations.len()
            )
        } else {
            self.status_message.clone()
        };

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + 5.0,
            text: msg,
            font_size: SMALL_TEXT,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(WINDOW_WIDTH - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

// ============================================================================
// Utility
// ============================================================================

fn format_size(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let _app = RenamerApp::new();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // --- Find & Replace ---

    #[test]
    fn test_find_replace_basic() {
        let op = RenameOp::FindReplace {
            find: "old".into(),
            replace: "new".into(),
            case_sensitive: true,
            replace_all: false,
        };
        assert_eq!(RenameEngine::apply(&op, "old_file.txt", 0), "new_file.txt");
    }

    #[test]
    fn test_find_replace_all() {
        let op = RenameOp::FindReplace {
            find: "a".into(),
            replace: "b".into(),
            case_sensitive: true,
            replace_all: true,
        };
        assert_eq!(RenameEngine::apply(&op, "aaa.txt", 0), "bbb.txt");
    }

    #[test]
    fn test_find_replace_case_insensitive() {
        let op = RenameOp::FindReplace {
            find: "HELLO".into(),
            replace: "world".into(),
            case_sensitive: false,
            replace_all: false,
        };
        assert_eq!(
            RenameEngine::apply(&op, "hello_file.txt", 0),
            "world_file.txt"
        );
    }

    #[test]
    fn test_find_replace_no_match() {
        let op = RenameOp::FindReplace {
            find: "xyz".into(),
            replace: "abc".into(),
            case_sensitive: true,
            replace_all: false,
        };
        assert_eq!(RenameEngine::apply(&op, "test.txt", 0), "test.txt");
    }

    // --- Insert ---

    #[test]
    fn test_insert_start() {
        let op = RenameOp::Insert {
            text: "prefix_".into(),
            position: InsertPosition::Start,
        };
        assert_eq!(RenameEngine::apply(&op, "file.txt", 0), "prefix_file.txt");
    }

    #[test]
    fn test_insert_end() {
        let op = RenameOp::Insert {
            text: "_suffix".into(),
            position: InsertPosition::End,
        };
        assert_eq!(RenameEngine::apply(&op, "file.txt", 0), "file_suffix.txt");
    }

    #[test]
    fn test_insert_at_position() {
        let op = RenameOp::Insert {
            text: "-mid-".into(),
            position: InsertPosition::At(4),
        };
        assert_eq!(
            RenameEngine::apply(&op, "filename.txt", 0),
            "file-mid-name.txt"
        );
    }

    // --- Remove ---

    #[test]
    fn test_remove_characters() {
        let op = RenameOp::Remove { from: 0, count: 5 };
        assert_eq!(RenameEngine::apply(&op, "prefix_file.txt", 0), "x_file.txt");
    }

    #[test]
    fn test_remove_middle() {
        let op = RenameOp::Remove { from: 2, count: 3 };
        assert_eq!(RenameEngine::apply(&op, "abcdefg.txt", 0), "abfg.txt");
    }

    #[test]
    fn test_remove_beyond_length() {
        let op = RenameOp::Remove {
            from: 0,
            count: 100,
        };
        assert_eq!(RenameEngine::apply(&op, "short.txt", 0), ".txt");
    }

    // --- Case change ---

    #[test]
    fn test_case_upper() {
        let op = RenameOp::ChangeCase(CaseMode::Upper);
        assert_eq!(RenameEngine::apply(&op, "hello.txt", 0), "HELLO.txt");
    }

    #[test]
    fn test_case_lower() {
        let op = RenameOp::ChangeCase(CaseMode::Lower);
        assert_eq!(RenameEngine::apply(&op, "HELLO.txt", 0), "hello.txt");
    }

    #[test]
    fn test_case_title() {
        let op = RenameOp::ChangeCase(CaseMode::Title);
        assert_eq!(
            RenameEngine::apply(&op, "hello world.txt", 0),
            "Hello World.txt"
        );
    }

    #[test]
    fn test_case_sentence() {
        let op = RenameOp::ChangeCase(CaseMode::Sentence);
        assert_eq!(
            RenameEngine::apply(&op, "HELLO WORLD.txt", 0),
            "Hello world.txt"
        );
    }

    #[test]
    fn test_case_toggle() {
        let op = RenameOp::ChangeCase(CaseMode::Toggle);
        assert_eq!(RenameEngine::apply(&op, "Hello.txt", 0), "hELLO.txt");
    }

    #[test]
    fn test_case_snake() {
        let op = RenameOp::ChangeCase(CaseMode::SnakeCase);
        assert_eq!(
            RenameEngine::apply(&op, "HelloWorld.txt", 0),
            "hello_world.txt"
        );
    }

    #[test]
    fn test_case_kebab() {
        let op = RenameOp::ChangeCase(CaseMode::KebabCase);
        assert_eq!(
            RenameEngine::apply(&op, "HelloWorld.txt", 0),
            "hello-world.txt"
        );
    }

    #[test]
    fn test_case_camel() {
        let op = RenameOp::ChangeCase(CaseMode::CamelCase);
        assert_eq!(
            RenameEngine::apply(&op, "hello_world.txt", 0),
            "helloWorld.txt"
        );
    }

    // --- Numbering ---

    #[test]
    fn test_number_start() {
        let op = RenameOp::Number {
            start: 1,
            step: 1,
            padding: 3,
            position: InsertPosition::Start,
            separator: "_".into(),
        };
        assert_eq!(RenameEngine::apply(&op, "file.txt", 0), "001_file.txt");
        assert_eq!(RenameEngine::apply(&op, "file.txt", 4), "005_file.txt");
    }

    #[test]
    fn test_number_end() {
        let op = RenameOp::Number {
            start: 1,
            step: 1,
            padding: 2,
            position: InsertPosition::End,
            separator: "-".into(),
        };
        assert_eq!(RenameEngine::apply(&op, "file.txt", 0), "file-01.txt");
    }

    // --- Date stamp ---

    #[test]
    fn test_date_stamp_ymd() {
        let op = RenameOp::DateStamp {
            format: DateFormat::YmdHyphen,
            position: InsertPosition::Start,
            separator: "_".into(),
        };
        let result = RenameEngine::apply(&op, "photo.jpg", 0);
        assert!(result.starts_with("2026-05-18_"));
        assert!(result.ends_with(".jpg"));
    }

    #[test]
    fn test_date_stamp_compact() {
        let op = RenameOp::DateStamp {
            format: DateFormat::YmdCompact,
            position: InsertPosition::End,
            separator: "_".into(),
        };
        let result = RenameEngine::apply(&op, "photo.jpg", 0);
        assert!(result.contains("20260518"));
    }

    // --- Extension ---

    #[test]
    fn test_extension_replace() {
        let op = RenameOp::Extension(ExtensionOp::Replace("png".into()));
        assert_eq!(RenameEngine::apply(&op, "image.jpg", 0), "image.png");
    }

    #[test]
    fn test_extension_add() {
        let op = RenameOp::Extension(ExtensionOp::Add("bak".into()));
        assert_eq!(RenameEngine::apply(&op, "file.txt", 0), "file.txt.bak");
    }

    #[test]
    fn test_extension_remove() {
        let op = RenameOp::Extension(ExtensionOp::Remove);
        assert_eq!(RenameEngine::apply(&op, "file.txt", 0), "file");
    }

    #[test]
    fn test_extension_lower() {
        let op = RenameOp::Extension(ExtensionOp::Lower);
        assert_eq!(RenameEngine::apply(&op, "file.TXT", 0), "file.txt");
    }

    #[test]
    fn test_extension_upper() {
        let op = RenameOp::Extension(ExtensionOp::Upper);
        assert_eq!(RenameEngine::apply(&op, "file.txt", 0), "file.TXT");
    }

    // --- Trim ---

    #[test]
    fn test_trim_whitespace() {
        let op = RenameOp::Trim {
            chars: String::new(),
            mode: TrimMode::Both,
        };
        assert_eq!(RenameEngine::apply(&op, "  file  .txt", 0), "file.txt");
    }

    #[test]
    fn test_trim_custom_chars() {
        let op = RenameOp::Trim {
            chars: "_-".into(),
            mode: TrimMode::Both,
        };
        assert_eq!(RenameEngine::apply(&op, "__file__.txt", 0), "file.txt");
    }

    #[test]
    fn test_trim_start() {
        let op = RenameOp::Trim {
            chars: String::new(),
            mode: TrimMode::Start,
        };
        assert_eq!(RenameEngine::apply(&op, "  file  .txt", 0), "file  .txt");
    }

    // --- Template ---

    #[test]
    fn test_template() {
        let op = RenameOp::Template {
            template: "{name}_{N}.{ext}".into(),
        };
        assert_eq!(RenameEngine::apply(&op, "photo.jpg", 5), "photo_005.jpg");
    }

    #[test]
    fn test_template_original() {
        let op = RenameOp::Template {
            template: "backup_{original}".into(),
        };
        assert_eq!(RenameEngine::apply(&op, "file.txt", 0), "backup_file.txt");
    }

    // --- App state ---

    #[test]
    fn test_app_add_file() {
        let mut app = RenamerApp::new();
        app.add_file("/home/test.txt", "test.txt", 1024, 0);
        assert_eq!(app.files.len(), 1);
        assert_eq!(app.files[0].original_name, "test.txt");
    }

    #[test]
    fn test_app_add_operation() {
        let mut app = RenamerApp::new();
        app.add_file("/home/old.txt", "old.txt", 0, 0);
        app.add_operation(RenameOp::FindReplace {
            find: "old".into(),
            replace: "new".into(),
            case_sensitive: true,
            replace_all: false,
        });
        assert_eq!(app.files[0].new_name, "new.txt");
    }

    #[test]
    fn test_app_operation_chain() {
        let mut app = RenamerApp::new();
        app.add_file("/home/file.txt", "file.txt", 0, 0);
        app.add_operation(RenameOp::ChangeCase(CaseMode::Upper));
        app.add_operation(RenameOp::Insert {
            text: "prefix_".into(),
            position: InsertPosition::Start,
        });
        assert_eq!(app.files[0].new_name, "prefix_FILE.txt");
    }

    #[test]
    fn test_app_conflict_detection() {
        let mut app = RenamerApp::new();
        app.add_file("/a.txt", "a.txt", 0, 0);
        app.add_file("/b.txt", "b.txt", 0, 0);
        // Rename both to the same name
        app.add_operation(RenameOp::FindReplace {
            find: "a".into(),
            replace: "same".into(),
            case_sensitive: true,
            replace_all: false,
        });
        app.add_operation(RenameOp::FindReplace {
            find: "b".into(),
            replace: "same".into(),
            case_sensitive: true,
            replace_all: false,
        });
        assert!(app.files.iter().any(|f| f.conflict));
    }

    #[test]
    fn test_app_remove_operation() {
        let mut app = RenamerApp::new();
        app.add_file("/test.txt", "test.txt", 0, 0);
        app.add_operation(RenameOp::ChangeCase(CaseMode::Upper));
        assert_eq!(app.files[0].new_name, "TEST.txt");
        app.remove_operation(0);
        assert_eq!(app.files[0].new_name, "test.txt");
    }

    #[test]
    fn test_app_select_all() {
        let mut app = RenamerApp::new();
        app.add_file("/a.txt", "a.txt", 0, 0);
        app.add_file("/b.txt", "b.txt", 0, 0);
        app.select_all(false);
        assert!(app.files.iter().all(|f| !f.selected));
        app.select_all(true);
        assert!(app.files.iter().all(|f| f.selected));
    }

    #[test]
    fn test_app_rename_count() {
        let mut app = RenamerApp::new();
        app.add_file("/a.txt", "a.txt", 0, 0);
        app.add_file("/b.txt", "b.txt", 0, 0);
        assert_eq!(app.rename_count(), 0);
        app.add_operation(RenameOp::ChangeCase(CaseMode::Upper));
        assert_eq!(app.rename_count(), 2);
    }

    #[test]
    fn test_app_clear_files() {
        let mut app = RenamerApp::new();
        app.add_file("/a.txt", "a.txt", 0, 0);
        app.clear_files();
        assert!(app.files.is_empty());
    }

    #[test]
    fn test_app_clear_operations() {
        let mut app = RenamerApp::new();
        app.add_file("/a.txt", "a.txt", 0, 0);
        app.add_operation(RenameOp::ChangeCase(CaseMode::Upper));
        app.clear_operations();
        assert!(app.operations.is_empty());
        assert_eq!(app.files[0].new_name, "a.txt");
    }

    #[test]
    fn test_app_render_nonempty() {
        let app = RenamerApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // --- Operation-list detail lines ---

    /// The operation-list detail lines drawn for `ops`.
    fn op_detail_lines(ops: Vec<RenameOp>) -> Vec<String> {
        let mut app = RenamerApp::new();
        app.operations = ops;
        app.render()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text, font_size, ..
                } if (font_size - OP_DETAIL_SIZE).abs() < f32::EPSILON => Some(text),
                _ => None,
            })
            .collect()
    }

    /// Whatever the user typed, the detail line fits the row it is drawn in.
    ///
    /// The row is a fixed 34px in a fixed-width sidebar, so a `Text` command's
    /// `max_width` would cut a long pattern silently — leaving two different
    /// operations reading identically in the list.
    #[test]
    fn an_operation_detail_fits_its_row() {
        let long = "W".repeat(120);
        let ops = vec![
            RenameOp::FindReplace {
                find: long.clone(),
                replace: long.clone(),
                replace_all: true,
                case_sensitive: true,
            },
            RenameOp::Insert {
                text: long.clone(),
                position: InsertPosition::Start,
            },
            RenameOp::Extension(ExtensionOp::Replace(long.clone())),
            RenameOp::Extension(ExtensionOp::Add(long)),
        ];
        let expected = ops.len();
        let mut checked = 0;
        for line in op_detail_lines(ops) {
            let measured = text::measure(&line, OP_DETAIL_SIZE, FontWeightHint::Regular);
            assert!(
                measured <= OP_DETAIL_WIDTH + 0.5,
                "detail {line:?} measures {measured} in a {OP_DETAIL_WIDTH} row",
            );
            checked += 1;
        }
        assert!(
            checked >= expected,
            "expected {expected} details, saw {checked}"
        );
    }

    /// A long search string must not push the replacement off the end: the
    /// replacement is the half that says what the rename will actually do.
    #[test]
    fn a_long_search_string_does_not_hide_the_replacement() {
        let lines = op_detail_lines(vec![RenameOp::FindReplace {
            find: "W".repeat(120),
            replace: "keepme".to_string(),
            replace_all: true,
            case_sensitive: true,
        }]);
        assert_eq!(lines.len(), 1, "expected one detail line: {lines:?}");
        assert!(
            lines[0].contains("keepme"),
            "the replacement was elided away: {:?}",
            lines[0],
        );
        assert!(
            lines[0].contains('…'),
            "expected the cut to be marked: {:?}",
            lines[0]
        );
    }

    /// The developer-authored frame survives, so an insert still says *where*
    /// it inserts even when the inserted text is far too long for the row.
    #[test]
    fn a_long_insert_still_says_where_it_inserts() {
        let lines = op_detail_lines(vec![RenameOp::Insert {
            text: "W".repeat(120),
            position: InsertPosition::End,
        }]);
        assert_eq!(lines.len(), 1, "expected one detail line: {lines:?}");
        assert!(
            lines[0].ends_with("\" at end"),
            "the position was elided away: {:?}",
            lines[0],
        );
    }

    /// Text that already fits is left alone — no ellipsis on the common case.
    #[test]
    fn a_short_operation_detail_is_not_elided() {
        let lines = op_detail_lines(vec![RenameOp::FindReplace {
            find: "a".to_string(),
            replace: "b".to_string(),
            replace_all: true,
            case_sensitive: true,
        }]);
        assert_eq!(lines, vec!["\"a\" → \"b\"".to_string()]);
    }

    #[test]
    fn test_app_execute_rename() {
        let mut app = RenamerApp::new();
        app.add_file("/old.txt", "old.txt", 100, 0);
        app.add_operation(RenameOp::FindReplace {
            find: "old".into(),
            replace: "new".into(),
            case_sensitive: true,
            replace_all: false,
        });
        app.execute_rename();
        assert_eq!(app.undo_stack.len(), 1);
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn test_move_operation() {
        let mut app = RenamerApp::new();
        app.add_file("/test.txt", "test.txt", 0, 0);
        app.add_operation(RenameOp::ChangeCase(CaseMode::Upper));
        app.add_operation(RenameOp::Insert {
            text: "x".into(),
            position: InsertPosition::Start,
        });
        // After: [Upper, Insert "x"] → "xTEST.txt"
        assert_eq!(app.files[0].new_name, "xTEST.txt");

        app.move_operation_up(1); // Move Insert before Upper
        // After: [Insert "x", Upper] → "XTEST.txt"
        assert_eq!(app.files[0].new_name, "XTEST.txt");
    }

    // --- Utility ---

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GiB");
    }

    #[test]
    fn test_split_name() {
        assert_eq!(FileEntry::split_name("file.txt"), ("file", ".txt"));
        assert_eq!(FileEntry::split_name("no_extension"), ("no_extension", ""));
        assert_eq!(FileEntry::split_name(".hidden"), (".hidden", ""));
        assert_eq!(FileEntry::split_name("a.b.c"), ("a.b", ".c"));
    }

    #[test]
    fn test_date_formats() {
        assert_eq!(
            DateFormat::YmdHyphen.format(2024, 1, 15, 14, 30, 0),
            "2024-01-15"
        );
        assert_eq!(
            DateFormat::DmyHyphen.format(2024, 1, 15, 14, 30, 0),
            "15-01-2024"
        );
        assert_eq!(
            DateFormat::YmdCompact.format(2024, 1, 15, 14, 30, 0),
            "20240115"
        );
        assert_eq!(
            DateFormat::Timestamp.format(2024, 1, 15, 14, 30, 45),
            "20240115_143045"
        );
    }

    #[test]
    fn test_filtered_files() {
        let mut app = RenamerApp::new();
        app.add_file("/a.txt", "a.txt", 0, 0);
        app.add_file("/b.jpg", "b.jpg", 0, 0);
        app.add_file("/c.txt", "c.txt", 0, 0);

        app.filter_extension = "txt".into();
        let filtered = app.filtered_files();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_search_filter() {
        let mut app = RenamerApp::new();
        app.add_file("/alpha.txt", "alpha.txt", 0, 0);
        app.add_file("/beta.txt", "beta.txt", 0, 0);
        app.add_file("/gamma.txt", "gamma.txt", 0, 0);

        app.search_text = "alpha".into();
        let filtered = app.filtered_files();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_case_insensitive_replace() {
        let result = RenameEngine::case_insensitive_replace("Hello World", "HELLO", "Hi", false);
        assert_eq!(result, "Hi World");
    }

    #[test]
    fn test_case_insensitive_replace_all() {
        let result = RenameEngine::case_insensitive_replace("aAbBaA", "a", "X", true);
        assert_eq!(result, "XXbBXX");
    }

    // --- Extension parsing ---

    #[test]
    fn an_extension_is_what_follows_the_last_dot() {
        let ext = |name: &str| FileEntry::new("/p", name, 0, 0).extension;
        assert_eq!(ext("photo.jpg"), "jpg");
        assert_eq!(ext("archive.tar.gz"), "gz");
        // A dotless name has no extension -- it is not its own extension.
        assert_eq!(ext("readme"), "");
        // A leading dot makes a name, not an extension.
        assert_eq!(ext(".bashrc"), "");
        // A trailing dot leaves an empty extension, not the stem.
        assert_eq!(ext("weird."), "");
    }

    #[test]
    fn a_dotfile_is_not_swept_up_by_an_extension_filter() {
        let mut app = RenamerApp::new();
        app.add_file("/a/.bashrc", ".bashrc", 0, 0);
        app.add_file("/a/backup.bashrc", "backup.bashrc", 0, 0);
        app.filter_extension = "bashrc".into();
        let filtered = app.filtered_files();
        assert_eq!(
            filtered.len(),
            1,
            "only the real .bashrc-suffixed file matches, got {:?}",
            filtered
                .iter()
                .map(|(_, f)| f.original_name.clone())
                .collect::<Vec<_>>()
        );
    }

    // --- File list layout ---

    /// Two files whose names are far wider than the 250px name columns, with a
    /// pending operation so both rows are in the "changed" state that draws
    /// every cell (arrow, bold new name, status).
    fn app_with_overlong_names() -> RenamerApp {
        let mut app = RenamerApp::new();
        app.add_file(
            "/media/Season 1/ep.mkv",
            "A Very Long Show Name - Season 01 - Episode 07 - The One With The Long Title.mkv",
            1_234_567,
            0,
        );
        app.add_file(
            "/media/Season 1/ep2.mkv",
            "A Very Long Show Name - Season 01 - Episode 08 - The One With The Other Title.mkv",
            2_345_678,
            0,
        );
        app.operations.push(RenameOp::FindReplace {
            find: "Episode".into(),
            replace: "Ep".into(),
            case_sensitive: true,
            replace_all: false,
        });
        app.apply_operations();
        app
    }

    /// Assert that every bounded text command sitting at a column's left edge
    /// draws inside that column, and that `expected` of them were actually
    /// inspected — without the count the assertion passes vacuously if the
    /// list stopped drawing rows at all.
    fn assert_file_cells_fit(cmds: &[RenderCommand], expected: usize) {
        let edges = Table::new(FILE_COLUMNS, SIDEBAR_WIDTH).spans();
        let mut checked = 0usize;
        for cmd in cmds {
            let RenderCommand::Text {
                x: tx,
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
            let Some(&(_, right)) = edges.iter().find(|(left, _)| (left - tx).abs() < 0.01) else {
                continue;
            };
            let drawn = tx + text::measure(text, *font_size, *font_weight);
            assert!(
                drawn <= right + 0.5,
                "cell {text:?} starting at {tx} runs to {drawn}, \
                 past its column's right edge {right}",
            );
            checked = checked.saturating_add(1);
        }
        assert!(
            checked >= expected,
            "only {checked} file-list cells checked, expected at least {expected}",
        );
    }

    /// The texts drawn in one column of the file list, header included.
    fn cells_in_column(cmds: &[RenderCommand], index: usize) -> Vec<String> {
        let left = Table::new(FILE_COLUMNS, SIDEBAR_WIDTH).left(index);
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x: tx,
                    text,
                    max_width: Some(_),
                    overflow: TextOverflow::Ellipsis,
                    ..
                } if (tx - left).abs() < 0.01 => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn no_file_row_cell_escapes_its_column() {
        let app = app_with_overlong_names();
        let mut cmds = Vec::new();
        // Render the list directly rather than the whole app: a full render
        // puts sidebar and toolbar text at x values that can coincide with a
        // column's left edge, and the assertion would then fail on chrome that
        // was never part of this table.
        app.render_file_list(&mut cmds);
        // 6 header labels + 2 rows x 5 cells (name, arrow, new name, size,
        // status).
        assert_file_cells_fit(&cmds, 16);
    }

    #[test]
    fn an_overlong_rename_preview_keeps_the_end_of_both_names() {
        let app = app_with_overlong_names();
        let mut cmds = Vec::new();
        app.render_file_list(&mut cmds);

        // The whole point of the preview is to show what changes. Both of
        // these names differ only in their tail, so a name cut the usual way
        // would render the two rows identically and hide the rename.
        let originals = cells_in_column(&cmds, COL_ORIGINAL);
        let old = originals
            .iter()
            .find(|t| t.contains("Episode 07"))
            .unwrap_or_else(|| panic!("row 1's original name should be drawn, got {originals:?}"));
        assert!(
            old.starts_with('…'),
            "the cut should be marked at the front, got {old:?}"
        );
        assert!(
            old.ends_with("The One With The Long Title.mkv"),
            "the tail identifies the file and must survive, got {old:?}"
        );

        let news = cells_in_column(&cmds, COL_NEW);
        let new = news
            .iter()
            .find(|t| t.contains("Ep 07"))
            .unwrap_or_else(|| panic!("row 1's new name should be drawn, got {news:?}"));
        assert!(
            new.ends_with(".mkv"),
            "a rename preview that drops the extension is unreviewable, got {new:?}"
        );
        assert_ne!(
            old, new,
            "the two names must not render identically or the preview shows nothing"
        );
    }

    #[test]
    fn a_short_name_is_drawn_verbatim() {
        let mut app = RenamerApp::new();
        app.add_file("/a.txt", "notes.txt", 12, 0);
        let mut cmds = Vec::new();
        app.render_file_list(&mut cmds);
        let originals = cells_in_column(&cmds, COL_ORIGINAL);
        assert!(
            originals.iter().any(|t| t == "notes.txt"),
            "a name that fits must not be marked as cut, got {originals:?}"
        );
    }

    #[test]
    fn the_header_and_the_rows_agree_on_where_a_column_starts() {
        let app = app_with_overlong_names();
        let mut cmds = Vec::new();
        app.render_file_list(&mut cmds);
        // The header label and the body cells of the size column share an x.
        // Three copies of a width is what let these drift apart before the
        // table became a single `&[Column]`.
        let sizes = cells_in_column(&cmds, COL_SIZE);
        assert!(
            sizes.contains(&"Size".to_string()),
            "the Size header should sit at the Size column's left edge, got {sizes:?}"
        );
        assert!(
            sizes.len() >= 3,
            "the header plus both rows' sizes should share that edge, got {sizes:?}"
        );
    }
    // --- Positional rules count characters, not bytes ---

    /// Names whose stems are not pure ASCII, so that a character position and a
    /// byte position are different numbers, and most byte positions are not
    /// character boundaries at all.
    fn adversarial_names() -> Vec<&'static str> {
        vec![
            "\u{65e5}\u{672c}\u{8a9e}.txt",        // 3 chars, 9 bytes
            "\u{03b1}\u{03b2}\u{03b3}\u{03b4}.md", // Greek, 2 bytes/char
            "\u{0440}\u{0435}\u{0437}\u{0443}\u{043c}\u{0435}.pdf", // Cyrillic
            "\u{1f600}\u{1f601}\u{1f602}.png",     // emoji, 4 bytes/char
            "caf\u{e9}_r\u{e9}sum\u{e9}.doc",      // mostly ASCII, some not
            "a\u{65e5}b\u{672c}c.jpg",             // alternating widths
            "\u{65e5}",                            // no extension at all
            "\u{4e2d}\u{6587}\u{6587}\u{4ef6}\u{540d}\u{79f0}.tar.gz",
        ]
    }

    /// Every rule that takes a position, at every position from before the name
    /// to past its end.
    fn positional_ops(pos: usize) -> Vec<RenameOp> {
        vec![
            RenameOp::Insert {
                text: "-mid-".into(),
                position: InsertPosition::At(pos),
            },
            RenameOp::Remove {
                from: pos,
                count: 2,
            },
            RenameOp::Number {
                start: 1,
                step: 1,
                padding: 3,
                position: InsertPosition::At(pos),
                separator: "_".into(),
            },
            RenameOp::DateStamp {
                format: DateFormat::YmdHyphen,
                position: InsertPosition::At(pos),
                separator: "_".into(),
            },
        ]
    }

    #[test]
    fn a_non_ascii_name_does_not_abort_a_positional_rule() {
        // A rename batch runs every rule over every file. One name whose byte
        // length exceeds the position the user typed used to slice inside a
        // character and abort the renamer partway through the batch — after
        // some files on disk had already been renamed.
        let mut checked = 0usize;
        for name in adversarial_names() {
            let (_, ext) = FileEntry::split_name(name);
            // Well past the longest stem here, in both characters and bytes.
            for pos in 0..24 {
                for op in positional_ops(pos) {
                    let out = RenameEngine::apply(&op, name, 0);
                    // Every positional rule rewrites the stem and re-appends
                    // the extension untouched.
                    assert!(
                        out.ends_with(ext),
                        "{op:?} at {pos} on {name:?} lost the extension: {out:?}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(
            checked >= 700,
            "expected the sweep to exercise every name/position/op, got {checked}"
        );
    }

    #[test]
    fn insert_at_a_position_counts_characters() {
        let op = RenameOp::Insert {
            text: "-mid-".into(),
            position: InsertPosition::At(1),
        };
        // Position 1 is after the first *character*. As a byte offset it lands
        // inside the first kanji, which is where this used to abort.
        assert_eq!(
            RenameEngine::apply(&op, "\u{65e5}\u{672c}\u{8a9e}.txt", 0),
            "\u{65e5}-mid-\u{672c}\u{8a9e}.txt"
        );
        let op = RenameOp::Insert {
            text: "!".into(),
            position: InsertPosition::At(2),
        };
        assert_eq!(
            RenameEngine::apply(&op, "\u{1f600}\u{1f601}\u{1f602}.png", 0),
            "\u{1f600}\u{1f601}!\u{1f602}.png"
        );
    }

    #[test]
    fn remove_counts_characters_at_both_ends() {
        // "remove 1 character starting at character 1" — not "1 byte at byte 1".
        let op = RenameOp::Remove { from: 1, count: 1 };
        assert_eq!(
            RenameEngine::apply(&op, "\u{65e5}\u{672c}\u{8a9e}.txt", 0),
            "\u{65e5}\u{8a9e}.txt"
        );
        // A count that runs past the end takes the rest of the stem and stops.
        let op = RenameOp::Remove { from: 1, count: 99 };
        assert_eq!(
            RenameEngine::apply(&op, "\u{65e5}\u{672c}\u{8a9e}.txt", 0),
            "\u{65e5}.txt"
        );
    }

    #[test]
    fn number_and_datestamp_insert_at_a_character_position() {
        let op = RenameOp::Number {
            start: 1,
            step: 1,
            padding: 3,
            position: InsertPosition::At(1),
            separator: "_".into(),
        };
        assert_eq!(
            RenameEngine::apply(&op, "\u{65e5}\u{672c}\u{8a9e}.txt", 0),
            "\u{65e5}_001_\u{672c}\u{8a9e}.txt"
        );
        let op = RenameOp::DateStamp {
            format: DateFormat::YmdHyphen,
            position: InsertPosition::At(2),
            separator: "_".into(),
        };
        assert_eq!(
            RenameEngine::apply(&op, "\u{65e5}\u{672c}\u{8a9e}.txt", 0),
            "\u{65e5}\u{672c}_2026-05-18_\u{8a9e}.txt"
        );
    }

    #[test]
    fn a_position_past_the_end_clamps_to_the_end() {
        // Clamping is what `.min(stem.len())` was trying to do; it just measured
        // the wrong quantity. Past-the-end must still mean "append".
        let op = RenameOp::Insert {
            text: "_z".into(),
            position: InsertPosition::At(99),
        };
        assert_eq!(
            RenameEngine::apply(&op, "\u{65e5}\u{672c}\u{8a9e}.txt", 0),
            "\u{65e5}\u{672c}\u{8a9e}_z.txt"
        );
        // Exactly at the end of a 3-character stem is the same thing.
        let op = RenameOp::Insert {
            text: "_z".into(),
            position: InsertPosition::At(3),
        };
        assert_eq!(
            RenameEngine::apply(&op, "\u{65e5}\u{672c}\u{8a9e}.txt", 0),
            "\u{65e5}\u{672c}\u{8a9e}_z.txt"
        );
    }

    #[test]
    fn an_ascii_name_is_positioned_exactly_as_before() {
        // For ASCII the character count and the byte count are the same number,
        // so this change must be invisible to every existing rule.
        let op = RenameOp::Insert {
            text: "-mid-".into(),
            position: InsertPosition::At(4),
        };
        assert_eq!(
            RenameEngine::apply(&op, "filename.txt", 0),
            "file-mid-name.txt"
        );
        let op = RenameOp::Remove { from: 2, count: 3 };
        assert_eq!(RenameEngine::apply(&op, "abcdefg.txt", 0), "abfg.txt");
    }

    // ------------------------------------------------------------------
    // Batch ordering, conflict detection and path rewriting
    // ------------------------------------------------------------------

    /// An app holding `names`, all in `/photos`, all selected.
    fn app_with(names: &[&str]) -> RenamerApp {
        let mut app = RenamerApp::new();
        for name in names {
            app.add_file(&format!("/photos/{name}"), name, 100, 0);
        }
        app
    }

    /// Set each file's previewed new name directly, bypassing the rule chain,
    /// and recompute the conflict flags — the same two things
    /// `apply_operations` does, minus the rules themselves.
    fn preview(app: &mut RenamerApp, new_names: &[&str]) {
        for (file, name) in app.files.iter_mut().zip(new_names.iter()) {
            file.new_name = (*name).to_string();
        }
        app.detect_conflicts();
    }

    fn names_of(app: &RenamerApp) -> Vec<String> {
        app.files.iter().map(|f| f.original_name.clone()).collect()
    }

    fn paths_of(app: &RenamerApp) -> Vec<String> {
        app.files.iter().map(|f| f.original_path.clone()).collect()
    }

    fn conflicts_of(app: &RenamerApp) -> Vec<bool> {
        app.files.iter().map(|f| f.conflict).collect()
    }

    #[test]
    fn renaming_a_file_updates_the_path_it_is_stored_under() {
        let mut app = app_with(&["old.txt"]);
        preview(&mut app, &["new.txt"]);
        app.execute_rename();
        assert_eq!(names_of(&app), vec!["new.txt"]);
        // The path used to be left naming the old file, so a second rename in
        // the same session would have been performed against a stale path.
        assert_eq!(paths_of(&app), vec!["/photos/new.txt"]);
    }

    #[test]
    fn a_directory_that_shares_the_file_name_is_not_rewritten() {
        // `path.replace(old, new)` rewrites *every* occurrence, not just the
        // last component, so a directory whose name contains the file's name
        // is renamed along with it -- leaving a path under a directory that
        // does not exist. Two shapes this really takes: a folder named after
        // the file it holds, and a folder whose name merely *contains* it.
        assert_eq!(
            replace_file_name("/music/Nirvana/Nirvana", "Nevermind"),
            "/music/Nirvana/Nevermind"
        );
        assert_eq!(
            replace_file_name("/report-archive/report", "summary"),
            "/report-archive/summary"
        );

        let mut app = RenamerApp::new();
        app.add_file("/music/Nirvana/Nirvana", "Nirvana", 100, 0);
        app.add_file("/report-archive/report", "report", 100, 0);
        preview(&mut app, &["Nevermind", "summary"]);
        app.execute_rename();
        assert_eq!(
            paths_of(&app),
            vec!["/music/Nirvana/Nevermind", "/report-archive/summary"]
        );
    }

    #[test]
    fn a_bare_name_and_a_backslash_path_both_replace_only_the_last_component() {
        assert_eq!(replace_file_name("a.txt", "b.txt"), "b.txt");
        assert_eq!(replace_file_name("d\\a.txt", "b.txt"), "d\\b.txt");
        assert_eq!(replace_file_name("/", "b.txt"), "/b.txt");
    }

    #[test]
    fn a_case_only_rename_is_not_a_conflict() {
        // Slate OS's filesystem is case-sensitive (`design.txt`), so
        // `photo.jpg` and `PHOTO.jpg` are two different files. The old
        // `eq_ignore_ascii_case` comparison refused this rename as a
        // collision with a file it cannot in fact collide with.
        let mut app = app_with(&["photo.JPG", "PHOTO.jpg"]);
        app.files[1].selected = false;
        preview(&mut app, &["photo.jpg", "PHOTO.jpg"]);
        assert_eq!(conflicts_of(&app), vec![false, false]);
    }

    #[test]
    fn shifting_a_numbered_sequence_is_allowed_and_ordered_safely() {
        // The single most common bulk rename there is. The old check compared
        // each new name against every other file's *original* name, so this
        // was flagged as three conflicts and refused outright.
        let mut app = app_with(&["1.jpg", "2.jpg", "3.jpg"]);
        preview(&mut app, &["2.jpg", "3.jpg", "4.jpg"]);
        assert_eq!(
            conflicts_of(&app),
            vec![false, false, false],
            "a name another file is vacating is an ordering constraint, not a collision"
        );

        app.execute_rename();
        assert_eq!(names_of(&app), vec!["2.jpg", "3.jpg", "4.jpg"]);
        assert_eq!(
            paths_of(&app),
            vec!["/photos/2.jpg", "/photos/3.jpg", "/photos/4.jpg"]
        );
    }

    #[test]
    fn a_shifted_sequence_is_planned_from_the_far_end_first() {
        let plan = rename_plan(
            &[
                "1.jpg".to_string(),
                "2.jpg".to_string(),
                "3.jpg".to_string(),
            ],
            &[
                ("1.jpg".to_string(), "2.jpg".to_string()),
                ("2.jpg".to_string(), "3.jpg".to_string()),
                ("3.jpg".to_string(), "4.jpg".to_string()),
            ],
        );
        assert_eq!(
            plan,
            vec![
                RenameStep {
                    from: "3.jpg".to_string(),
                    to: "4.jpg".to_string()
                },
                RenameStep {
                    from: "2.jpg".to_string(),
                    to: "3.jpg".to_string()
                },
                RenameStep {
                    from: "1.jpg".to_string(),
                    to: "2.jpg".to_string()
                },
            ]
        );
    }

    #[test]
    fn swapping_two_names_parks_one_under_a_temporary_name() {
        let plan = rename_plan(
            &["a.txt".to_string(), "b.txt".to_string()],
            &[
                ("a.txt".to_string(), "b.txt".to_string()),
                ("b.txt".to_string(), "a.txt".to_string()),
            ],
        );
        // Three steps, because no two-step order exists: whichever ran first
        // would overwrite the other file.
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].from, "a.txt");
        let temp = plan[0].to.clone();
        assert!(temp != "a.txt" && temp != "b.txt");
        assert_eq!(
            plan[1],
            RenameStep {
                from: "b.txt".to_string(),
                to: "a.txt".to_string()
            }
        );
        assert_eq!(
            plan[2],
            RenameStep {
                from: temp,
                to: "b.txt".to_string()
            }
        );
    }

    #[test]
    fn executing_a_swap_leaves_both_files_intact() {
        let mut app = app_with(&["a.txt", "b.txt"]);
        preview(&mut app, &["b.txt", "a.txt"]);
        assert_eq!(conflicts_of(&app), vec![false, false]);

        app.execute_rename();
        assert_eq!(names_of(&app), vec!["b.txt", "a.txt"]);
        assert_eq!(paths_of(&app), vec!["/photos/b.txt", "/photos/a.txt"]);
    }

    #[test]
    fn a_three_way_rotation_is_planned_without_loss() {
        let mut app = app_with(&["a", "b", "c"]);
        preview(&mut app, &["b", "c", "a"]);
        assert_eq!(conflicts_of(&app), vec![false, false, false]);

        app.execute_rename();
        assert_eq!(names_of(&app), vec!["b", "c", "a"]);
        let mut sorted = names_of(&app);
        sorted.sort();
        assert_eq!(
            sorted,
            vec!["a", "b", "c"],
            "no name may be lost or duplicated by the rotation"
        );
    }

    #[test]
    fn two_files_renamed_to_the_same_name_are_flagged() {
        let mut app = app_with(&["a.txt", "b.txt"]);
        preview(&mut app, &["same.txt", "same.txt"]);
        assert_eq!(conflicts_of(&app), vec![true, true]);

        // A conflicted rename is excluded from the batch, so nothing moves.
        app.execute_rename();
        assert_eq!(names_of(&app), vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn colliding_with_a_file_that_is_not_being_renamed_is_flagged() {
        let mut app = app_with(&["a.txt", "keep.txt"]);
        app.files[1].selected = false;
        preview(&mut app, &["keep.txt", "keep.txt"]);
        // Only the file that can be told to pick another name is flagged; the
        // bystander is the victim, not the cause.
        assert_eq!(conflicts_of(&app), vec![true, false]);
    }

    #[test]
    fn undoing_a_swap_restores_both_names() {
        let mut app = app_with(&["a.txt", "b.txt"]);
        preview(&mut app, &["b.txt", "a.txt"]);
        app.execute_rename();

        app.undo();
        assert_eq!(names_of(&app), vec!["a.txt", "b.txt"]);
        assert_eq!(paths_of(&app), vec!["/photos/a.txt", "/photos/b.txt"]);
    }

    #[test]
    fn undoing_and_redoing_a_shifted_sequence_restores_every_name() {
        let mut app = app_with(&["1.jpg", "2.jpg", "3.jpg"]);
        preview(&mut app, &["2.jpg", "3.jpg", "4.jpg"]);
        app.execute_rename();

        app.undo();
        assert_eq!(names_of(&app), vec!["1.jpg", "2.jpg", "3.jpg"]);
        assert_eq!(
            paths_of(&app),
            vec!["/photos/1.jpg", "/photos/2.jpg", "/photos/3.jpg"]
        );

        app.redo();
        assert_eq!(names_of(&app), vec!["2.jpg", "3.jpg", "4.jpg"]);
    }

    #[test]
    fn a_file_already_holding_the_temporary_name_is_not_clobbered() {
        let plan = rename_plan(
            &[
                "a.txt".to_string(),
                "b.txt".to_string(),
                ".renamer-tmp-0".to_string(),
            ],
            &[
                ("a.txt".to_string(), "b.txt".to_string()),
                ("b.txt".to_string(), "a.txt".to_string()),
            ],
        );
        // The parking name must be one nothing in the directory holds, or the
        // cycle-breaker destroys an innocent bystander.
        assert_eq!(plan[0].to, ".renamer-tmp-1");
        assert!(
            plan.iter()
                .all(|s| s.from != ".renamer-tmp-0" && s.to != ".renamer-tmp-0")
        );
    }

    #[test]
    fn a_name_that_does_not_change_produces_no_step() {
        // Without this filter an unchanged pair is permanently "blocked" (its
        // destination is occupied by itself) and gets mistaken for a cycle,
        // producing a pointless park-and-restore round trip.
        let plan = rename_plan(
            &["a.txt".to_string(), "b.txt".to_string()],
            &[
                ("a.txt".to_string(), "a.txt".to_string()),
                ("b.txt".to_string(), "c.txt".to_string()),
            ],
        );
        assert_eq!(
            plan,
            vec![RenameStep {
                from: "b.txt".to_string(),
                to: "c.txt".to_string()
            }]
        );
    }

    /// Run a plan against a simulated directory, asserting the invariant that
    /// actually matters at each step: the source exists and the destination is
    /// free, so no `fs::rename` in the plan can ever overwrite a file.
    /// Returns the resulting directory.
    fn simulate(existing: &[&str], renames: &[(&str, &str)]) -> BTreeSet<String> {
        let existing: Vec<String> = existing.iter().map(|s| (*s).to_string()).collect();
        let pairs: Vec<(String, String)> = renames
            .iter()
            .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
            .collect();
        let plan = rename_plan(&existing, &pairs);

        let mut disk: BTreeSet<String> = existing.iter().cloned().collect();
        for step in &plan {
            assert!(
                disk.contains(&step.from),
                "step {step:?} renames a name that is not there"
            );
            assert!(
                !disk.contains(&step.to),
                "step {step:?} would overwrite an existing file"
            );
            disk.remove(&step.from);
            disk.insert(step.to.clone());
        }
        disk
    }

    fn dir(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn no_step_in_any_plan_overwrites_a_file() {
        // Forward shift.
        assert_eq!(
            simulate(&["1", "2", "3"], &[("1", "2"), ("2", "3"), ("3", "4")]),
            dir(&["2", "3", "4"])
        );
        // Backward shift -- already conflict-free in emission order.
        assert_eq!(
            simulate(&["2", "3", "4"], &[("2", "1"), ("3", "2"), ("4", "3")]),
            dir(&["1", "2", "3"])
        );
        // Two-cycle.
        assert_eq!(
            simulate(&["a", "b"], &[("a", "b"), ("b", "a")]),
            dir(&["a", "b"])
        );
        // Three-cycle.
        assert_eq!(
            simulate(&["a", "b", "c"], &[("a", "b"), ("b", "c"), ("c", "a")]),
            dir(&["a", "b", "c"])
        );
        // Two independent cycles plus a bystander that is never touched.
        assert_eq!(
            simulate(
                &["a", "b", "x", "y", "keep"],
                &[("a", "b"), ("b", "a"), ("x", "y"), ("y", "x")]
            ),
            dir(&["a", "b", "x", "y", "keep"])
        );
        // A cycle with a tail hanging off it.
        assert_eq!(
            simulate(&["a", "b", "c"], &[("a", "b"), ("b", "a"), ("c", "d")]),
            dir(&["a", "b", "d"])
        );
    }
}
