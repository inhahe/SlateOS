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
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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
        let extension = name
            .rsplit('.')
            .next()
            .filter(|e| e.len() < name.len())
            .unwrap_or("")
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
                    InsertPosition::At(pos) => (*pos).min(stem.len()),
                };
                let mut new_stem =
                    String::with_capacity(stem.len().saturating_add(text.len()));
                new_stem.push_str(&stem[..insert_pos]);
                new_stem.push_str(text);
                new_stem.push_str(&stem[insert_pos..]);
                format!("{new_stem}{ext}")
            }
            RenameOp::Remove { from, count } => {
                let from_clamped = (*from).min(stem.len());
                let end = (from_clamped.saturating_add(*count)).min(stem.len());
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
                        let pos = (*pos).min(stem.len());
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
                        let pos = (*pos).min(stem.len());
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

    /// Detect naming conflicts (duplicate new names).
    fn detect_conflicts(&mut self) {
        // Clear all conflict flags
        for file in &mut self.files {
            file.conflict = false;
        }

        // Check for duplicates among selected files
        let names: Vec<String> = self
            .files
            .iter()
            .filter(|f| f.selected)
            .map(|f| f.new_name.to_ascii_lowercase())
            .collect();

        // Collect original names so we can check cross-collisions without borrowing self.files
        let originals: Vec<String> = self.files.iter().map(|f| f.original_name.clone()).collect();

        for (i, file) in self.files.iter_mut().enumerate() {
            if !file.selected {
                continue;
            }
            let lower = file.new_name.to_ascii_lowercase();
            // Count how many times this name appears
            let count = names.iter().filter(|n| **n == lower).count();
            if count > 1 {
                file.conflict = true;
            }
            // Also check if new name collides with original name of another file
            for (j, orig) in originals.iter().enumerate() {
                if i != j && orig.eq_ignore_ascii_case(&file.new_name) {
                    file.conflict = true;
                }
            }
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

        // Apply the renames (in real OS, would call filesystem rename)
        for file in &mut self.files {
            if file.selected && file.original_name != file.new_name && !file.conflict {
                file.original_name = file.new_name.clone();
                file.original_path = file
                    .original_path
                    .replace(&file.original_name, &file.new_name);
            }
        }

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
    fn undo(&mut self) {
        if let Some(record) = self.undo_stack.pop() {
            // Reverse the renames
            for (old_name, new_name) in &record.renames {
                for file in &mut self.files {
                    if file.original_name == *new_name {
                        file.original_name = old_name.clone();
                        file.new_name = old_name.clone();
                    }
                }
            }
            let count = record.renames.len();
            self.redo_stack.push(record);
            self.status_message = format!("Undid rename of {count} files");
            self.apply_operations();
        }
    }

    /// Redo the last undone rename.
    fn redo(&mut self) {
        if let Some(record) = self.redo_stack.pop() {
            for (_, new_name) in &record.renames {
                for file in &mut self.files {
                    if file.new_name == *new_name || file.original_name == *new_name {
                        file.original_name = new_name.clone();
                    }
                }
            }
            let count = record.renames.len();
            self.undo_stack.push(record);
            self.status_message = format!("Redid rename of {count} files");
            self.apply_operations();
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
            let bw = label.len() as f32 * 8.0 + 20.0;
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
            });
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + PADDING + 18.0,
                text: "Add operations to see a".into(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: y + PADDING + 34.0,
                text: "live rename preview.".into(),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
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
            });

            // Operation details
            let detail = match op {
                RenameOp::FindReplace { find, replace, .. } => {
                    format!("\"{find}\" → \"{replace}\"")
                }
                RenameOp::Insert { text, position } => format!(
                    "\"{text}\" at {}",
                    match position {
                        InsertPosition::Start => "start".into(),
                        InsertPosition::End => "end".into(),
                        InsertPosition::At(n) => format!("pos {n}"),
                    }
                ),
                RenameOp::ChangeCase(mode) => mode.label().into(),
                RenameOp::Number {
                    start,
                    step,
                    padding,
                    ..
                } => format!("from {start} step {step} pad {padding}"),
                RenameOp::Extension(ext_op) => match ext_op {
                    ExtensionOp::Replace(e) => format!("→ .{e}"),
                    ExtensionOp::Add(e) => format!("+ .{e}"),
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
                    font_size: SMALL_TEXT - 1.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(SIDEBAR_WIDTH - 40.0),
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

        let headers = [
            ("", 30.0),
            ("Original Name", 250.0),
            ("→", 20.0),
            ("New Name", 250.0),
            ("Size", 80.0),
            ("Status", 80.0),
        ];
        let mut hx = x + 4.0;
        for (label, col_w) in &headers {
            cmds.push(RenderCommand::Text {
                x: hx,
                y: y + 5.0,
                text: (*label).into(),
                font_size: SMALL_TEXT,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Bold,
                max_width: Some(*col_w),
            });
            hx += col_w + 8.0;
        }

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

            let mut cx = x + 4.0;

            // Checkbox
            let check_color = if file.selected { GREEN } else { SURFACE2 };
            cmds.push(RenderCommand::FillRect {
                x: cx + 4.0,
                y: ry + 4.0,
                width: 14.0,
                height: 14.0,
                color: check_color,
                corner_radii: CornerRadii::all(2.0),
            });
            if file.selected {
                cmds.push(RenderCommand::Text {
                    x: cx + 6.0,
                    y: ry + 4.0,
                    text: "✓".into(),
                    font_size: 10.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(12.0),
                });
            }
            cx += 38.0;

            // Original name
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 4.0,
                text: file.original_name.clone(),
                font_size: SMALL_TEXT,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(250.0),
            });
            cx += 258.0;

            // Arrow
            let changed = file.original_name != file.new_name;
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 4.0,
                text: if changed { "→" } else { "=" }.into(),
                font_size: SMALL_TEXT,
                color: if changed { GREEN } else { OVERLAY0 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(20.0),
            });
            cx += 28.0;

            // New name
            let new_color = if file.conflict {
                RED
            } else if changed {
                GREEN
            } else {
                SUBTEXT0
            };
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 4.0,
                text: file.new_name.clone(),
                font_size: SMALL_TEXT,
                color: new_color,
                font_weight: if changed {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(250.0),
            });
            cx += 258.0;

            // Size
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 4.0,
                text: format_size(file.size),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
            });
            cx += 88.0;

            // Status
            let status = if file.conflict {
                ("Conflict", RED)
            } else if changed {
                ("Changed", GREEN)
            } else {
                ("", OVERLAY0)
            };
            if !status.0.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: cx,
                    y: ry + 4.0,
                    text: status.0.into(),
                    font_size: SMALL_TEXT,
                    color: status.1,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(80.0),
                });
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
        });
    }
}

// ============================================================================
// Utility
// ============================================================================

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
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
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
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
}
