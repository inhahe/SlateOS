//! `Slate OS` Batch File Renamer
//!
//! A powerful batch file renaming tool with:
//! - Multiple rename operations (find/replace, insert, remove, case change,
//!   numbering, date stamp, regex), each added from the Add Rule menu and set
//!   up in the rule editor
//! - Live preview showing old → new names before committing, updated as a
//!   rule is typed
//! - Undo/redo for rename operations
//! - Operation chaining (apply multiple transforms in sequence)
//! - Name conflict detection, and an order of renames that never overwrites a
//!   file a later rename still needs
//! - Filtering by name, extension and conflict
//! - Drag-and-drop file addition -- **not yet**: a window is never handed a
//!   dropped file, because the toolkit has no drop event to hand it
//! - History of past rename sessions, with when each happened
//! - Template-based renaming with variables
//! - Extension handling (rename, add, remove, change)
//!
//! Uses the guitk library for UI rendering.
//!
//! # The pointer
//!
//! Every control is drawn and hit-tested by one walk, [`RenamerApp::frame`],
//! through [`guitk::frame::Frame`]: the toolbar, the sidebar's tabs, each rule
//! and its move and remove buttons, every box, choice and switch in the rule
//! editor, the list's filters, and each file's row and checkbox. The file list
//! and the operations panel scroll under the wheel, and the layout follows the
//! window's size. Until 2026-09-25 none of it took a pointer, and every rule
//! that needed something typed or chosen could not be added at all.

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

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::Color;
use guitk::dialog::{FileDialog, FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::menu::{ContextMenu, MenuAction, MenuItem};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::table::{Column, Fit, Table};
use guitk::text;
use guitk::textinput::TextInput;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

// ============================================================================
// Layout constants
// ============================================================================

const TOOLBAR_HEIGHT: f32 = 40.0;

/// Every key this program answers, and what it does.
///
/// This app is the reason `TD-C-A-HUNDRED-APPS-BIND-KEYS-NOBODY-CAN-FIND`
/// exists. Eight letter keys each add a rename rule -- and adding a rule is
/// the entire point of the program, since a pipeline with no rules previews
/// every name unchanged. The nearest thing to a mention anywhere in the crate
/// was `"kebab-case"`, which is the label of the *operation*, not of the `K`
/// that adds it.
///
/// **Each row is a key this program actually answers**, which is not a
/// property the list has on its own: `every_advertised_key_does_something`
/// walks it, reads each label with `guitk::shortcut` and presses every key it
/// names. `apps/rssreader` shipped an overlay of twenty-one shortcuts of which
/// about four worked.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Up / Down", "Move through the files"),
    ("Space", "Select or deselect this file"),
    ("Ctrl+A", "Select every file"),
    ("Ctrl+Delete", "Take every file off the list"),
    (
        "L / U / T",
        "Add a rule: lower case / UPPER CASE / Title Case",
    ),
    ("S / K", "Add a rule: snake_case / kebab-case"),
    ("W", "Add a rule: trim the spaces off both ends"),
    ("E / X", "Add a rule: lower-case the extension / remove it"),
    ("N", "Add a rule: number the files"),
    ("R", "Add a rule of any kind, from a menu"),
    ("F2", "Edit the selected rule; Tab moves between its boxes"),
    ("Ctrl+Up / Ctrl+Down", "Select the rule above / below"),
    ("Delete", "Remove the selected rule"),
    ("PageUp / PageDown", "Move the selected rule up / down"),
    ("Ctrl+Backspace", "Remove every rule"),
    ("Enter", "Rename the selected files"),
    ("Ctrl+Z / Ctrl+Y", "Undo / redo the rename"),
    ("Ctrl+O", "Open a folder"),
    ("/", "Search the file names"),
    ("Ctrl+E", "Show only one extension"),
    ("C", "Show only the names that would collide"),
    ("F1 / ?", "This list"),
];
const SIDEBAR_WIDTH: f32 = 280.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const PADDING: f32 = 8.0;
const LINE_HEIGHT: f32 = 22.0;
const SMALL_TEXT: f32 = 11.0;
const NORMAL_TEXT: f32 = 13.0;
const HEADER_TEXT: f32 = 15.0;
const TITLE_TEXT: f32 = 17.0;
const BUTTON_HEIGHT: f32 = 28.0;

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
///
/// Every kind is reachable: the Add Rule menu adds any of them and the rule
/// editor sets its parameters. For most of this app's life only the kinds that
/// needed nothing typed could be added (known-issues
/// `TD-C-RENAMER-CAN-ONLY-ADD-THE-RULES-THAT-NEED-NO-TYPING`).
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
    /// Regular-expression find and replace: a POSIX extended regular
    /// expression, the dialect `grep -E`, `sed -E` and the shell's `=~` use in
    /// this system, with `\1`...`\9` and `&` in the replacement as `sed` has
    /// them.
    Regex {
        pattern: String,
        replacement: String,
        case_sensitive: bool,
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

    fn color(&self, pal: &Palette) -> Color {
        match self {
            Self::FindReplace { .. } => pal.blue,
            Self::Insert { .. } => pal.green,
            Self::Remove { .. } => pal.red,
            Self::ChangeCase(_) => pal.mauve,
            Self::Number { .. } => pal.peach,
            Self::DateStamp { .. } => pal.teal,
            Self::Regex { .. } => pal.yellow,
            Self::Trim { .. } => pal.lavender,
            Self::Extension(_) => pal.overlay0,
            Self::Template { .. } => pal.subtext1,
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

/// Five of the eight have a key; all eight are in the rule editor's chooser.
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
    /// The name on disk, as the filesystem gave it.
    ///
    /// This is the key, and `original_name` is the text. Slate OS paths admit
    /// every byte but `/` and NUL, so a name is not text: decoding it to UTF-8
    /// is lossy and a lossy name is a *different* name. The full path is
    /// `folder.join(raw_name)` -- derived, never stored, so there is one
    /// statement of where a file is.
    raw_name: OsString,
    /// Original filename as text, for the rules and the display.
    original_name: String,
    /// Whether this file's name survives the trip through `String`.
    ///
    /// Every rename rule reads text and writes text, so a name that is not
    /// valid UTF-8 has no text form a rule could transform -- and renaming it
    /// from a lossy one would write a name **nobody asked for** to the disk.
    /// Such a file is listed, so the user can see it is there, and excluded
    /// from every rename.
    renameable: bool,
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
    /// When the file was last modified, in milliseconds since the epoch, as
    /// the folder listing read it: what a date-stamp rule stamps.
    modified_ms: u64,
}

impl FileEntry {
    /// **Tests only.** Production entries come from a directory listing, where
    /// the name arrives as an `OsStr` and may not be text at all.
    #[cfg(test)]
    fn new(name: &str, size: u64, modified_ms: u64) -> Self {
        Self::from_os_name(OsStr::new(name), size, modified_ms)
    }

    /// From a name as the filesystem gave it.
    fn from_os_name(raw: &OsStr, size: u64, modified_ms: u64) -> Self {
        // A name that is text is the name, and the rules work on it. One that
        // is not cannot be renamed here (the rules are text rules), and is
        // *shown* by its bytes: a lossy decode showed two such names as the
        // same row of replacement characters.
        let renameable = raw.to_str().is_some();
        let name = raw.to_str().map_or_else(
            || quoting::escape_unprintable(raw.as_encoded_bytes()),
            str::to_owned,
        );
        // An extension is what follows the *last* dot, and only when there is
        // something before that dot. `rsplit('.').next()` does not say that:
        // the `len() < name.len()` guard it was paired with rejects a dotless
        // name but still accepts a leading dot, so `.bashrc` was indexed with
        // extension `bashrc` -- shown in the details pane as its type, and
        // swept up by a `bashrc` extension filter alongside real `x.bashrc`
        // files.
        let extension = name
            .as_str()
            .rsplit_once('.')
            .filter(|(stem, _)| !stem.is_empty())
            .map_or("", |(_, ext)| ext)
            .to_string();
        Self {
            raw_name: raw.to_os_string(),
            original_name: name.clone(),
            new_name: name,
            renameable,
            size,
            // A file whose name is not text cannot be renamed, so it does not
            // start out ticked -- a Rename that silently skips half a ticked
            // list is the same defect as one that claims to have renamed it.
            selected: renameable,
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

/// A file time as a date-stamp format writes it.
///
/// In UTC, explicitly: there is no per-process zone to read yet (known-issues
/// `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`), and `Tz::utc()` is the mark that
/// convention leaves so every such place can be found when there is one.
fn stamp_for(format: DateFormat, modified_ms: u64) -> String {
    let secs = i64::try_from(modified_ms / 1000).unwrap_or(i64::MAX);
    let at = guitk::datetime::DateTime::at(secs, &guitk::tzrules::Tz::utc());
    let date = at.date();
    format.format(
        u16::try_from(date.year()).unwrap_or(0),
        u8::try_from(date.month()).unwrap_or(1),
        u8::try_from(date.day()).unwrap_or(1),
        u8::try_from(at.hour()).unwrap_or(0),
        u8::try_from(at.minute()).unwrap_or(0),
        u8::try_from(at.second()).unwrap_or(0),
    )
}

/// Why a regular-expression rule cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RegexProblem {
    /// The pattern does not compile; the text says why.
    Pattern(&'static str),
    /// A backreference search ran past its budget.
    TooHard,
    /// The replacement produced bytes that are not text.
    NotText,
}

impl RegexProblem {
    fn message(&self) -> &'static str {
        match self {
            Self::Pattern(why) => why,
            Self::TooHard => "the pattern takes too long to match",
            Self::NotText => "the replacement would make a name that is not text",
        }
    }
}

/// Replace every match of `pattern` in `name` with `replacement`, as `sed`'s
/// `s///g` does: `&` is the whole match, `\1` to `\9` are its groups, and a
/// backslash makes the next character itself (`\&`, `\\`).
///
/// Through `ere`, the one POSIX regular-expression engine the shell, `grep`,
/// `sed` and `awk` share, so a pattern that works at the prompt works here.
fn regex_replace(
    pattern: &str,
    replacement: &str,
    case_insensitive: bool,
    name: &str,
) -> Result<String, RegexProblem> {
    let re = ere::Regex::new_flags(pattern.as_bytes(), case_insensitive)
        .map_err(|e| RegexProblem::Pattern(e.message()))?;
    let subject = name.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(subject.len());
    let mut copied = 0usize;
    for groups in re.capture_spans_iter(subject) {
        let groups = groups.map_err(|_| RegexProblem::TooHard)?;
        let Some(Some((start, end))) = groups.first().copied() else {
            continue;
        };
        out.extend_from_slice(subject.get(copied..start).unwrap_or(&[]));
        let mut chars = replacement.chars();
        while let Some(c) = chars.next() {
            match c {
                '&' => out.extend_from_slice(subject.get(start..end).unwrap_or(&[])),
                '\\' => match chars.next() {
                    Some(d @ '0'..='9') => {
                        let group = d.to_digit(10).and_then(|g| usize::try_from(g).ok());
                        if let Some(Some((gs, ge))) = group.and_then(|g| groups.get(g)).copied() {
                            out.extend_from_slice(subject.get(gs..ge).unwrap_or(&[]));
                        }
                    }
                    Some(other) => {
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                    }
                    None => out.push(b'\\'),
                },
                other => {
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                }
            }
        }
        copied = end;
    }
    out.extend_from_slice(subject.get(copied..).unwrap_or(&[]));
    String::from_utf8(out).map_err(|_| RegexProblem::NotText)
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

// ============================================================================
// Rename engine
// ============================================================================

/// What a rule may need to know about the file it is renaming, beyond its
/// name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RuleContext {
    /// This file's position among the files being renamed, from 0: what a
    /// numbering rule counts. Files that are not being renamed are not
    /// counted, so ticking three files of ten numbers them 1, 2, 3.
    index: usize,
    /// When the file was last modified, in milliseconds since the epoch: the
    /// date a date-stamp rule stamps.
    modified_ms: u64,
}

/// The core rename engine that applies operations to filenames.
struct RenameEngine;

impl RenameEngine {
    /// Apply a single operation to a filename, with an index (for numbering)
    /// and no file behind it. **Tests only**: a date stamp needs the file's
    /// time, which only [`apply_in`](Self::apply_in) is given.
    #[cfg(test)]
    fn apply(op: &RenameOp, name: &str, index: usize) -> String {
        Self::apply_in(
            op,
            name,
            RuleContext {
                index,
                modified_ms: 0,
            },
        )
    }

    /// Apply a single operation to a filename.
    fn apply_in(op: &RenameOp, name: &str, ctx: RuleContext) -> String {
        let index = ctx.index;
        let (stem, ext) = FileEntry::split_name(name);

        match op {
            RenameOp::FindReplace {
                find,
                replace,
                case_sensitive,
                replace_all,
            } => {
                // An empty search matches between every character, and
                // `str::replace` would put the replacement between each pair:
                // a rule the user has only half written must change nothing.
                if find.is_empty() {
                    return name.to_string();
                }
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
                // The file's own modification time. This was the constant
                // 2026-05-18 14:30:00 -- "Mock date (in real OS, would use
                // system time)" -- so every file in every batch was stamped
                // with one day in May, whenever it was written.
                let date_str = stamp_for(*format, ctx.modified_ms);
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
                case_sensitive,
            } => {
                // A real regular expression. This was `name.replace(pattern,
                // replacement)` under "Simple regex: only support literal
                // patterns for now", so `IMG_(\d+)` matched only a name that
                // literally contained those nine characters.
                // An empty pattern matches everywhere; see Find & Replace.
                if pattern.is_empty() {
                    return name.to_string();
                }
                regex_replace(pattern, replacement, !*case_sensitive, name)
                    .unwrap_or_else(|_| name.to_string())
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
    /// When the rename was performed, in milliseconds since the epoch, as
    /// the History panel shows it.
    timestamp_ms: u64,
}

// ============================================================================
// App state
// ============================================================================

/// The batch file renamer application state.
struct RenamerApp {
    /// The folder being renamed in, once one has been chosen.
    ///
    /// `None` until then, and the list is empty. The app used to open with six
    /// invented files -- `/home/user/photos/IMG_0001.JPG` and friends -- that
    /// existed nowhere. A list of files the user does not recognise is bad on
    /// its own; a Rename button that then reports having renamed them is how
    /// somebody comes to believe their photos were reorganised.
    folder: Option<PathBuf>,
    /// The folder picker.
    picker: FilePicker,
    /// The size the last frame was drawn at.
    ///
    /// The app itself lays out from constants and ignores the size it is
    /// handed, so it had nothing to store. The picker does need it: it hit-
    /// tests clicks against the window it is drawn in, so a stale size sends
    /// a click to the wrong row. Taken from `render` rather than from
    /// `Resize`, because the first frame is drawn before any resize arrives.
    last_width: f32,
    last_height: f32,
    /// Files to rename.
    files: Vec<FileEntry>,
    /// Active rename operations (applied in order).
    operations: Vec<RenameOp>,
    /// Undo stack of rename records.
    undo_stack: Vec<RenameRecord>,
    /// Redo stack.
    redo_stack: Vec<RenameRecord>,
    /// The first file row shown.
    ///
    /// Was `scroll_offset: f32`, which nothing but the folder loader ever
    /// wrote: the list showed its first page and no key or wheel reached the
    /// rest, while Up and Down walked the cursor onto rows never drawn.
    file_scroll: usize,
    /// The wheel's remainder over the file list.
    files_wheel: wheel::Accumulator,
    /// How far the operations panel is scrolled, in pixels.
    ops_scroll: f32,
    /// Selected file index.
    selected_file: usize,
    /// Selected operation index in the sidebar.
    selected_op: usize,
    /// Which sidebar panel is active.
    sidebar_panel: SidebarPanel,
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
    /// Where the keyboard is: the list, or one of the boxes.
    ///
    /// Was `searching: bool`, the one box there was; the rule editor and the
    /// extension filter are boxes too now.
    focus: Focus,
    /// The text being typed into the box that has the keyboard.
    draft: TextInput,
    /// Whether what is in the box is a value its rule cannot take (a count
    /// that is not a number), shown as a red rule under the box.
    draft_invalid: bool,
    /// The Add Rule menu, while it is up.
    rule_menu: Option<ContextMenu>,
    /// What the pointer is over.
    hover: Option<Target>,
    /// Every box the last paint recorded, for hover and the wheel.
    last_hits: Vec<(Target, Rect)>,
    /// Whether the shortcut list is up.
    show_help: bool,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
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
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            folder: None,
            picker: FilePicker::default(),
            last_width: WINDOW_WIDTH_PX as f32,
            last_height: WINDOW_HEIGHT_PX as f32,
            files: Vec::new(),
            operations: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            file_scroll: 0,
            files_wheel: wheel::Accumulator::default(),
            ops_scroll: 0.0,
            selected_file: 0,
            selected_op: 0,
            sidebar_panel: SidebarPanel::Operations,
            status_message: String::new(),
            filter_extension: String::new(),
            filter_conflicts: false,
            history: Vec::new(),
            search_text: String::new(),
            focus: Focus::List,
            draft: TextInput::new(),
            draft_invalid: false,
            rule_menu: None,
            hover: None,
            last_hits: Vec::new(),
            show_help: false,
        }
    }

    /// Add a file to the rename list. **Tests only**; see `load_folder`.
    #[cfg(test)]
    fn add_file(&mut self, name: &str, size: u64, modified: u64) {
        if self.files.len() >= MAX_FILES {
            return;
        }
        self.files.push(FileEntry::new(name, size, modified));
        self.apply_operations();
    }

    /// List `dir`, replacing whatever was loaded before, and say what is in it.
    ///
    /// Directories are skipped: this program renames files, and listing a
    /// folder it will then refuse to rename is worse than not listing it.
    fn load_folder(&mut self, dir: &Path) -> String {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => return format!("Could not read {}: {err}", dir.display()),
        };

        self.files.clear();
        // The rules are kept: a set of rules built for one folder is exactly
        // what a person wants to run over the next, and opening a folder used
        // to throw them away without a word.
        //
        // The stacks describe renames in the old folder. Undo across a folder
        // change would look up names that are not here and silently do
        // nothing, which reads exactly like an undo that failed.
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.selected_file = 0;
        self.file_scroll = 0;

        let mut skipped_dirs = 0usize;
        let mut unreadable = 0usize;
        for entry in entries {
            let Ok(entry) = entry else {
                unreadable = unreadable.saturating_add(1);
                continue;
            };
            let meta = entry.metadata().ok();
            if meta.as_ref().is_some_and(std::fs::Metadata::is_dir) {
                skipped_dirs = skipped_dirs.saturating_add(1);
                continue;
            }
            if self.files.len() >= MAX_FILES {
                break;
            }
            let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
            let modified = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
            self.files
                .push(FileEntry::from_os_name(&entry.file_name(), size, modified));
        }

        // `read_dir` yields in whatever order the filesystem likes. Sorted
        // by name, so the list reads the way a person expects and two runs
        // over the same folder agree.
        self.files
            .sort_by(|a, b| a.original_name.cmp(&b.original_name));

        self.folder = Some(dir.to_path_buf());
        self.apply_operations();

        let unrenameable = self.files.iter().filter(|f| !f.renameable).count();
        let mut said = format!("{} file(s) in {}", self.files.len(), dir.display());
        if skipped_dirs > 0 {
            said.push_str(&format!(", {skipped_dirs} folder(s) skipped"));
        }
        if unrenameable > 0 {
            said.push_str(&format!(
                ", {unrenameable} whose names are not text and cannot be renamed"
            ));
        }
        if unreadable > 0 {
            said.push_str(&format!(", {unreadable} unreadable"));
        }
        said
    }

    /// Fill the list with an invented set of files. **Tests only.**
    ///
    /// This ran at startup until 2026-09-15, so the window opened showing six
    /// files that existed nowhere -- and the Rename button then reported
    /// having renamed them. The names are the awkward ones a bulk renamer is
    /// for (inconsistent separators, mixed case, a duplicate that collides
    /// under lowercasing, a space-padded name), which is why they are kept for
    /// the tests of the rules themselves.
    #[cfg(test)]
    fn seed_sample_files(&mut self) {
        for (name, size) in [
            ("IMG_0001.JPG", 2_400_000_u64),
            ("IMG_0002.JPG", 2_310_000),
            ("img_0002.jpg", 2_290_000),
            ("Holiday Snap .png", 850_000),
            ("report-final-FINAL.docx", 44_000),
            ("notes.txt", 1_200),
        ] {
            self.add_file(name, size, 0);
        }
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

    /// Apply all operations to the files being renamed and update previews.
    ///
    /// A file that is not ticked keeps its name in the preview, because it
    /// keeps it on disk; and the numbering counts only the ticked ones. It
    /// counted every file in the list, so renaming three photos out of ten
    /// numbered them 1, 4 and 9.
    fn apply_operations(&mut self) {
        let mut index = 0usize;
        for file in &mut self.files {
            if !(file.selected && file.renameable) {
                file.new_name.clone_from(&file.original_name);
                continue;
            }
            let ctx = RuleContext {
                index,
                modified_ms: file.modified_ms,
            };
            let mut name = file.original_name.clone();
            for op in &self.operations {
                name = RenameEngine::apply_in(op, &name, ctx);
            }
            file.new_name = name;
            index = index.saturating_add(1);
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
                .filter(|f| {
                    f.renameable && f.selected && f.original_name != f.new_name && !f.conflict
                })
                .map(|f| (f.original_name.clone(), f.new_name.clone()))
                .collect(),
            operations: self
                .operations
                .iter()
                .map(|o| o.label().to_string())
                .collect(),
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        };

        if record.renames.is_empty() {
            self.status_message = "No files to rename".into();
            return;
        }

        // Ordered so that no step overwrites a name a later step still
        // needs; see `rename_plan`. The order matters to the filesystem, not
        // to the preview, which is why it was built before there was one.
        let plan = rename_plan(&self.current_names(), &record.renames);
        let (done, failures) = self.perform(&plan);

        if done > 0 {
            self.undo_stack.push(record.clone());
            if self.undo_stack.len() > MAX_UNDO {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();

            self.history.push(record);
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }
        }

        self.status_message = Self::describe(done, &failures);
    }

    /// Carry out `plan` against the filesystem, and update the list to match.
    ///
    /// Returns how many steps succeeded and what went wrong with the rest.
    ///
    /// **A step that fails does not stop the batch, and does not update the
    /// entry either.** Stopping would leave the plan half-applied with no
    /// record of where it stopped -- and the plan's whole purpose is that its
    /// order is safe, so abandoning it midway is what creates the collision it
    /// was built to avoid. Each entry is updated only if its own rename
    /// succeeded, so the list continues to describe the directory.
    fn perform(&mut self, plan: &[RenameStep]) -> (usize, Vec<String>) {
        let Some(folder) = self.folder.clone() else {
            return (0, vec!["no folder is open".to_string()]);
        };

        let mut done = 0usize;
        let mut failures = Vec::new();
        for step in plan {
            // The source is the name **as the filesystem gave it**, not its
            // text form. For every file this will rename the two are equal --
            // `renameable` is exactly the test that they are -- but building
            // the path from the text is how they stop being equal the first
            // time that guard is loosened, and the failure would be silent on
            // every name that round-trips.
            //
            // A step whose `from` names no file is a temporary this plan
            // invented to break a cycle; those names are this code's own and
            // are exact as text.
            let from_raw = self
                .files
                .iter()
                .find(|f| f.original_name == step.from)
                .map_or_else(|| OsString::from(&step.from), |f| f.raw_name.clone());
            let from = folder.join(&from_raw);
            let to = folder.join(&step.to);
            match std::fs::rename(&from, &to) {
                Ok(()) => {
                    done = done.saturating_add(1);
                    if let Some(file) = self.files.iter_mut().find(|f| f.original_name == step.from)
                    {
                        file.original_name.clone_from(&step.to);
                        file.raw_name = OsString::from(&step.to);
                    }
                }
                Err(err) => failures.push(format!("{} -> {}: {err}", step.from, step.to)),
            }
        }

        // Every file's preview should now show the name it actually has, or
        // the next `detect_conflicts` compares against stale targets.
        for file in &mut self.files {
            file.new_name.clone_from(&file.original_name);
        }
        (done, failures)
    }

    /// What to say about a batch that partly worked.
    ///
    /// The worst error is reported, not the last one, and the count of
    /// successes is reported beside it: "Renamed 3; 2 failed" is actionable
    /// where either half alone is misleading.
    fn describe(done: usize, failures: &[String]) -> String {
        match (done, failures.first()) {
            (0, None) => "Nothing to rename".to_string(),
            (n, None) => format!("Renamed {n} file(s)"),
            (0, Some(first)) => format!("Renamed nothing. {first}"),
            (n, Some(first)) => {
                format!("Renamed {n} file(s); {} failed. {first}", failures.len())
            }
        }
    }

    /// Choose a folder to work in.
    fn open_folder(&mut self) {
        self.picker.put_up(
            FileDialog::select_folder().with_initial_path(FilePicker::default_start()),
            false,
        );
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
            let (done, failures) = self.perform(&plan);

            // The record goes to the redo stack only if something was undone.
            // Moving it regardless would offer to redo a rename that never
            // came back, and the redo would then fail against names that are
            // still in place.
            if done > 0 {
                self.redo_stack.push(record);
            }
            self.status_message = match Self::describe(done, &failures) {
                m if done > 0 && failures.is_empty() => m.replace("Renamed", "Put back"),
                m => m,
            };
            self.apply_operations();
        }
    }

    /// Redo the last undone rename.
    fn redo(&mut self) {
        if let Some(record) = self.redo_stack.pop() {
            let plan = rename_plan(&self.current_names(), &record.renames);
            let (done, failures) = self.perform(&plan);

            if done > 0 {
                self.undo_stack.push(record);
            }
            self.status_message = match Self::describe(done, &failures) {
                m if done > 0 && failures.is_empty() => m.replace("Renamed", "Redid"),
                m => m,
            };
            self.apply_operations();
        }
    }

    /// The name every file in the list currently has on disk.
    fn current_names(&self) -> Vec<String> {
        self.files.iter().map(|f| f.original_name.clone()).collect()
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
            .filter(|f| f.renameable && f.selected && f.original_name != f.new_name && !f.conflict)
            .count()
    }

    /// Count files with conflicts.
    fn conflict_count(&self) -> usize {
        self.files.iter().filter(|f| f.conflict).count()
    }

    /// Select or deselect all files.
    ///
    /// Selecting skips the files that cannot be renamed; deselecting does not
    /// need to. **`renameable` was applied once, at construction, as the
    /// entry's initial `selected` value** -- so Ctrl+A ticked every file in
    /// the list including the ones whose names are not text, and Space toggled
    /// them individually. A guard that only holds until the user touches
    /// something is not a guard.
    fn select_all(&mut self, selected: bool) {
        for file in &mut self.files {
            file.selected = selected && file.renameable;
        }
        self.apply_operations();
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

    // ------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------

    /// Route a compositor event into the app.
    fn handle_event(&mut self, event: &Event) -> EventResult {
        // The picker takes input first while it is up, or a folder name is
        // typed into the search box behind it.
        match self.picker.handle(event, self.last_width, self.last_height) {
            Picked::Chose(path) => {
                self.status_message = self.load_folder(&path);
                return EventResult::Consumed;
            }
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
                    self.last_width = *width as f32;
                    self.last_height = *height as f32;
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Apply a key press.
    ///
    /// The app had no input handling at all: the file list, the operation
    /// pipeline, undo, redo and the actual rename were reachable only by a
    /// caller invoking the method directly. **`Enter` performs the rename**,
    /// which is the one irreversible-looking action here — it is on a key of
    /// its own rather than sharing one, and undo covers it.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        // The Add Rule menu takes the keyboard while it is up: arrows, Enter,
        // Escape.
        if let Some(menu) = self.rule_menu.as_mut() {
            match menu.handle_key(key) {
                Some(MenuAction::Selected(id)) => {
                    self.rule_menu = None;
                    self.add_rule_of_kind(id);
                }
                Some(MenuAction::Closed) => self.rule_menu = None,
                Some(MenuAction::None) | None => {}
            }
            return EventResult::Consumed;
        }
        if self.focus != Focus::List && !matches!(key.key, Key::F1) {
            return self.handle_key_in_box(key);
        }
        let ctrl = key.modifiers.ctrl;
        match key.key {
            // Panels.
            Key::Tab => {
                self.sidebar_panel = match self.sidebar_panel {
                    SidebarPanel::Operations => SidebarPanel::Preview,
                    SidebarPanel::Preview => SidebarPanel::History,
                    SidebarPanel::History => SidebarPanel::Operations,
                };
                EventResult::Consumed
            }
            // The rules: which one the editor shows. Without these the
            // selection moved only when a rule was added, so every rule but
            // the newest could be neither edited, reordered nor removed.
            Key::Up if ctrl => {
                if self.selected_op == 0 {
                    return EventResult::Ignored;
                }
                self.selected_op = self.selected_op.saturating_sub(1);
                EventResult::Consumed
            }
            Key::Down if ctrl => {
                if self.selected_op.saturating_add(1) >= self.operations.len() {
                    return EventResult::Ignored;
                }
                self.selected_op = self.selected_op.saturating_add(1);
                EventResult::Consumed
            }
            Key::F2 => {
                let first = self
                    .operations
                    .get(self.selected_op)
                    .and_then(|op| rule_slots(op).first().copied());
                match first {
                    Some(slot) => {
                        self.sidebar_panel = SidebarPanel::Operations;
                        self.set_focus(Focus::Field(slot));
                        EventResult::Consumed
                    }
                    None => EventResult::Ignored,
                }
            }
            Key::R => {
                self.open_rule_menu();
                EventResult::Consumed
            }
            Key::E if ctrl => {
                self.set_focus(Focus::Extension);
                EventResult::Consumed
            }
            // The file list.
            Key::Up => {
                let moved = self.step_file(-1);
                self.keep_file_visible();
                moved
            }
            Key::Down => {
                let moved = self.step_file(1);
                self.keep_file_visible();
                moved
            }
            Key::Space => self.toggle_selected_file(),
            Key::A if ctrl => {
                // Select-all, and its opposite when everything already is.
                let all = !self.files.is_empty() && self.files.iter().all(|f| f.selected);
                self.select_all(!all);
                EventResult::Consumed
            }
            // The operation pipeline.
            // Before the plain `Key::Delete` below: a guard narrows only the
            // arm it is on, so an unguarded arm listed first swallows the
            // guarded case entirely — Ctrl+Delete would have removed an
            // operation rather than clearing the file list.
            Key::Delete if ctrl => {
                if self.files.is_empty() {
                    return EventResult::Ignored;
                }
                self.clear_files();
                EventResult::Consumed
            }
            Key::Delete => {
                if self.operations.is_empty() {
                    return EventResult::Ignored;
                }
                let idx = self.selected_op;
                self.remove_operation(idx);
                self.selected_op = self
                    .selected_op
                    .min(self.operations.len().saturating_sub(1));
                self.apply_operations();
                EventResult::Consumed
            }
            Key::PageUp => self.move_op(-1),
            Key::PageDown => self.move_op(1),
            // Doing it, and undoing it.
            Key::Enter => self.rename_selected(),
            Key::O if ctrl => {
                self.open_folder();
                EventResult::Consumed
            }
            Key::Z if ctrl => {
                if self.undo_stack.is_empty() {
                    return EventResult::Ignored;
                }
                self.undo();
                EventResult::Consumed
            }
            Key::Y if ctrl => {
                if self.redo_stack.is_empty() {
                    return EventResult::Ignored;
                }
                self.redo();
                EventResult::Consumed
            }
            // The shortcut list. Before the unguarded `Key::Slash` below,
            // which would otherwise take a shifted slash and open the search
            // box -- a guard narrows only the arm it is on.
            Key::F1 => {
                self.show_help = !self.show_help;
                EventResult::Consumed
            }
            Key::Slash if key.modifiers.shift => {
                self.show_help = !self.show_help;
                EventResult::Consumed
            }
            Key::Escape if self.show_help => {
                self.show_help = false;
                EventResult::Consumed
            }
            Key::Slash => {
                self.set_focus(Focus::Search);
                EventResult::Consumed
            }
            Key::C => {
                self.filter_conflicts = !self.filter_conflicts;
                self.file_scroll = 0;
                EventResult::Consumed
            }
            // The operations that need nothing typed. `add_operation` and
            // every one of these variants were written, tested, and had no
            // caller: the pipeline could not be given a single rule, so the
            // preview column showed each name unchanged and the program's whole
            // purpose did nothing. The ones that need a string — find/replace,
            // insert, regex — still have nowhere to type it.
            Key::L => self.add_op(RenameOp::ChangeCase(CaseMode::Lower)),
            Key::U => self.add_op(RenameOp::ChangeCase(CaseMode::Upper)),
            Key::T => self.add_op(RenameOp::ChangeCase(CaseMode::Title)),
            Key::S => self.add_op(RenameOp::ChangeCase(CaseMode::SnakeCase)),
            Key::K => self.add_op(RenameOp::ChangeCase(CaseMode::KebabCase)),
            Key::W => self.add_op(RenameOp::Trim {
                chars: String::from(" "),
                mode: TrimMode::Both,
            }),
            // Extension rules that need nothing typed either.
            Key::E => self.add_op(RenameOp::Extension(ExtensionOp::Lower)),
            Key::X => self.add_op(RenameOp::Extension(ExtensionOp::Remove)),
            // Start again: the pipeline, or the whole list.
            Key::Backspace if ctrl => {
                if self.operations.is_empty() {
                    return EventResult::Ignored;
                }
                self.clear_operations();
                self.selected_op = 0;
                EventResult::Consumed
            }
            Key::N => self.add_op(RenameOp::Number {
                start: 1,
                step: 1,
                padding: 3,
                position: InsertPosition::End,
                separator: String::from("_"),
            }),
            _ => EventResult::Ignored,
        }
    }

    /// Append an operation to the pipeline and re-run the preview.
    ///
    /// Reports `Ignored` when the pipeline is full, so the key does not redraw
    /// an unchanged frame — `add_operation` returns nothing, and silently
    /// doing nothing is the one thing a preview must not do.
    fn add_op(&mut self, op: RenameOp) -> EventResult {
        let before = self.operations.len();
        self.add_operation(op);
        if self.operations.len() == before {
            EventResult::Ignored
        } else {
            self.selected_op = self.operations.len().saturating_sub(1);
            EventResult::Consumed
        }
    }

    /// Move the file-list selection, stopping at the ends.
    fn step_file(&mut self, delta: isize) -> EventResult {
        if self.files.is_empty() {
            return EventResult::Ignored;
        }
        let Ok(current) = isize::try_from(self.selected_file) else {
            return EventResult::Ignored;
        };
        let Some(moved) = current.checked_add(delta) else {
            return EventResult::Ignored;
        };
        let Ok(moved) = usize::try_from(moved) else {
            return EventResult::Ignored; // off the top; stay put
        };
        if moved >= self.files.len() || moved == self.selected_file {
            return EventResult::Ignored;
        }
        self.selected_file = moved;
        EventResult::Consumed
    }

    /// Tick or untick the file under the cursor.
    fn toggle_selected_file(&mut self) -> EventResult {
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return EventResult::Ignored;
        };
        if !file.renameable {
            // Said rather than ignored: a tick that silently refuses to appear
            // reads as a broken key.
            let name = file.original_name.clone();
            self.status_message = format!(
                "{name} cannot be renamed: its name is not text, so no rule can transform it"
            );
            return EventResult::Consumed;
        }
        file.selected = !file.selected;
        // The preview and the numbering follow the ticks, and so do the
        // conflicts: a collision with a file that is no longer being renamed
        // is no collision, and one with a file that now is, is.
        self.apply_operations();
        EventResult::Consumed
    }

    /// Move the selected operation up or down the pipeline.
    ///
    /// The order matters: operations are applied in sequence, so moving one is
    /// a different result rather than a cosmetic change, which is why this
    /// re-runs the preview.
    fn move_op(&mut self, delta: isize) -> EventResult {
        if self.operations.len() < 2 {
            return EventResult::Ignored;
        }
        let idx = self.selected_op;
        let Ok(current) = isize::try_from(idx) else {
            return EventResult::Ignored;
        };
        let Some(moved) = current.checked_add(delta) else {
            return EventResult::Ignored;
        };
        let Ok(moved) = usize::try_from(moved) else {
            return EventResult::Ignored;
        };
        if moved >= self.operations.len() {
            return EventResult::Ignored;
        }
        if delta < 0 {
            self.move_operation_up(idx);
        } else {
            self.move_operation_down(idx);
        }
        self.selected_op = moved;
        self.apply_operations();
        EventResult::Consumed
    }

    fn render_preview_panel(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        if let Some(file) = self.files.get(self.selected_file) {
            let mut py = y + PADDING;

            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: py,
                text: "Selected File".into(),
                font_size: HEADER_TEXT,
                color: self.palette.text,
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
                color: self.palette.subtext0,
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
                color: self.palette.text,
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
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(80.0),
                overflow: TextOverflow::Ellipsis,
            });
            py += 16.0;
            let name_color = if file.conflict {
                self.palette.red
            } else if file.new_name != file.original_name {
                self.palette.green
            } else {
                self.palette.text
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
                    color: self.palette.ink(self.palette.red),
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
                color: self.palette.surface1,
                corner_radii: CornerRadii::ZERO,
            });
            py += 8.0;

            let size_str = format_size(file.size);
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: py,
                text: format!("Size: {size_str}"),
                font_size: SMALL_TEXT,
                color: self.palette.subtext0,
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
                color: self.palette.subtext0,
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
                color: self.palette.subtext0,
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
                color: self.palette.subtext0,
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
                color: if i % 2 == 0 {
                    self.palette.mantle
                } else {
                    self.palette.surface0
                },
                corner_radii: CornerRadii::all(3.0),
            });

            // When, as well as what: the record kept its time from the start
            // and nothing drew it.
            let when = guitk::datetime::stamp(
                i64::try_from(record.timestamp_ms / 1000).unwrap_or(i64::MAX),
                &guitk::tzrules::Tz::utc(),
            );
            let label = format!(
                "{when} — {} files — {}",
                record.renames.len(),
                record.operations.join(", ")
            );
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: hy + 7.0,
                text: label,
                font_size: SMALL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 20.0),
                overflow: TextOverflow::Ellipsis,
            });

            hy += 30.0;
        }
    }
}

// ============================================================================
// The rule editor: what each kind of rule asks for
// ============================================================================

/// A text box in the rule editor, by its position in the selected rule's form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    A,
    B,
    C,
    D,
    E,
}

/// Where a rule puts what it adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PositionKind {
    Start,
    End,
    At,
}

/// The five extension rules, as one choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtKind {
    Lower,
    Upper,
    Remove,
    Replace,
    Add,
}

/// One of a choice row's options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Case(CaseMode),
    Position(PositionKind),
    Format(DateFormat),
    Trim(TrimMode),
    Ext(ExtKind),
}

/// One of the rule editor's on/off switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flip {
    MatchCase,
    EveryOccurrence,
}

/// One control in the rule editor, as the selected rule's form lists it.
///
/// The form is *derived* from the rule on every frame rather than kept beside
/// it, so the editor and the rule cannot disagree about what the rule is.
#[derive(Debug, Clone)]
enum Control {
    /// A labelled text box for one of the rule's parameters.
    Field { slot: Slot, label: &'static str },
    /// A row of exclusive choices; `chosen` is the one in force.
    Choice {
        label: &'static str,
        options: Vec<(&'static str, Pick)>,
        chosen: Option<Pick>,
    },
    /// An on/off switch.
    Switch {
        label: &'static str,
        on: bool,
        flip: Flip,
    },
    /// A line of guidance, or -- when `error` -- what is wrong.
    Note { text: String, error: bool },
}

const POSITIONS: [(&str, Pick); 3] = [
    ("Start", Pick::Position(PositionKind::Start)),
    ("End", Pick::Position(PositionKind::End)),
    ("At a position", Pick::Position(PositionKind::At)),
];

fn position_kind(position: InsertPosition) -> PositionKind {
    match position {
        InsertPosition::Start => PositionKind::Start,
        InsertPosition::End => PositionKind::End,
        InsertPosition::At(_) => PositionKind::At,
    }
}

/// The controls a rule's form shows, top to bottom.
fn rule_controls(op: &RenameOp) -> Vec<Control> {
    let field = |slot, label| Control::Field { slot, label };
    let place = |position: InsertPosition| Control::Choice {
        label: "Where",
        options: POSITIONS.to_vec(),
        chosen: Some(Pick::Position(position_kind(position))),
    };
    match op {
        RenameOp::FindReplace {
            case_sensitive,
            replace_all,
            ..
        } => vec![
            field(Slot::A, "Find"),
            field(Slot::B, "Replace with"),
            Control::Switch {
                label: "Match case",
                on: *case_sensitive,
                flip: Flip::MatchCase,
            },
            Control::Switch {
                label: "Every occurrence, not just the first",
                on: *replace_all,
                flip: Flip::EveryOccurrence,
            },
        ],
        RenameOp::Insert { position, .. } => {
            let mut c = vec![field(Slot::A, "Text"), place(*position)];
            if let InsertPosition::At(_) = position {
                c.push(field(Slot::B, "Position, in characters from the start"));
            }
            c
        }
        RenameOp::Remove { .. } => vec![
            field(Slot::A, "From character (0 is the first)"),
            field(Slot::B, "How many characters"),
        ],
        RenameOp::ChangeCase(mode) => vec![Control::Choice {
            label: "Case",
            options: CASE_MODES
                .iter()
                .map(|m| (m.label(), Pick::Case(*m)))
                .collect(),
            chosen: Some(Pick::Case(*mode)),
        }],
        RenameOp::Number { position, .. } => {
            let mut c = vec![
                field(Slot::A, "Start at"),
                field(Slot::B, "Step"),
                field(Slot::C, "Digits (zero-padded)"),
                field(Slot::D, "Separator"),
                place(*position),
            ];
            if let InsertPosition::At(_) = position {
                c.push(field(Slot::E, "Position, in characters from the start"));
            }
            c
        }
        RenameOp::DateStamp {
            format, position, ..
        } => {
            let mut c = vec![
                Control::Choice {
                    label: "Format",
                    options: DATE_FORMATS
                        .iter()
                        .map(|f| (f.label(), Pick::Format(*f)))
                        .collect(),
                    chosen: Some(Pick::Format(*format)),
                },
                field(Slot::A, "Separator"),
                place(*position),
            ];
            if let InsertPosition::At(_) = position {
                c.push(field(Slot::B, "Position, in characters from the start"));
            }
            c.push(Control::Note {
                text: "Each file's own date: when it was last modified, in UTC".to_string(),
                error: false,
            });
            c
        }
        RenameOp::Regex {
            pattern,
            case_sensitive,
            ..
        } => {
            let note = match regex_replace(pattern, "", !*case_sensitive, "") {
                Err(problem) if !pattern.is_empty() => Control::Note {
                    text: format!("Not a pattern yet: {}", problem.message()),
                    error: true,
                },
                _ => Control::Note {
                    text: "& is the whole match, \\1 to \\9 its groups".to_string(),
                    error: false,
                },
            };
            vec![
                field(Slot::A, "Pattern (POSIX extended)"),
                field(Slot::B, "Replace with"),
                Control::Switch {
                    label: "Match case",
                    on: *case_sensitive,
                    flip: Flip::MatchCase,
                },
                note,
            ]
        }
        RenameOp::Trim { mode, .. } => vec![
            field(Slot::A, "Characters to trim (blank: spaces)"),
            Control::Choice {
                label: "From",
                options: vec![
                    ("Both ends", Pick::Trim(TrimMode::Both)),
                    ("The start", Pick::Trim(TrimMode::Start)),
                    ("The end", Pick::Trim(TrimMode::End)),
                ],
                chosen: Some(Pick::Trim(*mode)),
            },
        ],
        RenameOp::Extension(ext) => {
            let kind = ext_kind(ext);
            let mut c = vec![Control::Choice {
                label: "Extension",
                options: vec![
                    ("lower case", Pick::Ext(ExtKind::Lower)),
                    ("UPPER CASE", Pick::Ext(ExtKind::Upper)),
                    ("Remove it", Pick::Ext(ExtKind::Remove)),
                    ("Replace it", Pick::Ext(ExtKind::Replace)),
                    ("Add one", Pick::Ext(ExtKind::Add)),
                ],
                chosen: Some(Pick::Ext(kind)),
            }];
            if matches!(kind, ExtKind::Replace | ExtKind::Add) {
                c.push(field(Slot::A, "Extension"));
            }
            c
        }
        RenameOp::Template { .. } => vec![
            field(Slot::A, "Template"),
            Control::Note {
                text: "{name} {ext} {original}; {n} and {N} count from 0".to_string(),
                error: false,
            },
        ],
    }
}

/// Every case mode, in the order the chooser lists them.
const CASE_MODES: [CaseMode; 8] = [
    CaseMode::Lower,
    CaseMode::Upper,
    CaseMode::Title,
    CaseMode::Sentence,
    CaseMode::Toggle,
    CaseMode::CamelCase,
    CaseMode::SnakeCase,
    CaseMode::KebabCase,
];

/// Every date format, in the order the chooser lists them.
const DATE_FORMATS: [DateFormat; 5] = [
    DateFormat::YmdHyphen,
    DateFormat::YmdSlash,
    DateFormat::DmyHyphen,
    DateFormat::YmdCompact,
    DateFormat::Timestamp,
];

fn ext_kind(ext: &ExtensionOp) -> ExtKind {
    match ext {
        ExtensionOp::Lower => ExtKind::Lower,
        ExtensionOp::Upper => ExtKind::Upper,
        ExtensionOp::Remove => ExtKind::Remove,
        ExtensionOp::Replace(_) => ExtKind::Replace,
        ExtensionOp::Add(_) => ExtKind::Add,
    }
}

/// The text a rule's box `slot` shows.
fn field_text(op: &RenameOp, slot: Slot) -> String {
    let at = |position: &InsertPosition| match position {
        InsertPosition::At(n) => n.to_string(),
        _ => String::new(),
    };
    match (op, slot) {
        (RenameOp::FindReplace { find, .. }, Slot::A) => find.clone(),
        (RenameOp::FindReplace { replace, .. }, Slot::B) => replace.clone(),
        (RenameOp::Insert { text, .. }, Slot::A) => text.clone(),
        (RenameOp::Insert { position, .. }, Slot::B) => at(position),
        (RenameOp::Remove { from, .. }, Slot::A) => from.to_string(),
        (RenameOp::Remove { count, .. }, Slot::B) => count.to_string(),
        (RenameOp::Number { start, .. }, Slot::A) => start.to_string(),
        (RenameOp::Number { step, .. }, Slot::B) => step.to_string(),
        (RenameOp::Number { padding, .. }, Slot::C) => padding.to_string(),
        (RenameOp::Number { separator, .. }, Slot::D) => separator.clone(),
        (RenameOp::Number { position, .. }, Slot::E) => at(position),
        (RenameOp::DateStamp { separator, .. }, Slot::A) => separator.clone(),
        (RenameOp::DateStamp { position, .. }, Slot::B) => at(position),
        (RenameOp::Regex { pattern, .. }, Slot::A) => pattern.clone(),
        (RenameOp::Regex { replacement, .. }, Slot::B) => replacement.clone(),
        (RenameOp::Trim { chars, .. }, Slot::A) => chars.clone(),
        (RenameOp::Extension(ExtensionOp::Replace(e) | ExtensionOp::Add(e)), Slot::A) => e.clone(),
        (RenameOp::Template { template }, Slot::A) => template.clone(),
        _ => String::new(),
    }
}

/// The largest number a counting box takes. A position or a count past any
/// name's length already means "the end", and a padding of a thousand digits
/// would make a name no filesystem accepts.
const MAX_FIELD_NUMBER: usize = 255;

/// Write `text` into a rule's box `slot`. Returns whether it was taken: a
/// counting box refuses what is not a number from 0 to [`MAX_FIELD_NUMBER`],
/// and the rule keeps its last good value while the box shows what was typed.
fn set_field(op: &mut RenameOp, slot: Slot, text: &str) -> bool {
    let number = || {
        text.trim()
            .parse::<usize>()
            .ok()
            .filter(|n| *n <= MAX_FIELD_NUMBER)
    };
    let set_at = |position: &mut InsertPosition| match number() {
        Some(n) => {
            *position = InsertPosition::At(n);
            true
        }
        None => false,
    };
    match (op, slot) {
        (RenameOp::FindReplace { find, .. }, Slot::A) => *find = text.to_string(),
        (RenameOp::FindReplace { replace, .. }, Slot::B) => *replace = text.to_string(),
        (RenameOp::Insert { text: t, .. }, Slot::A) => *t = text.to_string(),
        (RenameOp::Insert { position, .. }, Slot::B) => return set_at(position),
        (RenameOp::Remove { from, .. }, Slot::A) => match number() {
            Some(n) => *from = n,
            None => return false,
        },
        (RenameOp::Remove { count, .. }, Slot::B) => match number() {
            Some(n) => *count = n,
            None => return false,
        },
        (RenameOp::Number { start, .. }, Slot::A) => match text.trim().parse::<usize>() {
            // A start is not bounded like the others: numbering from 1000
            // is ordinary.
            Ok(n) => *start = n,
            Err(_) => return false,
        },
        (RenameOp::Number { step, .. }, Slot::B) => match number() {
            Some(n) => *step = n,
            None => return false,
        },
        (RenameOp::Number { padding, .. }, Slot::C) => match number() {
            Some(n) => *padding = n,
            None => return false,
        },
        (RenameOp::Number { separator, .. }, Slot::D) => *separator = text.to_string(),
        (RenameOp::Number { position, .. }, Slot::E) => return set_at(position),
        (RenameOp::DateStamp { separator, .. }, Slot::A) => *separator = text.to_string(),
        (RenameOp::DateStamp { position, .. }, Slot::B) => return set_at(position),
        (RenameOp::Regex { pattern, .. }, Slot::A) => *pattern = text.to_string(),
        (RenameOp::Regex { replacement, .. }, Slot::B) => *replacement = text.to_string(),
        (RenameOp::Trim { chars, .. }, Slot::A) => *chars = text.to_string(),
        (RenameOp::Extension(ExtensionOp::Replace(e) | ExtensionOp::Add(e)), Slot::A) => {
            *e = text.to_string();
        }
        (RenameOp::Template { template }, Slot::A) => *template = text.to_string(),
        _ => return false,
    }
    true
}

/// Apply a choice to a rule.
fn apply_pick(op: &mut RenameOp, pick: Pick) {
    let move_to = |position: &mut InsertPosition, kind: PositionKind| {
        *position = match kind {
            PositionKind::Start => InsertPosition::Start,
            PositionKind::End => InsertPosition::End,
            // Keep a position already chosen; start a new one at the front.
            PositionKind::At => match *position {
                InsertPosition::At(n) => InsertPosition::At(n),
                _ => InsertPosition::At(0),
            },
        };
    };
    match (op, pick) {
        (RenameOp::ChangeCase(mode), Pick::Case(m)) => *mode = m,
        (
            RenameOp::Insert { position, .. }
            | RenameOp::Number { position, .. }
            | RenameOp::DateStamp { position, .. },
            Pick::Position(kind),
        ) => move_to(position, kind),
        (RenameOp::DateStamp { format, .. }, Pick::Format(f)) => *format = f,
        (RenameOp::Trim { mode, .. }, Pick::Trim(t)) => *mode = t,
        (RenameOp::Extension(ext), Pick::Ext(kind)) => {
            // The text typed for Replace survives a change to Add and back.
            let typed = match ext {
                ExtensionOp::Replace(e) | ExtensionOp::Add(e) => e.clone(),
                _ => String::new(),
            };
            *ext = match kind {
                ExtKind::Lower => ExtensionOp::Lower,
                ExtKind::Upper => ExtensionOp::Upper,
                ExtKind::Remove => ExtensionOp::Remove,
                ExtKind::Replace => ExtensionOp::Replace(typed),
                ExtKind::Add => ExtensionOp::Add(typed),
            };
        }
        _ => {}
    }
}

/// Flip one of a rule's switches.
fn apply_flip(op: &mut RenameOp, flip: Flip) {
    match (op, flip) {
        (RenameOp::FindReplace { case_sensitive, .. }, Flip::MatchCase)
        | (RenameOp::Regex { case_sensitive, .. }, Flip::MatchCase) => {
            *case_sensitive = !*case_sensitive;
        }
        (RenameOp::FindReplace { replace_all, .. }, Flip::EveryOccurrence) => {
            *replace_all = !*replace_all;
        }
        _ => {}
    }
}

/// The text boxes a rule's form has, in order: where Tab goes next.
fn rule_slots(op: &RenameOp) -> Vec<Slot> {
    rule_controls(op)
        .iter()
        .filter_map(|c| match c {
            Control::Field { slot, .. } => Some(*slot),
            _ => None,
        })
        .collect()
}

/// The kinds of rule the Add Rule menu offers, in its order, named as each
/// rule names itself in the pipeline (`RenameOp::label`) -- which
/// `the_add_rule_menu_adds_every_kind` holds them to.
const RULE_KINDS: [&str; 10] = [
    "Find & Replace",
    "Insert Text",
    "Remove Characters",
    "Change Case",
    "Add Numbering",
    "Date Stamp",
    "Regex Replace",
    "Trim",
    "Extension",
    "Template",
];

/// A new rule of kind `index` into [`RULE_KINDS`], with values that change
/// nothing until the user fills them in -- except the kinds that need nothing
/// filled in, which start doing their one obvious thing.
fn new_rule(index: usize) -> Option<RenameOp> {
    Some(match index {
        0 => RenameOp::FindReplace {
            find: String::new(),
            replace: String::new(),
            case_sensitive: false,
            replace_all: true,
        },
        1 => RenameOp::Insert {
            text: String::new(),
            position: InsertPosition::Start,
        },
        2 => RenameOp::Remove { from: 0, count: 0 },
        3 => RenameOp::ChangeCase(CaseMode::Lower),
        4 => RenameOp::Number {
            start: 1,
            step: 1,
            padding: 3,
            position: InsertPosition::End,
            separator: String::from("_"),
        },
        5 => RenameOp::DateStamp {
            format: DateFormat::YmdHyphen,
            position: InsertPosition::Start,
            separator: String::from("_"),
        },
        6 => RenameOp::Regex {
            pattern: String::new(),
            replacement: String::new(),
            case_sensitive: true,
        },
        7 => RenameOp::Trim {
            chars: String::new(),
            mode: TrimMode::Both,
        },
        8 => RenameOp::Extension(ExtensionOp::Lower),
        9 => RenameOp::Template {
            template: String::from("{original}"),
        },
        _ => return None,
    })
}

// ============================================================================
// Pointer targets and layout
// ============================================================================

/// Everything in the window a pointer can press, as the renderer records it.
///
/// The renamer drew a toolbar of five buttons, three sidebar tabs, a rule
/// list, a checkbox on every file and a conflicts filter, and handled no
/// pointer event (`known-issues.md` →
/// `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`). Every
/// variant is recorded by the walk that paints it ([`RenamerApp::frame`]), so
/// a control and the place a press finds it cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    OpenFolder,
    AddRule,
    Rename,
    Undo,
    Redo,
    ClearRules,
    /// A sidebar tab.
    Panel(SidebarPanel),
    /// A rule in the pipeline, which selects it for editing.
    Rule(usize),
    RuleUp(usize),
    RuleDown(usize),
    RuleRemove(usize),
    /// A box in the selected rule's editor.
    Field(Slot),
    /// An option in one of its choice rows.
    Choice(Pick),
    /// One of its switches.
    Switch(Flip),
    /// The operations panel itself, which scrolls under the wheel.
    Ops,
    SearchBox,
    ExtensionBox,
    ConflictsOnly,
    /// The file list itself, which scrolls under the wheel.
    Files,
    /// A file's row, by its index in `files`: moves the cursor there.
    File(usize),
    /// A file's checkbox: ticks or unticks it.
    Check(usize),
    /// The shortcut card; a press anywhere puts it away.
    HelpCard,
}

/// Height of the bar above the file list holding its filters.
const FILTER_BAR_H: f32 = 32.0;
/// Height of the sidebar's tabs.
const TABS_H: f32 = 28.0;
/// Height of the file list's column headings.
const FILE_HEADER_H: f32 = 24.0;
/// Height of one rule in the pipeline list.
const RULE_ROW_H: f32 = 30.0;
/// Distance from one rule's top to the next.
const RULE_PITCH: f32 = 34.0;
/// Height of a text box in the rule editor.
const FIELD_H: f32 = 24.0;
/// Height of a choice chip.
const CHIP_H: f32 = 22.0;

/// Where the window's regions go at a given size.
///
/// The renamer laid itself out from the constants it asks to open at, and
/// ignored the size it was given: a maximised window drew a 1100-by-750 island
/// in its corner, and a smaller one cut its own file list off. One function
/// for the drawing and for everything that needs a region between frames.
#[derive(Debug, Clone, Copy)]
struct Layout {
    toolbar: Rect,
    tabs: Rect,
    panel: Rect,
    filters: Rect,
    table: Rect,
    status: Rect,
}

impl Layout {
    fn of(width: f32, height: f32) -> Self {
        let body_y = TOOLBAR_HEIGHT;
        let body_h = (height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT).max(0.0);
        let side_w = SIDEBAR_WIDTH.min(width);
        let list_x = side_w;
        let list_w = (width - side_w).max(0.0);
        Self {
            toolbar: Rect::new(0.0, 0.0, width, TOOLBAR_HEIGHT),
            tabs: Rect::new(0.0, body_y, side_w, TABS_H),
            panel: Rect::new(
                0.0,
                body_y + TABS_H + 4.0,
                side_w,
                (body_h - TABS_H - 4.0).max(0.0),
            ),
            filters: Rect::new(list_x, body_y, list_w, FILTER_BAR_H),
            table: Rect::new(
                list_x,
                body_y + FILTER_BAR_H,
                list_w,
                (body_h - FILTER_BAR_H).max(0.0),
            ),
            status: Rect::new(0.0, height - STATUS_BAR_HEIGHT, width, STATUS_BAR_HEIGHT),
        }
    }

    /// How many whole file rows the list shows.
    fn file_rows(self) -> usize {
        ((self.table.h - FILE_HEADER_H) / LINE_HEIGHT).max(0.0) as usize
    }
}

/// The toolbar's buttons, left to right, with each one's key.
const TOOLBAR_BUTTONS: [(&str, Target); 6] = [
    ("Open Folder…", Target::OpenFolder),
    ("Add Rule ▾", Target::AddRule),
    ("Rename", Target::Rename),
    ("Undo", Target::Undo),
    ("Redo", Target::Redo),
    ("Clear Rules", Target::ClearRules),
];

impl Target {
    /// What pressing this does, with its key, for the status bar while the
    /// pointer is on it.
    fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::OpenFolder => "Choose the folder whose files to rename (Ctrl+O)",
            Self::AddRule => "Add a rule of any kind (R)",
            Self::Rename => "Rename the ticked files (Enter)",
            Self::Undo => "Put the last rename back (Ctrl+Z)",
            Self::Redo => "Do it again (Ctrl+Y)",
            Self::ClearRules => "Remove every rule (Ctrl+Backspace)",
            Self::RuleUp(_) => "Move this rule earlier (PageUp)",
            Self::RuleDown(_) => "Move this rule later (PageDown)",
            Self::RuleRemove(_) => "Remove this rule (Delete)",
            Self::SearchBox => "Show only names containing this (/)",
            Self::ExtensionBox => "Show only this extension (Ctrl+E)",
            Self::ConflictsOnly => "Show only the names that would collide (C)",
            Self::Check(_) => "Tick or untick this file (Space)",
            _ => return None,
        })
    }
}

/// Where the keyboard is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    /// The file list and the shortcuts.
    List,
    /// A box in the selected rule's editor.
    Field(Slot),
    /// The search box above the file list.
    Search,
    /// The extension filter above the file list.
    Extension,
}

/// Apply a keystroke to a text box, as every text box in the tree does.
/// Returns whether the key was an editing key.
fn edit_text(input: &mut TextInput, key: &KeyEvent) -> bool {
    let shift = key.modifiers.shift;
    let ctrl = key.modifiers.ctrl;
    match key.key {
        Key::Left => input.move_cursor_left(shift, NORMAL_TEXT, FontWeightHint::Regular),
        Key::Right => input.move_cursor_right(shift, NORMAL_TEXT, FontWeightHint::Regular),
        Key::Home => input.move_home(shift),
        Key::End => input.move_end(shift),
        Key::Backspace => input.backspace(),
        Key::Delete => input.delete(),
        Key::A if ctrl => input.select_all(),
        Key::C if ctrl => input.copy(),
        Key::X if ctrl => input.cut(),
        Key::V if ctrl => input.paste(),
        _ => {
            if key.text.is_empty() || ctrl {
                return false;
            }
            for ch in key.text.chars() {
                input.insert_char(ch);
            }
        }
    }
    true
}

impl RenamerApp {
    /// Draw the window at `width` by `height`, recording every control where
    /// it is drawn. Both the renderer and the hit test.
    fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        let l = Layout::of(width, height);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        self.draw_toolbar(&mut f, l.toolbar);
        self.draw_sidebar(&mut f, &l);
        self.draw_file_list(&mut f, &l);
        self.draw_status_bar(&mut f, l.status);
        if let Some(menu) = &self.rule_menu {
            f.extend(menu.render(&self.palette));
        }
        // And the shortcut list over everything, because it is the one thing a
        // reader asked for explicitly.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (width, height),
                TOOLBAR_HEIGHT,
                SHORTCUTS,
                "F1 or ? closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));
        }
        f
    }

    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`, so
    /// an app that keeps the name draws nothing and reports no error.
    ///
    /// **Tests only**: the window's `render` takes the frame itself, because it
    /// keeps the frame's boxes for the pointer as well as its commands.
    #[cfg(test)]
    fn render_commands(&self) -> Vec<RenderCommand> {
        self.frame(self.last_width, self.last_height)
            .into_tree()
            .commands
    }

    /// A small button, lit while the pointer is on it.
    fn draw_button(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        label: &str,
        color: Color,
        target: Target,
    ) {
        let surface = if self.hover == Some(target) {
            Surface::Panel
        } else {
            Surface::Card
        };
        self.palette
            .push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, surface);
        f.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + (rect.h - SMALL_TEXT) / 2.0 - 1.0,
            text: label.into(),
            font_size: SMALL_TEXT,
            color,
            font_weight: FontWeightHint::Bold,
            max_width: Some((rect.w - 12.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(target, rect);
    }

    fn draw_toolbar(&self, f: &mut Frame<Target>, bar: Rect) {
        self.palette.push_surface(
            f,
            bar.x,
            bar.y,
            bar.w,
            bar.h,
            0.0,
            Surface::Strip(Edge::Bottom),
        );
        f.push(RenderCommand::Text {
            x: PADDING,
            y: 10.0,
            text: "Batch File Renamer".into(),
            font_size: TITLE_TEXT,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Inked: these are button *labels*, so each takes the 4.5:1 floor.
        let mut bx = 220.0;
        for (label, target) in TOOLBAR_BUTTONS {
            let color = self.palette.ink(match target {
                Target::Rename => self.palette.green,
                Target::Undo | Target::Redo => self.palette.peach,
                Target::ClearRules => self.palette.red,
                _ => self.palette.blue,
            });
            let bw = text::padded_width(label, 10.0, 12.0, FontWeightHint::Bold);
            self.draw_button(
                f,
                Rect::new(bx, 6.0, bw, BUTTON_HEIGHT),
                label,
                color,
                target,
            );
            bx += bw + 6.0;
        }

        let count_text = format!(
            "{} files | {} to rename | {} conflicts",
            self.files.len(),
            self.rename_count(),
            self.conflict_count()
        );
        let count_x = (bar.w - 300.0).max(bx + 8.0);
        f.push(RenderCommand::Text {
            x: count_x,
            y: 14.0,
            text: count_text,
            font_size: SMALL_TEXT,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((bar.w - count_x - PADDING).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn draw_sidebar(&self, f: &mut Frame<Target>, l: &Layout) {
        let side = Rect::new(0.0, l.tabs.y, l.tabs.w, l.status.y - l.tabs.y);
        f.push(RenderCommand::FillRect {
            x: side.x,
            y: side.y,
            width: side.w,
            height: side.h,
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });

        let tabs = [
            ("Operations", SidebarPanel::Operations),
            ("Preview", SidebarPanel::Preview),
            ("History", SidebarPanel::History),
        ];
        let tab_w = l.tabs.w / 3.0;
        for (i, (label, panel)) in tabs.into_iter().enumerate() {
            let tab = Rect::new(l.tabs.x + i as f32 * tab_w, l.tabs.y, tab_w, TABS_H);
            let is_active = self.sidebar_panel == panel;
            f.push(RenderCommand::FillRect {
                x: tab.x,
                y: tab.y,
                width: tab.w,
                height: tab.h,
                color: if is_active || self.hover == Some(Target::Panel(panel)) {
                    self.palette.surface0
                } else {
                    self.palette.mantle
                },
                corner_radii: CornerRadii::ZERO,
            });
            if is_active {
                f.push(RenderCommand::FillRect {
                    x: tab.x,
                    y: tab.y,
                    width: tab.w,
                    height: 2.0,
                    color: self.palette.blue,
                    corner_radii: CornerRadii::ZERO,
                });
            }
            f.push(RenderCommand::Text {
                x: tab.x + 6.0,
                y: tab.y + 8.0,
                text: label.into(),
                font_size: SMALL_TEXT,
                color: if is_active {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab.w - 12.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::Panel(panel), tab);
        }

        let (x, y) = (l.panel.x, l.panel.y);
        match self.sidebar_panel {
            SidebarPanel::Operations => self.draw_operations_panel(f, l.panel),
            SidebarPanel::Preview => f.draw_with(|cmds| self.render_preview_panel(cmds, x, y)),
            SidebarPanel::History => f.draw_with(|cmds| self.render_history_panel(cmds, x, y)),
        }
    }

    /// The rule pipeline, the Add Rule button, and -- for the selected rule --
    /// its editor; scrolled by `ops_scroll` and clipped to the panel.
    ///
    /// The editor is the control the renamer never had. Find and replace,
    /// insert, remove, regex, date stamp, template and four of the five
    /// extension rules were implemented and tested, and none could be added:
    /// they need a string typed or a choice made, and the app drew no text box
    /// and no chooser anywhere (`TD-C-RENAMER-CAN-ONLY-ADD-THE-RULES-THAT-NEED-NO-TYPING`).
    fn draw_operations_panel(&self, f: &mut Frame<Target>, panel: Rect) {
        f.hit(Target::Ops, panel);
        f.clip(panel);
        self.draw_operations_content(f, panel, self.ops_scroll);
        f.unclip();
    }

    /// The operations panel's content from `panel`'s top, scrolled by
    /// `scroll`. Returns the `y` just below the last thing drawn, which is how
    /// [`ops_content_height`](Self::ops_content_height) measures it.
    fn draw_operations_content(&self, f: &mut Frame<Target>, panel: Rect, scroll: f32) -> f32 {
        let x = panel.x;
        let w = panel.w;
        let mut y = panel.y - scroll;

        if self.operations.is_empty() {
            for (i, line) in [
                "No rules yet.",
                "Add Rule, or R, adds any kind;",
                "L U T S K W E X N add the common ones.",
            ]
            .iter()
            .enumerate()
            {
                f.push(RenderCommand::Text {
                    x: x + PADDING,
                    y: y + PADDING + i as f32 * 16.0,
                    text: (*line).into(),
                    font_size: SMALL_TEXT,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(w - PADDING * 2.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            y += PADDING + 3.0 * 16.0 + 8.0;
        }

        for (i, op) in self.operations.iter().enumerate() {
            let row = Rect::new(x + 4.0, y, w - 8.0, RULE_ROW_H);
            let is_selected = i == self.selected_op;
            f.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: if is_selected || self.hover == Some(Target::Rule(i)) {
                    self.palette.surface0
                } else {
                    self.palette.mantle
                },
                corner_radii: CornerRadii::all(4.0),
            });
            f.push(RenderCommand::FillRect {
                x: x + 8.0,
                y: y + 6.0,
                width: 4.0,
                height: 18.0,
                color: op.color(&self.palette),
                corner_radii: CornerRadii::all(2.0),
            });
            let buttons_w = 3.0 * 22.0;
            f.push(RenderCommand::Text {
                x: x + 18.0,
                y: y + 4.0,
                text: format!("{}. {}", i.saturating_add(1), op.label()),
                font_size: SMALL_TEXT,
                color: if is_selected {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
                font_weight: FontWeightHint::Bold,
                max_width: Some((w - 40.0 - buttons_w).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            let detail = op_detail(op);
            if !detail.is_empty() {
                f.push(RenderCommand::Text {
                    x: x + 18.0,
                    y: y + 17.0,
                    text: detail,
                    font_size: OP_DETAIL_SIZE,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some((OP_DETAIL_WIDTH - buttons_w).max(0.0)),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            f.hit(Target::Rule(i), row);
            // The row's three buttons, recorded after the row so they win.
            let mut bx = row.right() - buttons_w;
            for (glyph, target) in [
                ("↑", Target::RuleUp(i)),
                ("↓", Target::RuleDown(i)),
                ("×", Target::RuleRemove(i)),
            ] {
                let b = Rect::new(bx + 2.0, y + 5.0, 20.0, 20.0);
                if self.hover == Some(target) {
                    f.push(RenderCommand::FillRect {
                        x: b.x,
                        y: b.y,
                        width: b.w,
                        height: b.h,
                        color: self.palette.surface1,
                        corner_radii: CornerRadii::all(4.0),
                    });
                }
                f.push(RenderCommand::Text {
                    x: b.x + 5.0,
                    y: b.y + 3.0,
                    text: glyph.into(),
                    font_size: SMALL_TEXT,
                    color: self.palette.subtext1,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                f.hit(target, b);
                bx += 22.0;
            }
            y += RULE_PITCH;
        }

        self.draw_button(
            f,
            Rect::new(x + 4.0, y, w - 8.0, BUTTON_HEIGHT),
            "Add Rule ▾  (R)",
            self.palette.ink(self.palette.blue),
            Target::AddRule,
        );
        y += BUTTON_HEIGHT + 10.0;

        if let Some(op) = self.operations.get(self.selected_op) {
            y = self.draw_rule_editor(f, op, x, y, w);
        }
        y
    }

    /// The selected rule's form. Returns the `y` below it.
    fn draw_rule_editor(
        &self,
        f: &mut Frame<Target>,
        op: &RenameOp,
        x: f32,
        mut y: f32,
        w: f32,
    ) -> f32 {
        f.push(RenderCommand::Text {
            x: x + PADDING,
            y,
            text: format!(
                "Rule {}: {}",
                self.selected_op.saturating_add(1),
                op.label()
            ),
            font_size: NORMAL_TEXT,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
        y += 22.0;
        let inner_x = x + PADDING;
        let inner_w = w - PADDING * 2.0;
        for control in rule_controls(op) {
            match control {
                Control::Field { slot, label } => {
                    self.field_label(f, inner_x, y, inner_w, label);
                    y += 16.0;
                    let rect = Rect::new(inner_x, y, inner_w, FIELD_H);
                    let focused = self.focus == Focus::Field(slot);
                    let text = if focused {
                        self.draft.text().to_string()
                    } else {
                        field_text(op, slot)
                    };
                    let invalid = focused && self.draft_invalid;
                    self.draw_text_box(f, rect, &text, focused, invalid, Target::Field(slot));
                    y += FIELD_H + 8.0;
                }
                Control::Choice {
                    label,
                    options,
                    chosen,
                } => {
                    self.field_label(f, inner_x, y, inner_w, label);
                    y += 16.0;
                    let mut cx = inner_x;
                    for (text, pick) in options {
                        let cw = text::padded_width(text, 8.0, SMALL_TEXT, FontWeightHint::Regular);
                        if cx + cw > inner_x + inner_w && cx > inner_x {
                            cx = inner_x;
                            y += CHIP_H + 4.0;
                        }
                        let chip = Rect::new(cx, y, cw, CHIP_H);
                        let lit = chosen == Some(pick);
                        let target = Target::Choice(pick);
                        let surface = if lit || self.hover == Some(target) {
                            Surface::Panel
                        } else {
                            Surface::Card
                        };
                        self.palette
                            .push_surface(f, chip.x, chip.y, chip.w, chip.h, 4.0, surface);
                        if lit {
                            f.push(RenderCommand::StrokeRect {
                                x: chip.x,
                                y: chip.y,
                                width: chip.w,
                                height: chip.h,
                                color: self.palette.blue,
                                line_width: 1.0,
                                corner_radii: CornerRadii::all(4.0),
                            });
                        }
                        f.push(RenderCommand::Text {
                            x: chip.x + 8.0,
                            y: chip.y + 4.0,
                            text: text.into(),
                            font_size: SMALL_TEXT,
                            color: if lit {
                                self.palette.ink(self.palette.blue)
                            } else {
                                self.palette.subtext1
                            },
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(chip.w - 12.0),
                            overflow: TextOverflow::Ellipsis,
                        });
                        f.hit(target, chip);
                        cx += cw + 4.0;
                    }
                    y += CHIP_H + 10.0;
                }
                Control::Switch { label, on, flip } => {
                    let row = Rect::new(inner_x, y, inner_w, 22.0);
                    f.push(RenderCommand::FillRect {
                        x: row.x,
                        y: row.y + 4.0,
                        width: 14.0,
                        height: 14.0,
                        color: if on {
                            self.palette.green
                        } else {
                            self.palette.surface2
                        },
                        corner_radii: CornerRadii::all(2.0),
                    });
                    if on {
                        f.push(RenderCommand::Text {
                            x: row.x + 2.0,
                            y: row.y + 4.0,
                            text: "✓".into(),
                            font_size: 10.0,
                            color: self.palette.crust,
                            font_weight: FontWeightHint::Bold,
                            max_width: Some(12.0),
                            overflow: TextOverflow::Ellipsis,
                        });
                    }
                    f.push(RenderCommand::Text {
                        x: row.x + 22.0,
                        y: row.y + 4.0,
                        text: label.into(),
                        font_size: SMALL_TEXT,
                        color: self.palette.subtext1,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(row.w - 22.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                    f.hit(Target::Switch(flip), row);
                    y += 26.0;
                }
                Control::Note { text, error } => {
                    f.push(RenderCommand::Text {
                        x: inner_x,
                        y,
                        text,
                        font_size: SMALL_TEXT,
                        color: if error {
                            self.palette.ink(self.palette.red)
                        } else {
                            self.palette.subtext0
                        },
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(inner_w),
                        overflow: TextOverflow::Ellipsis,
                    });
                    y += 18.0;
                }
            }
        }
        y
    }

    fn field_label(&self, f: &mut Frame<Target>, x: f32, y: f32, w: f32, label: &str) {
        f.push(RenderCommand::Text {
            x,
            y,
            text: label.into(),
            font_size: SMALL_TEXT,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// A text box: its text, a caret and selection while it has the keyboard
    /// (the toolkit's `textedit`, so it scrolls the same as every other box),
    /// and a red rule under it while what is typed is not a value the rule
    /// can take.
    fn draw_text_box(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        text: &str,
        focused: bool,
        invalid: bool,
        target: Target,
    ) {
        self.palette
            .push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, Surface::Card);
        if focused || invalid {
            f.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.bottom() - 2.0,
                width: rect.w,
                height: 2.0,
                color: if invalid {
                    self.palette.red
                } else {
                    self.palette.blue
                },
                corner_radii: CornerRadii::ZERO,
            });
        }
        let mut tree = RenderTree::new();
        let (cursor, anchor) = if focused {
            (self.draft.cursor(), self.draft.selection_anchor())
        } else {
            // Unfocused, the box shows the start of what it holds.
            (guitk::text::TextCursor::default(), None)
        };
        guitk::textedit::draw(
            &mut tree,
            &guitk::textedit::SingleLine {
                text,
                cursor,
                selection_anchor: anchor,
                focused,
                x: rect.x + 6.0,
                y: rect.y + 5.0,
                width: rect.w - 12.0,
                line_height: 16.0,
                font_size: NORMAL_TEXT,
                weight: FontWeightHint::Regular,
                color: self.palette.text,
                selection_bg: self.palette.blue,
                selection_fg: self.palette.crust,
                caret_width: 1.5,
            },
        );
        f.extend(tree.commands);
        f.hit(target, rect);
    }

    /// How tall the operations panel's content is, for the wheel's limit:
    /// drawn once, unscrolled, into a frame nobody shows.
    fn ops_content_height(&self, panel: Rect) -> f32 {
        let mut f = Frame::new(panel.w, panel.h);
        let bottom = self.draw_operations_content(&mut f, panel, 0.0);
        bottom - panel.y + 12.0
    }

    /// The file list: filter bar, headings, and the rows from `file_scroll`.
    fn draw_file_list(&self, f: &mut Frame<Target>, l: &Layout) {
        // Filters: search, extension, conflicts only. The extension filter
        // was a field `filtered_files` read and nothing wrote.
        let bar = l.filters;
        f.push(RenderCommand::FillRect {
            x: bar.x,
            y: bar.y,
            width: bar.w,
            height: bar.h,
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });
        let search = Rect::new(
            bar.x + 8.0,
            bar.y + 4.0,
            240.0_f32.min(bar.w * 0.4),
            FIELD_H,
        );
        let search_text = if self.focus == Focus::Search {
            self.draft.text().to_string()
        } else if self.search_text.is_empty() {
            String::new()
        } else {
            self.search_text.clone()
        };
        self.draw_text_box(
            f,
            search,
            &search_text,
            self.focus == Focus::Search,
            false,
            Target::SearchBox,
        );
        if search_text.is_empty() && self.focus != Focus::Search {
            f.push(RenderCommand::Text {
                x: search.x + 6.0,
                y: search.y + 5.0,
                text: "Search names  /".into(),
                font_size: SMALL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(search.w - 12.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let ext = Rect::new(search.right() + 8.0, bar.y + 4.0, 110.0, FIELD_H);
        let ext_text = if self.focus == Focus::Extension {
            self.draft.text().to_string()
        } else {
            self.filter_extension.clone()
        };
        self.draw_text_box(
            f,
            ext,
            &ext_text,
            self.focus == Focus::Extension,
            false,
            Target::ExtensionBox,
        );
        if ext_text.is_empty() && self.focus != Focus::Extension {
            f.push(RenderCommand::Text {
                x: ext.x + 6.0,
                y: ext.y + 5.0,
                text: "Extension".into(),
                font_size: SMALL_TEXT,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(ext.w - 12.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let chip_label = "Conflicts only  C";
        let chip_w = text::padded_width(chip_label, 8.0, SMALL_TEXT, FontWeightHint::Regular);
        let chip = Rect::new(ext.right() + 8.0, bar.y + 5.0, chip_w, CHIP_H);
        let surface = if self.filter_conflicts || self.hover == Some(Target::ConflictsOnly) {
            Surface::Panel
        } else {
            Surface::Card
        };
        self.palette
            .push_surface(f, chip.x, chip.y, chip.w, chip.h, 4.0, surface);
        if self.filter_conflicts {
            f.push(RenderCommand::StrokeRect {
                x: chip.x,
                y: chip.y,
                width: chip.w,
                height: chip.h,
                color: self.palette.red,
                line_width: 1.0,
                corner_radii: CornerRadii::all(4.0),
            });
        }
        f.push(RenderCommand::Text {
            x: chip.x + 8.0,
            y: chip.y + 4.0,
            text: chip_label.into(),
            font_size: SMALL_TEXT,
            color: if self.filter_conflicts {
                self.palette.ink(self.palette.red)
            } else {
                self.palette.subtext1
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(chip.w - 12.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::ConflictsOnly, chip);

        let table_rect = l.table;
        f.hit(Target::Files, table_rect);
        let (x, y, w) = (table_rect.x, table_rect.y, table_rect.w);
        f.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: FILE_HEADER_H,
            color: self.palette.surface0,
            corner_radii: CornerRadii::ZERO,
        });
        let table = Table::new(FILE_COLUMNS, x);
        f.draw_with(|cmds| table.header(cmds, y + 5.0, self.palette.subtext1, SMALL_TEXT));

        let filtered = self.filtered_files();
        let visible_rows = l.file_rows();
        let mut ry = y + FILE_HEADER_H;
        for (display_idx, (file_idx, file)) in filtered
            .iter()
            .enumerate()
            .skip(self.file_scroll)
            .take(visible_rows)
        {
            let row = Rect::new(x, ry, w, LINE_HEIGHT);
            let is_selected = *file_idx == self.selected_file;
            let bg = if is_selected || self.hover == Some(Target::File(*file_idx)) {
                self.palette.surface0
            } else if display_idx % 2 == 0 {
                self.palette.base
            } else {
                self.palette.mantle
            };
            f.push(RenderCommand::FillRect {
                x,
                y: ry,
                width: w,
                height: LINE_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });
            f.hit(Target::File(*file_idx), row);

            // The checkbox is a graphic, not a text cell, so it is placed
            // against its column's left edge by hand -- and it is its own
            // target, recorded after the row so a press on it ticks rather
            // than only moving the cursor.
            let cx = table.left(COL_CHECK);
            let check = Rect::new(cx - 3.0, ry + 1.0, 20.0, LINE_HEIGHT - 2.0);
            f.push(RenderCommand::FillRect {
                x: cx,
                y: ry + 4.0,
                width: 14.0,
                height: 14.0,
                color: if file.selected {
                    self.palette.green
                } else {
                    self.palette.surface2
                },
                corner_radii: CornerRadii::all(2.0),
            });
            if file.selected {
                f.push(RenderCommand::Text {
                    x: cx + 2.0,
                    y: ry + 4.0,
                    text: "✓".into(),
                    font_size: 10.0,
                    color: self.palette.crust,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(12.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            f.hit(Target::Check(*file_idx), check);

            let changed = file.original_name != file.new_name;
            let new_color = if file.conflict {
                self.palette.red
            } else if changed {
                self.palette.green
            } else {
                self.palette.subtext0
            };
            let status = if file.conflict {
                ("Conflict", self.palette.red)
            } else if changed {
                ("Changed", self.palette.green)
            } else {
                ("", self.palette.overlay0)
            };
            f.draw_with(|cmds| {
                // Both names are cut at the *end* (`Fit::End`). This list is a
                // rename preview, and its whole job is to let the user check
                // what is about to happen to their files before committing. A
                // name cut the usual way loses the extension and any numeric
                // suffix -- exactly the parts a rename usually changes.
                table.cell(
                    cmds,
                    COL_ORIGINAL,
                    ry + 4.0,
                    &file.original_name,
                    self.palette.text,
                    SMALL_TEXT,
                    Fit::End,
                );
                table.cell_weighted(
                    cmds,
                    COL_ARROW,
                    ry + 4.0,
                    if changed { "→" } else { "=" },
                    if changed {
                        self.palette.green
                    } else {
                        self.palette.overlay0
                    },
                    SMALL_TEXT,
                    Fit::Start,
                    FontWeightHint::Bold,
                );
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
                    self.palette.subtext0,
                    SMALL_TEXT,
                    Fit::Start,
                );
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
            });
            ry += LINE_HEIGHT;
        }
    }

    fn draw_status_bar(&self, f: &mut Frame<Target>, bar: Rect) {
        self.palette.push_surface(
            f,
            bar.x,
            bar.y,
            bar.w,
            bar.h,
            0.0,
            Surface::Strip(Edge::Top),
        );
        let msg = if let Some(tip) = self.hover.and_then(Target::tip) {
            tip.to_string()
        } else if self.status_message.is_empty() {
            format!(
                "Ready | {} files | {} selected | {} operations",
                self.files.len(),
                self.files.iter().filter(|f| f.selected).count(),
                self.operations.len()
            )
        } else {
            self.status_message.clone()
        };
        f.push(RenderCommand::Text {
            x: PADDING,
            y: bar.y + 5.0,
            text: msg,
            font_size: SMALL_TEXT,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((bar.w - PADDING * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// A rule's one-line summary under its name in the pipeline list.
///
/// The user-typed halves are elided against the row's real width; the row is
/// a fixed height, so wrapping is not an option and a silent cut would leave
/// two rules looking alike.
fn op_detail(op: &RenameOp) -> String {
    match op {
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
        RenameOp::Remove { from, count } => format!("{count} from character {from}"),
        RenameOp::ChangeCase(mode) => mode.label().into(),
        RenameOp::Number {
            start,
            step,
            padding,
            ..
        } => format!("from {start} step {step} pad {padding}"),
        RenameOp::DateStamp { format, .. } => format.label().into(),
        RenameOp::Regex {
            pattern,
            replacement,
            ..
        } => find_replace_detail(pattern, replacement, OP_DETAIL_WIDTH),
        RenameOp::Trim { chars, mode } => {
            let from = match mode {
                TrimMode::Both => "both ends",
                TrimMode::Start => "the start",
                TrimMode::End => "the end",
            };
            if chars.is_empty() {
                format!("spaces from {from}")
            } else {
                framed_detail("\"", chars, &format!("\" from {from}"), OP_DETAIL_WIDTH)
            }
        }
        RenameOp::Extension(ext_op) => match ext_op {
            ExtensionOp::Replace(e) => framed_detail("→ .", e, "", OP_DETAIL_WIDTH),
            ExtensionOp::Add(e) => framed_detail("+ .", e, "", OP_DETAIL_WIDTH),
            ExtensionOp::Remove => "remove".into(),
            ExtensionOp::Lower => "lowercase".into(),
            ExtensionOp::Upper => "UPPERCASE".into(),
        },
        RenameOp::Template { template } => framed_detail("", template, "", OP_DETAIL_WIDTH),
    }
}

// ============================================================================
// The pointer, the menu and the rule editor's keyboard
// ============================================================================

impl RenamerApp {
    /// What is under `(x, y)` in the frame last shown: for the pointer's
    /// movement and the wheel, which come in floods. A press draws afresh.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame(self.last_width, self.last_height).hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    /// Route a pointer event.
    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        // An open menu takes every press, and consumes it either way: a press
        // that dismisses a menu must not also land on what was behind it.
        if let Some(menu) = self.rule_menu.as_mut() {
            match event.kind {
                MouseEventKind::Press(_) => {
                    let chosen = menu.handle_click(event.x, event.y);
                    self.rule_menu = None;
                    if let Some(id) = chosen {
                        self.add_rule_of_kind(id);
                    }
                    return EventResult::Consumed;
                }
                MouseEventKind::Move => {
                    menu.handle_mouse_move(event.x, event.y);
                    return EventResult::Consumed;
                }
                _ => return EventResult::Ignored,
            }
        }
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let Some(target) = self
                    .frame(self.last_width, self.last_height)
                    .hit_test(event.x, event.y)
                else {
                    return EventResult::Ignored;
                };
                self.activate(target)
            }
            // A double press on a file ticks or unticks it -- the row's own
            // press already moved the cursor there. (Nothing produces this
            // event until `oswindow` synthesises it; see known-issues.md.)
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                match self
                    .frame(self.last_width, self.last_height)
                    .hit_test(event.x, event.y)
                {
                    Some(Target::File(i)) => {
                        self.selected_file = i;
                        self.toggle_selected_file()
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
                self.scroll(over, event.x, event.y, dy)
            }
            _ => EventResult::Ignored,
        }
    }

    /// Turn the wheel over `over`, at `(x, y)`, by `dy` notches.
    fn scroll(&mut self, over: Option<Target>, x: f32, y: f32, dy: f32) -> EventResult {
        let l = Layout::of(self.last_width, self.last_height);
        match over {
            Some(Target::Files | Target::File(_) | Target::Check(_)) => {
                let rows = self.files_wheel.rows(dy);
                let last_top = self.filtered_files().len().saturating_sub(l.file_rows());
                let before = self.file_scroll;
                self.file_scroll = self.file_scroll.saturating_add_signed(rows).min(last_top);
                if self.file_scroll == before {
                    EventResult::Ignored
                } else {
                    EventResult::Consumed
                }
            }
            // In the panel, not merely on one of its targets: Add Rule is on
            // the toolbar too, and the wheel over the toolbar scrolls nothing.
            Some(target)
                if self.sidebar_panel == SidebarPanel::Operations
                    && target.is_in_ops()
                    && l.panel.contains(x, y) =>
            {
                let limit = (self.ops_content_height(l.panel) - l.panel.h).max(0.0);
                let before = self.ops_scroll;
                self.ops_scroll = (self.ops_scroll + wheel::pixels(dy, 24.0)).clamp(0.0, limit);
                if (self.ops_scroll - before).abs() > f32::EPSILON {
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
        // A press anywhere but in the box being typed into takes the
        // keyboard back from it; a press on another box moves it there.
        if !matches!(
            target,
            Target::Field(_) | Target::SearchBox | Target::ExtensionBox
        ) {
            self.set_focus(Focus::List);
        }
        match target {
            Target::OpenFolder => {
                self.open_folder();
                EventResult::Consumed
            }
            Target::AddRule => {
                self.open_rule_menu();
                EventResult::Consumed
            }
            Target::Rename => self.rename_selected(),
            Target::Undo => {
                if self.undo_stack.is_empty() {
                    self.status_message = "Nothing to undo".to_string();
                } else {
                    self.undo();
                }
                EventResult::Consumed
            }
            Target::Redo => {
                if self.redo_stack.is_empty() {
                    self.status_message = "Nothing to redo".to_string();
                } else {
                    self.redo();
                }
                EventResult::Consumed
            }
            Target::ClearRules => {
                self.clear_operations();
                self.selected_op = 0;
                EventResult::Consumed
            }
            Target::Panel(panel) => {
                self.sidebar_panel = panel;
                EventResult::Consumed
            }
            Target::Rule(i) => {
                self.selected_op = i;
                EventResult::Consumed
            }
            Target::RuleUp(i) => {
                self.selected_op = i;
                self.move_op(-1);
                EventResult::Consumed
            }
            Target::RuleDown(i) => {
                self.selected_op = i;
                self.move_op(1);
                EventResult::Consumed
            }
            Target::RuleRemove(i) => {
                self.remove_operation(i);
                self.selected_op = self
                    .selected_op
                    .min(self.operations.len().saturating_sub(1));
                EventResult::Consumed
            }
            Target::Field(slot) => {
                self.set_focus(Focus::Field(slot));
                EventResult::Consumed
            }
            Target::Choice(pick) => {
                if let Some(op) = self.operations.get_mut(self.selected_op) {
                    apply_pick(op, pick);
                }
                self.apply_operations();
                EventResult::Consumed
            }
            Target::Switch(flip) => {
                if let Some(op) = self.operations.get_mut(self.selected_op) {
                    apply_flip(op, flip);
                }
                self.apply_operations();
                EventResult::Consumed
            }
            Target::SearchBox => {
                self.set_focus(Focus::Search);
                EventResult::Consumed
            }
            Target::ExtensionBox => {
                self.set_focus(Focus::Extension);
                EventResult::Consumed
            }
            Target::ConflictsOnly => {
                self.filter_conflicts = !self.filter_conflicts;
                self.file_scroll = 0;
                EventResult::Consumed
            }
            Target::File(i) => {
                self.selected_file = i;
                EventResult::Consumed
            }
            Target::Check(i) => {
                self.selected_file = i;
                self.toggle_selected_file()
            }
            Target::HelpCard => {
                self.show_help = false;
                EventResult::Consumed
            }
            Target::Ops | Target::Files => EventResult::Consumed,
        }
    }

    /// Rename the ticked files, or say why not.
    fn rename_selected(&mut self) -> EventResult {
        if self.files.iter().all(|f| !f.selected) {
            self.status_message = String::from("Nothing selected to rename");
            return EventResult::Consumed;
        }
        self.execute_rename();
        EventResult::Consumed
    }

    /// Put the Add Rule menu up, under its toolbar button.
    fn open_rule_menu(&mut self) {
        let items: Vec<MenuItem> = RULE_KINDS
            .iter()
            .enumerate()
            .map(|(i, label)| MenuItem::Action {
                id: i as u64,
                label: (*label).to_string(),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: None,
            })
            .collect();
        let at = self
            .frame(self.last_width, self.last_height)
            .rect_of(|t| *t == Target::AddRule)
            .unwrap_or(Rect::new(220.0, 6.0, 0.0, BUTTON_HEIGHT));
        let mut menu = ContextMenu::new(items);
        menu.show(at.x, at.bottom(), (self.last_width, self.last_height));
        self.rule_menu = Some(menu);
    }

    /// Add a rule of kind `id` (an index into [`RULE_KINDS`]), select it, and
    /// put the keyboard in its first box if it has one.
    fn add_rule_of_kind(&mut self, id: u64) {
        let Some(op) = usize::try_from(id).ok().and_then(new_rule) else {
            return;
        };
        let first = rule_slots(&op).first().copied();
        if self.add_op(op) == EventResult::Ignored {
            self.status_message = format!("The pipeline is full: {MAX_OPERATIONS} rules");
            return;
        }
        self.sidebar_panel = SidebarPanel::Operations;
        if let Some(slot) = first {
            self.set_focus(Focus::Field(slot));
        }
    }

    /// Move the keyboard to `focus`, loading the box it lands in.
    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        self.draft_invalid = false;
        let text = match focus {
            Focus::List => return,
            Focus::Field(slot) => self
                .operations
                .get(self.selected_op)
                .map_or_else(String::new, |op| field_text(op, slot)),
            Focus::Search => self.search_text.clone(),
            Focus::Extension => self.filter_extension.clone(),
        };
        self.draft = TextInput::new();
        self.draft.set_text(&text);
        self.draft.select_all();
    }

    /// A key while a box has the keyboard.
    ///
    /// Typing goes into the box and takes effect as it is typed -- the preview
    /// is live, so a rule is seen doing what it will do while it is being
    /// written. Tab and Shift+Tab move between the rule's boxes; Enter and Esc
    /// give the keyboard back to the list.
    fn handle_key_in_box(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape | Key::Enter => {
                self.set_focus(Focus::List);
                return EventResult::Consumed;
            }
            Key::Tab => {
                if let Focus::Field(slot) = self.focus
                    && let Some(op) = self.operations.get(self.selected_op)
                {
                    let slots = rule_slots(op);
                    let at = slots.iter().position(|s| *s == slot).unwrap_or(0);
                    let count = slots.len().max(1);
                    let next = if key.modifiers.shift {
                        at.checked_sub(1).unwrap_or(count.saturating_sub(1))
                    } else if at.saturating_add(1) >= count {
                        0
                    } else {
                        at.saturating_add(1)
                    };
                    if let Some(next) = slots.get(next).copied() {
                        self.set_focus(Focus::Field(next));
                    }
                }
                return EventResult::Consumed;
            }
            _ => {}
        }
        if !edit_text(&mut self.draft, key) {
            // A key a box has no use for changed nothing. It stops here all
            // the same -- `handle_key` came into this method before its
            // shortcuts -- so no key typed into a box can add a rule.
            return EventResult::Ignored;
        }
        let typed = self.draft.text().to_string();
        match self.focus {
            Focus::Field(slot) => {
                let taken = self
                    .operations
                    .get_mut(self.selected_op)
                    .is_some_and(|op| set_field(op, slot, &typed));
                self.draft_invalid = !taken;
                if taken {
                    self.apply_operations();
                }
            }
            Focus::Search => {
                self.search_text = typed;
                self.file_scroll = 0;
            }
            Focus::Extension => {
                self.filter_extension = typed.trim().trim_start_matches('.').to_string();
                self.file_scroll = 0;
            }
            Focus::List => {}
        }
        EventResult::Consumed
    }

    /// Scroll the file list so the cursor is on screen.
    fn keep_file_visible(&mut self) {
        let rows = Layout::of(self.last_width, self.last_height)
            .file_rows()
            .max(1);
        let Some(pos) = self
            .filtered_files()
            .iter()
            .position(|(i, _)| *i == self.selected_file)
        else {
            return;
        };
        if pos < self.file_scroll {
            self.file_scroll = pos;
        } else if pos >= self.file_scroll.saturating_add(rows) {
            self.file_scroll = pos.saturating_sub(rows.saturating_sub(1));
        }
    }
}

impl Target {
    /// Whether this target is part of the operations panel, which the wheel
    /// scrolls as one.
    fn is_in_ops(self) -> bool {
        matches!(
            self,
            Self::Ops
                | Self::AddRule
                | Self::Rule(_)
                | Self::RuleUp(_)
                | Self::RuleDown(_)
                | Self::RuleRemove(_)
                | Self::Field(_)
                | Self::Choice(_)
                | Self::Switch(_)
        )
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

impl App for RenamerApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        let selected = self.files.iter().filter(|f| f.selected).count();
        if selected == 0 {
            "Bulk Rename".to_owned()
        } else {
            format!("Bulk Rename — {selected} selected")
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH_PX, WINDOW_HEIGHT_PX)
    }

    /// No clock: nothing here ages. A rename's time is read when it happens.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.last_width = width;
        self.last_height = height;
        let frame = self.frame(width, height);
        // Kept for the pointer's movement and the wheel; see `target_at`.
        self.last_hits = frame.hits().to_vec();
        let mut commands = frame.into_tree().commands;
        // The picker last, so it draws over the file list rather than under
        // it. A dialog that takes input and paints nothing is invisible and
        // still swallowing keys.
        commands.extend(self.picker.render(&self.palette, width, height));
        RenderTree { commands }
    }
}

/// The size the window asks to open at.
const WINDOW_WIDTH_PX: u32 = 1100;
const WINDOW_HEIGHT_PX: u32 = 750;

fn main() -> ExitCode {
    let mut app = RenamerApp::new();
    app.status_message = "Ctrl+O to choose a folder".to_string();
    app::launch("renamer", &mut app)
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

    // ------------------------------------------------------------------
    // Events, and the rules that are now addable
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;
    use oswindow::app::App as _;

    fn seeded() -> RenamerApp {
        let mut app = RenamerApp::new();
        app.seed_sample_files();
        app
    }

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    /// **Every key the shortcut list advertises is one this program answers.**
    ///
    /// A list on screen and the handler behind it are two copies of one fact,
    /// and they drift: `apps/rssreader` shipped an overlay of twenty-one
    /// shortcuts of which about four worked. The label is read by
    /// `guitk::shortcut` rather than matched against a table written beside it
    /// here -- that table would be a third copy, drifting from both.
    ///
    /// The property is "some reachable state answers this key", not "this key
    /// is taken right now". `Delete` with no rule in the pipeline, `Enter`
    /// with no file selected and `Ctrl+Z` with nothing done all decline on
    /// purpose, and declining from its own arm is answering.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let event = Event::Key(stroke.clone());
                let answered = states()
                    .iter_mut()
                    .any(|app| app.handle_event(&event) == EventResult::Consumed);
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// Lists chosen so that between them every advertised key has work to do.
    fn states() -> Vec<RenamerApp> {
        let plain = seeded();

        // A file selected and a rule in the pipeline: what `Enter`, `Delete`
        // and the two reordering keys each need before they will act. The
        // `Down` first is for `Up`, which declines at the top of the list --
        // every other state here stands on the first file.
        let mut loaded = seeded();
        loaded.handle_event(&press(Key::Down));
        loaded.handle_event(&press(Key::Space));
        loaded.handle_event(&press(Key::L));
        loaded.handle_event(&press(Key::U));

        // ...and the selected rule lifted off the bottom of the pipeline, so
        // `PageDown` has somewhere to put it. Adding a rule selects it, and
        // the newest is the last, so no state has a rule with room both above
        // and below it.
        let mut reordered = seeded();
        for k in [Key::Space, Key::L, Key::U, Key::PageUp] {
            reordered.handle_event(&press(k));
        }

        // ...and that rename carried out, so undo has something to undo.
        //
        // `app_with`, not `seeded`: `execute_rename` renames files on the
        // filesystem and records nothing unless one actually moves, so a list
        // of names with no directory under it leaves the undo stack empty --
        // the trap `app_with`'s own doc comment was written about. `N` rather
        // than `L`, too, because lower-casing a name that is already lower
        // case changes nothing, and numbering always changes something.
        // No `Ctrl+A` here: files arrive *already* selected, so select-all
        // sees that and does its opposite, which is how the first version of
        // this deselected everything and renamed nothing.
        let renamed_keys = [press(Key::N), press(Key::Enter)];
        let mut renamed = app_with(&["one.txt", "two.txt"]);
        for e in &renamed_keys {
            renamed.handle_event(e);
        }
        assert!(
            !renamed.undo_stack.is_empty(),
            "nothing was renamed, so the undo state this test needs was never reached"
        );

        // ...and undone, so redo does.
        let mut undone = app_with(&["three.txt", "four.txt"]);
        for e in &renamed_keys {
            undone.handle_event(e);
        }
        undone.handle_event(&press_ctrl(Key::Z));

        // A rule with boxes selected, for F2, which declines on a rule that
        // has nothing to type into.
        let mut editable = seeded();
        editable.add_op(RenameOp::Insert {
            text: String::new(),
            position: InsertPosition::Start,
        });

        vec![plain, loaded, reordered, renamed, undone, editable]
    }

    /// **The shortcut list reaches the window.**
    ///
    /// The guard above reads the list against the handler; this reads it
    /// against the screen. `apps/netscan`'s `wol_note` was written by the
    /// model and drawn by nothing for three commits with every model-level
    /// test passing.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = seeded();
        assert!(
            !drawn_text(&app).contains("? closes this"),
            "the list is up before anybody asked for it"
        );

        app.handle_event(&press(Key::F1));
        let shown = drawn_text(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        app.handle_event(&press(Key::Escape));
        assert!(
            !drawn_text(&app).contains("? closes this"),
            "Escape did not close it"
        );
    }

    /// A shifted slash asks for help; a plain one still opens the search box.
    #[test]
    fn the_help_key_did_not_take_the_search_key_with_it() {
        let mut app = seeded();
        app.handle_event(&press(Key::Slash));
        assert!(
            app.focus == Focus::Search,
            "`/` no longer opens the search box"
        );
        assert!(
            !drawn_text(&app).contains("? closes this"),
            "`/` opened the shortcut list"
        );
    }

    /// Every string the window is drawing, joined.
    fn drawn_text(app: &RenamerApp) -> String {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn press_ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        })
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        })
    }

    #[test]
    fn the_window_opens_with_something_to_rename() {
        // `add_file` had no caller outside the tests, so the window would have
        // opened empty with no key that could put anything in it.
        let app = seeded();
        assert!(app.files.len() >= 4, "files: {}", app.files.len());
    }

    #[test]
    fn a_rule_can_be_added_and_it_changes_the_preview() {
        // `add_operation` and every rule variant were written and tested, and
        // the pipeline could not be given a single rule — so every preview
        // showed the name unchanged and the program did nothing.
        let mut app = seeded();
        assert!(app.operations.is_empty());
        let before: Vec<String> = app.files.iter().map(|f| f.new_name.clone()).collect();
        assert_eq!(app.handle_event(&press(Key::L)), EventResult::Consumed);
        assert_eq!(app.operations.len(), 1, "L should add a lowercase rule");
        let after: Vec<String> = app.files.iter().map(|f| f.new_name.clone()).collect();
        assert_ne!(before, after, "adding a rule changed no preview");
        // A case rule works on the *stem* and leaves the extension alone —
        // which is why lowercasing an extension is a rule of its own. Adding
        // both is what makes a whole name lower case.
        let stem = |n: &str| {
            n.rsplit_once('.')
                .map_or(n.to_owned(), |(s, _)| s.to_owned())
        };
        assert!(
            after.iter().all(|n| stem(n) == stem(n).to_lowercase()),
            "a lowercase rule left an upper-case stem: {after:?}"
        );
        assert!(
            after.iter().any(|n| n.ends_with(".JPG")),
            "the extension should have been left alone: {after:?}"
        );
        app.handle_event(&press(Key::E));
        let both: Vec<String> = app.files.iter().map(|f| f.new_name.clone()).collect();
        assert!(
            both.iter().all(|n| n == &n.to_lowercase()),
            "stem and extension rules together should lower the whole name: {both:?}"
        );
    }

    #[test]
    fn each_rule_key_adds_its_own_rule() {
        let mut app = seeded();
        for (k, n) in [
            (Key::L, 1),
            (Key::U, 2),
            (Key::T, 3),
            (Key::S, 4),
            (Key::K, 5),
            (Key::W, 6),
            (Key::N, 7),
            (Key::E, 8),
            (Key::X, 9),
        ] {
            assert_eq!(app.handle_event(&press(k)), EventResult::Consumed);
            assert_eq!(app.operations.len(), n, "{k:?} did not add a rule");
        }
    }

    #[test]
    fn ctrl_delete_clears_the_files_rather_than_removing_a_rule() {
        // A guard narrows only the arm it is on, so the unguarded `Key::Delete`
        // listed first swallowed this case entirely: Ctrl+Delete removed an
        // operation instead. The compiler said so, as an unreachable pattern.
        let mut app = seeded();
        app.handle_event(&press(Key::L));
        assert_eq!(app.operations.len(), 1);
        assert_eq!(
            app.handle_event(&press_ctrl(Key::Delete)),
            EventResult::Consumed
        );
        assert!(app.files.is_empty(), "Ctrl+Delete should clear the files");
        assert_eq!(app.operations.len(), 1, "and leave the rules alone");
    }

    #[test]
    fn ctrl_backspace_clears_the_rules_and_restores_the_original_names() {
        let mut app = seeded();
        let original: Vec<String> = app.files.iter().map(|f| f.new_name.clone()).collect();
        app.handle_event(&press(Key::U));
        assert_ne!(
            app.files
                .iter()
                .map(|f| f.new_name.clone())
                .collect::<Vec<_>>(),
            original
        );
        assert_eq!(
            app.handle_event(&press_ctrl(Key::Backspace)),
            EventResult::Consumed
        );
        assert!(app.operations.is_empty());
        assert_eq!(
            app.files
                .iter()
                .map(|f| f.new_name.clone())
                .collect::<Vec<_>>(),
            original,
            "clearing the rules should restore the previews"
        );
    }

    #[test]
    fn clearing_an_empty_pipeline_is_not_a_redraw() {
        let mut app = seeded();
        assert_eq!(
            app.handle_event(&press_ctrl(Key::Backspace)),
            EventResult::Ignored
        );
    }

    #[test]
    fn typing_a_search_does_not_add_rules() {
        // "l", "u", "t", "s", "k", "w", "n", "e" and "x" are all rule keys
        // outside the search box.
        let mut app = seeded();
        assert_eq!(app.handle_event(&press(Key::Slash)), EventResult::Consumed);
        assert!(app.focus == Focus::Search);
        for c in "lux".chars() {
            app.handle_event(&typed(c));
        }
        assert_eq!(app.search_text, "lux");
        assert!(
            app.operations.is_empty(),
            "typing a search added {} rule(s)",
            app.operations.len()
        );
    }

    #[test]
    fn space_ticks_the_file_under_the_cursor() {
        let mut app = seeded();
        app.select_all(false);
        app.selected_file = 1;
        assert_eq!(app.handle_event(&press(Key::Space)), EventResult::Consumed);
        assert!(
            app.files.get(1).is_some_and(|f| f.selected),
            "Space did not tick the file"
        );
        assert_eq!(
            app.files.iter().filter(|f| f.selected).count(),
            1,
            "Space ticked more than one"
        );
    }

    #[test]
    fn renaming_with_nothing_ticked_says_so_rather_than_doing_it() {
        let mut app = seeded();
        app.select_all(false);
        app.handle_event(&press(Key::L));
        let before: Vec<String> = app.files.iter().map(|f| f.original_name.clone()).collect();
        app.handle_event(&press(Key::Enter));
        assert_eq!(
            app.files
                .iter()
                .map(|f| f.original_name.clone())
                .collect::<Vec<_>>(),
            before,
            "a rename ran with nothing selected"
        );
        assert!(
            app.status_message.to_lowercase().contains("nothing"),
            "the status bar should say why: {:?}",
            app.status_message
        );
    }

    #[test]
    fn undo_with_nothing_to_undo_is_not_a_redraw() {
        let mut app = seeded();
        assert_eq!(app.handle_event(&press_ctrl(Key::Z)), EventResult::Ignored);
    }

    #[test]
    fn the_arrows_walk_the_file_list_and_stop_at_the_ends() {
        let mut app = seeded();
        app.selected_file = 0;
        assert_eq!(app.handle_event(&press(Key::Up)), EventResult::Ignored);
        for i in 1..app.files.len() {
            assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Consumed);
            assert_eq!(app.selected_file, i);
        }
        assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Ignored);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = seeded();
        let release = Event::Key(KeyEvent {
            key: Key::L,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert!(app.operations.is_empty());
    }

    #[test]
    fn the_title_counts_the_ticked_files() {
        let mut app = seeded();
        app.select_all(false);
        assert_eq!(app.title(), "Bulk Rename");
        app.select_all(true);
        let title = app.title();
        assert!(
            title.contains(&app.files.len().to_string()),
            "title {title:?} omits the count"
        );
    }

    #[test]
    fn rendering_draws_something_at_an_awkward_size() {
        let mut app = seeded();
        for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
            assert!(
                !app.render(w, h).commands.is_empty(),
                "drew nothing at {w}x{h}"
            );
        }
    }
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

    /// 2026-05-18 14:30:00 UTC, as a file's modification time.
    const MAY_18: u64 = 1_779_114_600_000;

    /// A file modified at `modified_ms`, first in the batch.
    fn at(modified_ms: u64) -> RuleContext {
        RuleContext {
            index: 0,
            modified_ms,
        }
    }

    #[test]
    fn test_date_stamp_ymd() {
        let op = RenameOp::DateStamp {
            format: DateFormat::YmdHyphen,
            position: InsertPosition::Start,
            separator: "_".into(),
        };
        let result = RenameEngine::apply_in(&op, "photo.jpg", at(MAY_18));
        assert!(result.starts_with("2026-05-18_"), "{result}");
        assert!(result.ends_with(".jpg"));
    }

    #[test]
    fn test_date_stamp_compact() {
        let op = RenameOp::DateStamp {
            format: DateFormat::YmdCompact,
            position: InsertPosition::End,
            separator: "_".into(),
        };
        let result = RenameEngine::apply_in(&op, "photo.jpg", at(MAY_18));
        assert!(result.contains("20260518"), "{result}");
    }

    /// **The stamp is each file's own date.** It was the constant 2026-05-18
    /// for every file ever renamed.
    #[test]
    fn a_date_stamp_is_the_files_own_date() {
        let op = RenameOp::DateStamp {
            format: DateFormat::Timestamp,
            position: InsertPosition::End,
            separator: "_".into(),
        };
        // 1999-12-31 23:59:58 UTC.
        let old = RenameEngine::apply_in(&op, "a.txt", at(946_684_798_000));
        assert_eq!(old, "a_19991231_235958.txt");
        let new = RenameEngine::apply_in(&op, "a.txt", at(MAY_18));
        assert_eq!(new, "a_20260518_143000.txt");
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
        app.add_file("test.txt", 1024, 0);
        assert_eq!(app.files.len(), 1);
        assert_eq!(app.files[0].original_name, "test.txt");
    }

    #[test]
    fn test_app_add_operation() {
        let mut app = RenamerApp::new();
        app.add_file("old.txt", 0, 0);
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
        app.add_file("file.txt", 0, 0);
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
        app.add_file("a.txt", 0, 0);
        app.add_file("b.txt", 0, 0);
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
        app.add_file("test.txt", 0, 0);
        app.add_operation(RenameOp::ChangeCase(CaseMode::Upper));
        assert_eq!(app.files[0].new_name, "TEST.txt");
        app.remove_operation(0);
        assert_eq!(app.files[0].new_name, "test.txt");
    }

    #[test]
    fn test_app_select_all() {
        let mut app = RenamerApp::new();
        app.add_file("a.txt", 0, 0);
        app.add_file("b.txt", 0, 0);
        app.select_all(false);
        assert!(app.files.iter().all(|f| !f.selected));
        app.select_all(true);
        assert!(app.files.iter().all(|f| f.selected));
    }

    #[test]
    fn test_app_rename_count() {
        let mut app = RenamerApp::new();
        app.add_file("a.txt", 0, 0);
        app.add_file("b.txt", 0, 0);
        assert_eq!(app.rename_count(), 0);
        app.add_operation(RenameOp::ChangeCase(CaseMode::Upper));
        assert_eq!(app.rename_count(), 2);
    }

    #[test]
    fn test_app_clear_files() {
        let mut app = RenamerApp::new();
        app.add_file("a.txt", 0, 0);
        app.clear_files();
        assert!(app.files.is_empty());
    }

    #[test]
    fn test_app_clear_operations() {
        let mut app = RenamerApp::new();
        app.add_file("a.txt", 0, 0);
        app.add_operation(RenameOp::ChangeCase(CaseMode::Upper));
        app.clear_operations();
        assert!(app.operations.is_empty());
        assert_eq!(app.files[0].new_name, "a.txt");
    }

    #[test]
    fn test_app_render_nonempty() {
        let app = RenamerApp::new();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    // --- Operation-list detail lines ---

    /// The operation-list detail lines drawn for `ops`.
    fn op_detail_lines(ops: Vec<RenameOp>) -> Vec<String> {
        let mut app = RenamerApp::new();
        app.operations = ops;
        app.render_commands()
            .into_iter()
            .filter_map(|c| match c {
                // At the detail line's own x, as well as its size: the rule
                // editor below the list draws its switches' ticks in the same
                // small size.
                RenderCommand::Text {
                    x, text, font_size, ..
                } if (font_size - OP_DETAIL_SIZE).abs() < f32::EPSILON
                    && (x - 18.0).abs() < 0.01 =>
                {
                    Some(text)
                }
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
        let mut app = app_with(&["old.txt"]);
        app.add_operation(RenameOp::FindReplace {
            find: "old".into(),
            replace: "new".into(),
            case_sensitive: true,
            replace_all: false,
        });
        app.execute_rename();
        assert_eq!(app.undo_stack.len(), 1);
        assert_eq!(app.history.len(), 1);
        // The undo stack used to be pushed whether or not anything happened,
        // which is what made an undo of nothing report "Undid rename of 1
        // files". It is pushed now only because a file really moved:
        assert_eq!(on_disk(&app), vec!["new.txt"]);
    }

    /// With no folder open, Rename does nothing and says so.
    ///
    /// The stacks must stay empty: an undo entry for a rename that did not
    /// happen is an undo that reports success and restores nothing.
    #[test]
    fn renaming_with_no_folder_open_changes_nothing() {
        let mut app = app_listing(&["old.txt"]);
        preview(&mut app, &["new.txt"]);
        app.execute_rename();

        assert!(
            app.undo_stack.is_empty(),
            "queued an undo for a rename that did not happen"
        );
        assert!(app.history.is_empty());
        assert_eq!(
            names_of(&app),
            vec!["old.txt"],
            "the list moved without the files"
        );
        assert!(
            !app.status_message.contains("Renamed 1"),
            "claimed a rename with no folder open: {}",
            app.status_message
        );
    }

    #[test]
    fn test_move_operation() {
        let mut app = RenamerApp::new();
        app.add_file("test.txt", 0, 0);
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
        app.add_file("a.txt", 0, 0);
        app.add_file("b.jpg", 0, 0);
        app.add_file("c.txt", 0, 0);

        app.filter_extension = "txt".into();
        let filtered = app.filtered_files();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_search_filter() {
        let mut app = RenamerApp::new();
        app.add_file("alpha.txt", 0, 0);
        app.add_file("beta.txt", 0, 0);
        app.add_file("gamma.txt", 0, 0);

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
        let ext = |name: &str| FileEntry::new(name, 0, 0).extension;
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
        app.add_file(".bashrc", 0, 0);
        app.add_file("backup.bashrc", 0, 0);
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
            "A Very Long Show Name - Season 01 - Episode 07 - The One With The Long Title.mkv",
            1_234_567,
            0,
        );
        app.add_file(
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
    /// The file list alone -- filter bar, headings and rows -- as the window
    /// draws it at the size it opens at.
    fn file_list_commands(app: &RenamerApp) -> Vec<RenderCommand> {
        let l = Layout::of(app.last_width, app.last_height);
        let mut f = Frame::new(app.last_width, app.last_height);
        app.draw_file_list(&mut f, &l);
        f.commands().to_vec()
    }

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
        // Render the list directly rather than the whole app: a full render
        // puts sidebar and toolbar text at x values that can coincide with a
        // column's left edge, and the assertion would then fail on chrome that
        // was never part of this table.
        let cmds = file_list_commands(&app);
        // 6 header labels + 2 rows x 5 cells (name, arrow, new name, size,
        // status).
        assert_file_cells_fit(&cmds, 16);
    }

    #[test]
    fn an_overlong_rename_preview_keeps_the_end_of_both_names() {
        let app = app_with_overlong_names();
        let cmds = file_list_commands(&app);

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
        app.add_file("notes.txt", 12, 0);
        let cmds = file_list_commands(&app);
        let originals = cells_in_column(&cmds, COL_ORIGINAL);
        assert!(
            originals.iter().any(|t| t == "notes.txt"),
            "a name that fits must not be marked as cut, got {originals:?}"
        );
    }

    #[test]
    fn the_header_and_the_rows_agree_on_where_a_column_starts() {
        let app = app_with_overlong_names();
        let cmds = file_list_commands(&app);
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
            RenameEngine::apply_in(&op, "\u{65e5}\u{672c}\u{8a9e}.txt", at(MAY_18)),
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
    fn ctrl(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: guitk::event::Modifiers::ctrl(),
            text: String::new(),
        })
    }

    /// The rename reaches the filesystem.
    ///
    /// The status line said "Renamed {count} files" while `apply_plan` edited
    /// a `Vec<FileEntry>`; the crate contained no `std::fs` at all. **A person
    /// told a batch rename succeeded may then look for their files under the
    /// new names, or delete the copy they kept.**
    #[test]
    fn a_rename_moves_the_actual_files() {
        let mut app = app_with(&["a.txt", "b.txt"]);
        preview(&mut app, &["x.txt", "y.txt"]);
        app.execute_rename();

        assert_eq!(on_disk(&app), vec!["x.txt", "y.txt"]);
        assert!(
            app.status_message.starts_with("Renamed 2"),
            "said: {}",
            app.status_message
        );
    }

    /// A rename that cannot happen is reported, not counted.
    ///
    /// The destination here is an existing *directory*, which `fs::rename`
    /// refuses. The batch continues -- the plan's order is what makes it safe,
    /// so abandoning it midway creates the collision it was built to avoid --
    /// and the file that did move is counted separately from the one that did
    /// not.
    #[test]
    fn a_failed_rename_is_reported_and_does_not_stop_the_batch() {
        let mut app = app_with(&["a.txt", "b.txt"]);
        let dir = app.folder.clone().expect("a folder");
        std::fs::create_dir(dir.join("x.txt")).expect("a directory in the way");

        preview(&mut app, &["x.txt", "y.txt"]);
        app.execute_rename();

        assert!(
            dir.join("y.txt").is_file(),
            "the second rename was abandoned because the first failed"
        );
        assert!(
            dir.join("a.txt").is_file(),
            "the file that could not move should still be under its own name"
        );
        assert!(
            app.status_message.contains("1 file(s)") && app.status_message.contains("failed"),
            "should count both halves, said: {}",
            app.status_message
        );
    }

    /// Undo puts the files back on disk, not just in the list.
    ///
    /// This is the more dangerous half of the same defect: an undo that
    /// reports success and restores nothing leaves the user believing the
    /// original names are recoverable when they are gone.
    #[test]
    fn undo_puts_the_files_back() {
        let mut app = app_with(&["1.jpg", "2.jpg", "3.jpg"]);
        preview(&mut app, &["2.jpg", "3.jpg", "4.jpg"]);
        app.execute_rename();
        assert_eq!(on_disk(&app), vec!["2.jpg", "3.jpg", "4.jpg"]);

        app.undo();
        assert_eq!(on_disk(&app), vec!["1.jpg", "2.jpg", "3.jpg"]);
        assert!(
            !app.status_message.contains("Renamed"),
            "an undo is not a rename, said: {}",
            app.status_message
        );

        app.redo();
        assert_eq!(on_disk(&app), vec!["2.jpg", "3.jpg", "4.jpg"]);
    }

    /// An open picker takes the keyboard, and the window behind it does not.
    ///
    /// The other door test pins that the key *opens* the picker -- but the key
    /// handler is what opens it, so cutting the picker's event routing
    /// entirely leaves that assertion true. This is the half routing actually
    /// decides: with a dialog up, a keystroke belongs to the dialog, and a
    /// window that scrolls underneath one is a modal that is not modal.
    ///
    /// Found by `scripts/find-unpinned-picker-routing.py`, which cuts the
    /// routing and reports whose tests notice. Sixteen of twenty did not.
    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_file_list() {
        let mut app = RenamerApp::new();
        app.add_file("one.txt", 10, 0);
        app.add_file("two.txt", 10, 0);
        app.selected_file = 0;

        app.handle_event(&ctrl(Key::O));
        assert!(app.picker.is_open(), "control: the picker must be up");

        app.handle_event(&press(Key::Down));
        assert_eq!(
            app.selected_file, 0,
            "Down at the open dialog moved the selection behind it"
        );
    }

    /// Ctrl+O asks for a folder, and choosing one lists it.
    #[test]
    fn ctrl_o_opens_the_folder_picker() {
        let dir = scratch("picked");
        std::fs::write(dir.join("one.txt"), b"1").expect("fixture");
        std::fs::write(dir.join("two.txt"), b"2").expect("fixture");

        let mut app = RenamerApp::new();
        assert!(
            app.files.is_empty(),
            "the window opens with nothing invented in it"
        );

        app.handle_event(&ctrl(Key::O));
        assert!(app.picker.is_open(), "Ctrl+O should ask for a folder");

        let said = app.load_folder(&dir);
        assert_eq!(names_of(&app), vec!["one.txt", "two.txt"]);
        assert!(said.contains("2 file(s)"), "said: {said}");
    }

    /// The open picker is drawn.
    #[test]
    fn the_open_picker_is_drawn() {
        let mut app = RenamerApp::new();
        let closed = app.render(1100.0, 750.0).commands.len();
        app.handle_event(&ctrl(Key::O));
        let own = app
            .render(1100.0, 750.0)
            .commands
            .len()
            .saturating_sub(closed);
        assert!(
            own > 0,
            "the open picker contributed {own} commands; it is not being drawn"
        );
    }

    /// Folders in the listing are not offered as files to rename.
    #[test]
    fn a_subfolder_is_not_listed_as_a_file() {
        let dir = scratch("subfolder");
        std::fs::write(dir.join("file.txt"), b"x").expect("fixture");
        std::fs::create_dir(dir.join("subdir")).expect("fixture");

        let mut app = RenamerApp::new();
        let said = app.load_folder(&dir);
        assert_eq!(names_of(&app), vec!["file.txt"]);
        assert!(said.contains("1 folder(s) skipped"), "said: {said}");
    }

    /// A name that is not text is listed, and cannot be renamed.
    ///
    /// Every rule reads text and writes text, so a name that does not survive
    /// the trip through `String` has no text form a rule could transform.
    /// Renaming it from the lossy form would write a name **nobody asked for**
    /// -- `to_string_lossy` replaces the bad bytes with U+FFFD, and that is a
    /// different name, so the file would land somewhere the user never chose
    /// and the original would be gone.
    ///
    /// The decision is tested rather than a file with such a name, because the
    /// host filesystem here is NTFS, which will not store one: the fixture
    /// that would demonstrate it cannot be created on the machine the tests
    /// run on. What can be checked is that the decision is made from the raw
    /// name and not from its text form.
    #[test]
    fn a_name_that_is_not_text_is_listed_but_not_renameable() {
        #[cfg(windows)]
        let raw: OsString = {
            use std::os::windows::ffi::OsStringExt;
            // An unpaired surrogate: valid UTF-16, no UTF-8 encoding.
            OsString::from_wide(&[0x0066, 0xD800, 0x0074])
        };
        #[cfg(not(windows))]
        let raw: OsString = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![b'f', 0xFF, b't'])
        };
        assert!(
            raw.to_str().is_none(),
            "control: the fixture must be unrepresentable as a `String`, or this asserts nothing"
        );

        let entry = FileEntry::from_os_name(&raw, 10, 0);
        assert!(
            !entry.renameable,
            "a name that is not text was marked renameable"
        );
        assert!(
            !entry.selected,
            "an unrenameable file must not start out ticked"
        );
        assert_eq!(
            entry.raw_name, raw,
            "the key must stay the bytes the filesystem gave"
        );
        // Shown by its bytes: the undecodable part as escapes, not as the
        // replacement character every such name would share.
        let shown = &entry.original_name;
        assert!(!shown.contains('\u{FFFD}'), "shown lossily: {shown:?}");
        assert!(
            shown.starts_with('f') && shown.ends_with('t') && shown.contains('\\'),
            "the bytes are not shown: {shown:?}"
        );
    }

    /// A name that is not text cannot be ticked, by any route.
    ///
    /// **This is the hole boot gate 52 pointed at.** The gate refuses a field
    /// that production writes and only a test reads, and `raw_name` was one:
    /// the rename built its source path from `original_name`, the *text* form.
    /// That was harmless only because `renameable` was applied once, at
    /// construction, as the entry's initial `selected` value -- and Ctrl+A set
    /// `selected = true` on every file regardless, while Space toggled them
    /// one at a time.
    ///
    /// So a user could tick a file whose name is not valid UTF-8 and press
    /// Enter, and the rename would be attempted against the lossy form --
    /// `to_string_lossy` having replaced the offending bytes with U+FFFD,
    /// which is a **different name**. The dead field was the symptom; a guard
    /// that stopped holding the moment the user touched anything was the
    /// defect.
    #[test]
    fn a_name_that_is_not_text_cannot_be_selected_by_any_route() {
        let mut app = RenamerApp::new();
        app.files.push(FileEntry::from_os_name(&not_text(), 10, 0));
        app.files.push(FileEntry::new("plain.txt", 10, 0));
        app.apply_operations();

        assert!(
            !app.files[0].renameable,
            "control: the fixture must not be text"
        );
        assert!(
            !app.files[0].selected,
            "an unrenameable file starts unticked"
        );

        // Ctrl+A.
        app.select_all(true);
        assert!(
            !app.files[0].selected,
            "select-all ticked a file whose name no rule can transform"
        );
        assert!(app.files[1].selected, "and left the ordinary file alone");

        // Space, on the unrenameable row.
        app.selected_file = 0;
        assert_eq!(app.toggle_selected_file(), EventResult::Consumed);
        assert!(!app.files[0].selected, "Space ticked it");
        assert!(
            app.status_message.contains("not text"),
            "refused silently, which reads as a broken key: {}",
            app.status_message
        );
    }

    /// Even if something ticks it, the rename does not pick it up.
    ///
    /// The guard is applied again where the cost is. Two checks for one rule
    /// is usually worth avoiding; here the second is what stands between a
    /// lossy name and `fs::rename`.
    ///
    /// **This asserts the decision, not a file on disk, and the first version
    /// asserted the wrong one.** It checked that the folder afterwards held
    /// only the renamed ordinary file -- which is true whether or not the
    /// unrenameable one was attempted, because a rename of a name NTFS cannot
    /// store fails either way and leaves the folder looking identical. The
    /// sabotage run said so: dropping the `renameable` check from the filter
    /// left it green. What can be checked on any host is which files the
    /// program decided to rename.
    #[test]
    fn an_unrenameable_file_is_not_renamed_even_if_it_is_selected() {
        let mut app = RenamerApp::new();
        app.files.push(FileEntry::new("plain.txt", 10, 0));
        app.files.push(FileEntry::from_os_name(&not_text(), 10, 0));

        // Reach past every control and set the bit directly, which is what a
        // future edit to a selection path would do by accident.
        app.files[1].selected = true;
        app.files[0].new_name = String::from("plain-renamed.txt");
        app.files[1].new_name = String::from("renamed.txt");

        assert_eq!(
            app.rename_count(),
            1,
            "the unrenameable file was counted as something to rename"
        );

        let queued: Vec<String> = app
            .files
            .iter()
            .filter(|f| f.renameable && f.selected && f.original_name != f.new_name && !f.conflict)
            .map(|f| f.new_name.clone())
            .collect();
        assert_eq!(
            queued,
            vec!["plain-renamed.txt"],
            "only the file whose name is text is queued"
        );
    }

    /// A fixture name that cannot be represented as a `String`.
    fn not_text() -> OsString {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            // An unpaired surrogate: valid UTF-16, no UTF-8 encoding.
            OsString::from_wide(&[0x0066, 0xD800, 0x0074])
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![b'f', 0xFF, b't'])
        }
    }

    /// A scratch directory of this test's own, emptied first.
    ///
    /// Unique per call: Rust runs tests on many threads, so a directory named
    /// for the fixture alone would have two tests renaming each other's files
    /// and failing in whichever order they happened to interleave.
    fn scratch(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SCRATCH_SEQ: AtomicUsize = AtomicUsize::new(0);
        let n = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("slateos-renamer-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// An app holding a real directory containing `names`.
    ///
    /// This used to call `add_file`, which put names in a `Vec` and no files
    /// anywhere. **Every test below that calls `execute_rename` was therefore
    /// checking a memory shuffle against a program whose status line said
    /// "Renamed 6 files".** They are the same tests; what changed is that
    /// there is now a directory for them to be right or wrong about.
    fn app_with(names: &[&str]) -> RenamerApp {
        let dir = scratch("app");
        for name in names {
            std::fs::write(dir.join(name), name.as_bytes()).expect("fixture");
        }
        let mut app = RenamerApp::new();
        let said = app.load_folder(&dir);
        assert_eq!(
            app.files.len(),
            names.len(),
            "the fixture did not load, or this test asserts nothing: {said}"
        );
        app
    }

    /// An app holding a list of names and no directory.
    ///
    /// For the tests of the pure parts -- the rules, the preview, conflict
    /// detection -- which are functions of the names alone. Those do not need
    /// a directory, and one of them **cannot have one on this host**: see
    /// `a_case_only_rename_is_not_a_conflict`.
    fn app_listing(names: &[&str]) -> RenamerApp {
        let mut app = RenamerApp::new();
        for name in names {
            app.add_file(name, 100, 0);
        }
        app
    }

    /// Every filename actually in the app's folder, sorted.
    fn on_disk(app: &RenamerApp) -> Vec<String> {
        let dir = app.folder.clone().expect("a folder is open");
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("listing")
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort();
        names
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

    fn conflicts_of(app: &RenamerApp) -> Vec<bool> {
        app.files.iter().map(|f| f.conflict).collect()
    }

    #[test]
    fn renaming_a_file_updates_the_path_it_is_stored_under() {
        let mut app = app_with(&["old.txt"]);
        preview(&mut app, &["new.txt"]);
        app.execute_rename();
        assert_eq!(names_of(&app), vec!["new.txt"]);
        assert_eq!(
            on_disk(&app),
            vec!["new.txt"],
            "the list and the folder disagree"
        );
    }

    /// A folder named after the file inside it keeps its own name.
    ///
    /// This replaces two tests of `replace_file_name`, a helper that did
    /// string surgery on a stored path: `path.replace(old, new)` rewrote
    /// *every* occurrence rather than the last component, so renaming
    /// `/music/Nirvana/Nirvana` moved the file under a directory that does not
    /// exist. **That defect cannot occur now and the helper is gone** -- the
    /// path is `folder.join(name)` and only the name is ever replaced, so
    /// there is no string in which a directory could be rewritten by accident.
    ///
    /// Checked against a real directory rather than against a string, because
    /// what the old tests actually cared about was where the file ends up.
    #[test]
    fn renaming_a_file_never_touches_the_folder_holding_it() {
        let dir = scratch("folder-name-collision").join("Nirvana");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        std::fs::write(dir.join("Nirvana"), b"x").expect("fixture");

        let mut app = RenamerApp::new();
        app.load_folder(&dir);
        preview(&mut app, &["Nevermind"]);
        app.execute_rename();

        assert!(
            dir.join("Nevermind").is_file(),
            "the file did not arrive: {}",
            app.status_message
        );
        assert!(dir.is_dir(), "the folder holding it was renamed too");
        assert!(!dir.join("Nirvana").exists(), "the old name is still there");
    }

    #[test]
    fn a_case_only_rename_is_not_a_conflict() {
        // Slate OS's filesystem is case-sensitive (`design.txt`), so
        // `photo.jpg` and `PHOTO.jpg` are two different files. The old
        // `eq_ignore_ascii_case` comparison refused this rename as a
        // collision with a file it cannot in fact collide with.
        //
        // **This one keeps a list rather than a directory, and the reason is
        // not convenience.** The tests run on Windows, whose filesystem is
        // case-INSENSITIVE, so writing `photo.JPG` and `PHOTO.jpg` into a
        // scratch directory produces *one* file -- the fixture guard in
        // `app_with` caught exactly that. The rule under test is a property of
        // the target filesystem, and the host cannot hold the fixture that
        // would demonstrate it. Conflict detection is a function of the names
        // alone, so a list is a complete fixture for it either way.
        let mut app = app_listing(&["photo.JPG", "PHOTO.jpg"]);
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
        assert_eq!(on_disk(&app), vec!["2.jpg", "3.jpg", "4.jpg"]);
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
        assert_eq!(
            on_disk(&app),
            vec!["a.txt", "b.txt"],
            "both names survive the swap"
        );
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
        assert_eq!(on_disk(&app), vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn undoing_and_redoing_a_shifted_sequence_restores_every_name() {
        let mut app = app_with(&["1.jpg", "2.jpg", "3.jpg"]);
        preview(&mut app, &["2.jpg", "3.jpg", "4.jpg"]);
        app.execute_rename();

        app.undo();
        assert_eq!(names_of(&app), vec!["1.jpg", "2.jpg", "3.jpg"]);
        assert_eq!(on_disk(&app), vec!["1.jpg", "2.jpg", "3.jpg"]);

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

        fn fills(app: &mut RenamerApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = RenamerApp::new();

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
    // The pointer, the Add Rule menu and the rule editor
    //
    // Driven through `handle_event` at the points the renderer says it drew
    // each control -- see `guitk::probe`.
    // ------------------------------------------------------------------

    use guitk::probe::{self, Probe};

    impl Probe for RenamerApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (1100.0, 750.0);

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
            (self.last_width, self.last_height) = size;
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
            (self.last_width, self.last_height) = size;
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, size: (f32, f32)) -> Option<EventResult> {
            (self.last_width, self.last_height) = size;
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    fn new_names(app: &RenamerApp) -> Vec<String> {
        app.files.iter().map(|f| f.new_name.clone()).collect()
    }

    /// Add a rule of `kind` (its name in the menu) the way a person does:
    /// the Add Rule button, then the menu's row.
    fn add_from_menu(app: &mut RenamerApp, kind: &str) {
        probe::click(app, Target::AddRule);
        let index = RULE_KINDS
            .iter()
            .position(|k| *k == kind)
            .expect("a kind the menu lists");
        assert!(app.rule_menu.is_some(), "Add Rule put no menu up");
        for _ in 0..=index {
            probe::key(app, &probe::press(Key::Down));
        }
        probe::key(app, &probe::press(Key::Enter));
        assert!(app.rule_menu.is_none(), "choosing did not close the menu");
    }

    /// **Every kind of rule can be added.** Only the ones that needed nothing
    /// typed ever could; find and replace, insert, remove, regex, date stamp,
    /// template and four of the five extension rules were implemented, tested
    /// and unreachable.
    #[test]
    fn the_add_rule_menu_adds_every_kind() {
        for kind in RULE_KINDS {
            let mut app = app_listing(&["a.txt"]);
            add_from_menu(&mut app, kind);
            assert_eq!(app.operations.len(), 1, "{kind} was not added");
            assert_eq!(app.operations[0].label(), kind);
            assert_eq!(app.selected_op, 0);
            // A kind with boxes opens its first one for typing.
            let wants_typing = !rule_slots(&app.operations[0]).is_empty();
            assert_eq!(
                matches!(app.focus, Focus::Field(_)),
                wants_typing,
                "{kind}: the keyboard is {:?}",
                app.focus
            );
        }
    }

    /// **A rule is typed into, and the preview follows as it is typed.**
    #[test]
    fn a_find_and_replace_rule_is_written_in_its_editor() {
        let mut app = app_listing(&["IMG_0001.JPG", "IMG_0002.JPG", "notes.txt"]);
        add_from_menu(&mut app, "Find & Replace");
        probe::type_str(&mut app, "IMG_");
        probe::key(&mut app, &probe::press(Key::Tab));
        assert_eq!(
            app.focus,
            Focus::Field(Slot::B),
            "Tab did not reach the second box"
        );
        probe::type_str(&mut app, "photo-");
        assert_eq!(
            new_names(&app),
            vec!["photo-0001.JPG", "photo-0002.JPG", "notes.txt"]
        );
        // Typing a letter in a box is text, not a shortcut: `L` did not add a
        // lower-case rule.
        assert_eq!(app.operations.len(), 1);
        probe::key(&mut app, &probe::press(Key::Escape));
        assert_eq!(app.focus, Focus::List);
    }

    /// A counting box refuses what is not a number, keeps the rule's last good
    /// value, and says so under the box.
    #[test]
    fn a_counting_box_refuses_what_is_not_a_number() {
        let mut app = app_listing(&["abcdef.txt"]);
        add_from_menu(&mut app, "Remove Characters");
        probe::type_str(&mut app, "1");
        probe::key(&mut app, &probe::press(Key::Tab));
        probe::type_str(&mut app, "x");
        assert!(app.draft_invalid, "an x in a count was taken");
        assert!(matches!(
            app.operations[0],
            RenameOp::Remove { from: 1, count: 0 }
        ));
        probe::key(&mut app, &probe::press(Key::Backspace));
        probe::type_str(&mut app, "2");
        assert!(!app.draft_invalid);
        assert_eq!(new_names(&app), vec!["adef.txt"]);
    }

    /// Choices and switches change the rule they belong to.
    #[test]
    fn choices_and_switches_change_the_rule() {
        let mut app = app_listing(&["hello world.txt"]);
        add_from_menu(&mut app, "Change Case");
        probe::click(&mut app, Target::Choice(Pick::Case(CaseMode::Title)));
        assert_eq!(new_names(&app), vec!["Hello World.txt"]);
        probe::click(&mut app, Target::Choice(Pick::Case(CaseMode::CamelCase)));
        assert_eq!(new_names(&app), vec!["helloWorld.txt"]);

        let mut app = app_listing(&["file.txt"]);
        add_from_menu(&mut app, "Insert Text");
        probe::type_str(&mut app, "X");
        assert!(!probe::is_visible(&app, Target::Field(Slot::B)));
        probe::click(&mut app, Target::Choice(Pick::Position(PositionKind::At)));
        assert!(
            probe::is_visible(&app, Target::Field(Slot::B)),
            "choosing a position offered no box to type it in"
        );
        probe::click(&mut app, Target::Field(Slot::B));
        probe::type_str(&mut app, "2");
        assert_eq!(new_names(&app), vec!["fiXle.txt"]);

        let mut app = app_listing(&["Aa aA.txt"]);
        add_from_menu(&mut app, "Find & Replace");
        probe::type_str(&mut app, "a");
        probe::key(&mut app, &probe::press(Key::Tab));
        probe::type_str(&mut app, "-");
        assert_eq!(new_names(&app), vec!["-- --.txt"]);
        probe::click(&mut app, Target::Switch(Flip::MatchCase));
        assert_eq!(new_names(&app), vec!["A- -A.txt"]);
        probe::click(&mut app, Target::Switch(Flip::EveryOccurrence));
        assert_eq!(new_names(&app), vec!["A- aA.txt"]);
    }

    /// **The regex rule is a regular expression.** It was a literal replace
    /// under "Simple regex: only support literal patterns for now".
    #[test]
    fn the_regex_rule_is_a_regular_expression() {
        let op = |pattern: &str, replacement: &str, case_sensitive| RenameOp::Regex {
            pattern: pattern.into(),
            replacement: replacement.into(),
            case_sensitive,
        };
        let ctx = RuleContext::default();
        assert_eq!(
            RenameEngine::apply_in(&op("IMG_([0-9]+)", "photo-\\1", true), "IMG_0042.JPG", ctx),
            "photo-0042.JPG"
        );
        assert_eq!(
            RenameEngine::apply_in(&op("[aeiou]", "<&>", true), "rename.txt", ctx),
            "r<e>n<a>m<e>.txt"
        );
        assert_eq!(
            RenameEngine::apply_in(&op("img", "pic", false), "IMG_1.jpg", ctx),
            "pic_1.jpg",
            "case-insensitive matching"
        );
        assert_eq!(
            RenameEngine::apply_in(&op("a\\&b", "x", true), "a&b.txt", ctx),
            "x.txt"
        );
        // A pattern that is not one changes nothing, and the editor says why.
        assert_eq!(
            RenameEngine::apply_in(&op("(unclosed", "x", true), "(unclosed.txt", ctx),
            "(unclosed.txt"
        );
        let notes: Vec<(String, bool)> = rule_controls(&op("(unclosed", "x", true))
            .into_iter()
            .filter_map(|c| match c {
                Control::Note { text, error } => Some((text, error)),
                _ => None,
            })
            .collect();
        assert!(notes.iter().any(|(_, error)| *error), "{notes:?}");
    }

    /// A rule the user has only half written changes nothing: an empty search
    /// matches between every character.
    #[test]
    fn an_empty_search_changes_nothing() {
        let ctx = RuleContext::default();
        let find = RenameOp::FindReplace {
            find: String::new(),
            replace: "x".into(),
            case_sensitive: true,
            replace_all: true,
        };
        assert_eq!(RenameEngine::apply_in(&find, "abc.txt", ctx), "abc.txt");
        let regex = RenameOp::Regex {
            pattern: String::new(),
            replacement: "x".into(),
            case_sensitive: true,
        };
        assert_eq!(RenameEngine::apply_in(&regex, "abc.txt", ctx), "abc.txt");
    }

    /// **Numbering counts the files being renamed**, not every file listed.
    #[test]
    fn numbering_counts_only_the_ticked_files() {
        let mut app = app_listing(&["a.txt", "b.txt", "c.txt"]);
        app.handle_event(&press(Key::N));
        assert_eq!(new_names(&app), vec!["a_001.txt", "b_002.txt", "c_003.txt"]);
        probe::click(&mut app, Target::Check(1));
        assert!(!app.files[1].selected);
        assert_eq!(new_names(&app), vec!["a_001.txt", "b.txt", "c_002.txt"]);
    }

    /// Unticking a file takes it out of a collision; the conflicts were
    /// computed once and went stale when the ticks moved.
    #[test]
    fn unticking_a_file_updates_the_conflicts() {
        let mut app = app_listing(&["A.txt", "a.txt"]);
        app.handle_event(&press(Key::L));
        // Both end as a.txt: the one renamed and the one already there.
        assert_eq!(app.conflict_count(), 2);
        // Untick the one being renamed, and nothing collides any more.
        app.selected_file = 0;
        app.handle_event(&press(Key::Space));
        assert_eq!(
            app.conflict_count(),
            0,
            "a collision with an unticked file remained"
        );
    }

    /// **Opening another folder keeps the rules.**
    #[test]
    fn the_rules_survive_opening_another_folder() {
        let mut app = app_with(&["One.txt"]);
        app.handle_event(&press(Key::L));
        let second = scratch("second");
        std::fs::write(second.join("Two.TXT"), b"2").unwrap();
        app.load_folder(&second);
        assert_eq!(
            app.operations.len(),
            1,
            "opening a folder threw the rules away"
        );
        assert_eq!(new_names(&app), vec!["two.TXT"]);
    }

    /// **The file list scrolls: with the wheel, and to follow the cursor.**
    #[test]
    fn the_file_list_scrolls() {
        let names: Vec<String> = (0..100).map(|i| format!("file{i:03}.txt")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut app = app_listing(&refs);
        assert!(!probe::is_visible(&app, Target::File(99)));
        for _ in 0..99 {
            probe::key(&mut app, &probe::press(Key::Down));
        }
        assert_eq!(app.selected_file, 99);
        assert!(
            probe::is_visible(&app, Target::File(99)),
            "the cursor left the screen"
        );
        for _ in 0..40 {
            probe::scroll_at_point(&mut app, Target::Files, 1.0);
        }
        assert!(
            probe::is_visible(&app, Target::File(0)),
            "the wheel does not reach the top"
        );
    }

    /// A press on a row moves the cursor; a press on its box ticks it.
    #[test]
    fn a_row_moves_the_cursor_and_its_box_ticks() {
        let mut app = app_listing(&["a.txt", "b.txt", "c.txt"]);
        probe::click(&mut app, Target::File(2));
        assert_eq!(app.selected_file, 2);
        assert!(
            app.files[2].selected,
            "a press on the row ticked nothing, as it should not"
        );
        probe::click(&mut app, Target::Check(2));
        assert!(!app.files[2].selected, "the box did not untick");
    }

    /// **Every rule can be selected, moved and removed** -- not only the
    /// newest, which was the only one the selection ever landed on.
    #[test]
    fn every_rule_can_be_selected_moved_and_removed() {
        let mut app = app_listing(&["Ab.txt"]);
        app.handle_event(&press(Key::L));
        app.handle_event(&press(Key::U));
        assert_eq!(app.selected_op, 1);
        probe::click(&mut app, Target::Rule(0));
        assert_eq!(app.selected_op, 0);
        probe::click(&mut app, Target::RuleDown(0));
        assert_eq!(app.operations[0].label(), "Change Case");
        assert!(matches!(
            app.operations[1],
            RenameOp::ChangeCase(CaseMode::Lower)
        ));
        assert_eq!(app.selected_op, 1, "the moved rule is not the selected one");
        probe::click(&mut app, Target::RuleRemove(0));
        assert_eq!(app.operations.len(), 1);
        assert_eq!(new_names(&app), vec!["ab.txt"]);

        // And from the keyboard.
        app.handle_event(&press(Key::U));
        app.handle_event(&Event::Key(probe::press_with(
            Key::Up,
            guitk::event::Modifiers {
                ctrl: true,
                ..guitk::event::Modifiers::NONE
            },
        )));
        assert_eq!(app.selected_op, 0, "Ctrl+Up selected nothing");
    }

    /// F2 puts the keyboard in the selected rule's first box.
    #[test]
    fn f2_edits_the_selected_rule() {
        let mut app = app_listing(&["a.txt"]);
        add_from_menu(&mut app, "Insert Text");
        probe::key(&mut app, &probe::press(Key::Escape));
        app.handle_event(&press(Key::F2));
        assert_eq!(app.focus, Focus::Field(Slot::A));
        probe::type_str(&mut app, "new-");
        assert_eq!(new_names(&app), vec!["new-a.txt"]);
    }

    /// The extension box and the conflicts chip filter the list.
    #[test]
    fn the_list_filters_answer_the_pointer() {
        let mut app = app_listing(&["a.jpg", "b.png", "c.JPG"]);
        probe::click(&mut app, Target::ExtensionBox);
        assert_eq!(app.focus, Focus::Extension);
        probe::type_str(&mut app, "jpg");
        let shown: Vec<usize> = app.filtered_files().iter().map(|(i, _)| *i).collect();
        assert_eq!(shown, vec![0, 2]);
        probe::click(&mut app, Target::ConflictsOnly);
        assert!(app.filter_conflicts);
        assert_eq!(
            app.focus,
            Focus::List,
            "a press elsewhere left the box typing"
        );
    }

    /// The toolbar's buttons do what their keys do.
    #[test]
    fn the_toolbar_answers_the_pointer() {
        let mut app = app_with(&["one.txt", "two.txt"]);
        probe::click(&mut app, Target::OpenFolder);
        assert!(app.picker.is_open(), "Open Folder put no picker up");
        app.picker.close();

        app.handle_event(&press(Key::U));
        probe::click(&mut app, Target::Rename);
        assert!(
            app.files.iter().any(|f| f.original_name == "ONE.txt"),
            "{}",
            app.status_message
        );
        probe::click(&mut app, Target::Undo);
        assert!(app.files.iter().any(|f| f.original_name == "one.txt"));
        probe::click(&mut app, Target::Redo);
        assert!(app.files.iter().any(|f| f.original_name == "ONE.txt"));
        probe::click(&mut app, Target::ClearRules);
        assert!(app.operations.is_empty());
    }

    /// **The History panel says when.** Each record kept its time, and the
    /// panel drew only what changed.
    #[test]
    fn the_history_says_when() {
        let mut app = app_with(&["one.txt"]);
        app.handle_event(&press(Key::U));
        app.handle_event(&press(Key::Enter));
        probe::click(&mut app, Target::Panel(SidebarPanel::History));
        let year = guitk::datetime::stamp(
            i64::try_from(app.history[0].timestamp_ms / 1000).unwrap(),
            &guitk::tzrules::Tz::utc(),
        );
        let text: String = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(text.contains(&year), "{text}");
        assert!(
            app.history[0].timestamp_ms > 1_700_000_000_000,
            "the time is not the clock's"
        );
    }

    /// **The layout follows the window.** It was drawn from the size the
    /// window opens at, whatever it had been resized to.
    #[test]
    fn the_layout_follows_the_window() {
        let app = app_listing(&["a.txt"]);
        let small = probe::rect_of_sized(&app, Target::Files, (1100.0, 750.0)).unwrap();
        let large = probe::rect_of_sized(&app, Target::Files, (1600.0, 1000.0)).unwrap();
        assert!(
            large.w > small.w + 400.0,
            "the list did not widen: {small:?} {large:?}"
        );
        assert!(
            large.h > small.h + 200.0,
            "the list did not deepen: {small:?} {large:?}"
        );
    }

    /// **A resize moves where presses land.** The size the layout is
    /// measured in was the size the window opened at, whatever it had been
    /// resized to since.
    #[test]
    fn a_resize_moves_where_presses_land() {
        let mut app = app_listing(&["a.txt"]);
        let press_at = |app: &mut RenamerApp, x, y| {
            app.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }))
        };
        // Beyond the right edge of the window the app opens at.
        assert_eq!(press_at(&mut app, 1500.0, 600.0), EventResult::Ignored);
        app.handle_event(&Event::Resize {
            width: 1600,
            height: 1000,
        });
        assert_eq!(
            press_at(&mut app, 1500.0, 600.0),
            EventResult::Consumed,
            "the widened list does not take a press"
        );
    }

    /// Hovering a control names its key in the status bar.
    #[test]
    fn hovering_a_control_names_its_key() {
        let mut app = app_listing(&["a.txt"]);
        let (x, y) = probe::rect_of(&app, Target::Rename).unwrap().centre();
        app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Move,
        }));
        let text: String = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(text.contains("Rename the ticked files (Enter)"), "{text}");
    }

    /// The operations panel scrolls when a rule's editor is taller than the
    /// window leaves room for.
    #[test]
    fn a_tall_editor_scrolls() {
        let mut app = app_listing(&["a.txt"]);
        (app.last_width, app.last_height) = (1100.0, 360.0);
        for _ in 0..6 {
            app.handle_event(&press(Key::L));
        }
        add_from_menu(&mut app, "Add Numbering");
        let size = (1100.0, 360.0);
        probe::key(&mut app, &probe::press(Key::Escape));
        // Set here rather than pressed: its chip is below the panel's fold,
        // which is the point of the test.
        let last = app.operations.len() - 1;
        apply_pick(&mut app.operations[last], Pick::Position(PositionKind::At));
        assert!(!probe::is_visible_sized(&app, Target::Field(Slot::E), size));
        for _ in 0..40 {
            probe::scroll_at_point_sized(&mut app, Target::Ops, -1.0, size);
        }
        assert!(
            probe::is_visible_sized(&app, Target::Field(Slot::E), size),
            "the last box of the editor cannot be reached"
        );
    }
}
