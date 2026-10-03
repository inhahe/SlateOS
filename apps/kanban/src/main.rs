//! Kanban Board / Project Management Application
//!
//! A feature-rich Kanban board for Slate OS with multiple boards, customizable
//! columns, rich cards (labels, priority, due dates, checklists, comments),
//! filtering, sorting, WIP limits, swimlanes, archiving, and JSON export/import.
//!
//! # What is kept
//!
//! Every board, in one file in the settings directory (`kanban/boards.txt`),
//! written after every key or click that changed one -- there is no Save.
//! Until 2026-09-25 nothing was: every card was gone when the window closed,
//! and a board's JSON export (Ctrl+E) was the only way to keep one. The file is
//! the notes library's kind (design-decisions §1205): tab-separated text, a
//! record a line (`boards_text`, `parse_boards`), read whole or not at all.
//! The window compares the boards' text after each event with what it last
//! wrote, so no change can go unkept whichever path made it.

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Event, Key, KeyEvent};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow, content_bottom};
use guitk::scroll_window;
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use pathtext::ShowPath;
use std::process::ExitCode;
use std::time::Duration;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use textfmt::tsv;

// =============================================================================
// Catppuccin Mocha palette
// =============================================================================

mod palette {}

// =============================================================================
// Domain types
// =============================================================================

/// Unique identifier for domain objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Id(u64);

/// The next id [`Id::new`] hands out. Every id read from a file moves it past
/// that id ([`Id::from_stored`]).
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl Id {
    /// The identifier a file says a thing has.
    ///
    /// Restoring a board has to use the ids that were written, not fresh ones
    /// from the counter: a column stores `card_ids`, so renumbering the cards
    /// would leave every column pointing at nothing while the board still
    /// looked whole.
    ///
    /// And the counter is moved past it. It was not, so after an import the
    /// next card made could be given the id of one just read -- and cards are
    /// kept in a map by id, so the new card replaced the imported one.
    fn from_stored(raw: u64) -> Self {
        NEXT_ID.fetch_max(raw.saturating_add(1), Ordering::Relaxed);
        Self(raw)
    }

    fn new() -> Self {
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Card priority levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl Priority {
    /// The inverse of [`label`](Self::label).
    ///
    /// Unknown text becomes `Medium` rather than failing the whole import: a
    /// board is worth more than a priority, and an unreadable priority is
    /// visible on the card, where a refused file is not visible at all.
    fn from_label(text: &str) -> Self {
        match text {
            "Low" => Self::Low,
            "High" => Self::High,
            "Critical" => Self::Critical,
            _ => Self::Medium,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Critical => "Critical",
        }
    }

    fn color(self, pal: &Palette) -> Color {
        match self {
            Self::Low => pal.teal,
            Self::Medium => pal.blue,
            Self::High => pal.peach,
            Self::Critical => pal.red,
        }
    }

    /// Every priority in order: what the filter bar's Ctrl+P steps through.
    fn all() -> &'static [Priority] {
        &[Self::Low, Self::Medium, Self::High, Self::Critical]
    }

    fn next(self) -> Self {
        match self {
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Critical,
            Self::Critical => Self::Low,
        }
    }
}

/// Predefined label types for cards.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Label {
    id: Id,
    name: String,
    color: Color,
}

impl Label {
    fn new(name: &str, color: Color) -> Self {
        Self {
            id: Id::new(),
            name: name.to_string(),
            color,
        }
    }
}

/// A checklist item on a card.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ChecklistItem {
    // Kept in the boards file and read back from it; the program itself
    // addresses boards, columns and cards by position.
    id: Id,
    text: String,
    done: bool,
}

impl ChecklistItem {
    fn new(text: &str) -> Self {
        Self {
            id: Id::new(),
            text: text.to_string(),
            done: false,
        }
    }
}

/// A comment on a card.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Comment {
    // Kept in the boards file and read back from it; the program itself
    // addresses boards, columns and cards by position.
    id: Id,
    author: String,
    text: String,
    // When it was written, kept in the boards file and read back.
    timestamp: u64,
}

impl Comment {
    fn new(author: &str, text: &str, timestamp: u64) -> Self {
        Self {
            id: Id::new(),
            author: author.to_string(),
            text: text.to_string(),
            timestamp,
        }
    }
}

/// A simple date representation (year, month, day).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SimpleDate {
    year: u16,
    month: u8,
    day: u8,
}

/// `#[allow(dead_code)]`: `new` had no caller once `create_sample_data`
/// became a fixture, and a date constructor is the API a real store would use.
#[allow(dead_code, reason = "date constructor; no store exists yet")]
impl SimpleDate {
    fn new(year: u16, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    /// Read back what [`display`](Self::display) wrote: `YYYY-MM-DD`.
    ///
    /// `None` on anything else. A due date that will not parse is dropped and
    /// the card keeps everything else, which is the same trade as an unknown
    /// priority: the card is worth more than the field.
    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let year: u16 = parts.next()?.parse().ok()?;
        let month: u8 = parts.next()?.parse().ok()?;
        let day: u8 = parts.next()?.parse().ok()?;
        if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        Some(Self { year, month, day })
    }

    fn display(&self) -> String {
        let m = self.month.clamp(1, 12);
        let d = self.day.clamp(1, 31);
        format!("{:04}-{:02}-{:02}", self.year, m, d)
    }
}

/// A Kanban card with all associated metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Card {
    id: Id,
    title: String,
    description: String,
    labels: Vec<Id>,
    priority: Priority,
    due_date: Option<SimpleDate>,
    assignee: String,
    checklist: Vec<ChecklistItem>,
    comments: Vec<Comment>,
    created_at: u64,
    archived: bool,
    /// The column an archived card came out of, by id, so Restore puts it
    /// back there. `None` for a card never archived, and for one archived
    /// before this was recorded -- which is restored to the first column.
    archived_from: Option<Id>,
    // Swimlanes: a second axis for the board, kept in the boards file and
    // never drawn. The board renderer lays out columns only, so there is
    // nowhere for a lane to appear.
    // See known-issues.md -> TD-C-KANBAN-HAS-AN-EXPORTER-AN-IMPORTER-AND-SWIMLANES-NONE-REACHABLE.
    swimlane: String,
}

/// Builders for a card.
///
/// Most are the tests' shorthand for a card with a field set: the running
/// program sets those fields from the keys (E, D, P on an open card), so the
/// builders are compiled for the tests only rather than kept alive in the
/// program with an allow.
impl Card {
    fn new(title: &str) -> Self {
        Self {
            id: Id::new(),
            title: title.to_string(),
            description: String::new(),
            labels: Vec::new(),
            priority: Priority::Medium,
            due_date: None,
            assignee: String::new(),
            checklist: Vec::new(),
            comments: Vec::new(),
            created_at: 0,
            archived: false,
            archived_from: None,
            swimlane: String::new(),
        }
    }

    #[cfg(test)]
    fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    #[cfg(test)]
    fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    #[cfg(test)]
    fn with_assignee(mut self, assignee: &str) -> Self {
        self.assignee = assignee.to_string();
        self
    }

    #[cfg(test)]
    fn with_due_date(mut self, date: SimpleDate) -> Self {
        self.due_date = Some(date);
        self
    }

    #[cfg(test)]
    fn with_label(mut self, label_id: Id) -> Self {
        if !self.labels.contains(&label_id) {
            self.labels.push(label_id);
        }
        self
    }

    #[cfg(test)]
    fn with_swimlane(mut self, lane: &str) -> Self {
        self.swimlane = lane.to_string();
        self
    }

    fn with_created_at(mut self, ts: u64) -> Self {
        self.created_at = ts;
        self
    }

    fn checklist_progress(&self) -> (usize, usize) {
        let total = self.checklist.len();
        let done = self.checklist.iter().filter(|c| c.done).count();
        (done, total)
    }

    fn has_label(&self, label_id: Id) -> bool {
        self.labels.contains(&label_id)
    }

    fn add_checklist_item(&mut self, text: &str) {
        self.checklist.push(ChecklistItem::new(text));
    }

    fn add_comment(&mut self, author: &str, text: &str, timestamp: u64) {
        self.comments.push(Comment::new(author, text, timestamp));
    }

    /// Ticked from the open card: Tab chooses an item, Space ticks it.
    fn toggle_checklist_item(&mut self, item_id: Id) {
        for item in &mut self.checklist {
            if item.id == item_id {
                item.done = !item.done;
                return;
            }
        }
    }
}

/// Sort criteria for cards within a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortBy {
    Priority,
    DueDate,
    CreatedAt,
    Title,
}

impl SortBy {
    fn label(self) -> &'static str {
        match self {
            Self::Priority => "Priority",
            Self::DueDate => "Due Date",
            Self::CreatedAt => "Created",
            Self::Title => "Title",
        }
    }

    fn all() -> &'static [SortBy] {
        &[Self::Priority, Self::DueDate, Self::CreatedAt, Self::Title]
    }

    /// The order after this one, round to the first: Shift+T.
    fn next(self) -> Self {
        let all = Self::all();
        let at = all.iter().position(|&o| o == self).unwrap_or(0);
        all.get(at.saturating_add(1))
            .copied()
            .unwrap_or(Self::Priority)
    }
}

/// A Kanban column holding an ordered list of cards.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Column {
    // Kept in the boards file and read back from it; the program itself
    // addresses boards, columns and cards by position.
    id: Id,
    name: String,
    card_ids: Vec<Id>,
    wip_limit: Option<usize>,
    sort_by: SortBy,
    /// Drawn narrow, with its cards hidden: Z on the board.
    collapsed: bool,
}

impl Column {
    fn new(name: &str) -> Self {
        Self {
            id: Id::new(),
            name: name.to_string(),
            card_ids: Vec::new(),
            wip_limit: None,
            sort_by: SortBy::Priority,
            collapsed: false,
        }
    }

    fn with_wip_limit(mut self, limit: usize) -> Self {
        self.wip_limit = Some(limit);
        self
    }

    fn is_over_wip_limit(&self) -> bool {
        if let Some(limit) = self.wip_limit {
            self.card_ids.len() > limit
        } else {
            false
        }
    }

    fn active_card_count(&self, cards: &HashMap<Id, Card>) -> usize {
        self.card_ids
            .iter()
            .filter(|cid| cards.get(cid).is_some_and(|c| !c.archived))
            .count()
    }
}

/// A Kanban board containing columns and cards.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Board {
    // Written to the boards file and read back, and not otherwise used: this
    // app addresses boards by position.
    id: Id,
    name: String,
    columns: Vec<Column>,
    cards: HashMap<Id, Card>,
    labels: Vec<Label>,
    archived_card_ids: Vec<Id>,
    // Swimlanes: a second axis for the board, kept in the boards file and
    // never drawn. The board renderer lays out columns only, so there is
    // nowhere for a lane to appear.
    // See known-issues.md -> TD-C-KANBAN-HAS-AN-EXPORTER-AN-IMPORTER-AND-SWIMLANES-NONE-REACHABLE.
    swimlanes_enabled: bool,
    swimlane_names: Vec<String>,
}

impl Board {
    fn new(name: &str) -> Self {
        Self {
            id: Id::new(),
            name: name.to_string(),
            columns: Vec::new(),
            cards: HashMap::new(),
            labels: Vec::new(),
            archived_card_ids: Vec::new(),
            swimlanes_enabled: false,
            swimlane_names: Vec::new(),
        }
    }

    /// The board a new user starts with.
    ///
    /// The label colours here are fixed values, not theme roles. A `Label`
    /// stores its colour, so it is the user's own data: nothing rewrites it
    /// when the theme changes, and a themed value would leave labels made
    /// before the change in the old scheme and ones made after in the new.
    /// Same rule as `snippets`' folder colours and `hexeditor`'s bookmarks.
    fn default_board() -> Self {
        let mut board = Self::new("My Project");

        // Default labels
        board
            .labels
            .push(Label::new("Bug", Color::from_hex(0xF38BA8)));
        board
            .labels
            .push(Label::new("Feature", Color::from_hex(0x89B4FA)));
        board
            .labels
            .push(Label::new("Enhancement", Color::from_hex(0xA6E3A1)));
        board
            .labels
            .push(Label::new("Documentation", Color::from_hex(0xB4BEFE)));
        board
            .labels
            .push(Label::new("Urgent", Color::from_hex(0xFAB387)));
        board
            .labels
            .push(Label::new("Design", Color::from_hex(0xCBA6F7)));
        board
            .labels
            .push(Label::new("Testing", Color::from_hex(0x94E2D5)));

        // Default columns
        board.columns.push(Column::new("Backlog"));
        board.columns.push(Column::new("Todo"));
        board
            .columns
            .push(Column::new("In Progress").with_wip_limit(5));
        board.columns.push(Column::new("Review").with_wip_limit(3));
        board.columns.push(Column::new("Done"));

        board
    }

    fn add_card_to_column(&mut self, card: Card, column_idx: usize) -> Option<Id> {
        let card_id = card.id;
        self.cards.insert(card_id, card);
        if let Some(col) = self.columns.get_mut(column_idx) {
            col.card_ids.push(card_id);
            Some(card_id)
        } else {
            None
        }
    }

    fn move_card(&mut self, card_id: Id, from_col: usize, to_col: usize, to_pos: usize) -> bool {
        if from_col >= self.columns.len() || to_col >= self.columns.len() {
            return false;
        }
        // Remove from source
        if let Some(col) = self.columns.get_mut(from_col) {
            if let Some(pos) = col.card_ids.iter().position(|&c| c == card_id) {
                col.card_ids.remove(pos);
            } else {
                return false;
            }
        }
        // Insert into destination
        if let Some(col) = self.columns.get_mut(to_col) {
            let insert_at = to_pos.min(col.card_ids.len());
            col.card_ids.insert(insert_at, card_id);
            true
        } else {
            false
        }
    }

    fn archive_card(&mut self, card_id: Id) -> bool {
        let from = self
            .find_card_column(card_id)
            .and_then(|i| self.columns.get(i))
            .map(|column| column.id);
        if let Some(card) = self.cards.get_mut(&card_id) {
            card.archived = true;
            card.archived_from = from;
            self.archived_card_ids.push(card_id);
            // Remove from all columns
            for col in &mut self.columns {
                col.card_ids.retain(|&c| c != card_id);
            }
            true
        } else {
            false
        }
    }

    /// Put an archived card back into column `column_idx`, at the bottom.
    ///
    /// The column is checked before anything changes. This cleared the
    /// card's archived mark and took it out of the archive first, then found
    /// no such column and answered `false` -- leaving a card in no column and
    /// not archived, which nothing on screen shows: gone, without a delete.
    fn unarchive_card(&mut self, card_id: Id, column_idx: usize) -> bool {
        if column_idx >= self.columns.len() || !self.archived_card_ids.contains(&card_id) {
            return false;
        }
        let Some(card) = self.cards.get_mut(&card_id) else {
            return false;
        };
        card.archived = false;
        card.archived_from = None;
        self.archived_card_ids.retain(|&c| c != card_id);
        if let Some(col) = self.columns.get_mut(column_idx) {
            col.card_ids.push(card_id);
        }
        true
    }

    /// Restore an archived card to the column it was archived from, or to
    /// the first column when that one is gone (or was never recorded).
    /// Answers the column it went to, or `None` when the board has no
    /// column to put it in.
    fn restore_card(&mut self, card_id: Id) -> Option<usize> {
        let from = self.cards.get(&card_id)?.archived_from;
        let column = from
            .and_then(|id| self.columns.iter().position(|c| c.id == id))
            .unwrap_or(0);
        self.unarchive_card(card_id, column).then_some(column)
    }

    fn delete_card(&mut self, card_id: Id) -> bool {
        for col in &mut self.columns {
            col.card_ids.retain(|&c| c != card_id);
        }
        self.archived_card_ids.retain(|&c| c != card_id);
        self.cards.remove(&card_id).is_some()
    }

    fn add_column(&mut self, name: &str) {
        self.columns.push(Column::new(name));
    }

    /// Take column `col_idx` off the board. Shift+Delete asks for it only for
    /// a column with no cards in it, so no card is ever left in no column.
    fn remove_column(&mut self, col_idx: usize) -> Option<Column> {
        if col_idx < self.columns.len() {
            Some(self.columns.remove(col_idx))
        } else {
            None
        }
    }

    fn find_card_column(&self, card_id: Id) -> Option<usize> {
        for (i, col) in self.columns.iter().enumerate() {
            if col.card_ids.contains(&card_id) {
                return Some(i);
            }
        }
        None
    }

    fn column_stats(&self) -> Vec<ColumnStats> {
        self.columns
            .iter()
            .map(|col| {
                let active = col.active_card_count(&self.cards);
                ColumnStats {
                    name: col.name.clone(),
                    card_count: active,
                    over_wip: col.is_over_wip_limit(),
                    wip_limit: col.wip_limit,
                }
            })
            .collect()
    }

    fn completion_rate(&self) -> f32 {
        let total = self.cards.len();
        if total == 0 {
            return 0.0;
        }
        let archived = self.archived_card_ids.len();
        // Cards in the last column ("Done") + archived cards
        let done_count = self
            .columns
            .last()
            .map_or(0, |col| col.active_card_count(&self.cards));
        let completed: usize = done_count.saturating_add(archived);
        (completed as f32) / (total as f32) * 100.0
    }

    fn sort_column(&mut self, col_idx: usize) {
        if let Some(col) = self.columns.get_mut(col_idx) {
            let cards_ref = &self.cards;
            let sort_by = col.sort_by;
            col.card_ids.sort_by(|a, b| {
                let card_a = cards_ref.get(a);
                let card_b = cards_ref.get(b);
                match (card_a, card_b) {
                    (Some(ca), Some(cb)) => match sort_by {
                        SortBy::Priority => cb.priority.cmp(&ca.priority),
                        // Soonest first, and a card with no date after every
                        // card with one: `None < Some`, so plain `cmp` put the
                        // undated cards at the top of a column sorted by date.
                        SortBy::DueDate => match (ca.due_date, cb.due_date) {
                            (Some(a), Some(b)) => a.cmp(&b),
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (None, None) => std::cmp::Ordering::Equal,
                        },
                        SortBy::CreatedAt => ca.created_at.cmp(&cb.created_at),
                        SortBy::Title => ca.title.cmp(&cb.title),
                    },
                    _ => std::cmp::Ordering::Equal,
                }
            });
        }
    }

    fn get_label_by_id(&self, id: Id) -> Option<&Label> {
        self.labels.iter().find(|l| l.id == id)
    }

    // Swimlanes: a second axis for the board, modelled end to end and
    // never drawn. The board renderer lays out columns only, so there is
    // nowhere for a lane to appear.
    // See known-issues.md -> TD-C-KANBAN-HAS-AN-EXPORTER-AN-IMPORTER-AND-SWIMLANES-NONE-REACHABLE.
    #[allow(dead_code, reason = "swimlanes have no layout mode")]
    fn swimlane_cards(&self, col_idx: usize, swimlane: &str) -> Vec<Id> {
        if let Some(col) = self.columns.get(col_idx) {
            col.card_ids
                .iter()
                .filter(|cid| {
                    self.cards
                        .get(cid)
                        .is_some_and(|c| !c.archived && c.swimlane == swimlane)
                })
                .copied()
                .collect()
        } else {
            Vec::new()
        }
    }
}

/// Statistics for a column.
#[derive(Clone, Debug)]
struct ColumnStats {
    name: String,
    card_count: usize,
    over_wip: bool,
    wip_limit: Option<usize>,
}

// =============================================================================
// Filter state
// =============================================================================

/// Active filters for displaying cards.
#[derive(Clone, Debug, Default)]
struct FilterState {
    label_filter: Option<Id>,
    priority_filter: Option<Priority>,
    assignee_filter: String,
    search_text: String,
}

impl FilterState {
    fn is_active(&self) -> bool {
        self.label_filter.is_some()
            || self.priority_filter.is_some()
            || !self.assignee_filter.is_empty()
            || !self.search_text.is_empty()
    }

    fn matches(&self, card: &Card) -> bool {
        if let Some(label_id) = self.label_filter
            && !card.has_label(label_id)
        {
            return false;
        }
        if let Some(priority) = self.priority_filter
            && card.priority != priority
        {
            return false;
        }
        if !self.assignee_filter.is_empty()
            && !card
                .assignee
                .to_lowercase()
                .contains(&self.assignee_filter.to_lowercase())
        {
            return false;
        }
        if !self.search_text.is_empty() {
            let needle = self.search_text.to_lowercase();
            let in_title = card.title.to_lowercase().contains(&needle);
            let in_desc = card.description.to_lowercase().contains(&needle);
            if !in_title && !in_desc {
                return false;
            }
        }
        true
    }

    fn clear(&mut self) {
        self.label_filter = None;
        self.priority_filter = None;
        self.assignee_filter.clear();
        self.search_text.clear();
    }
}

// =============================================================================
// JSON export/import
// =============================================================================

/// Simple JSON serialization for boards (no external dependency).
//
// Reachable through `KanbanApp::write_board`, which the save picker calls and
// which writes the result atomically through `safeio`. The comment here used
// to say the serialiser had no caller and nowhere to put its output; both
// stopped being true when the picker landed.
//
// `export_json` was deleted with this edit. It called `export_board` and
// returned the string, under a comment saying "it has nowhere to put its
// result: this program cannot write a file yet" -- superseded by `write_board`
// and left behind. The compiler found it, after I wrote a comment here
// claiming it was the live path: a claim about reachability is exactly the
// kind a build already answers, and I asserted instead of asking.
struct JsonExporter;

impl JsonExporter {
    // The serialiser's body. Ten tests, no caller — see the struct above.
    fn escape_json(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c < '\x20' => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out
    }

    fn export_card(card: &Card) -> String {
        let labels_json: Vec<String> = card.labels.iter().map(|l| format!("{}", l.0)).collect();
        let checklist_json: Vec<String> = card
            .checklist
            .iter()
            .map(|ci| {
                format!(
                    "{{\"text\":\"{}\",\"done\":{}}}",
                    Self::escape_json(&ci.text),
                    ci.done
                )
            })
            .collect();
        let comments_json: Vec<String> = card
            .comments
            .iter()
            .map(|c| {
                format!(
                    "{{\"author\":\"{}\",\"text\":\"{}\",\"timestamp\":{}}}",
                    Self::escape_json(&c.author),
                    Self::escape_json(&c.text),
                    c.timestamp
                )
            })
            .collect();
        let due_str = card
            .due_date
            .map_or_else(|| "null".to_string(), |d| format!("\"{}\"", d.display()));

        format!(
            "{{\"id\":{},\"title\":\"{}\",\"description\":\"{}\",\"labels\":[{}],\
             \"priority\":\"{}\",\"due_date\":{},\"assignee\":\"{}\",\
             \"checklist\":[{}],\"comments\":[{}],\"created_at\":{},\
             \"archived\":{},\"archived_from\":{},\"swimlane\":\"{}\"}}",
            card.id.0,
            Self::escape_json(&card.title),
            Self::escape_json(&card.description),
            labels_json.join(","),
            card.priority.label(),
            due_str,
            Self::escape_json(&card.assignee),
            checklist_json.join(","),
            comments_json.join(","),
            card.created_at,
            card.archived,
            card.archived_from
                .map_or_else(|| "null".to_string(), |id| id.0.to_string()),
            Self::escape_json(&card.swimlane),
        )
    }

    fn export_column(col: &Column) -> String {
        let card_ids: Vec<String> = col.card_ids.iter().map(|c| format!("{}", c.0)).collect();
        let wip_str = col
            .wip_limit
            .map_or_else(|| "null".to_string(), |l| format!("{}", l));
        format!(
            "{{\"id\":{},\"name\":\"{}\",\"card_ids\":[{}],\"wip_limit\":{},\"sort_by\":\"{}\"}}",
            col.id.0,
            Self::escape_json(&col.name),
            card_ids.join(","),
            wip_str,
            col.sort_by.label(),
        )
    }

    fn export_label(label: &Label) -> String {
        format!(
            "{{\"id\":{},\"name\":\"{}\",\"color\":\"#{:02x}{:02x}{:02x}\"}}",
            label.id.0,
            Self::escape_json(&label.name),
            label.color.r,
            label.color.g,
            label.color.b,
        )
    }

    fn export_board(board: &Board) -> String {
        let cols: Vec<String> = board.columns.iter().map(Self::export_column).collect();
        let cards: Vec<String> = board.cards.values().map(Self::export_card).collect();
        let labels: Vec<String> = board.labels.iter().map(Self::export_label).collect();
        let swimlanes: Vec<String> = board
            .swimlane_names
            .iter()
            .map(|s| format!("\"{}\"", Self::escape_json(s)))
            .collect();

        format!(
            "{{\"name\":\"{}\",\"columns\":[{}],\"cards\":[{}],\"labels\":[{}],\
             \"swimlanes_enabled\":{},\"swimlane_names\":[{}]}}",
            Self::escape_json(&board.name),
            cols.join(","),
            cards.join(","),
            labels.join(","),
            board.swimlanes_enabled,
            swimlanes.join(","),
        )
    }
}

// =============================================================================
// The boards file
// =============================================================================

/// The boards file's first field, which says what the file is.
const BOARDS_MAGIC: &str = "slateos-kanban";

/// The version of the boards file this writes, and the newest it reads.
///
/// 2 adds `origin` lines, the column each archived card came from. A
/// format 1 file -- every file written before 2026-09-27 -- is still read,
/// and its archived cards restore to the first column.
const BOARDS_FORMAT: u32 = 2;

/// The oldest boards file this reads.
const BOARDS_OLDEST: u32 = 1;

/// The largest boards file this will read. One cut short would be read as
/// fewer boards with no sign any were missing, so a larger file is refused
/// rather than read in part.
const MAX_BOARDS_BYTES: usize = 64 * 1024 * 1024;

/// Why nothing is kept, when the environment names no home directory.
const NO_HOME: &str = "Nothing is kept: no home directory is set";

/// Where the boards are kept, or `None` when the environment names no home
/// directory.
fn boards_path() -> Option<std::path::PathBuf> {
    settingsfile::config_dir().map(|dir| dir.join("kanban").join("boards.txt"))
}

/// Milliseconds since 1970 by the clock; 0 if it reads earlier than that.
fn clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// A yes-or-no as written.
fn flag(on: bool) -> &'static str {
    if on { "1" } else { "0" }
}

/// A yes-or-no as read, or `None` for anything [`flag`] never writes.
fn read_flag(field: &str) -> Option<bool> {
    match field {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// A colour as written: `#RRGGBB`, or `#RRGGBBAA` when it is not opaque.
fn colour_hex(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
    }
}

/// A colour read back from `#RRGGBB` or `#RRGGBBAA`.
fn parse_colour(text: &str) -> Option<Color> {
    let hex = text.strip_prefix('#')?;
    if !hex.is_ascii() || !(hex.len() == 6 || hex.len() == 8) {
        return None;
    }
    let byte = |at: usize| {
        hex.get(at..at.saturating_add(2))
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
    };
    let a = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, a))
}

fn priority_key(p: Priority) -> &'static str {
    match p {
        Priority::Low => "low",
        Priority::Medium => "medium",
        Priority::High => "high",
        Priority::Critical => "critical",
    }
}

fn priority_from_key(key: &str) -> Option<Priority> {
    Some(match key {
        "low" => Priority::Low,
        "medium" => Priority::Medium,
        "high" => Priority::High,
        "critical" => Priority::Critical,
        _ => return None,
    })
}

fn sort_key(sort: SortBy) -> &'static str {
    match sort {
        SortBy::Priority => "priority",
        SortBy::DueDate => "due",
        SortBy::CreatedAt => "created",
        SortBy::Title => "title",
    }
}

fn sort_from_key(key: &str) -> Option<SortBy> {
    Some(match key {
        "priority" => SortBy::Priority,
        "due" => SortBy::DueDate,
        "created" => SortBy::CreatedAt,
        "title" => SortBy::Title,
        _ => return None,
    })
}

/// Ids as the trailing fields of a line.
fn push_ids(out: &mut String, ids: &[Id]) {
    for id in ids {
        out.push('\t');
        out.push_str(&id.0.to_string());
    }
}

/// Every board as text: a first line naming the format, the board that was
/// open, then each board -- its swimlanes, labels and cards (each followed by
/// its labels, checklist and comments), then its columns, each naming its
/// cards in order, and the archived.
///
/// ```text
/// slateos-kanban  1
/// active   <index of the board that was open>
/// board    <id>  <swimlanes 1|0>  <name>
/// lane     <name>
/// label    <id>  <#RRGGBB>  <name>
/// card     <id>  <low|medium|high|critical>  <due YYYY-MM-DD, or nothing>
///          <created>  <archived 1|0>  <title>  <assignee>  <swimlane>  <description>
/// tag      <label id>
/// item     <id>  <done 1|0>  <text>
/// comment  <id>  <time>  <author>  <text>
/// column   <id>  <limit, or nothing>  <priority|due|created|title>  <collapsed 1|0>
///          <name>  <card id>...
/// archived <card id>...
/// origin   <archived card id>  <column id>        (format 2)
/// ```
///
/// Fields are separated by tabs and escaped with `textfmt::tsv`; times are
/// milliseconds since 1970. A card is written once, and a column lists the
/// ids of the cards in it, so a card in no column and not archived is kept
/// too.
fn boards_text(boards: &[Board], active: usize) -> String {
    let mut out = format!("{BOARDS_MAGIC}\t{BOARDS_FORMAT}\nactive\t{active}\n");
    for board in boards {
        out.push_str(&format!(
            "board\t{}\t{}\t{}\n",
            board.id.0,
            flag(board.swimlanes_enabled),
            tsv::escape(&board.name)
        ));
        for lane in &board.swimlane_names {
            out.push_str(&format!("lane\t{}\n", tsv::escape(lane)));
        }
        for label in &board.labels {
            out.push_str(&format!(
                "label\t{}\t{}\t{}\n",
                label.id.0,
                colour_hex(label.color),
                tsv::escape(&label.name)
            ));
        }
        // By id, so the same board is always the same text.
        let mut cards: Vec<&Card> = board.cards.values().collect();
        cards.sort_by_key(|c| c.id.0);
        for card in cards {
            out.push_str(&format!(
                "card\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                card.id.0,
                priority_key(card.priority),
                card.due_date.map_or_else(String::new, |d| format!(
                    "{:04}-{:02}-{:02}",
                    d.year, d.month, d.day
                )),
                card.created_at,
                flag(card.archived),
                tsv::escape(&card.title),
                tsv::escape(&card.assignee),
                tsv::escape(&card.swimlane),
                tsv::escape(&card.description)
            ));
            for label in &card.labels {
                out.push_str(&format!("tag\t{}\n", label.0));
            }
            for item in &card.checklist {
                out.push_str(&format!(
                    "item\t{}\t{}\t{}\n",
                    item.id.0,
                    flag(item.done),
                    tsv::escape(&item.text)
                ));
            }
            for comment in &card.comments {
                out.push_str(&format!(
                    "comment\t{}\t{}\t{}\t{}\n",
                    comment.id.0,
                    comment.timestamp,
                    tsv::escape(&comment.author),
                    tsv::escape(&comment.text)
                ));
            }
        }
        for column in &board.columns {
            out.push_str(&format!(
                "column\t{}\t{}\t{}\t{}\t{}",
                column.id.0,
                column.wip_limit.map_or_else(String::new, |l| l.to_string()),
                sort_key(column.sort_by),
                flag(column.collapsed),
                tsv::escape(&column.name)
            ));
            push_ids(&mut out, &column.card_ids);
            out.push('\n');
        }
        out.push_str("archived");
        push_ids(&mut out, &board.archived_card_ids);
        out.push('\n');
        for card_id in &board.archived_card_ids {
            if let Some(from) = board.cards.get(card_id).and_then(|c| c.archived_from) {
                out.push_str(&format!("origin\t{}\t{}\n", card_id.0, from.0));
            }
        }
    }
    out
}

/// Every board read from its text, and the one that was open -- or why they
/// cannot be, naming the line.
///
/// All or nothing, for the finance ledger's reason (design-decisions §1202):
/// boards read in part and then kept again would lose, without a word,
/// whatever was not read. So is a board with two cards of one number, a
/// column or the archive naming a card the board does not have, or a card in
/// two places.
fn parse_boards(text: &str) -> Result<(Vec<Board>, usize), String> {
    let mut lines = text.lines().enumerate();
    let first = lines.next().map_or("", |(_, line)| line);
    let head: Vec<&str> = first.split('\t').collect();
    let version = match head.as_slice() {
        [BOARDS_MAGIC, version] => version
            .parse::<u32>()
            .map_err(|_| String::from("line 1 names no format"))?,
        _ => return Err(String::from("it is not a SlateOS kanban file")),
    };
    if version > BOARDS_FORMAT {
        return Err(format!(
            "it is a later format ({version}) than this version reads ({BOARDS_FORMAT})"
        ));
    }
    if version < BOARDS_OLDEST {
        return Err(format!("format {version} is not one this program wrote"));
    }

    let mut boards: Vec<Board> = Vec::new();
    let mut active: Option<(usize, usize)> = None;
    // The card lines of the board being read, in order, for its sub-lines.
    let mut last_card: Option<Id> = None;
    // Every card placed so far on the board being read.
    let mut placed: HashSet<Id> = HashSet::new();
    let mut board_ids = HashSet::new();
    for (i, line) in lines {
        let at = i.saturating_add(1);
        if line.is_empty() {
            continue;
        }
        let bad = |why: &str| format!("line {at}: {why}");
        let text = |field: &str| {
            tsv::unescape(field)
                .ok_or_else(|| bad("a text holds an escape this program never writes"))
        };
        let number = |field: &str, what: &str| {
            field
                .parse::<u64>()
                .map_err(|_| bad(&format!("{what} is not a number")))
        };
        let yes_no =
            |field: &str| read_flag(field).ok_or_else(|| bad("a yes-or-no is neither 1 nor 0"));
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["active", index] => {
                if active.is_some() {
                    return Err(bad("the open board is named twice"));
                }
                let index = usize::try_from(number(index, "the open board")?)
                    .map_err(|_| bad("the open board is not a number"))?;
                active = Some((at, index));
            }
            ["board", id, swimlanes, name] => {
                let id = number(id, "a board's number")?;
                if !board_ids.insert(id) {
                    return Err(bad("two boards have one number"));
                }
                let mut board = Board::new(&text(name)?);
                board.id = Id::from_stored(id);
                board.swimlanes_enabled = yes_no(swimlanes)?;
                boards.push(board);
                last_card = None;
                placed.clear();
            }
            ["lane", name] => {
                let name = text(name)?;
                boards
                    .last_mut()
                    .ok_or_else(|| bad("a swimlane comes before any board"))?
                    .swimlane_names
                    .push(name);
            }
            ["label", id, colour, name] => {
                let mut label = Label::new(
                    &text(name)?,
                    parse_colour(colour).ok_or_else(|| bad("a colour is not one"))?,
                );
                label.id = Id::from_stored(number(id, "a label's number")?);
                boards
                    .last_mut()
                    .ok_or_else(|| bad("a label comes before any board"))?
                    .labels
                    .push(label);
            }
            [
                "card",
                id,
                priority,
                due,
                created,
                archived,
                title,
                assignee,
                swimlane,
                description,
            ] => {
                let board = boards
                    .last_mut()
                    .ok_or_else(|| bad("a card comes before any board"))?;
                let id = Id::from_stored(number(id, "a card's number")?);
                if board.cards.contains_key(&id) {
                    return Err(bad("two cards on one board have one number"));
                }
                let mut card = Card::new(&text(title)?);
                card.id = id;
                card.priority = priority_from_key(priority)
                    .ok_or_else(|| bad("a priority this version does not know"))?;
                card.due_date = if due.is_empty() {
                    None
                } else {
                    Some(SimpleDate::parse(due).ok_or_else(|| bad("a due date is not a date"))?)
                };
                card.created_at = number(created, "when a card was made")?;
                card.archived = yes_no(archived)?;
                card.assignee = text(assignee)?;
                card.swimlane = text(swimlane)?;
                card.description = text(description)?;
                board.cards.insert(id, card);
                last_card = Some(id);
            }
            ["tag", label] => {
                let label = Id::from_stored(number(label, "a label's number")?);
                card_of(&mut boards, last_card)
                    .ok_or_else(|| bad("a card's label comes before any card"))?
                    .labels
                    .push(label);
            }
            ["item", id, done, item] => {
                let item = ChecklistItem {
                    id: Id::from_stored(number(id, "a checklist item's number")?),
                    text: text(item)?,
                    done: yes_no(done)?,
                };
                card_of(&mut boards, last_card)
                    .ok_or_else(|| bad("a checklist item comes before any card"))?
                    .checklist
                    .push(item);
            }
            ["comment", id, time, author, said] => {
                let comment = Comment {
                    id: Id::from_stored(number(id, "a comment's number")?),
                    author: text(author)?,
                    text: text(said)?,
                    timestamp: number(time, "when a comment was made")?,
                };
                card_of(&mut boards, last_card)
                    .ok_or_else(|| bad("a comment comes before any card"))?
                    .comments
                    .push(comment);
            }
            ["column", id, limit, sort, collapsed, name, cards @ ..] => {
                let board = boards
                    .last_mut()
                    .ok_or_else(|| bad("a column comes before any board"))?;
                let mut column = Column::new(&text(name)?);
                column.id = Id::from_stored(number(id, "a column's number")?);
                column.wip_limit = if limit.is_empty() {
                    None
                } else {
                    Some(
                        usize::try_from(number(limit, "a column's limit")?)
                            .map_err(|_| bad("a column's limit is not a number"))?,
                    )
                };
                column.sort_by = sort_from_key(sort)
                    .ok_or_else(|| bad("a sort order this version does not know"))?;
                column.collapsed = yes_no(collapsed)?;
                column.card_ids = placed_ids(cards, board, &mut placed, &bad)?;
                board.columns.push(column);
            }
            ["archived", cards @ ..] => {
                let board = boards
                    .last_mut()
                    .ok_or_else(|| bad("the archive comes before any board"))?;
                board.archived_card_ids = placed_ids(cards, board, &mut placed, &bad)?;
            }
            ["origin", card, column] if version >= 2 => {
                let card = Id::from_stored(number(card, "an archived card's number")?);
                let column = Id::from_stored(number(column, "a column's number")?);
                let board = boards
                    .last_mut()
                    .ok_or_else(|| bad("an origin comes before any board"))?;
                if !board.archived_card_ids.contains(&card) {
                    return Err(bad("an origin names a card that is not archived"));
                }
                if let Some(archived) = board.cards.get_mut(&card) {
                    archived.archived_from = Some(column);
                }
            }
            _ => {
                return Err(bad(
                    "it is not a line this version reads, or has the wrong number of fields",
                ));
            }
        }
    }
    if boards.is_empty() {
        return Err(String::from("it holds no boards"));
    }
    let active = match active {
        None => 0,
        Some((at, index)) if index >= boards.len() => {
            return Err(format!("line {at}: the open board is not one of them"));
        }
        Some((_, index)) => index,
    };
    Ok((boards, active))
}

/// The card `id` names on the last board read, to hang its sub-lines on.
fn card_of(boards: &mut [Board], id: Option<Id>) -> Option<&mut Card> {
    boards.last_mut()?.cards.get_mut(&id?)
}

/// Card ids listed on a column or archive line: each a card the board has,
/// and none placed before.
fn placed_ids(
    fields: &[&str],
    board: &Board,
    placed: &mut HashSet<Id>,
    bad: &dyn Fn(&str) -> String,
) -> Result<Vec<Id>, String> {
    let mut ids = Vec::with_capacity(fields.len());
    for field in fields {
        let raw = field
            .parse::<u64>()
            .map_err(|_| bad("a card's number is not a number"))?;
        let id = Id::from_stored(raw);
        if !board.cards.contains_key(&id) {
            return Err(bad(&format!("card {raw} is not on the board")));
        }
        if !placed.insert(id) {
            return Err(bad(&format!("card {raw} is in two places")));
        }
        ids.push(id);
    }
    Ok(ids)
}

/// `#rrggbb` as written by `export_label`.
fn parse_hex_color(text: &str) -> Option<Color> {
    let body = text.strip_prefix('#')?;
    if body.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(body.get(0..2)?, 16).ok()?;
    let g = u8::from_str_radix(body.get(2..4)?, 16).ok()?;
    let b = u8::from_str_radix(body.get(4..6)?, 16).ok()?;
    Some(Color::rgb(r, g, b))
}

/// One card, or `None` when it has no id and therefore nothing to be.
fn card_from_json(item: &JsonValue) -> Option<Card> {
    let id = u64::try_from(item.get("id").and_then(JsonValue::as_i64)?).ok()?;
    let mut card = Card::new(item.get("title").and_then(JsonValue::as_str).unwrap_or(""));
    card.id = Id::from_stored(id);
    card.description = item
        .get("description")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_owned();
    card.labels = item
        .get("labels")
        .and_then(JsonValue::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(JsonValue::as_i64)
        .filter_map(|n| u64::try_from(n).ok())
        .map(Id::from_stored)
        .collect();
    card.priority = item
        .get("priority")
        .and_then(JsonValue::as_str)
        .map_or(Priority::Medium, Priority::from_label);
    card.due_date = item
        .get("due_date")
        .and_then(JsonValue::as_str)
        .and_then(SimpleDate::parse);
    card.assignee = item
        .get("assignee")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_owned();
    card.checklist = item
        .get("checklist")
        .and_then(JsonValue::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(|c| {
            Some(ChecklistItem {
                id: Id::new(),
                text: c.get("text").and_then(JsonValue::as_str)?.to_owned(),
                done: c.get("done").and_then(JsonValue::as_bool).unwrap_or(false),
            })
        })
        .collect();
    card.comments = item
        .get("comments")
        .and_then(JsonValue::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(|c| {
            Some(Comment {
                // A fresh id: nothing addresses a comment by one, and the
                // format does not carry it.
                id: Id::new(),
                author: c.get("author").and_then(JsonValue::as_str)?.to_owned(),
                text: c
                    .get("text")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("")
                    .to_owned(),
                timestamp: c
                    .get("timestamp")
                    .and_then(JsonValue::as_i64)
                    .and_then(|n| u64::try_from(n).ok())
                    .unwrap_or(0),
            })
        })
        .collect();
    card.created_at = item
        .get("created_at")
        .and_then(JsonValue::as_i64)
        .and_then(|n| u64::try_from(n).ok())
        .unwrap_or(0);
    card.archived = item
        .get("archived")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    card.archived_from = item
        .get("archived_from")
        .and_then(JsonValue::as_i64)
        .and_then(|n| u64::try_from(n).ok())
        .map(Id::from_stored);
    card.swimlane = item
        .get("swimlane")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_owned();
    Some(card)
}

/// One JSON value.
///
/// Small on purpose: this reads back what `JsonExporter` writes, which uses
/// objects, arrays, strings, integers, booleans and null and nothing else. A
/// float would be rejected rather than silently truncated.
#[derive(Clone, Debug, PartialEq)]
enum JsonValue {
    Null,
    Bool(bool),
    Num(i64),
    Str(String),
    Arr(Vec<JsonValue>),
    Obj(Vec<(String, JsonValue)>),
}

impl JsonValue {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Num(n) => Some(*n),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Arr(v) => Some(v),
            _ => None,
        }
    }

    /// The value of `key`, or `None` if this is not an object or has no such
    /// key. A missing key and a `null` are deliberately different: the caller
    /// decides which of them is acceptable for each field.
    fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// Reads back what [`JsonExporter`] writes: a value parser over the tokeniser
/// below it, and [`Self::import_board`] over that.
//
// **Reachable since the file picker landed.** This comment used to say the
// importer was a parser waiting on a chooser, and a `dead_code` allow beneath
// it said the same. Both outlived the fix: `import_board` has a caller, the
// picker exists, and removing the four allows leaves the crate compiling
// without a warning -- which is the check that decided it, rather than
// reading the comment.
struct JsonImporter;

impl JsonImporter {
    /// Parse a JSON string value, returning the unescaped content and next offset.
    ///
    /// Unescaped stretches are copied out as whole `&str` slices rather than a
    /// byte at a time. Pushing `byte as char` would reinterpret each UTF-8
    /// byte as the Unicode scalar of that value — a Latin-1 reading that turns
    /// every non-ASCII character into mojibake (`日` = E6 97 A5 becomes three
    /// chars `æ\u{97}¥`) and then persists the damage on the next save.
    fn parse_string(data: &str, start: usize) -> Option<(String, usize)> {
        let bytes = data.as_bytes();
        if bytes.get(start).copied() != Some(b'"') {
            return None;
        }
        let mut result = String::new();
        let mut i = start.saturating_add(1);
        // Start of the current run of literal (unescaped) text.
        let mut run_start = i;
        while i < bytes.len() {
            let b = bytes.get(i).copied()?;
            // `"` and `\` are ASCII, and an ASCII byte can never occur inside a
            // multi-byte UTF-8 sequence, so `i` is always a character boundary
            // here and slicing `run_start..i` can never split a character.
            if b == b'"' {
                result.push_str(data.get(run_start..i)?);
                return Some((result, i.saturating_add(1)));
            }
            if b != b'\\' {
                i = i.saturating_add(1);
                continue;
            }
            result.push_str(data.get(run_start..i)?);
            let after = i.saturating_add(1);
            // Take a whole character, not a byte: an unknown escape may be
            // followed by a multi-byte character, and consuming one byte of it
            // would leave `run_start` stranded inside a UTF-8 sequence.
            let esc = data.get(after..)?.chars().next()?;
            let mut next = after.saturating_add(esc.len_utf8());
            match esc {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                'b' => result.push('\u{08}'),
                'f' => result.push('\u{0c}'),
                'u' => {
                    let (ch, after_escape) = Self::parse_unicode_escape(data, next)?;
                    result.push(ch);
                    next = after_escape;
                }
                other => {
                    // Unknown escape: keep it verbatim rather than silently
                    // dropping the backslash and changing the user's text.
                    result.push('\\');
                    result.push(other);
                }
            }
            i = next;
            run_start = i;
        }
        None
    }

    /// Decode a `\u` escape whose four hex digits begin at `start` (just past
    /// the `u`), returning the character and the offset just past the escape.
    ///
    /// A leading surrogate is combined with a following `\uXXXX` trailing
    /// surrogate, which is how JSON spells characters outside the BMP.
    fn parse_unicode_escape(data: &str, start: usize) -> Option<(char, usize)> {
        let (hi, after_hi) = Self::parse_hex4(data, start)?;
        let bytes = data.as_bytes();
        if (0xD800..0xDC00).contains(&hi)
            && bytes.get(after_hi).copied() == Some(b'\\')
            && bytes.get(after_hi.saturating_add(1)).copied() == Some(b'u')
            && let Some((lo, after_lo)) = Self::parse_hex4(data, after_hi.saturating_add(2))
            && (0xDC00..0xE000).contains(&lo)
            && let Some(combined) = hi
                .saturating_sub(0xD800)
                .checked_shl(10)
                .and_then(|high| high.checked_add(lo.saturating_sub(0xDC00)))
                .and_then(|offset| offset.checked_add(0x1_0000))
            && let Some(ch) = char::from_u32(combined)
        {
            return Some((ch, after_lo));
        }
        // An unpaired surrogate has no scalar value. Substitute U+FFFD rather
        // than failing the whole import over one malformed escape.
        Some((char::from_u32(hi).unwrap_or('\u{FFFD}'), after_hi))
    }

    /// Read exactly four ASCII hex digits at `start`, returning their value and
    /// the offset just past them.
    fn parse_hex4(data: &str, start: usize) -> Option<(u32, usize)> {
        let end = start.checked_add(4)?;
        let hex = data.get(start..end)?;
        // `from_str_radix` would accept a leading `+`; require plain digits so
        // a malformed escape is rejected rather than silently reinterpreted.
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u32::from_str_radix(hex, 16).ok().map(|v| (v, end))
    }

    /// Parse a JSON number (integer), returning value and next offset.
    fn parse_number(data: &str, start: usize) -> Option<(i64, usize)> {
        let rest = data.get(start..)?;
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '-')
            .unwrap_or(rest.len());
        let num_str = rest.get(..end)?;
        let val: i64 = num_str.parse().ok()?;
        Some((val, start.saturating_add(end)))
    }

    /// Rebuild a board from what `JsonExporter::export_board` wrote.
    ///
    /// # What a failure means here
    ///
    /// `Err` is reserved for "this is not one of our boards": the text did not
    /// parse as JSON, or the top level is not an object with a `name`. A field
    /// that is present and unreadable does NOT fail the import -- an unknown
    /// priority becomes Medium, a due date that will not parse is dropped --
    /// because **a board is worth more than a field, and a dropped field is
    /// visible on the card where a refused file is not visible at all.**
    ///
    /// # Errors
    ///
    /// A string describing which of the two happened, for the status line.
    fn import_board(text: &str) -> Result<Board, String> {
        let (value, _) = Self::parse_value(text, 0)
            .ok_or_else(|| String::from("that file is not JSON this program can read"))?;
        let name = value
            .get("name")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| String::from("that JSON has no board name, so it is not a board"))?;

        let mut board = Board::new(name);
        board.columns.clear();
        board.cards.clear();
        board.labels.clear();

        for item in value
            .get("labels")
            .and_then(JsonValue::as_array)
            .unwrap_or(&[])
        {
            let Some(id) = item
                .get("id")
                .and_then(JsonValue::as_i64)
                .and_then(|n| u64::try_from(n).ok())
            else {
                continue;
            };
            let label_name = item.get("name").and_then(JsonValue::as_str).unwrap_or("");
            let color = item
                .get("color")
                .and_then(JsonValue::as_str)
                .and_then(parse_hex_color)
                .unwrap_or(Color::rgb(0x80, 0x80, 0x80));
            let mut label = Label::new(label_name, color);
            label.id = Id::from_stored(id);
            board.labels.push(label);
        }

        for item in value
            .get("cards")
            .and_then(JsonValue::as_array)
            .unwrap_or(&[])
        {
            let Some(card) = card_from_json(item) else {
                continue;
            };
            board.cards.insert(card.id, card);
        }

        for item in value
            .get("columns")
            .and_then(JsonValue::as_array)
            .unwrap_or(&[])
        {
            let Some(id) = item
                .get("id")
                .and_then(JsonValue::as_i64)
                .and_then(|n| u64::try_from(n).ok())
            else {
                continue;
            };
            let mut column =
                Column::new(item.get("name").and_then(JsonValue::as_str).unwrap_or(""));
            column.id = Id::from_stored(id);
            column.card_ids = item
                .get("card_ids")
                .and_then(JsonValue::as_array)
                .unwrap_or(&[])
                .iter()
                .filter_map(JsonValue::as_i64)
                .filter_map(|n| u64::try_from(n).ok())
                .map(Id::from_stored)
                .collect();
            column.wip_limit = item
                .get("wip_limit")
                .and_then(JsonValue::as_i64)
                .and_then(|n| usize::try_from(n).ok());
            board.columns.push(column);
        }

        // A column may name a card only once, and only one that is on the
        // board, and a card may be in only one column: the JSON says what it
        // says, but a board that breaks those is not one this program can
        // keep, and the first place a card is named is where it is.
        let mut placed = HashSet::new();
        for column in &mut board.columns {
            column
                .card_ids
                .retain(|id| board.cards.contains_key(id) && placed.insert(*id));
        }

        board.swimlanes_enabled = value
            .get("swimlanes_enabled")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        board.swimlane_names = value
            .get("swimlane_names")
            .and_then(JsonValue::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(JsonValue::as_str)
            .map(str::to_owned)
            .collect();

        Ok(board)
    }

    /// Parse one JSON value at `start`, returning it and the next offset.
    ///
    /// The piece that was missing. `parse_string`, `parse_number`,
    /// `parse_unicode_escape` and `skip_ws` were all written and all tested,
    /// and nothing assembled them -- so the type called `JsonImporter` was a
    /// tokeniser, and the tracking entry that called it "a complete JSON
    /// importer needing only a file chooser" was wrong by half a parser.
    fn parse_value(data: &str, start: usize) -> Option<(JsonValue, usize)> {
        let i = Self::skip_ws(data, start);
        let rest = data.get(i..)?;
        let first = rest.chars().next()?;
        match first {
            '"' => {
                let (s, next) = Self::parse_string(data, i)?;
                Some((JsonValue::Str(s), next))
            }
            '[' => Self::parse_seq(data, i, ']', |items, data, at| {
                let (v, next) = Self::parse_value(data, at)?;
                items.push(v);
                Some(next)
            })
            .map(|(items, next)| (JsonValue::Arr(items), next)),
            '{' => Self::parse_seq(data, i, '}', |pairs, data, at| {
                let (key, after_key) = Self::parse_string(data, at)?;
                let colon = Self::skip_ws(data, after_key);
                if data.get(colon..)?.chars().next()? != ':' {
                    return None;
                }
                let (v, next) = Self::parse_value(data, colon.checked_add(1)?)?;
                pairs.push((key, v));
                Some(next)
            })
            .map(|(pairs, next)| (JsonValue::Obj(pairs), next)),
            _ => {
                if rest.starts_with("true") {
                    return Some((JsonValue::Bool(true), i.checked_add(4)?));
                }
                if rest.starts_with("false") {
                    return Some((JsonValue::Bool(false), i.checked_add(5)?));
                }
                if rest.starts_with("null") {
                    return Some((JsonValue::Null, i.checked_add(4)?));
                }
                let (n, next) = Self::parse_number(data, i)?;
                Some((JsonValue::Num(n), next))
            }
        }
    }

    /// The comma-separated body shared by arrays and objects.
    ///
    /// One function rather than two near-identical loops: the bracket and what
    /// one element is are the only differences, and two copies of a
    /// comma-and-close loop is exactly the shape that drifts.
    fn parse_seq<T>(
        data: &str,
        open: usize,
        close: char,
        mut element: impl FnMut(&mut Vec<T>, &str, usize) -> Option<usize>,
    ) -> Option<(Vec<T>, usize)> {
        let mut items = Vec::new();
        let mut at = Self::skip_ws(data, open.checked_add(1)?);
        if data.get(at..)?.chars().next()? == close {
            return Some((items, at.checked_add(1)?));
        }
        loop {
            at = element(&mut items, data, Self::skip_ws(data, at))?;
            at = Self::skip_ws(data, at);
            match data.get(at..)?.chars().next()? {
                ',' => at = at.checked_add(1)?,
                c if c == close => return Some((items, at.checked_add(1)?)),
                _ => return None,
            }
        }
    }

    /// Skip whitespace.
    ///
    /// This doc comment lived three hundred lines up, above `import_board`,
    /// where it became that function's first rustdoc line -- so the public
    /// documentation for "rebuild a board from a file" opened with "Skip
    /// whitespace." A doc comment attaches to the next *item*, and there were
    /// two more doc blocks between this one and any function.
    fn skip_ws(data: &str, start: usize) -> usize {
        let bytes = data.as_bytes();
        let mut i = start;
        while i < bytes.len() {
            match bytes.get(i) {
                Some(b' ' | b'\t' | b'\n' | b'\r') => i = i.saturating_add(1),
                _ => break,
            }
        }
        i
    }

    // `validate_export` was here. Its name, its doc comment ("validate that we
    // can round-trip a board through export") and its `bool` all promised a
    // check, and it exported the board and asked whether the string was
    // non-empty. `export_board` always writes at least a header, so **it could
    // not return false.** A validator that cannot fail is not a weaker check
    // than a real one; it is a false statement about the code, and the next
    // person to wire up the importer would reasonably have called it and read
    // a pass as evidence. Deleted rather than fixed: the round trip it claimed
    // to test is now tested for real, by `a_board_survives_a_round_trip`.
}

// =============================================================================
// Application state
// =============================================================================

/// Which view is currently showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Board,
    CardDetail,
    Archive,
    Statistics,
    BoardList,
}

/// The top-level Kanban application state.
/// The keys this program answers, raised by `F1`.
///
/// `?` is not a second way in: naming a card, a column or a search takes typed
/// text, so a `?` has somewhere to go -- the `apps/spreadsheet` case in
/// design-decisions 863.
///
/// Seven rows name the thing they need. `P`, `M`, `B`, `Ctrl+D` and `Ctrl+A`
/// all act on the selected card and are refused without one; `N`, `Shift+C`
/// and `T` belong to the board view; `Ctrl+S` needs the filter bar open.
/// Those refusals are correct, and a list that did not say so would be
/// advertising keys that look broken.
///
/// **`Ctrl+S` is not save.** This app bound it to the search bar long before
/// it had a file door, and the handler's own comment says the app's vocabulary
/// outranks consistency with its neighbours. `Ctrl+E` writes the board out.
/// The first draft of this list said "Open / save" for `Ctrl+O / Ctrl+S`,
/// which was written from habit rather than from the handler; the guard
/// caught it.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Up / Down", "Another card, or scroll an open card"),
    ("Left / Right", "The next column over"),
    ("PageUp / PageDown", "A screenful at a time"),
    ("Enter", "Open the card, or confirm"),
    ("E / D", "Edit an open card's title / description"),
    ("C / L", "Add a comment / a checklist item to it"),
    ("Tab / Space", "Choose a checklist item / tick it"),
    ("Esc", "Back, or cancel what you are typing"),
    ("N", "A new card, on the board"),
    ("Shift+C", "A new column, on the board"),
    ("T / Shift+T", "Sort this column / by the next order"),
    ("R", "Rename this column"),
    ("Z", "Collapse or open this column"),
    ("Shift+Delete", "Remove this column, if it is empty"),
    ("P", "Cycle this card's priority"),
    ("M / B", "Move this card on / back a column"),
    ("Ctrl+D", "Delete this card"),
    ("Ctrl+A", "Archive it"),
    ("Alt+1-4", "Board, statistics, archive, boards"),
    ("Ctrl+F", "Show or hide the filter bar"),
    ("Ctrl+S", "Type a search term, with that bar open"),
    (
        "Ctrl+P / Ctrl+L",
        "Show one priority / one label, with it open",
    ),
    ("Ctrl+U", "Show one person's cards, with it open"),
    ("Ctrl+O", "Open a board"),
    ("Ctrl+E", "Export this one"),
    ("F1", "This list"),
];

struct KanbanApp {
    /// The open or save picker. Holds the dialog, the saving flag and the
    /// routing thirteen applications used to write out by hand.
    picker: FilePicker,
    /// What the last open or save did, for the status line.
    last_file_action: Option<String>,
    /// Why the last key did nothing, when that needs saying -- "move or
    /// archive its cards first". Cleared by the next key, so it describes the
    /// key just pressed and nothing older.
    note: Option<String>,
    /// The size the last frame was drawn at, so a click on the picker is
    /// answered against the window the user is looking at.
    win_width: f32,
    /// See `win_width`.
    win_height: f32,
    boards: Vec<Board>,
    active_board_idx: usize,
    view: View,
    filter: FilterState,
    selected_card: Option<Id>,
    selected_column: usize,
    /// Index of the first card drawn in each column of the board view.
    ///
    /// A card index rather than a pixel offset: columns draw whole cards only,
    /// so a pixel offset could only express positions the renderer then rounds
    /// away. One offset serves every column, each clamped against its own card
    /// count — see design-decisions.md §471. A value past the end of a column
    /// is not an error; that column shows its last page.
    scroll_offset: usize,
    /// Pixels the card-detail modal's body is scrolled down by.
    ///
    /// Pixels here and cards in `scroll_offset` is deliberate — see
    /// [`DETAIL_LINE_STEP`]. Like `scroll_offset`, this is *not* clamped where
    /// it is written: how far the body can scroll depends on the window size
    /// and on how much prose the card holds, neither of which the key handler
    /// knows. The renderer clamps against what it is actually drawing.
    ///
    /// It used to be written by nobody and read by nobody, so the modal drew
    /// its comments straight over the desktop with no way to reach them.
    detail_scroll: f32,
    /// The checklist item of the open card that Space ticks, moved by Tab.
    /// `None` until Tab is first pressed on a card, so opening one never
    /// shows a focus nobody asked for.
    checklist_focus: Option<usize>,
    /// Who a comment is signed by: the login name, when there is one.
    author: String,
    /// The archived card the archive view has chosen, by its place in the
    /// list the view draws.
    archive_cursor: usize,
    /// The board the board list has chosen, by its place in the list.
    ///
    /// Its own field. It was `selected_column`, borrowed while the list was
    /// up -- so the list opened with the cursor on whatever row number the
    /// board's column happened to be, and leaving it with Escape left the
    /// board's column set to a row of the list.
    board_cursor: usize,
    show_filter_bar: bool,
    input_buffer: String,
    input_mode: InputMode,
    /// The last stamp [`stamp`](Self::stamp) gave, so the next is later.
    ///
    /// It was a counter from 1000, which after a restart would have stamped
    /// every new card earlier than every kept one.
    last_stamp: u64,
    /// Whether changes are kept. Off in `new`, so no test can write the user's
    /// boards; `from_settings`, which `main` uses, turns it on, and a file that
    /// cannot be read turns it off again.
    persist: bool,
    /// The boards' text as last written or read. What they say now is
    /// compared with it after every key or click, and written when it
    /// differs.
    kept_text: String,
    /// Why the boards are not being kept, drawn for as long as it is true:
    /// the file could not be read (and so is left exactly as it is), there is
    /// nowhere to keep it, or the last save failed.
    store_error: Option<String>,
    /// The question asked when the window is closed while a save is failing.
    question: Option<unsaved::Question<Pending>>,
    /// Set when the question has been answered with leave.
    quit: bool,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// Whether the shortcut card is up.
    show_help: bool,
}

/// What the user is currently typing into.
///
/// Until 2026-09-27 only the new card's title, a new column's name and a
/// search were ever entered; the rest were the model of an editor with no
/// keys. Each is reached now: the open card's E, D, C and L; the board's R;
/// the board list's N; the filter bar's Ctrl+U.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputMode {
    None,
    NewCardTitle,
    SearchFilter,
    AssigneeFilter,
    NewBoardName,
    NewColumnName,
    CardDescription,
    AddComment,
    AddChecklistItem,
    EditCardTitle,
    RenameColumn,
}

impl KanbanApp {
    fn new() -> Self {
        let default_board = Board::default_board();
        Self {
            show_help: false,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            picker: FilePicker::new(),
            last_file_action: None,
            note: None,
            win_width: INITIAL_WIDTH as f32,
            win_height: INITIAL_HEIGHT as f32,
            boards: vec![default_board],
            active_board_idx: 0,
            view: View::Board,
            filter: FilterState::default(),
            selected_card: None,
            selected_column: 0,
            scroll_offset: 0,
            detail_scroll: 0.0,
            checklist_focus: None,
            author: String::from("You"),
            archive_cursor: 0,
            board_cursor: 0,
            show_filter_bar: false,
            input_buffer: String::new(),
            input_mode: InputMode::None,
            last_stamp: 0,
            persist: false,
            kept_text: String::new(),
            store_error: None,
            question: None,
            quit: false,
        }
    }

    /// The window's boards: the ones kept last time -- or, on a first run,
    /// the one board `new` starts with -- and every change kept from here on.
    fn from_settings() -> Self {
        let mut app = Self::new();
        // Signed by whoever is logged in; "You" when nothing says. It was
        // "User" for everybody.
        if let Some(name) = ["USER", "LOGNAME", "USERNAME"]
            .iter()
            .find_map(|var| std::env::var(var).ok().filter(|n| !n.trim().is_empty()))
        {
            app.author = name;
        }
        match boards_path() {
            Some(path) => {
                app.persist = true;
                app.load_boards(&path);
            }
            // Nowhere to keep anything, and never will be while this window
            // is open: said once, and not asked about again at every close.
            None => app.store_error = Some(String::from(NO_HOME)),
        }
        // What is there now is what is kept: a first run's starting board is
        // not written until something on it changes.
        app.kept_text = boards_text(&app.boards, app.active_board_idx);
        app
    }

    /// Read the boards at `path`; with none there yet, this is a first run.
    ///
    /// A file that cannot be read whole is left exactly as it is: nothing is
    /// saved over it, and the window says so for as long as it is open.
    fn load_boards(&mut self, path: &std::path::Path) {
        self.load_boards_within(path, MAX_BOARDS_BYTES);
    }

    /// [`load_boards`](Self::load_boards) with the size limit given, so a
    /// test can reach it without writing sixty-four megabytes.
    fn load_boards_within(&mut self, path: &std::path::Path, max_bytes: usize) {
        let refused = |why: String| {
            format!(
                "{} was not read ({why}), so nothing is saved over it",
                path.shown()
            )
        };
        let read = match safeio::read_to_string_capped(path, max_bytes) {
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => {
                self.persist = false;
                self.store_error = Some(refused(err.to_string()));
                return;
            }
        };
        if read.truncated {
            self.persist = false;
            self.store_error = Some(refused(format!(
                "it is larger than {} MiB",
                max_bytes / (1024 * 1024)
            )));
            return;
        }
        match parse_boards(&read.text) {
            Ok((boards, active)) => {
                // Every time on a card or a comment, so a change made now is
                // later than all of them even if the clock has been set back.
                let latest = boards
                    .iter()
                    .flat_map(|b| b.cards.values())
                    .flat_map(|c| {
                        std::iter::once(c.created_at).chain(c.comments.iter().map(|m| m.timestamp))
                    })
                    .max()
                    .unwrap_or(0);
                self.last_stamp = self.last_stamp.max(latest);
                self.boards = boards;
                self.active_board_idx = active;
                self.selected_card = None;
                self.selected_column = 0;
            }
            Err(why) => {
                self.persist = false;
                self.store_error = Some(refused(why));
            }
        }
    }

    /// Write the boards, if they have changed since they were last written
    /// and this window keeps anything.
    ///
    /// A failure is kept in `store_error`, drawn on the status line, and the
    /// next event tries again.
    fn keep(&mut self) {
        if !self.persist {
            return;
        }
        let text = boards_text(&self.boards, self.active_board_idx);
        if text == self.kept_text {
            return;
        }
        let Some(path) = boards_path() else {
            self.store_error = Some(String::from(NO_HOME));
            return;
        };
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| safeio::write_str_atomically(&path, &text));
        match written {
            Ok(()) => {
                self.kept_text = text;
                self.store_error = None;
            }
            Err(err) => {
                self.store_error = Some(format!("Not saved to {}: {err}", path.shown()));
            }
        }
    }

    /// Whether the boards say something that is not written.
    fn unkept(&self) -> bool {
        self.persist && boards_text(&self.boards, self.active_board_idx) != self.kept_text
    }

    /// Where the boards are kept -- or why they are not -- for the status
    /// line.
    fn keeping_line(&self) -> String {
        if let Some(error) = &self.store_error {
            return error.clone();
        }
        // Asked first, so a window that keeps nothing never reads where the
        // settings are.
        if !self.persist {
            return String::from("Nothing here is kept.");
        }
        boards_path().map_or_else(
            || String::from(NO_HOME),
            |path| format!("The boards are kept in {}.", path.shown()),
        )
    }

    /// Whether the window may close now: at once, unless the boards have
    /// changes a save is failing to write, which closing would lose.
    fn request_close(&mut self) -> bool {
        self.keep();
        if !self.unkept() {
            return true;
        }
        // The question replaces whatever is up: a picker would take the keys
        // it needs, and be drawn over it.
        self.picker.close();
        self.show_help = false;
        let detail = self.store_error.clone().unwrap_or_default();
        self.question = Some(unsaved::Question::new(
            "Your latest changes to your boards are not saved.",
            &format!("{detail} -- try saving again before closing?"),
            Pending::Close,
        ));
        false
    }

    /// Act on the close question's answer.
    fn answer(&mut self, choice: unsaved::Choice) {
        match choice {
            // Leave only if the save now works; if it fails again the error is
            // on screen and the window stays, which is what Save asked for.
            unsaved::Choice::Save => {
                self.keep();
                self.quit = !self.unkept();
            }
            unsaved::Choice::Discard => self.quit = true,
            unsaved::Choice::Cancel => {}
        }
    }

    /// Write the active board to `path` as JSON.
    ///
    /// Refuses a board with no cards rather than writing one. A board file
    /// holding empty columns is valid and imports as an empty board, which the
    /// user cannot tell apart from a save that failed -- and by then it has
    /// replaced whatever was at that path. See design-decisions 854.
    fn write_board(&mut self, path: &std::path::Path) -> String {
        let board = self.active_board();
        if board.cards.is_empty() {
            return String::from("That board has no cards -- nothing to write");
        }
        let text = JsonExporter::export_board(board);
        let (name, cards) = (board.name.clone(), board.cards.len());
        match safeio::write_str_atomically(path, &text) {
            Ok(()) => format!("Wrote {cards} card(s) from {name} to {}", path.shown()),
            Err(err) => format!("Could not write {}: {err}", path.shown()),
        }
    }

    /// Read `path` as a board and open it.
    ///
    /// The three outcomes are kept apart, because "0 cards" would cover all
    /// of them: a file this program cannot read, a board that genuinely has
    /// no cards, and a read that failed before any parse was attempted.
    fn read_board(&mut self, path: &std::path::Path) -> String {
        let read = match safeio::read_to_string_capped(path, MAX_BOARD_BYTES) {
            Ok(read) => read,
            Err(err) => return format!("Could not read {}: {err}", path.shown()),
        };
        let note = read.note(MAX_BOARD_BYTES);
        match JsonImporter::import_board(&read.text) {
            Ok(board) => {
                let (name, cards) = (board.name.clone(), board.cards.len());
                self.boards.push(board);
                self.active_board_idx = self.boards.len().saturating_sub(1);
                format!("{note}Opened {name} with {cards} card(s)")
            }
            Err(why) => format!("{note}Could not open {}: {why}", path.shown()),
        }
    }

    // `expect` rather than a fallback, because the invariant it asserts is
    // enforced at every site that could break it and there are only four:
    // `new()` seeds exactly one board with the index at 0, `switch_board`
    // assigns only an index it has already bounds-checked, `add_board` and
    // `read_board` assign `len() - 1` immediately after a push, and a boards
    // file is read only if it holds a board and names an open one it has.
    // Nothing removes a board, so `boards` is never empty. Returning
    // `Option<&Board>` instead would push that unreachable `None` into all
    // 40-odd call sites, where each would invent its own way of ignoring it —
    // which is strictly worse than one documented assertion here.
    #[allow(clippy::expect_used)]
    fn active_board(&self) -> &Board {
        self.boards
            .get(self.active_board_idx)
            .expect("active_board_idx must be valid")
    }

    #[allow(clippy::expect_used)]
    fn active_board_mut(&mut self) -> &mut Board {
        self.boards
            .get_mut(self.active_board_idx)
            .expect("active_board_idx must be valid")
    }

    /// A stamp for a change made now: the clock's reading in milliseconds
    /// since 1970 -- but always later than every stamp already given or read,
    /// so a later card sorts later even within one millisecond, or after the
    /// clock has been set back.
    fn next_timestamp(&mut self) -> u64 {
        self.last_stamp = clock_ms().max(self.last_stamp.saturating_add(1));
        self.last_stamp
    }

    /// Where the chosen card is in its column's list as drawn -- filtered,
    /// archived cards left out -- or `None` when no card is chosen or it is
    /// not in the chosen column.
    fn selected_row(&self) -> Option<usize> {
        let card = self.selected_card?;
        self.choosable_ids(self.selected_column)
            .iter()
            .position(|&id| id == card)
    }

    /// The cards of column `col` the arrows can land on: those drawn, which
    /// is none for a collapsed column.
    fn choosable_ids(&self, col: usize) -> Vec<Id> {
        if self
            .active_board()
            .columns
            .get(col)
            .is_some_and(|c| c.collapsed)
        {
            return Vec::new();
        }
        self.filtered_card_ids(col)
    }

    /// Choose the card at `row` of column `col` (the last, if the column is
    /// shorter), or none if it has no cards; and scroll it into sight.
    fn choose_in_column(&mut self, col: usize, row: usize) {
        self.selected_column = col;
        let cards = self.choosable_ids(col);
        self.selected_card = cards.get(row.min(cards.len().saturating_sub(1))).copied();
        self.reveal_selected();
    }

    /// Move the choice `delta` cards up or down the chosen column. With no
    /// card chosen there, the first press lands on the first card going down
    /// and the last going up. Answers whether the choice moved.
    fn step_card(&mut self, delta: isize) -> bool {
        let cards = self.choosable_ids(self.selected_column);
        let Some(last) = cards.len().checked_sub(1) else {
            return false;
        };
        let to = match (self.selected_row(), delta.is_negative()) {
            (None, true) => last,
            (None, false) => 0,
            (Some(row), true) => row.saturating_sub(delta.unsigned_abs()),
            (Some(row), false) => row.saturating_add(delta.unsigned_abs()).min(last),
        };
        if self.selected_row() == Some(to) {
            return false;
        }
        self.choose_in_column(self.selected_column, to);
        true
    }

    /// Scroll the board so the chosen card is drawn.
    ///
    /// Against the window the last frame was drawn in and the same room
    /// the renderer gives a column (`column_card_room`); the offset is one
    /// for every column, as the renderer reads it.
    fn reveal_selected(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if row < self.scroll_offset {
            self.scroll_offset = row;
            return;
        }
        let board = self.active_board();
        let heights: Vec<f32> = self
            .filtered_card_ids(self.selected_column)
            .iter()
            .filter_map(|id| board.cards.get(id))
            .map(|card| card_height(card) + CARD_GAP)
            .collect();
        let top = board_top(self);
        let room = column_card_room(top, (self.win_height - STATUS_H).max(top));
        // One card at a time, and never past the chosen one: the room holds
        // at least the chosen card, or nothing can make it fit.
        while self.scroll_offset < row {
            let shown = scroll_window::visible_variable(&heights, room, self.scroll_offset);
            if row < shown.end() {
                break;
            }
            self.scroll_offset = self.scroll_offset.saturating_add(1);
        }
    }

    fn add_card(&mut self, title: &str, col_idx: usize) -> Option<Id> {
        let ts = self.next_timestamp();
        let card = Card::new(title).with_created_at(ts);
        self.active_board_mut().add_card_to_column(card, col_idx)
    }

    /// A board with cards, for tests.
    ///
    /// `#[cfg(test)]` since 2026-09-15. It built a project board -- "Implement
    /// dark mode toggle" and the rest, with priorities, labels and created-at
    /// timestamps -- in the place the user's own work belongs.
    #[cfg(test)]
    fn create_sample_data(&mut self) {
        let board = self.active_board_mut();
        let bug_label = board.labels.first().map(|l| l.id);
        let feature_label = board.labels.get(1).map(|l| l.id);
        let enhance_label = board.labels.get(2).map(|l| l.id);

        // Backlog cards
        let mut c1 = Card::new("Implement dark mode toggle")
            .with_description("Add a toggle in settings to switch between light and dark themes")
            .with_priority(Priority::Medium)
            .with_created_at(100);
        if let Some(lid) = feature_label {
            c1 = c1.with_label(lid);
        }
        c1.add_checklist_item("Design toggle UI");
        c1.add_checklist_item("Implement theme switching logic");
        c1.add_checklist_item("Test with all widgets");
        board.add_card_to_column(c1, 0);

        let mut c2 = Card::new("Fix memory leak in allocator")
            .with_description("Page allocator leaks when failing mid-batch")
            .with_priority(Priority::Critical)
            .with_assignee("Alice")
            .with_due_date(SimpleDate::new(2026, 6, 15))
            .with_created_at(101);
        if let Some(lid) = bug_label {
            c2 = c2.with_label(lid);
        }
        board.add_card_to_column(c2, 0);

        // Todo cards
        let mut c3 = Card::new("Add keyboard navigation")
            .with_description("Support Tab/Shift+Tab and arrow keys for navigating cards")
            .with_priority(Priority::High)
            .with_created_at(102);
        if let Some(lid) = enhance_label {
            c3 = c3.with_label(lid);
        }
        board.add_card_to_column(c3, 1);

        // In Progress
        let c4 = Card::new("Implement drag and drop")
            .with_description("Allow moving cards between columns with mouse drag")
            .with_priority(Priority::High)
            .with_assignee("Bob")
            .with_created_at(103);
        board.add_card_to_column(c4, 2);

        // Review
        let mut c5 = Card::new("Update documentation")
            .with_description("Refresh API docs and add examples")
            .with_priority(Priority::Low)
            .with_assignee("Carol")
            .with_created_at(104);
        c5.add_comment("Carol", "First draft ready for review.", 200);
        board.add_card_to_column(c5, 3);

        // Done
        let c6 = Card::new("Set up CI pipeline")
            .with_description("Automated build and test on each push")
            .with_priority(Priority::Medium)
            .with_created_at(105);
        board.add_card_to_column(c6, 4);
    }

    /// After a filter changed what is drawn: a chosen card the filter now
    /// hides is chosen no longer -- the first card still drawn is, so the
    /// next key acts on something on screen. Nothing chosen stays so.
    fn refit_choice(&mut self) {
        if self.selected_card.is_some() && self.selected_row().is_none() {
            self.choose_in_column(self.selected_column, 0);
        } else {
            self.reveal_selected();
        }
    }

    /// After the card at `row` of the chosen column left it -- deleted or
    /// archived -- choose the one that took its place, so the next key acts
    /// on a card the user can see; or the one above, if it was the last.
    fn choose_neighbour(&mut self, row: Option<usize>) {
        self.selected_card = None;
        if let Some(row) = row {
            self.choose_in_column(self.selected_column, row);
        }
    }

    fn switch_board(&mut self, idx: usize) {
        if idx < self.boards.len() {
            self.active_board_idx = idx;
            self.selected_card = None;
            self.selected_column = 0;
            self.view = View::Board;
        }
    }

    fn add_board(&mut self, name: &str) {
        let board = Board::new(name);
        self.boards.push(board);
        self.active_board_idx = self.boards.len().saturating_sub(1);
        // Nothing chosen on the new board: a card id or column from the old
        // one would name something this board does not have.
        self.selected_card = None;
        self.selected_column = 0;
        self.scroll_offset = 0;
        // Add default columns
        let board = self.active_board_mut();
        board.add_column("Backlog");
        board.add_column("Todo");
        board.add_column("In Progress");
        board.add_column("Review");
        board.add_column("Done");
    }

    fn filtered_card_ids(&self, col_idx: usize) -> Vec<Id> {
        let board = self.active_board();
        if let Some(col) = board.columns.get(col_idx) {
            col.card_ids
                .iter()
                .filter(|cid| {
                    board
                        .cards
                        .get(cid)
                        .is_some_and(|card| !card.archived && self.filter.matches(card))
                })
                .copied()
                .collect()
        } else {
            Vec::new()
        }
    }
}

// =============================================================================
// Rendering helpers
// =============================================================================

/// Render the toolbar at the top.
fn render_toolbar(tree: &mut RenderTree, app: &KanbanApp, width: f32) {
    let toolbar_h: f32 = TOOLBAR_H;

    // Background
    app.palette.push_surface(
        tree,
        0.0,
        0.0,
        width,
        toolbar_h,
        0.0,
        Surface::Strip(Edge::Bottom),
    );

    // App title
    tree.push(RenderCommand::Text {
        x: 12.0,
        y: 10.0,
        text: "Kanban Board".to_string(),
        color: app.palette.ink(app.palette.blue),
        font_size: 16.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(200.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Board name
    let board_name = &app.active_board().name;
    tree.push(RenderCommand::Text {
        x: 160.0,
        y: 12.0,
        text: format!("/ {}", board_name),
        color: app.palette.subtext0,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(200.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Toolbar buttons
    let button_y: f32 = 7.0;
    let button_h: f32 = 26.0;
    let btn_radius = CornerRadii::all(4.0);

    // Board List button
    render_toolbar_button(
        tree,
        380.0,
        button_y,
        70.0,
        button_h,
        "Boards",
        app.palette.surface0,
        app.palette.text,
        btn_radius,
    );

    // Filter button
    let filter_color = if app.filter.is_active() {
        app.palette.blue
    } else {
        app.palette.surface0
    };
    render_toolbar_button(
        tree,
        460.0,
        button_y,
        60.0,
        button_h,
        "Filter",
        filter_color,
        app.palette.text,
        btn_radius,
    );

    // Stats button
    render_toolbar_button(
        tree,
        530.0,
        button_y,
        55.0,
        button_h,
        "Stats",
        app.palette.surface0,
        app.palette.text,
        btn_radius,
    );

    // Archive button
    render_toolbar_button(
        tree,
        595.0,
        button_y,
        65.0,
        button_h,
        "Archive",
        app.palette.surface0,
        app.palette.text,
        btn_radius,
    );

    // Export button
    render_toolbar_button(
        tree,
        670.0,
        button_y,
        60.0,
        button_h,
        "Export",
        app.palette.surface0,
        app.palette.text,
        btn_radius,
    );

    // New Card button
    render_toolbar_button(
        tree,
        width - 110.0,
        button_y,
        100.0,
        button_h,
        "+ New Card",
        app.palette.blue,
        app.palette.crust,
        btn_radius,
    );

    // Bottom border line
    tree.push(RenderCommand::Line {
        x1: 0.0,
        y1: toolbar_h,
        x2: width,
        y2: toolbar_h,
        color: app.palette.surface0,
        width: 1.0,
    });
}

// Toolbar button: rect (x,y,w,h) + label + bg/fg + radii. Same shape as the
// underlying render command; grouping would only add noise.
#[allow(clippy::too_many_arguments)]
fn render_toolbar_button(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
    bg: Color,
    fg: Color,
    radii: CornerRadii,
) {
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: bg,
        corner_radii: radii,
    });
    tree.push(RenderCommand::Text {
        x: x + 8.0,
        y: y + 5.0,
        text: label.to_string(),
        color: fg,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(w - 16.0),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Render the filter bar below the toolbar.
fn render_filter_bar(tree: &mut RenderTree, app: &KanbanApp, width: f32, y_offset: f32) -> f32 {
    if !app.show_filter_bar {
        return y_offset;
    }

    let bar_h: f32 = FILTER_BAR_H;

    app.palette
        .push_surface(tree, 0.0, y_offset, width, bar_h, 0.0, Surface::Card);

    // Search icon area
    tree.push(RenderCommand::Text {
        x: 12.0,
        y: y_offset + 9.0,
        text: "Search:".to_string(),
        color: app.palette.subtext0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Search input
    let search_text = if app.filter.search_text.is_empty() {
        "type to search..."
    } else {
        &app.filter.search_text
    };
    let search_color = if app.filter.search_text.is_empty() {
        app.palette.overlay0
    } else {
        app.palette.text
    };
    app.palette
        .push_surface(tree, 70.0, y_offset + 5.0, 180.0, 26.0, 3.0, Surface::Card);
    tree.push(RenderCommand::Text {
        x: 78.0,
        y: y_offset + 9.0,
        text: search_text.to_string(),
        color: search_color,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(164.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Priority filter
    tree.push(RenderCommand::Text {
        x: 270.0,
        y: y_offset + 9.0,
        text: "Priority:".to_string(),
        color: app.palette.subtext0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    let priority_label = app.filter.priority_filter.map_or("All", |p| p.label());
    app.palette
        .push_surface(tree, 332.0, y_offset + 5.0, 70.0, 26.0, 3.0, Surface::Card);
    tree.push(RenderCommand::Text {
        x: 340.0,
        y: y_offset + 9.0,
        text: priority_label.to_string(),
        color: app.palette.text,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(54.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Assignee and label, which Ctrl+U and Ctrl+L set. The filter could hold
    // both and nothing could set either.
    let assignee = if app.filter.assignee_filter.is_empty() {
        "Anyone".to_string()
    } else {
        app.filter.assignee_filter.clone()
    };
    let label = app
        .filter
        .label_filter
        .and_then(|id| app.active_board().get_label_by_id(id))
        .map_or_else(|| "Any".to_string(), |l| l.name.clone());
    tree.push(RenderCommand::Text {
        x: 420.0,
        y: y_offset + 9.0,
        text: format!("Assignee: {assignee}   Label: {label}"),
        color: app.palette.subtext0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((width - 500.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });

    // Clear button
    if app.filter.is_active() {
        render_toolbar_button(
            tree,
            width - 70.0,
            y_offset + 5.0,
            60.0,
            26.0,
            "Clear",
            app.palette.red,
            app.palette.text,
            CornerRadii::all(3.0),
        );
    }

    y_offset + bar_h
}

// The vertical pieces of a card, in draw order. `render_card` steps through
// these in sequence and `card_height` sums them, so the height a column
// *reserves* for a card and the height it *draws* cannot disagree — which is
// what lets a column work out which cards fit before drawing any of them.
/// Blank space above the priority bar.
/// What the status line says on a board with no cards, before where the
/// boards are kept.
///
/// These were two lines drawn at the top of the window -- on every board, not
/// only an empty one -- under the toolbar, which painted over them: they were
/// never seen. The second said nothing was kept, which stopped being true
/// when the boards were (2026-09-25).
const NO_CARDS_YET: &str = "No cards yet -- N adds one.";

/// How tall the status line along the bottom of the window is.
const STATUS_H: f32 = 22.0;

const CARD_TOP_PAD: f32 = 12.0;
/// The coloured priority stripe across the top of a card.
const CARD_PRIORITY_BAR_H: f32 = 3.0;
/// The title line, always present.
const CARD_TITLE_H: f32 = 18.0;
/// The row of label chips, when the card has any.
const CARD_LABELS_H: f32 = 20.0;
/// The assignee/due-date line, when either is set.
const CARD_META_H: f32 = 14.0;
/// The `[done/total]` checklist line, when the card has a checklist.
const CARD_CHECKLIST_H: f32 = 14.0;
/// The comment-count line, when the card has comments.
const CARD_COMMENTS_H: f32 = 14.0;
/// Blank space below the last line.
const CARD_BOTTOM_PAD: f32 = 8.0;

/// Space kept at the bottom of every column for the "+N more" line, reserved
/// whether or not the column is actually hiding anything.
const COLUMN_FOOTER_H: f32 = 14.0;

/// Cards a Page Up / Page Down moves the board by.
///
/// A fixed count rather than "one screenful": cards are variable height, so a
/// screenful differs per column, and a key that moved each column by a
/// different amount would tear a board that scrolls as one.
const BOARD_PAGE_STEP: isize = 5;

/// The assignee/due-date pieces of a card's metadata line, in display order.
///
/// Shared by [`card_height`] and [`render_card`] so that "is there a metadata
/// line?" is answered the same way by the measurement and by the drawing.
fn card_meta_parts(card: &Card) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    if !card.assignee.is_empty() {
        parts.push(card.assignee.clone());
    }
    if let Some(date) = card.due_date {
        parts.push(date.display());
    }
    parts
}

/// How tall `card` will be when drawn, without drawing it.
///
/// A column needs this *before* it draws, to decide how many cards fit; the
/// alternative — measuring by rendering into a scratch tree and throwing it
/// away — would make the answer depend on the drawing succeeding.
fn card_height(card: &Card) -> f32 {
    let mut h = CARD_TOP_PAD + CARD_PRIORITY_BAR_H + CARD_TITLE_H;
    if !card.labels.is_empty() {
        h += CARD_LABELS_H;
    }
    if !card_meta_parts(card).is_empty() {
        h += CARD_META_H;
    }
    if card.checklist_progress().1 > 0 {
        h += CARD_CHECKLIST_H;
    }
    if !card.comments.is_empty() {
        h += CARD_COMMENTS_H;
    }
    h + CARD_BOTTOM_PAD
}

/// Render a single card.
fn render_card(
    tree: &mut RenderTree,
    pal: &Palette,
    card: &Card,
    board: &Board,
    x: f32,
    y: f32,
    card_width: f32,
    selected: bool,
) -> f32 {
    let padding: f32 = 8.0;
    let mut card_h: f32 = CARD_TOP_PAD;

    // Priority indicator bar at top
    let priority_bar_h: f32 = CARD_PRIORITY_BAR_H;
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: card_width,
        height: priority_bar_h,
        color: card.priority.color(pal),
        corner_radii: CornerRadii {
            top_left: 6.0,
            top_right: 6.0,
            bottom_left: 0.0,
            bottom_right: 0.0,
        },
    });
    card_h += priority_bar_h;

    // Card background
    let bg_color = if selected { pal.surface1 } else { pal.surface0 };

    // Title
    tree.push(RenderCommand::Text {
        x: x + padding,
        y: y + card_h,
        text: card.title.clone(),
        color: pal.text,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(card_width - padding * 2.0),
        overflow: TextOverflow::Ellipsis,
    });
    card_h += CARD_TITLE_H;

    // Labels row
    if !card.labels.is_empty() {
        let mut label_x = x + padding;
        for label_id in &card.labels {
            if let Some(label) = board.get_label_by_id(*label_id) {
                let lw = (label.name.len() as f32) * 7.0 + 12.0;
                tree.push(RenderCommand::FillRect {
                    x: label_x,
                    y: y + card_h,
                    width: lw,
                    height: 16.0,
                    color: label.color,
                    corner_radii: CornerRadii::all(3.0),
                });
                tree.push(RenderCommand::Text {
                    x: label_x + 6.0,
                    y: y + card_h + 2.0,
                    text: label.name.clone(),
                    color: pal.crust,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(lw - 10.0),
                    overflow: TextOverflow::Ellipsis,
                });
                label_x += lw + 4.0;
            }
        }
        card_h += CARD_LABELS_H;
    }

    // Metadata row: assignee, due date
    let meta_parts = card_meta_parts(card);

    if !meta_parts.is_empty() {
        tree.push(RenderCommand::Text {
            x: x + padding,
            y: y + card_h,
            text: meta_parts.join(" | "),
            color: pal.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(card_width - padding * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
        card_h += CARD_META_H;
    }

    // Checklist progress
    let (done, total) = card.checklist_progress();
    if total > 0 {
        let progress_text = format!("[{}/{}]", done, total);
        tree.push(RenderCommand::Text {
            x: x + padding,
            y: y + card_h,
            text: progress_text,
            color: if done == total {
                pal.ink(pal.green)
            } else {
                pal.ink(pal.yellow)
            },
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        card_h += CARD_CHECKLIST_H;
    }

    // Comment count
    if !card.comments.is_empty() {
        let comment_text = format!("{} comment(s)", card.comments.len());
        tree.push(RenderCommand::Text {
            x: x + padding,
            y: y + card_h,
            text: comment_text,
            color: pal.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        card_h += CARD_COMMENTS_H;
    }

    card_h += CARD_BOTTOM_PAD;

    // Now render the card background (behind text via ordering - we render bg first)
    // We need to insert this before the text commands, so we render in a separate pass
    // For simplicity, the card bg is rendered here and text overwrites
    // In practice, insert bg commands at the start. Here we just draw it.
    // The actual card background was drawn early.

    // Full card background (draw under text)
    tree.push(RenderCommand::FillRect {
        x,
        y: y + priority_bar_h,
        width: card_width,
        height: card_h - priority_bar_h,
        color: bg_color,
        corner_radii: CornerRadii {
            top_left: 0.0,
            top_right: 0.0,
            bottom_left: 6.0,
            bottom_right: 6.0,
        },
    });

    if selected {
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: card_width,
            height: card_h,
            color: pal.blue,
            line_width: 2.0,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    // The two must agree, or a column reserves one height and draws another —
    // the card after this one would then overlap it or float above it, and the
    // last card in a column would cross the bottom edge. Cheap to check, and it
    // fires in the tests rather than in front of a user.
    debug_assert!(
        (card_h - card_height(card)).abs() < 0.001,
        "card drawn {card_h}px tall but measured {}px",
        card_height(card)
    );
    card_h
}

/// Render a column header.
fn render_column_header(
    tree: &mut RenderTree,
    pal: &Palette,
    col: &Column,
    board: &Board,
    x: f32,
    y: f32,
    col_width: f32,
) {
    let header_h: f32 = 36.0;

    // Header background
    pal.push_surface_radii(
        tree,
        x,
        y,
        col_width,
        header_h,
        CornerRadii {
            top_left: 6.0,
            top_right: 6.0,
            bottom_left: 0.0,
            bottom_right: 0.0,
        },
        Surface::Card,
    );

    // Column name
    tree.push(RenderCommand::Text {
        x: x + 10.0,
        y: y + 9.0,
        text: col.name.clone(),
        color: pal.text,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(col_width - 80.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Card count badge
    let active_count = col.active_card_count(&board.cards);
    let count_text = format!("{}", active_count);

    let badge_color = if col.is_over_wip_limit() {
        pal.red
    } else {
        pal.surface1
    };

    let badge_x = x + col_width - 50.0;
    tree.push(RenderCommand::FillRect {
        x: badge_x,
        y: y + 8.0,
        width: 22.0,
        height: 20.0,
        color: badge_color,
        corner_radii: CornerRadii::all(10.0),
    });
    tree.push(RenderCommand::Text {
        x: badge_x + 6.0,
        y: y + 11.0,
        text: count_text,
        color: pal.text,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // WIP limit indicator
    if let Some(limit) = col.wip_limit {
        let wip_text = format!("/{}", limit);
        tree.push(RenderCommand::Text {
            x: badge_x + 24.0,
            y: y + 11.0,
            text: wip_text,
            color: if col.is_over_wip_limit() {
                pal.ink(pal.red)
            } else {
                pal.subtext0
            },
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

/// Height of the toolbar across the top.
const TOOLBAR_H: f32 = 40.0;
/// Height of the filter bar, when it is open.
const FILTER_BAR_H: f32 = 36.0;
/// Gap between the top of the board and the top of its columns.
const COLUMN_TOP_GAP: f32 = 8.0;
/// Height of a column's header, above its first card.
const COLUMN_HEADER_H: f32 = 36.0;
/// Gap between two cards in a column, and above the first.
const CARD_GAP: f32 = 6.0;

/// Where the board starts: under the toolbar, and under the filter bar when
/// it is open.
fn board_top(app: &KanbanApp) -> f32 {
    if app.show_filter_bar {
        TOOLBAR_H + FILTER_BAR_H
    } else {
        TOOLBAR_H
    }
}

/// The height a column has for its cards, for a board drawn from `y_start`
/// down to `bottom`.
///
/// One function for the renderer and the keyboard: a card the arrows scroll
/// into sight is a card the renderer then draws.
fn column_card_room(y_start: f32, bottom: f32) -> f32 {
    let card_y = y_start + COLUMN_TOP_GAP + COLUMN_HEADER_H + CARD_GAP;
    // The "+N more" line's space is reserved whether or not it is needed,
    // so the number of cards that fit does not depend on how many fit.
    (bottom - 8.0) - card_y - COLUMN_FOOTER_H
}

/// Width of a collapsed column: its name, its count, no cards.
const COLLAPSED_W: f32 = 120.0;

/// Where each column is across the board: its left edge and width.
///
/// A collapsed column takes [`COLLAPSED_W`]; the others share what is left,
/// at least 180 each.
fn column_spans(columns: &[Column], width: f32) -> Vec<(f32, f32)> {
    let col_gap: f32 = 8.0;
    let col_margin: f32 = 8.0;
    let count = columns.len();
    let collapsed = columns.iter().filter(|c| c.collapsed).count();
    let open = count.saturating_sub(collapsed);
    #[allow(
        clippy::cast_precision_loss,
        reason = "a board's column count, far inside f32's exact range"
    )]
    let (gaps, narrow, open_n) = (
        col_gap * count.saturating_sub(1) as f32,
        COLLAPSED_W * collapsed as f32,
        open.max(1) as f32,
    );
    let open_w = ((width - col_margin * 2.0 - gaps - narrow) / open_n).max(180.0);
    let mut x = col_margin;
    columns
        .iter()
        .map(|column| {
            let w = if column.collapsed {
                COLLAPSED_W
            } else {
                open_w
            };
            let span = (x, w);
            x += w + col_gap;
            span
        })
        .collect()
}

/// Render the board view with columns of cards.
fn render_board_view(
    tree: &mut RenderTree,
    app: &KanbanApp,
    width: f32,
    height: f32,
    y_start: f32,
) {
    let board = app.active_board();
    let col_count = board.columns.len();
    if col_count == 0 {
        tree.push(RenderCommand::Text {
            x: width / 2.0 - 80.0,
            y: y_start + 50.0,
            text: "No columns yet. Shift+C adds one.".to_string(),
            color: app.palette.subtext0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        return;
    }

    let spans = column_spans(&board.columns, width);
    for ((ci, col), &(col_x, col_width)) in board.columns.iter().enumerate().zip(&spans) {
        let col_y = y_start + COLUMN_TOP_GAP;

        // Column background
        tree.push(RenderCommand::FillRect {
            x: col_x,
            y: col_y,
            width: col_width,
            height: height - col_y - 8.0,
            color: app.palette.base,
            corner_radii: CornerRadii::all(6.0),
        });

        // WIP limit warning overlay
        if col.is_over_wip_limit() {
            tree.push(RenderCommand::StrokeRect {
                x: col_x,
                y: col_y,
                width: col_width,
                height: height - col_y - 8.0,
                color: Color::rgba(243, 139, 168, 60),
                line_width: 2.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // The chosen column, marked: R, Z, Shift+T, Shift+Delete and N all
        // act on it, and nothing showed which one it was.
        if ci == app.selected_column && app.view == View::Board {
            tree.push(RenderCommand::StrokeRect {
                x: col_x,
                y: col_y,
                width: col_width,
                height: height - col_y - 8.0,
                color: app.palette.accent,
                line_width: 2.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Column header
        render_column_header(tree, &app.palette, col, board, col_x, col_y, col_width);

        // A collapsed column is its header and a count, no cards.
        if col.collapsed {
            let hidden = app.filtered_card_ids(ci).len();
            tree.push(RenderCommand::Text {
                x: col_x + 10.0,
                y: col_y + COLUMN_HEADER_H + CARD_GAP,
                text: format!(
                    "{hidden} card{} hidden -- Z shows them",
                    if hidden == 1 { "" } else { "s" }
                ),
                color: app.palette.subtext0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((col_width - 16.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            continue;
        }

        // Cards
        let card_gap = CARD_GAP;
        let card_margin: f32 = 6.0;
        let card_width = col_width - card_margin * 2.0;
        let mut card_y = col_y + COLUMN_HEADER_H + card_gap;

        // Only the cards this column has room for. Without this the loop drew
        // every card in the column, so a column with more cards than fit ran
        // off the bottom of the window and over whatever was beneath it — and
        // since the list was cut from the top by the screen edge rather than by
        // a scroll position, the cards past the fold could not be reached at
        // all. `scroll_offset` is a *row* index, shared by every column and
        // clamped separately against each one, so a short column sits at its
        // last page while a long one is still scrolling; see
        // design-decisions.md §471.
        let filtered_ids: Vec<Id> = app
            .filtered_card_ids(ci)
            .into_iter()
            .filter(|id| board.cards.contains_key(id))
            .collect();
        // Each row reserves the gap that follows it. That over-reserves by one
        // gap for the last visible card, which errs towards leaving a strip of
        // background rather than towards drawing over the column's edge.
        let heights: Vec<f32> = filtered_ids
            .iter()
            .filter_map(|id| board.cards.get(id))
            .map(|card| card_height(card) + card_gap)
            .collect();
        let room = column_card_room(y_start, height);
        let window = scroll_window::visible_variable(&heights, room, app.scroll_offset);

        for card_id in filtered_ids
            .get(window.start..window.end())
            .unwrap_or_default()
        {
            if let Some(card) = board.cards.get(card_id) {
                let is_selected = app.selected_card == Some(*card_id);
                let ch = render_card(
                    tree,
                    &app.palette,
                    card,
                    board,
                    col_x + card_margin,
                    card_y,
                    card_width,
                    is_selected,
                );
                card_y += ch + card_gap;
            }
        }

        // A column that is hiding cards says so, in the space the window
        // deliberately did not fill.
        let hidden = filtered_ids.len().saturating_sub(window.count);
        if hidden > 0 {
            tree.push(RenderCommand::Text {
                x: col_x + card_margin,
                y: card_y,
                text: format!("+{hidden} more"),
                color: app.palette.subtext0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(card_width),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

/// Vertical room the card description keeps even when it is a single line, so
/// that adding wrapping does not shift the whole pane for the common case.
const DESC_MIN_HEIGHT: f32 = 30.0;
/// Horizontal padding inside a comment card, and its bottom padding.
const COMMENT_PAD: f32 = 8.0;
/// Offset of a comment's body below the top of its card, leaving room for the
/// author line drawn at +4.
const COMMENT_BODY_TOP: f32 = 18.0;
/// The height a comment card keeps even when its body is a single short line.
const COMMENT_MIN_HEIGHT: f32 = 36.0;
/// Vertical gap between consecutive comment cards.
const COMMENT_GAP: f32 = 6.0;

/// Padding between the modal's edge and its content, on all four sides.
const DETAIL_PAD: f32 = 16.0;
/// Height of the modal's fixed title row, which does not scroll with the body.
const DETAIL_TITLE_ROW: f32 = 30.0;
/// Pixels one Up/Down moves the card-detail body.
///
/// A line, not a card: the body has no rows to count. Its sections are a
/// wrapped description, a checklist, and comment cards whose heights depend on
/// how much prose is in them, so there is no index that could name a position
/// part way down one — which is why [`KanbanApp::detail_scroll`] is in pixels
/// while [`KanbanApp::scroll_offset`] is in cards.
const DETAIL_LINE_STEP: f32 = 18.0;
/// Pixels one PageUp/PageDown moves the card-detail body.
const DETAIL_PAGE_STEP: f32 = 300.0;

/// Where the card-detail modal sits, and where its scrolling body sits inside
/// it, for a window of a given size.
///
/// This existed as four `let`s inside the renderer, which was fine while the
/// renderer was the only thing that needed to know. Bounding the scroll offset
/// needs the body's height as well, and a second copy of `600.0.min(width -
/// 40.0)` is the layout-divergence bug in `known-issues.md` waiting to happen.
#[derive(Clone, Copy, Debug)]
struct DetailModal {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl DetailModal {
    fn for_window(width: f32, height: f32) -> Self {
        // `.max(0.0)`: a window smaller than the padding would otherwise give a
        // negative size, and a negative height would make `max_detail_scroll`
        // claim the body scrolls further than its content is long.
        let w = 600.0_f32.min(width - 40.0).max(0.0);
        let h = 500.0_f32.min(height - 60.0).max(0.0);
        Self {
            x: (width - w) / 2.0,
            y: (height - h) / 2.0,
            w,
            h,
        }
    }

    fn content_x(&self) -> f32 {
        self.x + 20.0
    }

    fn content_w(&self) -> f32 {
        (self.w - 40.0).max(0.0)
    }

    /// Top edge of the scrolling body: below the fixed title row.
    fn body_top(&self) -> f32 {
        self.y + DETAIL_PAD + DETAIL_TITLE_ROW
    }

    /// Height of the scrolling body: what is left of the modal below the title
    /// row, less the line of keys and the bottom padding.
    fn body_height(&self) -> f32 {
        (self.h - DETAIL_PAD - DETAIL_TITLE_ROW - DETAIL_FOOTER_H - DETAIL_PAD).max(0.0)
    }

    /// Top of the fixed line naming the card's keys, under the body.
    fn footer_top(&self) -> f32 {
        self.body_top() + self.body_height() + 4.0
    }
}

/// Height of the line of keys along the bottom of the open card.
const DETAIL_FOOTER_H: f32 = 20.0;

/// The open card's keys, as its footer names them.
const DETAIL_KEYS: &str =
    "E title · D description · C comment · L checklist item · Tab, Space tick · Esc close";

/// Height of one checklist row in the open card.
const CHECK_ROW_H: f32 = 18.0;

/// How far the card-detail body can scroll before its last line reaches the
/// bottom of the modal.
///
/// Measured by drawing the body into a list that is thrown away — see
/// [`guitk::render::content_bottom`]. A `measure_card_detail` written beside
/// `render_card_detail_body` would be a second derivation of the same layout,
/// which is the class of bug tracked as
/// `C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY`; measuring the
/// renderer's own output cannot drift from it.
fn max_detail_scroll(app: &KanbanApp, width: f32, height: f32) -> f32 {
    let Some(card_id) = app.selected_card else {
        return 0.0;
    };
    let board = app.active_board();
    let Some(card) = board.cards.get(&card_id) else {
        return 0.0;
    };
    let modal = DetailModal::for_window(width, height);
    let mut scratch = RenderTree::new();
    render_card_detail_body(
        &mut scratch,
        &app.palette,
        board,
        card,
        0.0,
        0.0,
        modal.content_w(),
        None,
    );
    let content = content_bottom(&scratch.commands).unwrap_or(0.0).max(0.0);
    (content - modal.body_height()).max(0.0)
}

/// Render card detail view.
fn render_card_detail(tree: &mut RenderTree, app: &KanbanApp, width: f32, height: f32) {
    let card_id = match app.selected_card {
        Some(id) => id,
        None => return,
    };
    let board = app.active_board();
    let card = match board.cards.get(&card_id) {
        Some(c) => c,
        None => return,
    };

    // Overlay background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width,
        height,
        color: Color::rgba(0, 0, 0, 180),
        corner_radii: CornerRadii::ZERO,
    });

    // Modal panel
    let modal = DetailModal::for_window(width, height);
    let (modal_x, modal_y, modal_w, modal_h) = (modal.x, modal.y, modal.w, modal.h);

    // Shadow
    tree.push(RenderCommand::BoxShadow {
        x: modal_x,
        y: modal_y,
        width: modal_w,
        height: modal_h,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 20.0,
        spread: 0.0,
        color: Color::rgba(0, 0, 0, 100),
        corner_radii: CornerRadii::all(8.0),
    });

    // Modal background
    tree.push(RenderCommand::FillRect {
        x: modal_x,
        y: modal_y,
        width: modal_w,
        height: modal_h,
        color: app.palette.base,
        corner_radii: CornerRadii::all(8.0),
    });

    // Modal border
    tree.push(RenderCommand::StrokeRect {
        x: modal_x,
        y: modal_y,
        width: modal_w,
        height: modal_h,
        color: app.palette.surface1,
        line_width: 1.0,
        corner_radii: CornerRadii::all(8.0),
    });

    let content_x = modal.content_x();
    let content_w = modal.content_w();

    // Title and close button do not scroll: a modal whose heading slid away
    // would leave the user with no way to tell which card they were reading.
    tree.push(RenderCommand::Text {
        x: content_x,
        y: modal_y + DETAIL_PAD,
        text: card.title.clone(),
        color: app.palette.text,
        font_size: 18.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(content_w - 60.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Close button
    render_toolbar_button(
        tree,
        modal_x + modal_w - 40.0,
        modal_y + DETAIL_PAD,
        28.0,
        24.0,
        "X",
        app.palette.surface0,
        app.palette.red,
        CornerRadii::all(4.0),
    );

    // The body is clipped to the modal, so what overflows is hidden rather than
    // drawn across the rest of the desktop. That is also why it has to be
    // scrollable: before this, a card with a long description and a few
    // comments drew its comments over the window and there was no key that
    // could reach them.
    let scroll = app
        .detail_scroll
        .clamp(0.0, max_detail_scroll(app, width, height));
    tree.push(RenderCommand::PushClip {
        x: modal_x,
        y: modal.body_top(),
        width: modal_w,
        height: modal.body_height(),
    });
    render_card_detail_body(
        tree,
        &app.palette,
        board,
        card,
        content_x,
        modal.body_top() - scroll,
        content_w,
        app.checklist_focus,
    );
    tree.push(RenderCommand::PopClip);

    // The card's keys, which nothing on screen named: E, D, C and L did
    // nothing at all until 2026-09-27, and the checklist could not be ticked.
    tree.push(RenderCommand::Text {
        x: content_x,
        y: modal.footer_top(),
        text: DETAIL_KEYS.to_string(),
        color: app.palette.subtext0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(content_w),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Where each checklist row of `card` starts, below the top of the open
/// card's body -- read off the renderer's own drawing, so the rows Tab
/// scrolls to are the rows it draws.
fn checklist_rows(app: &KanbanApp, board: &Board, card: &Card) -> Vec<f32> {
    let modal = DetailModal::for_window(app.win_width, app.win_height);
    let mut scratch = RenderTree::new();
    render_card_detail_body(
        &mut scratch,
        &app.palette,
        board,
        card,
        0.0,
        0.0,
        modal.content_w(),
        None,
    )
}

/// Draw everything below the card-detail modal's title row, with the top of the
/// first item at `y`.
///
/// Split out from [`render_card_detail`] so [`max_detail_scroll`] can measure it
/// by drawing it into a list it throws away. Takes the board and card rather
/// than the app because the measuring caller has already resolved them, and
/// resolving them twice is one more thing that could resolve differently.
///
/// Answers where each checklist row starts, relative to `y`; `focus` is the
/// row drawn as the one Space ticks.
#[allow(
    clippy::too_many_arguments,
    reason = "the body's place and width, what it shows, and the one focus it marks"
)]
fn render_card_detail_body(
    tree: &mut RenderTree,
    pal: &Palette,
    board: &Board,
    card: &Card,
    content_x: f32,
    y: f32,
    content_w: f32,
    focus: Option<usize>,
) -> Vec<f32> {
    let mut rows = Vec::new();
    let mut cy = y;

    // Priority badge
    tree.push(RenderCommand::FillRect {
        x: content_x,
        y: cy,
        width: 80.0,
        height: 22.0,
        color: card.priority.color(pal),
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: content_x + 8.0,
        y: cy + 4.0,
        text: card.priority.label().to_string(),
        color: pal.crust,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Assignee
    if !card.assignee.is_empty() {
        tree.push(RenderCommand::Text {
            x: content_x + 90.0,
            y: cy + 4.0,
            text: format!("Assigned: {}", card.assignee),
            color: pal.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // Due date
    if let Some(date) = card.due_date {
        tree.push(RenderCommand::Text {
            x: content_x + 280.0,
            y: cy + 4.0,
            text: format!("Due: {}", date.display()),
            color: pal.ink(pal.yellow),
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    cy += 30.0;

    // Labels
    if !card.labels.is_empty() {
        tree.push(RenderCommand::Text {
            x: content_x,
            y: cy,
            text: "Labels:".to_string(),
            color: pal.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        let mut label_x = content_x + 60.0;
        for label_id in &card.labels {
            if let Some(label) = board.get_label_by_id(*label_id) {
                let lw = (label.name.len() as f32) * 7.5 + 14.0;
                tree.push(RenderCommand::FillRect {
                    x: label_x,
                    y: cy - 2.0,
                    width: lw,
                    height: 20.0,
                    color: label.color,
                    corner_radii: CornerRadii::all(4.0),
                });
                tree.push(RenderCommand::Text {
                    x: label_x + 7.0,
                    y: cy + 1.0,
                    text: label.name.clone(),
                    color: pal.crust,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(lw - 12.0),
                    overflow: TextOverflow::Ellipsis,
                });
                label_x += lw + 6.0;
            }
        }
        cy += 26.0;
    }

    // Separator
    tree.push(RenderCommand::Line {
        x1: content_x,
        y1: cy,
        x2: content_x + content_w,
        y2: cy,
        color: pal.surface0,
        width: 1.0,
    });
    cy += 10.0;

    // Description
    tree.push(RenderCommand::Text {
        x: content_x,
        y: cy,
        text: "Description".to_string(),
        color: pal.text,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    cy += 18.0;

    let desc_text = if card.description.is_empty() {
        "No description provided."
    } else {
        &card.description
    };
    // A description is prose the user is meant to read in full, and everything
    // below it sits on this running cursor -- so the height has to come from
    // the lines actually drawn. `max_width` on a Text command clips to one
    // line, it does not wrap (known-issues.md TD-GUI-TEXT-COMMAND-DOES-NOT-WRAP).
    let desc_used = text::Paragraph::new(
        desc_text,
        if card.description.is_empty() {
            pal.overlay0
        } else {
            pal.subtext0
        },
    )
    .at(content_x, cy, content_w)
    .font(12.0, FontWeightHint::Regular)
    .draw(tree);
    // Keep the original 30 px allowance for the common one-line case so the
    // rest of the pane does not shift; grow only when it genuinely wraps.
    cy += desc_used.max(DESC_MIN_HEIGHT);

    // Checklist
    if !card.checklist.is_empty() {
        let (done, total) = card.checklist_progress();
        tree.push(RenderCommand::Text {
            x: content_x,
            y: cy,
            text: format!("Checklist ({}/{})", done, total),
            color: pal.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 20.0;

        // Progress bar
        let bar_w = content_w.min(300.0);
        tree.push(RenderCommand::FillRect {
            x: content_x,
            y: cy,
            width: bar_w,
            height: 6.0,
            color: pal.surface0,
            corner_radii: CornerRadii::all(3.0),
        });
        if total > 0 {
            let progress_w = bar_w * (done as f32 / total as f32);
            tree.push(RenderCommand::FillRect {
                x: content_x,
                y: cy,
                width: progress_w,
                height: 6.0,
                color: pal.green,
                corner_radii: CornerRadii::all(3.0),
            });
        }
        cy += 12.0;

        for (index, item) in card.checklist.iter().enumerate() {
            rows.push(cy - y);
            if focus == Some(index) {
                tree.push(RenderCommand::FillRect {
                    x: content_x,
                    y: cy - 2.0,
                    width: content_w,
                    height: CHECK_ROW_H,
                    color: pal.surface1,
                    corner_radii: CornerRadii::all(3.0),
                });
            }
            let check_mark = if item.done { "[x]" } else { "[ ]" };
            let item_color = if item.done { pal.overlay0 } else { pal.text };
            tree.push(RenderCommand::Text {
                x: content_x + 4.0,
                y: cy,
                text: format!("{} {}", check_mark, item.text),
                color: item_color,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 8.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += CHECK_ROW_H;
        }
        cy += 8.0;
    }

    // Comments
    if !card.comments.is_empty() {
        tree.push(RenderCommand::Text {
            x: content_x,
            y: cy,
            text: format!("Comments ({})", card.comments.len()),
            color: pal.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 20.0;

        for comment in &card.comments {
            // A comment body is prose, and its card is drawn *before* the text
            // it contains -- so the body is measured first and the card sized
            // from that measurement, rather than the two being computed
            // separately and left to disagree.
            let body = text::Paragraph::new(&comment.text, pal.subtext0)
                .at(
                    content_x + COMMENT_PAD,
                    cy + COMMENT_BODY_TOP,
                    content_w - COMMENT_PAD * 2.0,
                )
                .font(11.0, FontWeightHint::Regular);
            let card_h = (COMMENT_BODY_TOP + body.height() + COMMENT_PAD).max(COMMENT_MIN_HEIGHT);
            pal.push_surface(tree, content_x, cy, content_w, card_h, 4.0, Surface::Card);
            tree.push(RenderCommand::Text {
                x: content_x + COMMENT_PAD,
                y: cy + 4.0,
                text: comment.author.clone(),
                color: pal.ink(pal.blue),
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            body.draw(tree);
            cy += card_h + COMMENT_GAP;
        }
    }
    rows
}

/// Render the archive view.
fn render_archive_view(tree: &mut RenderTree, app: &KanbanApp, width: f32, y_start: f32) {
    let board = app.active_board();

    tree.push(RenderCommand::Text {
        x: 20.0,
        y: y_start + 16.0,
        text: "Archived Cards".to_string(),
        color: app.palette.text,
        font_size: 16.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    if board.archived_card_ids.is_empty() {
        tree.push(RenderCommand::Text {
            x: 20.0,
            y: y_start + 50.0,
            text: "No archived cards.".to_string(),
            color: app.palette.subtext0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        return;
    }

    tree.push(RenderCommand::Text {
        x: 180.0,
        y: y_start + 19.0,
        text: "Up / Down choose \u{00b7} Enter restores to its column \u{00b7} Ctrl+D deletes"
            .to_string(),
        color: app.palette.subtext0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((width - 200.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });

    let mut cy = y_start + 44.0;
    for (row, card_id) in board.archived_card_ids.iter().enumerate() {
        if let Some(card) = board.cards.get(card_id) {
            app.palette
                .push_surface(tree, 20.0, cy, width - 40.0, 40.0, 4.0, Surface::Card);
            if row == app.archive_cursor {
                tree.push(RenderCommand::StrokeRect {
                    x: 20.0,
                    y: cy,
                    width: width - 40.0,
                    height: 40.0,
                    color: app.palette.accent,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            tree.push(RenderCommand::Text {
                x: 32.0,
                y: cy + 6.0,
                text: card.title.clone(),
                color: app.palette.text,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 200.0),
                overflow: TextOverflow::Ellipsis,
            });
            tree.push(RenderCommand::Text {
                x: 32.0,
                y: cy + 22.0,
                text: format!("Priority: {} | {}", card.priority.label(), card.assignee),
                color: app.palette.subtext0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            // Where Enter would put it back.
            let to = card
                .archived_from
                .and_then(|id| board.columns.iter().find(|c| c.id == id))
                .or_else(|| board.columns.first())
                .map_or_else(String::new, |c| format!("back to {}", c.name));
            tree.push(RenderCommand::Text {
                x: width - 200.0,
                y: cy + 13.0,
                text: to,
                color: app.palette.subtext0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(170.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 48.0;
        }
    }
}

/// Render the statistics view.
fn render_stats_view(tree: &mut RenderTree, app: &KanbanApp, _width: f32, y_start: f32) {
    let board = app.active_board();
    let stats = board.column_stats();

    tree.push(RenderCommand::Text {
        x: 20.0,
        y: y_start + 16.0,
        text: "Board Statistics".to_string(),
        color: app.palette.text,
        font_size: 16.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    let mut cy = y_start + 48.0;

    // Completion rate
    let rate = board.completion_rate();
    tree.push(RenderCommand::Text {
        x: 20.0,
        y: cy,
        text: format!("Completion Rate: {:.1}%", rate),
        color: app.palette.ink(app.palette.green),
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    cy += 28.0;

    // Completion bar
    let bar_w: f32 = 300.0;
    tree.push(RenderCommand::FillRect {
        x: 20.0,
        y: cy,
        width: bar_w,
        height: 12.0,
        color: app.palette.surface0,
        corner_radii: CornerRadii::all(6.0),
    });
    tree.push(RenderCommand::FillRect {
        x: 20.0,
        y: cy,
        width: bar_w * (rate / 100.0),
        height: 12.0,
        color: app.palette.green,
        corner_radii: CornerRadii::all(6.0),
    });
    cy += 30.0;

    // Total cards
    tree.push(RenderCommand::Text {
        x: 20.0,
        y: cy,
        text: format!(
            "Total Cards: {} | Archived: {}",
            board.cards.len(),
            board.archived_card_ids.len()
        ),
        color: app.palette.subtext0,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    cy += 32.0;

    // Per-column stats
    tree.push(RenderCommand::Text {
        x: 20.0,
        y: cy,
        text: "Cards per Column:".to_string(),
        color: app.palette.text,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    cy += 24.0;

    for stat in &stats {
        // Column bar
        let max_count: usize = stats.iter().map(|s| s.card_count).max().unwrap_or(1).max(1);
        let bar_fraction = stat.card_count as f32 / max_count as f32;
        let stat_bar_w: f32 = 200.0;

        tree.push(RenderCommand::Text {
            x: 30.0,
            y: cy,
            text: stat.name.clone(),
            color: app.palette.text,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
            overflow: TextOverflow::Ellipsis,
        });

        tree.push(RenderCommand::FillRect {
            x: 160.0,
            y: cy + 2.0,
            width: stat_bar_w,
            height: 14.0,
            color: app.palette.surface0,
            corner_radii: CornerRadii::all(3.0),
        });
        tree.push(RenderCommand::FillRect {
            x: 160.0,
            y: cy + 2.0,
            width: stat_bar_w * bar_fraction,
            height: 14.0,
            color: if stat.over_wip {
                app.palette.red
            } else {
                app.palette.blue
            },
            corner_radii: CornerRadii::all(3.0),
        });

        let wip_text = stat.wip_limit.map_or_else(
            || format!("{}", stat.card_count),
            |l| format!("{}/{}", stat.card_count, l),
        );
        tree.push(RenderCommand::Text {
            x: 370.0,
            y: cy,
            text: wip_text,
            color: if stat.over_wip {
                app.palette.ink(app.palette.red)
            } else {
                app.palette.subtext0
            },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cy += 24.0;
    }
}

/// Render the board list view.
fn render_board_list(tree: &mut RenderTree, app: &KanbanApp, width: f32, y_start: f32) {
    tree.push(RenderCommand::Text {
        x: 20.0,
        y: y_start + 16.0,
        text: "All Boards".to_string(),
        color: app.palette.text,
        font_size: 16.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    tree.push(RenderCommand::Text {
        x: 140.0,
        y: y_start + 19.0,
        text: "Up / Down choose \u{00b7} Enter opens \u{00b7} N adds a board".to_string(),
        color: app.palette.subtext0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((width - 160.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });

    let mut cy = y_start + 50.0;
    for (i, board) in app.boards.iter().enumerate() {
        let is_active = i == app.active_board_idx;
        // The row the arrows are on, which Enter opens. It was not drawn, so
        // Up and Down moved a choice nobody could see.
        if i == app.board_cursor {
            tree.push(RenderCommand::FillRect {
                x: 14.0,
                y: cy,
                width: 4.0,
                height: 50.0,
                color: app.palette.accent,
                corner_radii: CornerRadii::all(2.0),
            });
        }
        let bg = if is_active {
            app.palette.surface1
        } else {
            app.palette.surface0
        };

        tree.push(RenderCommand::FillRect {
            x: 20.0,
            y: cy,
            width: width - 40.0,
            height: 50.0,
            color: bg,
            corner_radii: CornerRadii::all(6.0),
        });

        if is_active {
            tree.push(RenderCommand::StrokeRect {
                x: 20.0,
                y: cy,
                width: width - 40.0,
                height: 50.0,
                color: app.palette.blue,
                line_width: 2.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        tree.push(RenderCommand::Text {
            x: 36.0,
            y: cy + 8.0,
            text: board.name.clone(),
            color: app.palette.text,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 120.0),
            overflow: TextOverflow::Ellipsis,
        });
        tree.push(RenderCommand::Text {
            x: 36.0,
            y: cy + 28.0,
            text: format!(
                "{} columns, {} cards",
                board.columns.len(),
                board.cards.len()
            ),
            color: app.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cy += 58.0;
    }

    // New Board button
    render_toolbar_button(
        tree,
        20.0,
        cy + 8.0,
        120.0,
        30.0,
        "+ New Board (N)",
        app.palette.blue,
        app.palette.crust,
        CornerRadii::all(6.0),
    );
}

/// Render input overlay (for card title entry, etc.).
fn render_input_overlay(tree: &mut RenderTree, app: &KanbanApp, width: f32, height: f32) {
    if app.input_mode == InputMode::None {
        return;
    }

    let prompt = match app.input_mode {
        InputMode::NewCardTitle => "New Card Title:",
        InputMode::SearchFilter => "Search:",
        InputMode::AssigneeFilter => "Assignee:",
        InputMode::NewBoardName => "New Board Name:",
        InputMode::NewColumnName => "New Column Name:",
        InputMode::CardDescription => "Description:",
        InputMode::AddComment => "Add Comment:",
        InputMode::AddChecklistItem => "Checklist Item:",
        InputMode::EditCardTitle => "Edit Title:",
        InputMode::RenameColumn => "Column Name:",
        InputMode::None => return,
    };

    // Overlay
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width,
        height,
        color: Color::rgba(0, 0, 0, 150),
        corner_radii: CornerRadii::ZERO,
    });

    // Dialog
    let dlg_w: f32 = 400.0;
    let dlg_h: f32 = 120.0;
    let dlg_x = (width - dlg_w) / 2.0;
    let dlg_y = (height - dlg_h) / 2.0;

    tree.push(RenderCommand::FillRect {
        x: dlg_x,
        y: dlg_y,
        width: dlg_w,
        height: dlg_h,
        color: app.palette.base,
        corner_radii: CornerRadii::all(8.0),
    });
    tree.push(RenderCommand::StrokeRect {
        x: dlg_x,
        y: dlg_y,
        width: dlg_w,
        height: dlg_h,
        color: app.palette.surface1,
        line_width: 1.0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Prompt text
    tree.push(RenderCommand::Text {
        x: dlg_x + 16.0,
        y: dlg_y + 16.0,
        text: prompt.to_string(),
        color: app.palette.text,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Input field
    app.palette.push_surface(
        tree,
        dlg_x + 16.0,
        dlg_y + 42.0,
        dlg_w - 32.0,
        30.0,
        4.0,
        Surface::Card,
    );
    tree.push(RenderCommand::Text {
        x: dlg_x + 24.0,
        y: dlg_y + 48.0,
        text: app.input_buffer.clone(),
        color: app.palette.text,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(dlg_w - 48.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Hint
    tree.push(RenderCommand::Text {
        x: dlg_x + 16.0,
        y: dlg_y + 84.0,
        text: "Enter to confirm, Escape to cancel".to_string(),
        color: app.palette.subtext0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// Full application render.
fn render_app(app: &KanbanApp, width: f32, height: f32) -> RenderTree {
    let mut tree = RenderTree::new();

    // The window itself. A surface is something drawn *on* the page.
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width,
        height,
        color: app.palette.crust,
        corner_radii: CornerRadii::ZERO,
    });

    // Toolbar
    render_toolbar(&mut tree, app, width);
    let mut content_y: f32 = TOOLBAR_H;

    // Filter bar
    content_y = render_filter_bar(&mut tree, app, width, content_y);

    // The views stop above the status line, which is drawn after them.
    let body_h = (height - STATUS_H).max(content_y);

    // Main view
    match app.view {
        View::Board => render_board_view(&mut tree, app, width, body_h, content_y),
        View::CardDetail => {
            render_board_view(&mut tree, app, width, body_h, content_y);
            render_status(&mut tree, app, width, height);
            render_card_detail(&mut tree, app, width, height);
        }
        View::Archive => render_archive_view(&mut tree, app, width, content_y),
        View::Statistics => render_stats_view(&mut tree, app, width, content_y),
        View::BoardList => render_board_list(&mut tree, app, width, content_y),
    }
    // Under the card detail modal, which covers it, and over every other view.
    if app.view != View::CardDetail {
        render_status(&mut tree, app, width, height);
    }

    // Input overlay
    render_input_overlay(&mut tree, app, width, height);

    // Over the input overlay too: the list is the one thing on screen a
    // reader asked for explicitly.
    if app.show_help {
        guitk::shortcut::render_card(
            &mut tree,
            &app.palette,
            (width, height),
            0.0,
            SHORTCUTS,
            "F1 closes this",
        );
    }

    tree
}

/// The status line along the bottom: why the boards are not being kept, if
/// they are not; else what the last import or export did -- which was
/// recorded and drawn nowhere -- else, on a board with no cards, how to start
/// and where the boards are kept.
fn render_status(tree: &mut RenderTree, app: &KanbanApp, width: f32, height: f32) {
    let y = (height - STATUS_H).max(0.0);
    app.palette.push_surface(
        tree,
        0.0,
        y,
        width,
        STATUS_H,
        0.0,
        Surface::Strip(Edge::Top),
    );
    let (text, colour) = if let Some(error) = &app.store_error {
        (error.clone(), app.palette.ink(app.palette.red))
    } else if let Some(note) = &app.note {
        (note.clone(), app.palette.subtext0)
    } else if let Some(action) = &app.last_file_action {
        let failed = action.starts_with("Could not");
        (
            action.clone(),
            if failed {
                app.palette.ink(app.palette.red)
            } else {
                app.palette.subtext0
            },
        )
    } else if app.active_board().cards.is_empty() {
        (
            format!("{NO_CARDS_YET} {}", app.keeping_line()),
            app.palette.subtext0,
        )
    } else {
        return;
    };
    tree.push(RenderCommand::Text {
        x: 8.0,
        y: y + 5.0,
        text,
        color: colour,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some((width - 16.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
}

// =============================================================================
// Event handling
// =============================================================================

/// Handle keyboard events.
fn handle_key_event(app: &mut KanbanApp, key: &KeyEvent) -> bool {
    if !key.pressed {
        return false;
    }

    // Three kinds of key, each asked for as itself: Ctrl chords, not Ctrl
    // held (AltGr arrives as Ctrl+Alt and types, and AltGr+D -- Ctrl+Alt+D
    // -- deleted the chosen card); the views' Alt+1 to Alt+4, Alt alone; and
    // every other key plain, nothing held but Shift -- a chord with Alt or
    // the Windows key arrives carrying its key, and Alt+M moved a card.
    let plain = textline::is_plain(key.modifiers);

    // Above the input router: naming a card takes typed text and `F1` is not
    // text, so a reader half-way through a title still gets the keys.
    if key.key == Key::F1 && plain {
        app.show_help = !app.show_help;
        return true;
    }
    if app.show_help {
        // Modal. Letting keys through would mean deleting a card the reader
        // cannot see.
        if plain && matches!(key.key, Key::Escape | Key::Enter | Key::F1) {
            app.show_help = false;
        }
        return true;
    }

    // A note describes the key it was the answer to, and this is a new key.
    app.note = None;

    // If in input mode, route to input handler
    if app.input_mode != InputMode::None {
        return handle_input_key(app, key);
    }

    // The open card's own keys, looked at first and only while a card is
    // open: on the board the same letters mean other things.
    if app.view == View::CardDetail
        && let Some(answered) = handle_detail_key(app, key)
    {
        return answered;
    }

    // The archive: choose a card, and put it back or delete it. Archiving was
    // one way -- the view drew a Restore button nothing could press.
    if app.view == View::Archive
        && let Some(answered) = handle_archive_key(app, key)
    {
        return answered;
    }

    if textline::is_ctrl_chord(key.modifiers) {
        return handle_ctrl_chord(app, key);
    }
    if key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key {
        return handle_alt_chord(app, key);
    }
    if !plain {
        return false;
    }

    // The board list is a *chooser*, and until this arm existed it was only a
    // list: `switch_board` had a test and no caller, so Alt+4 showed every
    // board and no key picked one.
    if app.view == View::BoardList {
        match key.key {
            // A new board. `add_board` had tests and no key.
            Key::N => {
                app.input_mode = InputMode::NewBoardName;
                app.input_buffer.clear();
                return true;
            }
            Key::Up => {
                // `saturating_sub` alone would answer "handled" at the top of
                // the list and redraw the same frame on every press.
                let Some(next) = app.board_cursor.checked_sub(1) else {
                    return false;
                };
                app.board_cursor = next;
                return true;
            }
            Key::Down => {
                let last = app.boards.len().saturating_sub(1);
                if app.board_cursor >= last {
                    return false;
                }
                app.board_cursor = app.board_cursor.saturating_add(1);
                return true;
            }
            Key::Enter => {
                app.switch_board(app.board_cursor);
                return true;
            }
            _ => {}
        }
    }

    match key.key {
        // ESC to go back from sub-views
        Key::Escape => {
            match app.view {
                // Closing the card leaves it chosen: the arrows go on from
                // where the user was, and Enter opens it again.
                View::CardDetail => {
                    app.view = View::Board;
                    app.checklist_focus = None;
                }
                View::Archive | View::Statistics | View::BoardList => {
                    app.view = View::Board;
                }
                View::Board => {
                    app.selected_card = None;
                }
            }
            true
        }

        // N = new card
        Key::N => {
            if app.view == View::Board {
                app.input_mode = InputMode::NewCardTitle;
                app.input_buffer.clear();
                return true;
            }
            false
        }

        // C = new column
        Key::C if key.modifiers.shift => {
            if app.view == View::Board {
                app.input_mode = InputMode::NewColumnName;
                app.input_buffer.clear();
                return true;
            }
            false
        }

        // Left and Right choose the next column over, and the card at the
        // same height in it -- the one the eye is already level with.
        Key::Left if app.view == View::Board => {
            let Some(to) = app.selected_column.checked_sub(1) else {
                return false;
            };
            let row = app.selected_row().unwrap_or(0);
            app.choose_in_column(to, row);
            true
        }
        Key::Right if app.view == View::Board => {
            let to = app.selected_column.saturating_add(1);
            if to >= app.active_board().columns.len() {
                return false;
            }
            let row = app.selected_row().unwrap_or(0);
            app.choose_in_column(to, row);
            true
        }

        // Up and Down choose the card above or below, and the board follows.
        // They only scrolled, and nothing else chose a card either -- so in
        // the running program no card could ever be opened, moved, archived
        // or deleted: every one of those keys wants a chosen card, and only
        // the tests ever set one.
        Key::Up if app.view == View::Board => app.step_card(-1),
        Key::Down if app.view == View::Board => app.step_card(1),

        // PageUp/PageDown scroll the columns by a screenful, leaving the
        // choice where it is. Not clamped here: the number of cards that fit
        // depends on the window size and on which cards are filtered in. The
        // renderer clamps against what it is actually drawing, so an offset
        // past the end shows the last page rather than a blank column.
        Key::PageUp | Key::PageDown if app.view == View::Board => {
            let delta = if key.key == Key::PageUp {
                BOARD_PAGE_STEP.saturating_neg()
            } else {
                BOARD_PAGE_STEP
            };
            app.scroll_offset = scroll_window::shift(app.scroll_offset, delta);
            true
        }

        // The same four keys scroll the card-detail modal's body. Unclamped at
        // the top end for the same reason as the board's: only the renderer
        // knows how tall this card's content came out. Clamped at zero here
        // because that bound needs nothing but the offset itself, and letting
        // it go negative would mean scrolling back down did nothing until the
        // debt was repaid — the failure this whole sweep keeps finding.
        Key::Up | Key::Down | Key::PageUp | Key::PageDown if app.view == View::CardDetail => {
            let step = match key.key {
                Key::PageUp | Key::PageDown => DETAIL_PAGE_STEP,
                _ => DETAIL_LINE_STEP,
            };
            let delta = if matches!(key.key, Key::Up | Key::PageUp) {
                -step
            } else {
                step
            };
            app.detail_scroll = (app.detail_scroll + delta).max(0.0);
            true
        }

        // Enter on selected card opens detail
        Key::Enter => {
            if let Some(_card_id) = app.selected_card
                && app.view == View::Board
            {
                app.view = View::CardDetail;
                // Open at the top. Carrying the previous card's offset over
                // would open a short card scrolled past its own content, which
                // the renderer then clamps back — so the modal would appear to
                // ignore the first few keypresses.
                app.detail_scroll = 0.0;
                app.checklist_focus = None;
                return true;
            }
            false
        }

        // P = cycle priority on selected card
        Key::P => {
            if let Some(card_id) = app.selected_card
                && let Some(card) = app.active_board_mut().cards.get_mut(&card_id)
            {
                card.priority = card.priority.next();
                return true;
            }
            false
        }

        // M = move card right one column. The choice goes with the card, so
        // M again moves it on.
        Key::M => {
            if let Some(card_id) = app.selected_card {
                let board = app.active_board();
                if let Some(from_col) = board.find_card_column(card_id) {
                    let to_col = from_col.saturating_add(1);
                    if to_col < board.columns.len() {
                        app.active_board_mut()
                            .move_card(card_id, from_col, to_col, 0);
                        app.selected_column = to_col;
                        app.reveal_selected();
                        return true;
                    }
                }
            }
            false
        }

        // B = move card left one column
        Key::B => {
            if let Some(card_id) = app.selected_card {
                let board = app.active_board();
                if let Some(from_col) = board.find_card_column(card_id)
                    && from_col > 0
                {
                    let to_col = from_col.saturating_sub(1);
                    app.active_board_mut()
                        .move_card(card_id, from_col, to_col, 0);
                    app.selected_column = to_col;
                    app.reveal_selected();
                    return true;
                }
            }
            false
        }

        // T = sort the chosen column by its order; Shift+T = the next order.
        Key::T if app.view == View::Board => {
            let col = app.selected_column;
            let Some(order) = app.active_board().columns.get(col).map(|c| c.sort_by) else {
                return false;
            };
            let order = if key.modifiers.shift {
                let next = order.next();
                if let Some(column) = app.active_board_mut().columns.get_mut(col) {
                    column.sort_by = next;
                }
                next
            } else {
                order
            };
            app.active_board_mut().sort_column(col);
            app.reveal_selected();
            app.note = Some(format!("Sorted by {}", order.label().to_lowercase()));
            true
        }

        // R = rename the chosen column, starting from its name.
        Key::R if app.view == View::Board => {
            let Some(name) = app
                .active_board()
                .columns
                .get(app.selected_column)
                .map(|c| c.name.clone())
            else {
                return false;
            };
            app.input_mode = InputMode::RenameColumn;
            app.input_buffer = name;
            true
        }

        // Z = collapse or open the chosen column.
        Key::Z if app.view == View::Board => {
            let col = app.selected_column;
            let Some(column) = app.active_board_mut().columns.get_mut(col) else {
                return false;
            };
            column.collapsed = !column.collapsed;
            let collapsed = column.collapsed;
            // A collapsed column shows no cards, so none of them can stay
            // chosen; opening it again chooses nothing until an arrow does.
            if collapsed {
                app.selected_card = None;
            }
            true
        }

        // Shift+Delete = take the chosen column off the board, if it is
        // empty. A column with cards says so rather than dropping them.
        Key::Delete if app.view == View::Board && key.modifiers.shift => {
            let col = app.selected_column;
            let board = app.active_board();
            let Some(column) = board.columns.get(col) else {
                return false;
            };
            let cards = column.card_ids.len();
            if cards > 0 {
                app.note = Some(format!(
                    "{} still has {cards} card{} -- move or archive {} first",
                    column.name,
                    if cards == 1 { "" } else { "s" },
                    if cards == 1 { "it" } else { "them" }
                ));
                return true;
            }
            app.active_board_mut().remove_column(col);
            let last = app.active_board().columns.len().saturating_sub(1);
            app.selected_column = col.min(last);
            app.selected_card = None;
            true
        }

        _ => false,
    }
}

/// A Ctrl chord: Ctrl without Alt or the Windows key, Shift as it is.
fn handle_ctrl_chord(app: &mut KanbanApp, key: &KeyEvent) -> bool {
    match key.key {
        // F = toggle filter bar
        Key::F => {
            app.show_filter_bar = !app.show_filter_bar;
            if !app.show_filter_bar {
                app.filter.clear();
            }
            true
        }

        // E = write the board out, O = read one back. NOT Ctrl+S, which
        // this app bound to the search bar long before it had a door: the
        // app's own vocabulary outranks consistency with its neighbours, and
        // a second `Key::S if ctrl` arm would simply never be reached.
        Key::E => {
            let name = sanitise_board_name(&app.active_board().name);
            app.picker.open_to_write(format!("{name}.json"));
            true
        }
        Key::O => {
            app.picker.open_to_read();
            true
        }

        // S = toggle search
        Key::S => {
            if app.show_filter_bar {
                app.input_mode = InputMode::SearchFilter;
                app.input_buffer = app.filter.search_text.clone();
                return true;
            }
            false
        }

        // D = delete card
        Key::D => {
            if let Some(card_id) = app.selected_card {
                let row = app.selected_row();
                app.active_board_mut().delete_card(card_id);
                app.choose_neighbour(row);
                return true;
            }
            false
        }

        // A = archive card
        Key::A if !key.modifiers.shift => {
            if let Some(card_id) = app.selected_card {
                let row = app.selected_row();
                app.active_board_mut().archive_card(card_id);
                app.choose_neighbour(row);
                return true;
            }
            false
        }

        // With the filter bar open: Ctrl+P steps the priority shown, Ctrl+U
        // types an assignee, Ctrl+L steps the label shown.
        Key::P if app.show_filter_bar => {
            let all = Priority::all();
            app.filter.priority_filter = match app.filter.priority_filter {
                None => all.first().copied(),
                Some(p) => {
                    let at = all.iter().position(|&q| q == p).unwrap_or(0);
                    all.get(at.saturating_add(1)).copied()
                }
            };
            app.refit_choice();
            true
        }
        Key::U if app.show_filter_bar => {
            app.input_mode = InputMode::AssigneeFilter;
            app.input_buffer = app.filter.assignee_filter.clone();
            true
        }
        Key::L if app.show_filter_bar => {
            let labels: Vec<Id> = app.active_board().labels.iter().map(|l| l.id).collect();
            app.filter.label_filter = match app.filter.label_filter {
                None => labels.first().copied(),
                Some(id) => {
                    let at = labels.iter().position(|&l| l == id);
                    at.and_then(|at| labels.get(at.saturating_add(1)).copied())
                }
            };
            app.refit_choice();
            true
        }
        _ => false,
    }
}

/// A chord with Alt alone: the four views.
fn handle_alt_chord(app: &mut KanbanApp, key: &KeyEvent) -> bool {
    match key.key {
        Key::Num1 => {
            app.view = View::Board;
            true
        }
        Key::Num2 => {
            app.view = View::Statistics;
            true
        }
        Key::Num3 => {
            app.view = View::Archive;
            app.archive_cursor = 0;
            true
        }
        Key::Num4 => {
            app.view = View::BoardList;
            // On the board that is open, which is where the user is.
            app.board_cursor = app.active_board_idx;
            true
        }
        _ => false,
    }
}

/// A key for the archive view: `Some(answered)` for one of its own, `None`
/// for the rest (Escape, the view switches).
fn handle_archive_key(app: &mut KanbanApp, key: &KeyEvent) -> Option<bool> {
    let count = app.active_board().archived_card_ids.len();
    // Plain is nothing held but Shift: the Windows key's chords were let
    // through, and Windows+R restored a card.
    let plain = textline::is_plain(key.modifiers);
    match key.key {
        Key::Up if plain => {
            let Some(to) = app.archive_cursor.checked_sub(1) else {
                return Some(false);
            };
            app.archive_cursor = to;
            Some(true)
        }
        Key::Down if plain => {
            let to = app.archive_cursor.saturating_add(1);
            if to >= count {
                return Some(false);
            }
            app.archive_cursor = to;
            Some(true)
        }
        Key::Enter | Key::R if plain => {
            let card_id = *app
                .active_board()
                .archived_card_ids
                .get(app.archive_cursor)?;
            let title = app
                .active_board()
                .cards
                .get(&card_id)
                .map(|c| c.title.clone())
                .unwrap_or_default();
            match app.active_board_mut().restore_card(card_id) {
                Some(column) => {
                    let name = app
                        .active_board()
                        .columns
                        .get(column)
                        .map(|c| c.name.clone())
                        .unwrap_or_default();
                    app.note = Some(format!("Restored {title} to {name}"));
                }
                None => {
                    app.note =
                        Some("There is no column to restore it to: Shift+C adds one".to_string());
                }
            }
            app.archive_cursor = app
                .archive_cursor
                .min(app.active_board().archived_card_ids.len().saturating_sub(1));
            Some(true)
        }
        // A Ctrl chord, not Ctrl held: AltGr+D -- Ctrl+Alt -- types.
        Key::D if textline::is_ctrl_chord(key.modifiers) => {
            let card_id = *app
                .active_board()
                .archived_card_ids
                .get(app.archive_cursor)?;
            app.active_board_mut().delete_card(card_id);
            app.archive_cursor = app
                .archive_cursor
                .min(app.active_board().archived_card_ids.len().saturating_sub(1));
            Some(true)
        }
        _ => None,
    }
}

/// A key for the open card: `Some(answered)` for one of its own, `None` for
/// one it leaves to the rest -- Escape, the scrolling keys, and P, M and B,
/// which mean the same with the card open or not.
///
/// E, D, C and L begin typing into the input line, E and D with what is
/// there already so an edit is an edit; Tab and Shift+Tab choose a
/// checklist item and Space ticks it. Every one of these was modelled --
/// the input modes, `toggle_checklist_item` -- and no key reached it.
fn handle_detail_key(app: &mut KanbanApp, key: &KeyEvent) -> Option<bool> {
    // Nothing held but Shift: Windows+E began editing the title.
    let plain = textline::is_plain(key.modifiers);
    let card_id = app.selected_card?;
    let card = app.active_board().cards.get(&card_id)?;
    let (mode, seed) = match key.key {
        Key::E if plain => (InputMode::EditCardTitle, card.title.clone()),
        Key::D if plain => (InputMode::CardDescription, card.description.clone()),
        Key::C if plain && !key.modifiers.shift => (InputMode::AddComment, String::new()),
        Key::L if plain => (InputMode::AddChecklistItem, String::new()),
        Key::Tab if plain => {
            let delta = if key.modifiers.shift { -1 } else { 1 };
            return Some(app.step_checklist(delta));
        }
        Key::Space if plain => return Some(app.tick_checklist()),
        _ => return None,
    };
    app.input_mode = mode;
    app.input_buffer = seed;
    Some(true)
}

impl KanbanApp {
    /// Move the checklist focus of the open card `delta` items, stopping at
    /// either end; from no focus, the first press lands on the first item
    /// going forward and the last going back. Answers whether it moved.
    fn step_checklist(&mut self, delta: isize) -> bool {
        let Some(card) = self
            .selected_card
            .and_then(|id| self.active_board().cards.get(&id))
        else {
            return false;
        };
        let Some(last) = card.checklist.len().checked_sub(1) else {
            self.note = Some("This card has no checklist: L adds an item".to_string());
            return true;
        };
        let to = match (self.checklist_focus, delta.is_negative()) {
            (None, true) => last,
            (None, false) => 0,
            (Some(at), true) => at.saturating_sub(delta.unsigned_abs()),
            (Some(at), false) => at.saturating_add(delta.unsigned_abs()).min(last),
        };
        if self.checklist_focus == Some(to) {
            return false;
        }
        self.checklist_focus = Some(to);
        self.reveal_checklist_focus();
        true
    }

    /// Tick or untick the focused checklist item of the open card.
    fn tick_checklist(&mut self) -> bool {
        let Some(card_id) = self.selected_card else {
            return false;
        };
        let Some(index) = self.checklist_focus else {
            self.note = Some("Tab chooses a checklist item, then Space ticks it".to_string());
            return true;
        };
        let Some(card) = self.active_board_mut().cards.get_mut(&card_id) else {
            return false;
        };
        let Some(item_id) = card.checklist.get(index).map(|item| item.id) else {
            return false;
        };
        card.toggle_checklist_item(item_id);
        true
    }

    /// Scroll the open card so its focused checklist item is in sight.
    fn reveal_checklist_focus(&mut self) {
        let (Some(index), Some(card_id)) = (self.checklist_focus, self.selected_card) else {
            return;
        };
        let board = self.active_board();
        let Some(card) = board.cards.get(&card_id) else {
            return;
        };
        let Some(&row) = checklist_rows(self, board, card).get(index) else {
            return;
        };
        let body = DetailModal::for_window(self.win_width, self.win_height).body_height();
        if row < self.detail_scroll {
            self.detail_scroll = row;
        } else if row + CHECK_ROW_H > self.detail_scroll + body {
            self.detail_scroll = row + CHECK_ROW_H - body;
        }
    }
}

/// Handle key events during input mode.
fn handle_input_key(app: &mut KanbanApp, key: &KeyEvent) -> bool {
    // What a key typed goes in -- AltGr's among it, and not a command's
    // letter, which a chord carries: Ctrl+S typed an `s` into a card's title.
    // The line's own keys are plain: Alt+Enter added the card.
    if textline::types_into_field(key) {
        app.input_buffer.extend(key.typed());
        return true;
    }
    if !textline::is_plain(key.modifiers) {
        return false;
    }
    match key.key {
        Key::Escape => {
            app.input_mode = InputMode::None;
            app.input_buffer.clear();
            true
        }
        Key::Enter => {
            let text = app.input_buffer.clone();
            let mode = app.input_mode;
            app.input_mode = InputMode::None;
            app.input_buffer.clear();

            // Nothing typed is nothing done -- except for a description,
            // which starts from what is there, so emptying it is the way to
            // clear it.
            if text.is_empty()
                && !matches!(mode, InputMode::CardDescription | InputMode::AssigneeFilter)
            {
                return true;
            }

            match mode {
                InputMode::NewCardTitle => {
                    // The new card is chosen, so the next key acts on it.
                    let col = app.selected_column;
                    if let Some(id) = app.add_card(&text, col) {
                        app.selected_card = Some(id);
                        app.reveal_selected();
                    }
                }
                InputMode::SearchFilter => {
                    app.filter.search_text = text;
                }
                InputMode::AssigneeFilter => {
                    app.filter.assignee_filter = text;
                    app.refit_choice();
                }
                InputMode::NewBoardName => {
                    app.add_board(&text);
                    app.view = View::Board;
                }
                InputMode::NewColumnName => {
                    app.active_board_mut().add_column(&text);
                }
                InputMode::CardDescription => {
                    if let Some(card_id) = app.selected_card
                        && let Some(card) = app.active_board_mut().cards.get_mut(&card_id)
                    {
                        card.description = text;
                    }
                }
                InputMode::AddComment => {
                    if let Some(card_id) = app.selected_card {
                        let ts = app.next_timestamp();
                        let author = app.author.clone();
                        if let Some(card) = app.active_board_mut().cards.get_mut(&card_id) {
                            card.add_comment(&author, &text, ts);
                        }
                    }
                }
                InputMode::AddChecklistItem => {
                    if let Some(card_id) = app.selected_card
                        && let Some(card) = app.active_board_mut().cards.get_mut(&card_id)
                    {
                        card.add_checklist_item(&text);
                    }
                }
                InputMode::EditCardTitle => {
                    if let Some(card_id) = app.selected_card
                        && let Some(card) = app.active_board_mut().cards.get_mut(&card_id)
                    {
                        card.title = text;
                    }
                }
                InputMode::RenameColumn => {
                    let col = app.selected_column;
                    if let Some(column) = app.active_board_mut().columns.get_mut(col) {
                        column.name = text;
                    }
                }
                InputMode::None => {}
            }
            true
        }
        Key::Backspace => {
            app.input_buffer.pop();
            true
        }
        _ => false,
    }
}

// =============================================================================
// Main
// =============================================================================

impl App for KanbanApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        self.boards
            .get(self.active_board_idx)
            .map_or_else(|| "Kanban".to_owned(), |b| format!("Kanban — {}", b.name))
    }

    fn initial_size(&self) -> (u32, u32) {
        (INITIAL_WIDTH, INITIAL_HEIGHT)
    }

    /// No clock.
    ///
    /// Nothing here advances on its own: a card moves when it is moved and the
    /// board scrolls when it is scrolled. There is no animation and no data
    /// that ages, so a tick would redraw an identical frame.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return if self.request_close() {
                Response::Exit
            } else {
                Response::KeepOpen
            };
        }
        let error_before = self.store_error.clone();
        let response = self.route_event(event);
        // Whatever a key or a click changed is kept before the next event.
        if matches!(event, Event::Key(_) | Event::Mouse(_)) {
            self.keep();
        }
        if self.quit {
            return Response::Exit;
        }
        if self.store_error == error_before {
            response
        } else {
            Response::Redraw
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The renderer is a free function taking the size, so there is no
        // stored dimension to reconcile: whatever the compositor grants is what
        // gets drawn, including on the first frame before any `Resize`.
        //
        // Remembered anyway, because a click on the picker has to be answered
        // against the window the user is looking at, and `on_event` is handed
        // no size.
        self.win_width = width;
        self.win_height = height;
        let mut tree = render_app(self, width, height);
        // Last, so it is above everything. Forgetting this is how a picker
        // ends up open and invisible, taking every keystroke with nothing on
        // screen to say why.
        tree.commands
            .extend(self.picker.render(&self.palette, width, height));
        // Over everything, the picker included: they are never up together.
        let palette = self.palette;
        if let Some(question) = self.question.as_mut() {
            question.render(&palette, width, height, &mut tree);
        }
        tree
    }
}

impl KanbanApp {
    /// What an event does, before the boards are kept.
    fn route_event(&mut self, event: &Event) -> Response {
        // The close question takes every key and click while it is up.
        if let Some(question) = self.question.as_mut()
            && matches!(event, Event::Key(_) | Event::Mouse(_))
        {
            if let Some(choice) = question.handle(event) {
                self.question = None;
                self.answer(choice);
            }
            return Response::Redraw;
        }
        // The picker takes input first while it is up, or a keystroke meant
        // for a filename reaches the board -- where a bare letter starts a
        // new card and Delete removes the selected one.
        //
        // This also gives the picker its mouse events: the match below has no
        // `Event::Mouse` arm at all, so without this the dialog could be seen
        // and not clicked.
        match self.picker.handle(event, self.win_width, self.win_height) {
            Picked::Chose(path) => {
                let saving = self.picker.is_saving();
                self.last_file_action = Some(if saving {
                    self.write_board(&path)
                } else {
                    self.read_board(&path)
                });
                return Response::Redraw;
            }
            // Cancelled grouped with Handled: this caller keeps no dialog
            // state of its own that could go stale.
            Picked::Handled | Picked::Cancelled => return Response::Redraw,
            Picked::Ignored => {}
        }
        match event {
            Event::CloseRequested => Response::Exit,
            Event::Key(key_ev) => {
                if !key_ev.pressed {
                    return Response::Idle;
                }
                // `handle_key_event` already reports whether it did anything —
                // it is one of the few in this tree that does — so there is no
                // fingerprint to compare here.
                if handle_key_event(self, key_ev) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            _ => Response::Idle,
        }
    }
}

/// What the close question is holding up. Only the close: nothing else in
/// this app can lose the boards' changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    /// The window, asked to close while a save is failing.
    Close,
}

/// A board name reduced to something that can be a filename.
///
/// Only the three characters a path cannot contain are replaced: the name is
/// the user's, and rewriting more of it than necessary means they cannot find
/// the file by the name they gave the board.
fn sanitise_board_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\u{0}') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        String::from("board")
    } else {
        trimmed.to_owned()
    }
}

/// The most of a board file one open will read.
///
/// Reported when it bites. A cut JSON document does not parse, so the import
/// fails outright rather than losing part of a board -- but the reason must
/// say which of the two happened, or a long file reads as a corrupt one.
pub const MAX_BOARD_BYTES: usize = 8 * 1024 * 1024;

/// The size the window asks to open at.
///
/// Only an opening request: `render_app` lays out from the size it is handed,
/// so nothing depends on getting these.
const INITIAL_WIDTH: u32 = 1200;
const INITIAL_HEIGHT: u32 = 800;

fn main() -> ExitCode {
    // Opens on the boards kept last time; on a first run, on one empty board.
    // An empty board looks unhelpful, and filling it with made-up cards --
    // which this did until 2026-09-15 -- is not the remedy.
    let mut app = KanbanApp::from_settings();
    app::launch("kanban", &mut app)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    // ------------------------------------------------------------------
    // The board chooser, and the compositor wiring
    // ------------------------------------------------------------------

    fn key_press(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        }
    }

    /// Every string the window draws, joined.
    fn drawn(app: &KanbanApp) -> String {
        render_app(app, 1200.0, 800.0)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// A board with a card selected, which is what most of these keys need.
    fn with_card() -> KanbanApp {
        let mut app = KanbanApp::new();
        app.view = View::Board;
        // Column 1, not 0: `B` moves a card back a column and is correctly
        // refused at the left edge, `M` moves it on and is refused at the
        // right. A card in the middle is the only place both have work.
        let id = app.add_card("A card", 1).expect("a card");
        app.selected_card = Some(id);
        app.selected_column = 1;
        // Open, so `Ctrl+S` has a search field to type into. Closed it answers
        // `false`, which is correct and is not a missing binding.
        app.show_filter_bar = true;
        app
    }

    /// **Each kind of key is asked for as itself**: AltGr+D -- Ctrl+Alt, which
    /// types -- deleted the chosen card as Ctrl+D does; AltGr+2 changed the
    /// view as Alt+2 does; Alt+M and Windows+P moved the card and stepped its
    /// priority, each chord carrying its key; Ctrl+S typed an `s` into a
    /// card's title; and Alt+Enter added the card. Ctrl+D and Alt+2 still do
    /// theirs.
    #[test]
    fn each_kind_of_key_is_asked_for_as_itself() {
        let altgr = Modifiers {
            alt: true,
            ..Modifiers::ctrl()
        };
        let key = |k: Key, text: &str, modifiers: Modifiers| KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: text.to_owned(),
        };
        let mut app = with_card();
        let card = app.selected_card;
        let column = app.selected_column;
        let priority = |app: &KanbanApp| {
            card.and_then(|id| app.active_board().cards.get(&id).map(|c| c.priority))
        };
        let before = priority(&app);
        for (k, m) in [
            (Key::D, altgr),
            (Key::Num2, altgr),
            (Key::M, Modifiers::alt()),
            (Key::P, Modifiers::super_key()),
            (Key::N, Modifiers::alt()),
            (Key::Enter, Modifiers::alt()),
            (Key::F1, Modifiers::alt()),
            (Key::Escape, Modifiers::super_key()),
        ] {
            assert!(
                !handle_key_event(&mut app, &key(k, "", m)),
                "{m:?} {k:?} was taken"
            );
        }
        assert_eq!(
            app.selected_card, card,
            "a chord deleted or let go of the card"
        );
        assert_eq!(app.selected_column, column, "a chord moved the card");
        assert_eq!(priority(&app), before, "a chord stepped the priority");
        assert_eq!(app.view, View::Board, "a chord changed the view");
        assert_eq!(app.input_mode, InputMode::None, "a chord began typing");
        assert!(!app.show_help, "a chord raised the keys");

        // The input line types what a key typed; its own keys are plain.
        assert!(handle_key_event(
            &mut app,
            &key(Key::N, "n", Modifiers::NONE)
        ));
        assert_eq!(
            app.input_mode,
            InputMode::NewCardTitle,
            "control: N names a card"
        );
        handle_key_event(&mut app, &key(Key::S, "s", Modifiers::ctrl()));
        handle_key_event(&mut app, &key(Key::X, "x", Modifiers::alt()));
        handle_key_event(&mut app, &key(Key::S, "ś", altgr));
        handle_key_event(&mut app, &key(Key::Enter, "", Modifiers::alt()));
        assert_eq!(
            app.input_buffer, "ś",
            "the line typed a command or lost AltGr's ś"
        );
        assert_eq!(
            app.input_mode,
            InputMode::NewCardTitle,
            "Alt+Enter added the card"
        );
        handle_key_event(&mut app, &key(Key::Escape, "", Modifiers::NONE));

        // The open card's keys are plain too.
        app.view = View::CardDetail;
        assert!(!handle_key_event(
            &mut app,
            &key(Key::E, "", Modifiers::super_key())
        ));
        assert_eq!(
            app.input_mode,
            InputMode::None,
            "Windows+E began editing the title"
        );
        app.view = View::Board;

        // So are the archive's: Windows+R restored a card, and AltGr+D
        // deleted one there as Ctrl+D does.
        let archived = app.selected_card.expect("the card");
        app.active_board_mut().archive_card(archived);
        app.view = View::Archive;
        app.archive_cursor = 0;
        for (k, m) in [(Key::R, Modifiers::super_key()), (Key::D, altgr)] {
            handle_key_event(&mut app, &key(k, "", m));
        }
        assert_eq!(
            app.active_board().archived_card_ids,
            vec![archived],
            "a chord restored or deleted the archived card"
        );
        app.active_board_mut().restore_card(archived);
        app.selected_card = Some(archived);
        app.selected_column = column;
        app.view = View::Board;

        // The chords themselves still work.
        assert!(handle_key_event(
            &mut app,
            &key(Key::Num2, "", Modifiers::alt())
        ));
        assert_eq!(
            app.view,
            View::Statistics,
            "Alt+2 no longer changes the view"
        );
        app.view = View::Board;
        assert!(handle_key_event(
            &mut app,
            &key(Key::D, "", Modifiers::ctrl())
        ));
        assert_ne!(app.selected_card, card, "Ctrl+D no longer deletes");
    }

    /// **Every key the list advertises is one this program answers.**
    ///
    /// Two states. `P`, `M`, `B`, `Ctrl+D` and `Ctrl+A` all act on the
    /// selected card and are correctly refused without one; the card-detail
    /// view is what gives `Up`/`Down` a card to scroll rather than a list to
    /// step. Every refusal involved is right, which is why the list names the
    /// thing each key needs.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = [View::Board, View::CardDetail].into_iter().any(|view| {
                    let mut app = with_card();
                    app.view = view;
                    handle_key_event(&mut app, &stroke)
                });
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// **The shortcut list reaches the window, and nothing acts behind it.**
    ///
    /// The control is the half that matters: `Ctrl+D` behind the card would
    /// delete a card the reader cannot see, and asserting only that it does
    /// not would pass on an app that had lost `Ctrl+D` altogether.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = with_card();
        assert!(
            !drawn(&app).contains("F1 closes this"),
            "the list is up before anybody asked for it"
        );

        handle_key_event(&mut app, &key_press(Key::F1));
        let shown = drawn(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        let cards = app.active_board().cards.len();
        let mut ctrl_d = key_press(Key::D);
        ctrl_d.modifiers.ctrl = true;
        handle_key_event(&mut app, &ctrl_d);
        assert_eq!(
            app.active_board().cards.len(),
            cards,
            "Ctrl+D deleted a card through the shortcut card"
        );

        handle_key_event(&mut app, &key_press(Key::Escape));
        assert!(
            !drawn(&app).contains("F1 closes this"),
            "Escape did not close it"
        );

        handle_key_event(&mut app, &ctrl_d);
        assert_ne!(
            app.active_board().cards.len(),
            cards,
            "control: Ctrl+D does nothing even with the card down"
        );
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
    fn an_open_picker_takes_the_keyboard_from_the_board() {
        use oswindow::app::App as _;

        let mut app = KanbanApp::new();
        // Two things the fixture needs, both found by measuring rather than
        // reading: a second board, because Down steps BETWEEN boards; and the
        // board-list view, because that arm is inside `if app.view ==
        // View::BoardList` and a new app opens on the board itself. Without
        // either, Down cannot move and the assertion below holds whether or
        // not the dialog took the key.
        app.add_board("Second");
        app.view = View::BoardList;
        app.selected_column = 0;
        assert!(
            app.boards.len() > 1,
            "control: the fixture needs more than one board to move between"
        );

        let mut ctrl = Modifiers::NONE;
        ctrl.ctrl = true;
        app.on_event(&Event::Key(KeyEvent {
            key: Key::O,
            pressed: true,
            modifiers: ctrl,
            text: String::new(),
        }));
        assert!(app.picker.is_open(), "control: the picker must be up");

        app.on_event(&Event::Key(press(Key::Down)));
        assert_eq!(
            app.selected_column, 0,
            "Down at the open dialog moved the column behind it"
        );
    }

    #[test]
    fn the_board_list_can_actually_choose_a_board() {
        // `switch_board` had a test and no caller: Alt+4 showed every board and
        // no key picked one, so the list was a list rather than a chooser.
        let mut app = KanbanApp::new();
        app.create_sample_data();
        while app.boards.len() < 2 {
            app.boards.push(Board::new("Second"));
        }
        app.view = View::BoardList;
        app.board_cursor = 0;
        assert!(handle_key_event(&mut app, &key_press(Key::Down)));
        assert_eq!(app.board_cursor, 1);
        assert!(handle_key_event(&mut app, &key_press(Key::Enter)));
        assert_eq!(app.active_board_idx, 1, "Enter did not switch the board");
        assert_eq!(app.view, View::Board, "choosing a board should show it");
    }

    #[test]
    fn the_board_list_stops_at_its_ends() {
        let mut app = KanbanApp::new();
        app.create_sample_data();
        app.view = View::BoardList;
        app.board_cursor = 0;
        assert!(
            !handle_key_event(&mut app, &key_press(Key::Up)),
            "Up at the top should report that it did nothing"
        );
        let last = app.boards.len().saturating_sub(1);
        app.board_cursor = last;
        assert!(
            !handle_key_event(&mut app, &key_press(Key::Down)),
            "Down at the bottom should report that it did nothing"
        );
        assert_eq!(app.board_cursor, last);
    }

    /// The list opens on the board that is open, and leaves the board's
    /// chosen column alone -- the list's cursor was that column, borrowed.
    #[test]
    fn the_board_list_opens_on_the_open_board_and_keeps_the_column() {
        let mut app = KanbanApp::new();
        app.add_board("Second");
        app.add_board("Third");
        app.switch_board(1);
        app.selected_column = 3;
        handle_key_event(
            &mut app,
            &make_key(
                Key::Num4,
                Modifiers {
                    alt: true,
                    ..Modifiers::NONE
                },
            ),
        );
        assert_eq!(app.view, View::BoardList);
        assert_eq!(
            app.board_cursor, 1,
            "the list did not open on the open board"
        );
        handle_key_event(&mut app, &key_press(Key::Down));
        handle_key_event(&mut app, &key_press(Key::Escape));
        assert_eq!(app.view, View::Board);
        assert_eq!(
            app.selected_column, 3,
            "moving in the list moved the board's column"
        );
    }

    #[test]
    fn n_in_the_board_list_makes_a_board_and_opens_it() {
        let mut app = KanbanApp::new();
        app.view = View::BoardList;
        handle_key_event(&mut app, &key_press(Key::N));
        assert_eq!(app.input_mode, InputMode::NewBoardName);
        for c in "Garden".chars() {
            handle_key_event(&mut app, &make_char_key(c));
        }
        handle_key_event(&mut app, &key_press(Key::Enter));
        assert_eq!(app.boards.len(), 2);
        assert_eq!(app.active_board().name, "Garden");
        assert_eq!(app.view, View::Board);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = KanbanApp::new();
        app.create_sample_data();
        let before = app.view;
        let release = KeyEvent {
            key: Key::Escape,
            pressed: false,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        };
        assert!(!handle_key_event(&mut app, &release));
        assert_eq!(app.view, before);
    }

    #[test]
    fn the_title_names_the_active_board() {
        let mut app = KanbanApp::new();
        app.create_sample_data();
        let name = app
            .boards
            .get(app.active_board_idx)
            .map(|b| b.name.clone())
            .expect("a board exists");
        assert!(
            app.title().contains(&name),
            "the window title {:?} does not name the board {name:?}",
            app.title()
        );
    }

    #[test]
    fn rendering_draws_something_in_every_view_at_an_awkward_size() {
        let mut app = KanbanApp::new();
        app.create_sample_data();
        for view in [
            View::Board,
            View::CardDetail,
            View::Archive,
            View::Statistics,
            View::BoardList,
        ] {
            app.view = view;
            for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
                assert!(
                    !app.render(w, h).is_empty(),
                    "{view:?} drew nothing at {w}x{h}"
                );
            }
        }
    }
    // Used by the layout tests below and nowhere else: `main` used to build a
    // widget container it then never drew, and that scaffolding is gone.
    use oswindow::app::App as _;

    use guitk::layout::FlexDirection;
    use guitk::widget::{Widget, WidgetTree};

    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.

    use super::*;

    /// An empty board says how to start and whether it is kept -- where it
    /// can be seen.
    ///
    /// The two lines this replaced were in the frame and on no screen: drawn
    /// at the top of the window before the toolbar, whose surface covered
    /// them. The test that pinned them read the command list, where they
    /// were, and so passed. This one also asks that nothing drawn after the
    /// line covers it.
    #[test]
    fn the_empty_board_says_how_to_start_where_it_can_be_seen() {
        let mut app = KanbanApp::new();
        app.active_board_mut().cards.clear();
        for column in &mut app.active_board_mut().columns {
            column.card_ids.clear();
        }
        let commands = render_app(&app, TEST_W, TEST_H).commands;
        let want = format!("{NO_CARDS_YET} Nothing here is kept.");
        let (at, x, y) = commands
            .iter()
            .enumerate()
            .find_map(|(i, c)| match c {
                RenderCommand::Text { text, x, y, .. } if *text == want => Some((i, *x, *y)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the window never said {want:?}"));
        let covered = commands.iter().skip(at + 1).any(|c| {
            matches!(c, RenderCommand::FillRect { x: rx, y: ry, width, height, .. }
                if x >= *rx && x < rx + width && y >= *ry && y < ry + height)
        });
        assert!(!covered, "the empty board's line is painted over");

        // With a card on the board, the line is not drawn.
        app.add_card("One", 0);
        let texts: Vec<String> = render_app(&app, TEST_W, TEST_H)
            .commands
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(!texts.iter().any(|t| t.starts_with(NO_CARDS_YET)));
    }

    /// A board survives a write and a read through the door.
    #[test]
    fn a_board_survives_the_door() {
        let dir = std::env::temp_dir().join("slateos-kanban-door-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("board.json");
        let _ = std::fs::remove_file(&path);

        // The default board has columns and labels and no cards, so the
        // fixture adds one. The assertion below is what caught that: without
        // it the test would have written an empty board, been refused, and
        // failed somewhere less informative.
        let mut app = KanbanApp::new();
        let card = Card::new("Something to save");
        let card_id = card.id;
        app.active_board_mut().cards.insert(card_id, card);
        if let Some(column) = app.active_board_mut().columns.first_mut() {
            column.card_ids.push(card_id);
        }
        let cards = app.active_board().cards.len();
        assert!(cards > 0, "the fixture board has no cards to write");
        let said = app.write_board(&path);
        assert!(said.starts_with("Wrote "), "said: {said}");

        let before = app.boards.len();
        let said = app.read_board(&path);
        assert!(said.starts_with("Opened "), "said: {said}");
        assert_eq!(app.boards.len(), before + 1, "no board was opened");
        assert_eq!(
            app.active_board().cards.len(),
            cards,
            "the opened board lost cards"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// A file that is not a board says so rather than opening an empty one.
    #[test]
    fn a_file_that_is_not_a_board_is_refused_with_a_reason() {
        let dir = std::env::temp_dir().join("slateos-kanban-door-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("notaboard.json");
        std::fs::write(&path, "{\"unrelated\":true}").expect("write");

        let mut app = KanbanApp::new();
        let before = app.boards.len();
        let said = app.read_board(&path);
        assert!(said.contains("no board name"), "said: {said}");
        assert_eq!(app.boards.len(), before, "an empty board was opened anyway");

        let _ = std::fs::remove_file(&path);
    }

    /// An empty board is refused rather than written. See design-decisions 854.
    #[test]
    fn a_board_with_no_cards_is_not_written() {
        let path = std::env::temp_dir().join("slateos-kanban-should-not-exist.json");
        let _ = std::fs::remove_file(&path);

        let mut app = KanbanApp::new();
        app.active_board_mut().cards.clear();
        let said = app.write_board(&path);
        assert_eq!(said, "That board has no cards -- nothing to write");
        assert!(!path.exists(), "nothing should have been created");
    }

    /// Ctrl+E opens the picker; Ctrl+S still reaches the search bar.
    ///
    /// The reason the door is not on Ctrl+S: this app bound that to the search
    /// bar long before it had a file. A second `Key::S if ctrl` arm would
    /// never be reached, and the failure would be silent.
    #[test]
    fn ctrl_e_opens_the_picker_and_ctrl_s_still_searches() {
        let mut app = KanbanApp::new();
        app.show_filter_bar = true;

        let mut ctrl = Modifiers::NONE;
        ctrl.ctrl = true;
        let key = |k: Key| KeyEvent {
            key: k,
            pressed: true,
            modifiers: ctrl,
            text: String::new(),
        };

        assert!(handle_key_event(&mut app, &key(Key::S)));
        assert_eq!(
            app.input_mode,
            InputMode::SearchFilter,
            "Ctrl+S stopped reaching the search bar"
        );

        let mut app = KanbanApp::new();
        let before = app.render(TEST_W, TEST_H).commands.len();
        assert!(handle_key_event(&mut app, &key(Key::E)));
        assert!(app.picker.is_open(), "Ctrl+E did not open the picker");

        // `is_open` is not enough, and assuming it was is how apps/flashcards
        // shipped a picker that took every keystroke and painted nothing.
        // Deleting the render line above leaves `is_open` true and this
        // assertion is the only one that notices.
        assert!(
            app.render(TEST_W, TEST_H).commands.len() > before,
            "the picker is open and nothing was drawn for it"
        );
    }

    /// **The round trip `validate_export` claimed to test and did not.**
    ///
    /// That function exported a board and asked whether the string was
    /// non-empty. `export_board` always writes at least a header, so it could
    /// not return false. This is the check it was named for.
    #[test]
    fn a_board_survives_a_round_trip() {
        let mut board = Board::new("Sprint");
        board
            .labels
            .push(Label::new("urgent", Color::rgb(0xff, 0x00, 0x00)));
        let label_id = board.labels.first().expect("a label").id;

        let mut card = Card::new("Write the importer");
        card.description = String::from("with \"quotes\" and a \\ backslash");
        card.priority = Priority::Critical;
        card.due_date = Some(SimpleDate::new(2026, 9, 15));
        card.assignee = String::from("lane C");
        card.labels = vec![label_id];
        card.checklist.push(ChecklistItem {
            id: Id::new(),
            text: String::from("parse values"),
            done: true,
        });
        card.comments.push(Comment {
            id: Id::new(),
            author: String::from("reviewer"),
            text: String::from("ship it"),
            timestamp: 1_700_000_000,
        });
        card.archived = false;
        let card_id = card.id;
        board.cards.insert(card_id, card);

        let mut column = Column::new("Doing");
        column.card_ids = vec![card_id];
        column.wip_limit = Some(3);
        let column_id = column.id;
        board.columns.push(column);

        board.swimlanes_enabled = true;
        board.swimlane_names = vec![String::from("Frontend"), String::from("Backend")];

        let json = JsonExporter::export_board(&board);
        let back = JsonImporter::import_board(&json).expect("our own export must import");

        assert_eq!(back.name, "Sprint");
        assert_eq!(back.columns.len(), 1, "the column did not survive");
        let col = back.columns.first().expect("a column");
        assert_eq!(col.id, column_id, "the column was renumbered");
        assert_eq!(col.name, "Doing");
        assert_eq!(col.wip_limit, Some(3));

        // The part that would break silently if ids were reassigned: a column
        // stores card ids, so fresh numbering leaves the board looking whole
        // and every column pointing at nothing.
        assert_eq!(col.card_ids, vec![card_id], "the column lost its card");

        let got = back.cards.get(&card_id).expect("the card came back by id");
        assert_eq!(got.title, "Write the importer");
        assert_eq!(got.description, "with \"quotes\" and a \\ backslash");
        assert_eq!(got.priority, Priority::Critical);
        assert_eq!(
            got.due_date.map(|d| d.display()),
            Some(String::from("2026-09-15"))
        );
        assert_eq!(got.assignee, "lane C");
        assert_eq!(got.labels, vec![label_id], "the card lost its label");
        assert_eq!(got.checklist.len(), 1);
        assert!(got.checklist.first().expect("an item").done);
        assert_eq!(got.comments.len(), 1);
        assert_eq!(
            got.comments.first().expect("a comment").timestamp,
            1_700_000_000
        );

        assert!(back.swimlanes_enabled, "the swimlane flag was lost");
        assert_eq!(back.swimlane_names.len(), 2);
        assert_eq!(back.labels.len(), 1);
        assert_eq!(back.labels.first().expect("a label").id, label_id);
    }

    /// A file that is not JSON is refused, and says which of the two failures
    /// it was.
    #[test]
    fn a_file_that_is_not_ours_is_refused_with_a_reason() {
        let err = JsonImporter::import_board("this is not json at all")
            .expect_err("junk must not import");
        assert!(err.contains("not JSON"), "said: {err}");

        let err = JsonImporter::import_board(r#"{"columns":[]}"#)
            .expect_err("JSON without a board name is not a board");
        assert!(err.contains("no board name"), "said: {err}");
    }

    /// A field that is present and unreadable does NOT fail the import.
    ///
    /// A board is worth more than a field, and a dropped field is visible on
    /// the card where a refused file is not visible at all.
    #[test]
    fn an_unreadable_field_costs_the_field_and_not_the_board() {
        let json = r#"{"name":"B","columns":[],"cards":[{"id":7,"title":"T",
            "priority":"Nonsense","due_date":"not-a-date","archived":false}],
            "labels":[],"swimlanes_enabled":false,"swimlane_names":[]}"#;
        let board = JsonImporter::import_board(json).expect("the board should still import");
        let card = board
            .cards
            .get(&Id::from_stored(7))
            .expect("the card survived");
        assert_eq!(
            card.priority,
            Priority::Medium,
            "an unknown priority defaults"
        );
        assert_eq!(card.due_date, None, "an unreadable date is dropped");
        assert_eq!(card.title, "T", "the rest of the card is intact");
    }

    /// Nested structure the tokeniser alone could never have handled.
    #[test]
    fn the_value_parser_handles_nesting_and_escapes() {
        let (v, _) = JsonImporter::parse_value(r#"{"a":[1,{"b":"x\"y"},null,true]}"#, 0)
            .expect("valid JSON");
        let arr = v.get("a").and_then(JsonValue::as_array).expect("an array");
        assert_eq!(arr.len(), 4);
        assert_eq!(arr.first().and_then(JsonValue::as_i64), Some(1));
        assert_eq!(
            arr.get(1)
                .and_then(|o| o.get("b"))
                .and_then(JsonValue::as_str),
            Some("x\"y")
        );
        assert_eq!(arr.get(2), Some(&JsonValue::Null));
        assert_eq!(arr.get(3).and_then(JsonValue::as_bool), Some(true));
    }

    /// A truncated document is rejected rather than half-read.
    #[test]
    fn a_truncated_document_is_rejected() {
        assert!(JsonImporter::parse_value(r#"{"a":[1,2"#, 0).is_none());
        assert!(JsonImporter::parse_value(r#"{"a":"#, 0).is_none());
        assert!(JsonImporter::import_board(r#"{"name":"B","columns":[{"id":1"#).is_err());
    }

    use guitk::event::Modifiers;

    // ---- Id tests ----

    #[test]
    fn test_id_uniqueness() {
        let a = Id::new();
        let b = Id::new();
        assert_ne!(a, b);
    }

    #[test]
    fn test_id_equality() {
        let a = Id(42);
        let b = Id(42);
        assert_eq!(a, b);
    }

    // ---- Priority tests ----

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Low < Priority::Medium);
        assert!(Priority::Medium < Priority::High);
        assert!(Priority::High < Priority::Critical);
    }

    #[test]
    fn test_priority_label() {
        assert_eq!(Priority::Low.label(), "Low");
        assert_eq!(Priority::Critical.label(), "Critical");
    }

    #[test]
    fn test_priority_color_not_default() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let c = Priority::High.color(&pal);
        assert_ne!(c, Color::BLACK);
    }

    #[test]
    fn test_priority_all() {
        assert_eq!(Priority::all().len(), 4);
    }

    #[test]
    fn test_priority_cycle() {
        assert_eq!(Priority::Low.next(), Priority::Medium);
        assert_eq!(Priority::Medium.next(), Priority::High);
        assert_eq!(Priority::High.next(), Priority::Critical);
        assert_eq!(Priority::Critical.next(), Priority::Low);
    }

    // ---- SimpleDate tests ----

    #[test]
    fn test_simple_date_display() {
        let d = SimpleDate::new(2026, 3, 15);
        assert_eq!(d.display(), "2026-03-15");
    }

    #[test]
    fn test_simple_date_ordering() {
        let a = SimpleDate::new(2026, 1, 1);
        let b = SimpleDate::new(2026, 6, 15);
        assert!(a < b);
    }

    #[test]
    fn test_date_edge_display() {
        let d = SimpleDate::new(2026, 0, 0);
        // clamps to 1
        assert_eq!(d.display(), "2026-01-01");
    }

    // ---- Label tests ----

    #[test]
    fn test_label_creation() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let l = Label::new("Bug", pal.red);
        assert_eq!(l.name, "Bug");
        assert_eq!(l.color, pal.red);
    }

    // ---- ChecklistItem tests ----

    #[test]
    fn test_checklist_item_default_unchecked() {
        let item = ChecklistItem::new("Write tests");
        assert!(!item.done);
        assert_eq!(item.text, "Write tests");
    }

    // ---- Comment tests ----

    #[test]
    fn test_comment_creation() {
        let c = Comment::new("Alice", "Looks good!", 1234);
        assert_eq!(c.author, "Alice");
        assert_eq!(c.text, "Looks good!");
        assert_eq!(c.timestamp, 1234);
    }

    // ---- Card tests ----

    #[test]
    fn test_card_new() {
        let card = Card::new("Test card");
        assert_eq!(card.title, "Test card");
        assert_eq!(card.priority, Priority::Medium);
        assert!(card.labels.is_empty());
        assert!(!card.archived);
    }

    #[test]
    fn test_card_builder() {
        let card = Card::new("Task")
            .with_description("A task description")
            .with_priority(Priority::High)
            .with_assignee("Bob");
        assert_eq!(card.description, "A task description");
        assert_eq!(card.priority, Priority::High);
        assert_eq!(card.assignee, "Bob");
    }

    #[test]
    fn test_card_with_due_date() {
        let card = Card::new("Task").with_due_date(SimpleDate::new(2026, 12, 25));
        assert_eq!(card.due_date, Some(SimpleDate::new(2026, 12, 25)));
    }

    #[test]
    fn test_card_with_label() {
        let lid = Id::new();
        let card = Card::new("Task").with_label(lid);
        assert!(card.has_label(lid));
    }

    #[test]
    fn test_card_label_no_duplicates() {
        let lid = Id::new();
        let card = Card::new("Task").with_label(lid).with_label(lid);
        assert_eq!(card.labels.len(), 1);
    }

    #[test]
    fn test_card_swimlane() {
        let card = Card::new("Task").with_swimlane("Frontend");
        assert_eq!(card.swimlane, "Frontend");
    }

    #[test]
    fn test_card_checklist_progress_empty() {
        let card = Card::new("Task");
        assert_eq!(card.checklist_progress(), (0, 0));
    }

    #[test]
    fn test_card_checklist_progress() {
        let mut card = Card::new("Task");
        card.add_checklist_item("A");
        card.add_checklist_item("B");
        let item_id = card.checklist.first().map(|i| i.id).unwrap();
        card.toggle_checklist_item(item_id);
        assert_eq!(card.checklist_progress(), (1, 2));
    }

    #[test]
    fn test_card_toggle_checklist_twice() {
        let mut card = Card::new("Task");
        card.add_checklist_item("A");
        let item_id = card.checklist.first().map(|i| i.id).unwrap();
        card.toggle_checklist_item(item_id);
        card.toggle_checklist_item(item_id);
        assert_eq!(card.checklist_progress(), (0, 1));
    }

    #[test]
    fn test_card_add_comment() {
        let mut card = Card::new("Task");
        card.add_comment("Alice", "Hello", 100);
        assert_eq!(card.comments.len(), 1);
    }

    // ---- SortBy tests ----

    #[test]
    fn test_sort_by_label() {
        assert_eq!(SortBy::Priority.label(), "Priority");
        assert_eq!(SortBy::Title.label(), "Title");
    }

    #[test]
    fn test_sort_by_all() {
        assert_eq!(SortBy::all().len(), 4);
    }

    // ---- Column tests ----

    #[test]
    fn test_column_new() {
        let col = Column::new("Backlog");
        assert_eq!(col.name, "Backlog");
        assert!(col.card_ids.is_empty());
        assert!(col.wip_limit.is_none());
    }

    #[test]
    fn test_column_wip_limit() {
        let col = Column::new("In Progress").with_wip_limit(3);
        assert_eq!(col.wip_limit, Some(3));
    }

    #[test]
    fn test_column_not_over_wip() {
        let col = Column::new("Col").with_wip_limit(5);
        assert!(!col.is_over_wip_limit());
    }

    #[test]
    fn test_column_over_wip() {
        let mut col = Column::new("Col").with_wip_limit(1);
        col.card_ids.push(Id::new());
        col.card_ids.push(Id::new());
        assert!(col.is_over_wip_limit());
    }

    #[test]
    fn test_column_no_wip_limit_never_over() {
        let mut col = Column::new("Col");
        for _ in 0..100 {
            col.card_ids.push(Id::new());
        }
        assert!(!col.is_over_wip_limit());
    }

    // ---- Board tests ----

    #[test]
    fn test_board_default() {
        let board = Board::default_board();
        assert_eq!(board.columns.len(), 5);
        assert!(!board.labels.is_empty());
    }

    #[test]
    fn test_board_add_card() {
        let mut board = Board::default_board();
        let card = Card::new("Test");
        let id = board.add_card_to_column(card, 0);
        assert!(id.is_some());
        assert_eq!(board.cards.len(), 1);
    }

    #[test]
    fn test_board_add_card_invalid_column() {
        let mut board = Board::default_board();
        let card = Card::new("Test");
        let id = board.add_card_to_column(card, 99);
        assert!(id.is_none());
    }

    #[test]
    fn test_board_move_card() {
        let mut board = Board::default_board();
        let card = Card::new("Test");
        let card_id = card.id;
        board.add_card_to_column(card, 0);
        let result = board.move_card(card_id, 0, 1, 0);
        assert!(result);
        assert!(!board.columns.first().unwrap().card_ids.contains(&card_id));
        assert!(board.columns.get(1).unwrap().card_ids.contains(&card_id));
    }

    #[test]
    fn test_board_move_card_invalid_source() {
        let mut board = Board::default_board();
        let result = board.move_card(Id(9999), 0, 1, 0);
        assert!(!result);
    }

    #[test]
    fn test_board_move_card_invalid_column() {
        let mut board = Board::default_board();
        let result = board.move_card(Id(1), 99, 0, 0);
        assert!(!result);
    }

    #[test]
    fn test_board_archive_card() {
        let mut board = Board::default_board();
        let card = Card::new("Test");
        let card_id = card.id;
        board.add_card_to_column(card, 0);
        let result = board.archive_card(card_id);
        assert!(result);
        assert!(board.cards.get(&card_id).unwrap().archived);
        assert!(board.archived_card_ids.contains(&card_id));
        assert!(!board.columns.first().unwrap().card_ids.contains(&card_id));
    }

    #[test]
    fn test_board_unarchive_card() {
        let mut board = Board::default_board();
        let card = Card::new("Test");
        let card_id = card.id;
        board.add_card_to_column(card, 0);
        board.archive_card(card_id);
        let result = board.unarchive_card(card_id, 2);
        assert!(result);
        assert!(!board.cards.get(&card_id).unwrap().archived);
        assert!(board.columns.get(2).unwrap().card_ids.contains(&card_id));
    }

    #[test]
    fn test_board_delete_card() {
        let mut board = Board::default_board();
        let card = Card::new("Test");
        let card_id = card.id;
        board.add_card_to_column(card, 0);
        let result = board.delete_card(card_id);
        assert!(result);
        assert!(!board.cards.contains_key(&card_id));
    }

    #[test]
    fn test_board_delete_nonexistent() {
        let mut board = Board::default_board();
        let result = board.delete_card(Id(9999));
        assert!(!result);
    }

    #[test]
    fn test_board_add_column() {
        let mut board = Board::default_board();
        let initial = board.columns.len();
        board.add_column("Testing");
        assert_eq!(board.columns.len(), initial + 1);
    }

    #[test]
    fn test_board_remove_column() {
        let mut board = Board::default_board();
        let initial = board.columns.len();
        let removed = board.remove_column(0);
        assert!(removed.is_some());
        assert_eq!(board.columns.len(), initial - 1);
    }

    #[test]
    fn test_board_remove_column_invalid() {
        let mut board = Board::default_board();
        let removed = board.remove_column(99);
        assert!(removed.is_none());
    }

    #[test]
    fn test_board_find_card_column() {
        let mut board = Board::default_board();
        let card = Card::new("Test");
        let card_id = card.id;
        board.add_card_to_column(card, 2);
        assert_eq!(board.find_card_column(card_id), Some(2));
    }

    #[test]
    fn test_board_find_card_column_not_found() {
        let board = Board::default_board();
        assert_eq!(board.find_card_column(Id(9999)), None);
    }

    #[test]
    fn test_board_column_stats() {
        let board = Board::default_board();
        let stats = board.column_stats();
        assert_eq!(stats.len(), 5);
    }

    #[test]
    fn test_board_completion_rate_empty() {
        let board = Board::default_board();
        // No cards: 0 / 0 = 0.0
        assert_eq!(board.completion_rate(), 0.0);
    }

    #[test]
    fn test_board_completion_rate_with_cards() {
        let mut board = Board::default_board();
        board.add_card_to_column(Card::new("A"), 4); // Done column
        board.add_card_to_column(Card::new("B"), 0); // Backlog
        // 1 done out of 2 = 50%
        assert!((board.completion_rate() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_board_sort_column_by_priority() {
        let mut board = Board::default_board();
        board.add_card_to_column(Card::new("Low").with_priority(Priority::Low), 0);
        board.add_card_to_column(Card::new("Critical").with_priority(Priority::Critical), 0);
        board.add_card_to_column(Card::new("High").with_priority(Priority::High), 0);
        board.sort_column(0);

        let ids: Vec<Id> = board.columns.first().unwrap().card_ids.clone();
        let priorities: Vec<Priority> = ids
            .iter()
            .map(|id| board.cards.get(id).unwrap().priority)
            .collect();
        // Sorted descending by priority
        assert_eq!(priorities.first(), Some(&Priority::Critical));
        assert_eq!(priorities.last(), Some(&Priority::Low));
    }

    #[test]
    fn test_board_sort_column_by_title() {
        let mut board = Board::default_board();
        board.add_card_to_column(Card::new("Zebra"), 0);
        board.add_card_to_column(Card::new("Apple"), 0);
        if let Some(col) = board.columns.get_mut(0) {
            col.sort_by = SortBy::Title;
        }
        board.sort_column(0);

        let ids: Vec<Id> = board.columns.first().unwrap().card_ids.clone();
        let titles: Vec<&str> = ids
            .iter()
            .map(|id| board.cards.get(id).unwrap().title.as_str())
            .collect();
        assert_eq!(titles.first().copied(), Some("Apple"));
        assert_eq!(titles.last().copied(), Some("Zebra"));
    }

    #[test]
    fn test_board_swimlane_cards() {
        let mut board = Board::default_board();
        board.add_card_to_column(Card::new("A").with_swimlane("Frontend"), 0);
        board.add_card_to_column(Card::new("B").with_swimlane("Backend"), 0);
        board.add_card_to_column(Card::new("C").with_swimlane("Frontend"), 0);

        let frontend = board.swimlane_cards(0, "Frontend");
        assert_eq!(frontend.len(), 2);
        let backend = board.swimlane_cards(0, "Backend");
        assert_eq!(backend.len(), 1);
    }

    #[test]
    fn test_board_get_label_by_id() {
        let board = Board::default_board();
        let first_label = board.labels.first().unwrap();
        let found = board.get_label_by_id(first_label.id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, first_label.name);
    }

    #[test]
    fn test_board_get_label_not_found() {
        let board = Board::default_board();
        assert!(board.get_label_by_id(Id(99999)).is_none());
    }

    // ---- FilterState tests ----

    #[test]
    fn test_filter_default_inactive() {
        let f = FilterState::default();
        assert!(!f.is_active());
    }

    #[test]
    fn test_filter_active_with_search() {
        let f = FilterState {
            search_text: "bug".to_string(),
            ..Default::default()
        };
        assert!(f.is_active());
    }

    #[test]
    fn test_filter_active_with_priority() {
        let f = FilterState {
            priority_filter: Some(Priority::High),
            ..Default::default()
        };
        assert!(f.is_active());
    }

    #[test]
    fn test_filter_matches_all() {
        let f = FilterState::default();
        let card = Card::new("Test");
        assert!(f.matches(&card));
    }

    #[test]
    fn test_filter_matches_priority() {
        let f = FilterState {
            priority_filter: Some(Priority::High),
            ..Default::default()
        };
        let card_match = Card::new("A").with_priority(Priority::High);
        let card_no = Card::new("B").with_priority(Priority::Low);
        assert!(f.matches(&card_match));
        assert!(!f.matches(&card_no));
    }

    #[test]
    fn test_filter_matches_search_title() {
        let f = FilterState {
            search_text: "memory".to_string(),
            ..Default::default()
        };
        let card = Card::new("Fix memory leak");
        assert!(f.matches(&card));
    }

    #[test]
    fn test_filter_matches_search_description() {
        let f = FilterState {
            search_text: "allocator".to_string(),
            ..Default::default()
        };
        let card = Card::new("Bug").with_description("Issue with allocator");
        assert!(f.matches(&card));
    }

    #[test]
    fn test_filter_no_match_search() {
        let f = FilterState {
            search_text: "nonexistent".to_string(),
            ..Default::default()
        };
        let card = Card::new("Some card");
        assert!(!f.matches(&card));
    }

    #[test]
    fn test_filter_matches_assignee() {
        let f = FilterState {
            assignee_filter: "alice".to_string(),
            ..Default::default()
        };
        let card = Card::new("Task").with_assignee("Alice");
        assert!(f.matches(&card));
    }

    #[test]
    fn test_filter_matches_label() {
        let lid = Id::new();
        let f = FilterState {
            label_filter: Some(lid),
            ..Default::default()
        };
        let card_yes = Card::new("A").with_label(lid);
        let card_no = Card::new("B");
        assert!(f.matches(&card_yes));
        assert!(!f.matches(&card_no));
    }

    #[test]
    fn test_filter_clear() {
        let mut f = FilterState {
            search_text: "hello".to_string(),
            priority_filter: Some(Priority::High),
            assignee_filter: "Alice".to_string(),
            label_filter: Some(Id::new()),
        };
        f.clear();
        assert!(!f.is_active());
    }

    // ---- JSON export tests ----

    #[test]
    fn test_json_escape() {
        assert_eq!(JsonExporter::escape_json("hello"), "hello");
        assert_eq!(JsonExporter::escape_json("a\"b"), "a\\\"b");
        assert_eq!(JsonExporter::escape_json("a\\b"), "a\\\\b");
        assert_eq!(JsonExporter::escape_json("a\nb"), "a\\nb");
    }

    #[test]
    fn test_json_export_card() {
        let card = Card::new("Test").with_priority(Priority::High);
        let json = JsonExporter::export_card(&card);
        assert!(json.contains("\"title\":\"Test\""));
        assert!(json.contains("\"priority\":\"High\""));
    }

    #[test]
    fn test_json_export_column() {
        let col = Column::new("Backlog");
        let json = JsonExporter::export_column(&col);
        assert!(json.contains("\"name\":\"Backlog\""));
    }

    #[test]
    fn test_json_export_label() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let label = Label::new("Bug", pal.red);
        let json = JsonExporter::export_label(&label);
        assert!(json.contains("\"name\":\"Bug\""));
        assert!(json.contains("\"color\":\"#"));
    }

    #[test]
    fn test_json_export_board() {
        let board = Board::default_board();
        let json = JsonExporter::export_board(&board);
        assert!(json.contains("\"name\":\"My Project\""));
        assert!(json.contains("\"columns\":["));
        assert!(json.contains("\"labels\":["));
    }

    #[test]
    fn test_json_export_with_cards() {
        let mut board = Board::default_board();
        board.add_card_to_column(Card::new("First task"), 0);
        let json = JsonExporter::export_board(&board);
        assert!(json.contains("\"title\":\"First task\""));
    }

    #[test]
    fn test_json_parse_string() {
        let data = "\"hello world\"";
        let (val, end) = JsonImporter::parse_string(data, 0).unwrap();
        assert_eq!(val, "hello world");
        assert_eq!(end, data.len());
    }

    #[test]
    fn test_json_parse_escaped_string() {
        let data = "\"a\\\"b\"";
        let (val, _) = JsonImporter::parse_string(data, 0).unwrap();
        assert_eq!(val, "a\"b");
    }

    /// A card title is arbitrary user text. Importing it byte-at-a-time as
    /// `byte as char` reads UTF-8 as Latin-1 and silently mojibakes every
    /// non-ASCII character — damage that is then written back on the next save.
    #[test]
    fn a_non_ascii_string_is_imported_unchanged() {
        for text in ["日本語のカード", "Ωμέγα", "café", "🚀 ship it", "Ω日🚀"] {
            let data = format!("\"{text}\"");
            let (val, end) = JsonImporter::parse_string(&data, 0)
                .unwrap_or_else(|| panic!("failed to parse {data:?}"));
            assert_eq!(val, text, "content changed for {text:?}");
            assert_eq!(end, data.len(), "wrong end offset for {text:?}");
        }
    }

    /// The exporter is the only producer the importer must understand, so
    /// everything it can emit must come back out identical.
    #[test]
    fn export_escaping_round_trips_through_import() {
        for text in [
            "plain",
            "with \"quotes\"",
            "back\\slash",
            "line\nbreak\ttab\r",
            "control\u{01}char",
            "日本語 \"引用\" と\\バックスラッシュ",
            "emoji 😀 and\ttab",
        ] {
            let escaped = JsonExporter::escape_json(text);
            let data = format!("\"{escaped}\"");
            let (val, end) = JsonImporter::parse_string(&data, 0)
                .unwrap_or_else(|| panic!("failed to parse {data:?}"));
            assert_eq!(
                val, text,
                "round trip changed {text:?} (escaped {escaped:?})"
            );
            assert_eq!(end, data.len(), "wrong end offset for {text:?}");
        }
    }

    /// `\uXXXX` is what the exporter emits for control characters, and what any
    /// other JSON producer may emit for anything at all.
    #[test]
    fn unicode_escapes_including_surrogate_pairs_are_decoded() {
        let cases = [
            (r#""\u0041""#, "A"),
            (r#""\u00e9""#, "é"),
            (r#""\u65e5\u672c""#, "日本"),
            // Astral characters are spelled as a surrogate pair in JSON.
            (r#""\ud83d\ude00""#, "😀"),
            (r#""a\u0001b""#, "a\u{01}b"),
        ];
        for (data, want) in cases {
            let (val, end) = JsonImporter::parse_string(data, 0)
                .unwrap_or_else(|| panic!("failed to parse {data}"));
            assert_eq!(val, want, "wrong decode of {data}");
            assert_eq!(end, data.len(), "wrong end offset for {data}");
        }
    }

    /// An unpaired surrogate has no scalar value; one malformed escape must not
    /// abort the import of an otherwise-good board.
    #[test]
    fn a_lone_surrogate_becomes_the_replacement_character() {
        let (val, _) = JsonImporter::parse_string(r#""x\ud83dy""#, 0).expect("should not fail");
        assert_eq!(val, "x\u{FFFD}y");
    }

    /// Control: the ASCII path must be byte-for-byte what it always was.
    #[test]
    fn ascii_parsing_is_unchanged() {
        let cases = [
            (r#""hello world""#, "hello world"),
            (r#""a\"b""#, "a\"b"),
            (r#""a\\b""#, "a\\b"),
            (r#""a\nb\tc""#, "a\nb\tc"),
            (r#""""#, ""),
        ];
        for (data, want) in cases {
            let (val, end) = JsonImporter::parse_string(data, 0)
                .unwrap_or_else(|| panic!("failed to parse {data}"));
            assert_eq!(val, want, "wrong parse of {data}");
            assert_eq!(end, data.len(), "wrong end offset for {data}");
        }
    }

    #[test]
    fn test_json_parse_number() {
        let data = "12345,";
        let (val, end) = JsonImporter::parse_number(data, 0).unwrap();
        assert_eq!(val, 12345);
        assert_eq!(end, 5);
    }

    #[test]
    fn test_json_skip_whitespace() {
        let data = "   hello";
        let pos = JsonImporter::skip_ws(data, 0);
        assert_eq!(pos, 3);
    }

    /// The default board survives a round trip.
    ///
    /// This replaces `test_json_validate_export`, which asserted that
    /// `validate_export` returned true. That function exported the board and
    /// asked whether the string was non-empty, and `export_board` always
    /// writes at least a header -- **so the function could not return false
    /// and the test could not fail.** A vacuous test guarding a vacuous
    /// validator, each making the other look covered.
    #[test]
    fn the_default_board_survives_a_round_trip() {
        let board = Board::default_board();
        let json = JsonExporter::export_board(&board);
        let back = JsonImporter::import_board(&json).expect("our own export must import");

        assert_eq!(back.name, board.name);
        assert_eq!(
            back.columns.len(),
            board.columns.len(),
            "a column was lost in the round trip"
        );
        assert_eq!(back.cards.len(), board.cards.len(), "a card was lost");
        for column in &board.columns {
            let got = back
                .columns
                .iter()
                .find(|c| c.id == column.id)
                .unwrap_or_else(|| panic!("column {:?} did not come back", column.name));
            assert_eq!(got.card_ids, column.card_ids, "column {:?}", column.name);
        }
    }

    // ---- KanbanApp tests ----

    #[test]
    fn test_app_new() {
        let app = KanbanApp::new();
        assert_eq!(app.boards.len(), 1);
        assert_eq!(app.view, View::Board);
        assert!(app.selected_card.is_none());
    }

    #[test]
    fn test_app_add_card() {
        let mut app = KanbanApp::new();
        let id = app.add_card("New Task", 0);
        assert!(id.is_some());
    }

    #[test]
    fn test_app_create_sample_data() {
        let mut app = KanbanApp::new();
        app.create_sample_data();
        assert!(!app.active_board().cards.is_empty());
    }

    #[test]
    fn test_app_switch_board() {
        let mut app = KanbanApp::new();
        app.add_board("Second Board");
        assert_eq!(app.active_board_idx, 1);
        app.switch_board(0);
        assert_eq!(app.active_board_idx, 0);
    }

    #[test]
    fn test_app_add_board() {
        let mut app = KanbanApp::new();
        app.add_board("New Board");
        assert_eq!(app.boards.len(), 2);
        assert_eq!(app.active_board().name, "New Board");
    }

    #[test]
    fn test_app_filtered_cards() {
        let mut app = KanbanApp::new();
        app.add_card("Visible", 0);
        let ids = app.filtered_card_ids(0);
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn test_app_filtered_cards_with_filter() {
        let mut app = KanbanApp::new();
        app.add_card("Visible", 0);
        app.add_card("Hidden", 0);
        app.filter.search_text = "Visible".to_string();
        let ids = app.filtered_card_ids(0);
        assert_eq!(ids.len(), 1);
    }

    /// Exporting writes a file, and the file reads back as the same board.
    ///
    /// **This replaces `test_app_export_json`, and the replacement is the
    /// point.** That test called `KanbanApp::export_json`, which returned the
    /// serialiser's string and did nothing with it, and asserted the string
    /// was non-empty and contained the board's name. Both assertions were
    /// true, and neither could fail for any reason a user would notice --
    /// `export_board` always writes at least a header.
    ///
    /// Worse, the test was the only thing keeping `export_json` alive.
    /// `write_board` had superseded it -- that one takes a path, writes
    /// atomically through `safeio`, and is what the save picker calls -- and
    /// the leftover sat behind a comment saying "this program cannot write a
    /// file yet", which had stopped being true. A test on the wrong function
    /// is how a superseded function survives being superseded.
    ///
    /// So this asserts the whole door: bytes on disk, read back, parsed, and
    /// the same board on the other side.
    #[test]
    fn exporting_writes_a_file_that_reads_back_as_the_same_board() {
        let dir =
            std::env::temp_dir().join(format!("kanban-export-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&dir).expect("fixture");
        let path = dir.join("board.json");
        let _ = std::fs::remove_file(&path);

        let mut app = KanbanApp::new();
        app.create_sample_data();
        let wanted_cards = app.active_board().cards.len();
        assert!(
            wanted_cards > 0,
            "control: the sample board must have cards, or writing is refused"
        );

        let said = app.write_board(&path);
        assert!(
            said.starts_with("Wrote"),
            "the export did not report writing: {said}"
        );

        let text = std::fs::read_to_string(&path).expect("the file the export named");
        let back = JsonImporter::import_board(&text).expect("our own export must import");
        assert_eq!(back.name, "My Project");
        assert_eq!(
            back.cards.len(),
            wanted_cards,
            "the board that came back has a different number of cards"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_app_timestamp_increment() {
        let mut app = KanbanApp::new();
        let t1 = app.next_timestamp();
        let t2 = app.next_timestamp();
        assert!(t2 > t1);
    }

    // ---- Rendering tests ----

    #[test]
    fn test_render_app_nonempty() {
        let mut app = KanbanApp::new();
        app.create_sample_data();
        let tree = render_app(&app, 1200.0, 800.0);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_toolbar() {
        let app = KanbanApp::new();
        let mut tree = RenderTree::new();
        render_toolbar(&mut tree, &app, 1200.0);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_filter_bar_hidden() {
        let app = KanbanApp::new();
        let mut tree = RenderTree::new();
        let y = render_filter_bar(&mut tree, &app, 1200.0, 40.0);
        // Not shown, same y
        assert_eq!(y, 40.0);
    }

    #[test]
    fn test_render_filter_bar_visible() {
        let mut app = KanbanApp::new();
        app.show_filter_bar = true;
        let mut tree = RenderTree::new();
        let y = render_filter_bar(&mut tree, &app, 1200.0, 40.0);
        assert!(y > 40.0);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_card_detail_no_card() {
        let app = KanbanApp::new();
        let mut tree = RenderTree::new();
        render_card_detail(&mut tree, &app, 800.0, 600.0);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_render_card_detail_with_card() {
        let mut app = KanbanApp::new();
        app.create_sample_data();
        // Select first card
        let first_card_id = app
            .active_board()
            .columns
            .first()
            .and_then(|c| c.card_ids.first().copied());
        app.selected_card = first_card_id;
        app.view = View::CardDetail;
        let tree = render_app(&app, 1200.0, 800.0);
        assert!(!tree.is_empty());
    }

    /// Select the first sample card and give it `description` and `comments`.
    fn app_with_card_prose(description: &str, comments: &[&str]) -> KanbanApp {
        let mut app = KanbanApp::new();
        app.create_sample_data();
        let card_id = app
            .active_board()
            .columns
            .first()
            .and_then(|c| c.card_ids.first().copied())
            .expect("sample data has at least one card");
        {
            let board = app.active_board_mut();
            let card = board.cards.get_mut(&card_id).expect("card exists");
            card.description = description.to_string();
            card.comments.clear();
            for (i, body) in comments.iter().enumerate() {
                card.add_comment("alice", body, i as u64);
            }
        }
        app.selected_card = Some(card_id);
        app.view = View::CardDetail;
        app
    }

    /// Every `Text` command in `tree`, as (y, text).
    fn text_rows(tree: &RenderTree) -> Vec<(f32, String)> {
        tree.commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { y, text, .. } => Some((*y, text.clone())),
                _ => None,
            })
            .collect()
    }

    /// The comment cards, as (y, height), sorted top to bottom.
    ///
    /// Anchored to the "Comments (n)" header rather than matched on colour and
    /// corner radius alone: the pane draws other rounded `SURFACE0` chips (a
    /// label pill sits well above this section) and a looser filter picks them
    /// up too.
    fn comment_cards(tree: &RenderTree) -> Vec<(f32, f32)> {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let header_y = text_rows(tree)
            .into_iter()
            .find(|(_, t)| t.starts_with("Comments ("))
            .expect("the comments section has a header")
            .0;
        // A comment card is filled under the Cards theme and outlined under
        // Borders, so both commands have to count -- and the outline's geometry
        // has to be un-inset back to the rectangle the caller asked for, since
        // `push_surface` strokes half a line inside it. Without that, a body
        // line sitting exactly on a card's top edge reads as outside it.
        let mut cards: Vec<(f32, f32)> = tree
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    y,
                    height,
                    color,
                    corner_radii,
                    ..
                } if *color == pal.surface0 && corner_radii.top_left == 4.0 && *y > header_y => {
                    Some((*y, *height))
                }
                RenderCommand::StrokeRect {
                    y,
                    height,
                    color,
                    corner_radii,
                    line_width,
                    ..
                } if *color == pal.border && corner_radii.top_left == 4.0 && *y > header_y => {
                    Some((y - line_width / 2.0, height + line_width))
                }
                _ => None,
            })
            .collect();
        cards.sort_by(|a, b| a.0.total_cmp(&b.0));
        cards
    }

    #[test]
    fn a_wrapping_description_is_drawn_as_more_than_one_line() {
        // `max_width` on a Text command clips to a single line rather than
        // wrapping, so a long description used to be shown as its first line
        // and nothing else, with no marker that anything was dropped.
        let long = "wrap ".repeat(80);
        let mut tree = RenderTree::new();
        render_card_detail(&mut tree, &app_with_card_prose(&long, &[]), 1200.0, 800.0);
        let desc_lines = text_rows(&tree)
            .into_iter()
            .filter(|(_, t)| t.contains("wrap"))
            .count();
        assert!(
            desc_lines > 1,
            "a long description should be drawn as several lines, got {desc_lines}"
        );
    }

    #[test]
    fn a_long_description_pushes_the_comments_below_it_down() {
        // The whole pane hangs off one running cursor, so a description that
        // reserves a flat height regardless of its length draws the section
        // below it straight through its own last lines.
        let short_y = {
            let mut tree = RenderTree::new();
            render_card_detail(
                &mut tree,
                &app_with_card_prose("short", &["c"]),
                1200.0,
                800.0,
            );
            text_rows(&tree)
                .into_iter()
                .find(|(_, t)| t == "c")
                .expect("comment body drawn")
                .0
        };
        let long_y = {
            let mut tree = RenderTree::new();
            let app = app_with_card_prose(&"wrap ".repeat(80), &["c"]);
            render_card_detail(&mut tree, &app, 1200.0, 800.0);
            text_rows(&tree)
                .into_iter()
                .find(|(_, t)| t == "c")
                .expect("comment body drawn")
                .0
        };
        assert!(
            long_y > short_y,
            "a wrapped description must move the comments below it down \
             ({long_y} should exceed {short_y})"
        );
    }

    #[test]
    fn a_comment_card_contains_the_body_it_draws() {
        // The card is filled before the body is drawn, so its height is a
        // second calculation of the same quantity -- the defect class this
        // whole sweep is about. Assert the box actually contains its text.
        let mut tree = RenderTree::new();
        let app = app_with_card_prose("d", &[&"comment ".repeat(40)]);
        render_card_detail(&mut tree, &app, 1200.0, 800.0);

        let cards = comment_cards(&tree);
        // Lines *of* the body, which is "comment comment ...": the footer
        // naming the card's keys says "C comment" too, and is not in a card.
        let body_rows: Vec<f32> = text_rows(&tree)
            .into_iter()
            .filter(|(_, t)| t.starts_with("comment"))
            .map(|(y, _)| y)
            .collect();
        assert!(body_rows.len() > 1, "the comment body should have wrapped");

        for y in body_rows {
            assert!(
                cards.iter().any(|(cy, ch)| y >= *cy && y <= cy + ch),
                "a comment body line at y={y} falls outside every comment card {cards:?}"
            );
        }
    }

    #[test]
    fn consecutive_comment_cards_do_not_overlap() {
        let mut tree = RenderTree::new();
        let app = app_with_card_prose("d", &[&"first ".repeat(40), "second", "third"]);
        render_card_detail(&mut tree, &app, 1200.0, 800.0);

        let cards = comment_cards(&tree);
        assert_eq!(cards.len(), 3, "one card per comment: {cards:?}");
        for pair in cards.windows(2) {
            let (y, h) = pair[0];
            assert!(
                pair[1].0 >= y + h,
                "comment card at {:?} overlaps the one at {:?}",
                pair[1],
                pair[0]
            );
        }
    }

    // -- Scrolling the card-detail modal --
    //
    // Every assertion below is stated in pixels the body actually moved, and
    // sized from `DETAIL_LINE_STEP`/`DETAIL_PAGE_STEP` rather than from a
    // literal. A test that asserted `detail_scroll > 0.0` would pass against a
    // renderer that ignored the field entirely — which is precisely what this
    // app shipped, and what 143 passing tests failed to notice.

    /// A window small enough that a chatty card overflows the modal.
    ///
    /// The modal is capped at 500 px tall regardless, so what makes the fixture
    /// scrollable is the card's content, not the window. Asserts it *is*
    /// scrollable: a fixture that cannot overflow cannot fail these tests.
    fn app_with_scrollable_detail() -> KanbanApp {
        let comments: Vec<String> = (0..12).map(|i| format!("comment number {i}")).collect();
        let refs: Vec<&str> = comments.iter().map(String::as_str).collect();
        let app = app_with_card_prose(&"wrap ".repeat(60), &refs);
        assert!(
            max_detail_scroll(&app, 1200.0, 800.0) > DETAIL_LINE_STEP,
            "fixture must overflow the modal by more than one step, or these \
             tests cannot fail"
        );
        app
    }

    /// The `y` of every `Text` command drawn between the body's clip commands.
    ///
    /// Taken from the clip rather than filtered by coordinate: the title above
    /// the clip does *not* scroll, so a coordinate filter that let it through
    /// would compare the title against itself and report no movement at all,
    /// however broken the body was.
    fn clipped_text_tops(tree: &RenderTree) -> Vec<f32> {
        let mut inside = false;
        let mut tops = Vec::new();
        for cmd in &tree.commands {
            match cmd {
                RenderCommand::PushClip { .. } => inside = true,
                RenderCommand::PopClip => inside = false,
                RenderCommand::Text { y, .. } if inside => tops.push(*y),
                _ => {}
            }
        }
        tops
    }

    fn press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    #[test]
    fn the_detail_body_is_clipped_to_the_modal() {
        // Without this the comments of a chatty card were drawn straight over
        // the desktop, below the modal they were supposed to be inside.
        let app = app_with_scrollable_detail();
        let mut tree = RenderTree::new();
        render_card_detail(&mut tree, &app, 1200.0, 800.0);

        let modal = DetailModal::for_window(1200.0, 800.0);
        let clip = tree
            .commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::PushClip { y, height, .. } => Some((*y, *height)),
                _ => None,
            })
            .expect("the body is clipped");
        assert!(
            (clip.0 - modal.body_top()).abs() < 0.5 && (clip.1 - modal.body_height()).abs() < 0.5,
            "the clip {clip:?} must be the body rectangle ({}, {})",
            modal.body_top(),
            modal.body_height()
        );
    }

    #[test]
    fn one_down_moves_the_detail_body_up_by_a_line() {
        let mut app = app_with_scrollable_detail();
        let before = {
            let mut tree = RenderTree::new();
            render_card_detail(&mut tree, &app, 1200.0, 800.0);
            clipped_text_tops(&tree)
        };
        assert!(handle_key_event(&mut app, &press(Key::Down)));
        let after = {
            let mut tree = RenderTree::new();
            render_card_detail(&mut tree, &app, 1200.0, 800.0);
            clipped_text_tops(&tree)
        };

        let (Some(b), Some(a)) = (before.first(), after.first()) else {
            panic!("the body draws text in both renders");
        };
        assert!(
            (b - a - DETAIL_LINE_STEP).abs() < 0.5,
            "one Down should raise the body by {DETAIL_LINE_STEP} px, moved {}",
            b - a
        );
    }

    #[test]
    fn a_page_moves_the_detail_body_further_than_a_line() {
        let mut line = app_with_scrollable_detail();
        let mut page = app_with_scrollable_detail();
        assert!(handle_key_event(&mut line, &press(Key::Down)));
        assert!(handle_key_event(&mut page, &press(Key::PageDown)));
        assert!(
            page.detail_scroll > line.detail_scroll,
            "PageDown ({}) must move further than Down ({})",
            page.detail_scroll,
            line.detail_scroll
        );
    }

    #[test]
    fn the_detail_body_cannot_scroll_into_empty_space() {
        // The offset itself is allowed past the end -- only the renderer knows
        // how tall this card came out -- but what is *drawn* must not be.
        let mut app = app_with_scrollable_detail();
        for _ in 0..500 {
            handle_key_event(&mut app, &press(Key::PageDown));
        }
        let mut tree = RenderTree::new();
        render_card_detail(&mut tree, &app, 1200.0, 800.0);

        let modal = DetailModal::for_window(1200.0, 800.0);
        let max = max_detail_scroll(&app, 1200.0, 800.0);
        let mut scratch = RenderTree::new();
        let board = app.active_board();
        let card = board
            .cards
            .get(&app.selected_card.expect("a card is open"))
            .expect("the open card exists");
        render_card_detail_body(
            &mut scratch,
            &app.palette,
            board,
            card,
            0.0,
            0.0,
            modal.content_w(),
            None,
        );
        let content = content_bottom(&scratch.commands).expect("the body draws something");

        // Scrolled to the end, the last thing drawn sits exactly on the body's
        // bottom edge -- never above it, which would be blank space below the
        // content with more content still hidden above.
        let drawn_bottom = modal.body_top() - max + content;
        assert!(
            (drawn_bottom - (modal.body_top() + modal.body_height())).abs() < 0.5,
            "at the end of the scroll the content should finish at the body's \
             bottom ({}), got {drawn_bottom}",
            modal.body_top() + modal.body_height()
        );
    }

    #[test]
    fn the_detail_body_cannot_scroll_above_its_top() {
        let mut app = app_with_scrollable_detail();
        for _ in 0..50 {
            handle_key_event(&mut app, &press(Key::PageUp));
        }
        assert!(
            app.detail_scroll.abs() < f32::EPSILON,
            "scrolling up from the top must not bank a debt that later Downs \
             have to repay first, got {}",
            app.detail_scroll
        );

        // And one Down from there must move the body immediately.
        assert!(handle_key_event(&mut app, &press(Key::Down)));
        assert!(
            (app.detail_scroll - DETAIL_LINE_STEP).abs() < 0.5,
            "got {}",
            app.detail_scroll
        );
    }

    #[test]
    fn a_card_that_fits_cannot_be_scrolled() {
        let app = app_with_card_prose("short", &[]);
        assert!(
            max_detail_scroll(&app, 1200.0, 800.0).abs() < f32::EPSILON,
            "a card whose content fits the modal has nowhere to scroll to"
        );
    }

    #[test]
    fn opening_a_card_returns_the_detail_body_to_the_top() {
        let mut app = app_with_scrollable_detail();
        handle_key_event(&mut app, &press(Key::PageDown));
        assert!(app.detail_scroll > 0.0);

        // Escape back to the board and open the card again.
        assert!(handle_key_event(&mut app, &press(Key::Escape)));
        let card_id = app
            .active_board()
            .columns
            .first()
            .and_then(|c| c.card_ids.first().copied())
            .expect("sample data has a card");
        app.selected_card = Some(card_id);
        assert!(handle_key_event(&mut app, &press(Key::Enter)));
        assert!(
            app.detail_scroll.abs() < f32::EPSILON,
            "a freshly opened card must start at the top, got {}",
            app.detail_scroll
        );
    }

    #[test]
    fn the_detail_keys_do_not_scroll_the_board_behind_the_modal() {
        // Both views bind the same four keys; the guards must not overlap, or
        // closing the modal would reveal a board that had scrolled itself.
        let mut app = app_with_scrollable_detail();
        let board_before = app.scroll_offset;
        for _ in 0..5 {
            handle_key_event(&mut app, &press(Key::Down));
        }
        assert_eq!(app.scroll_offset, board_before);
        assert!(app.detail_scroll > 0.0);
    }

    #[test]
    fn an_impossibly_small_window_still_bounds_the_detail_scroll() {
        // `600.0.min(width - 40.0)` goes negative for a tiny window, and a
        // negative body height would make the bound claim the body scrolls
        // further than its content is long.
        let app = app_with_scrollable_detail();
        for (w, h) in [(0.0, 0.0), (10.0, 10.0), (39.0, 59.0)] {
            let modal = DetailModal::for_window(w, h);
            assert!(modal.w >= 0.0 && modal.h >= 0.0, "{w}x{h} gave {modal:?}");
            assert!(modal.body_height() >= 0.0, "{w}x{h} gave {modal:?}");
            assert!(
                max_detail_scroll(&app, w, h).is_finite(),
                "{w}x{h} gave a non-finite bound"
            );
        }
    }

    #[test]
    fn test_render_archive_view_empty() {
        let app = KanbanApp::new();
        let mut tree = RenderTree::new();
        render_archive_view(&mut tree, &app, 1200.0, 40.0);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_stats_view() {
        let mut app = KanbanApp::new();
        app.create_sample_data();
        let mut tree = RenderTree::new();
        render_stats_view(&mut tree, &app, 1200.0, 40.0);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_board_list() {
        let app = KanbanApp::new();
        let mut tree = RenderTree::new();
        render_board_list(&mut tree, &app, 1200.0, 40.0);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_input_overlay_hidden() {
        let app = KanbanApp::new();
        let mut tree = RenderTree::new();
        render_input_overlay(&mut tree, &app, 800.0, 600.0);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_render_input_overlay_visible() {
        let mut app = KanbanApp::new();
        app.input_mode = InputMode::NewCardTitle;
        app.input_buffer = "New task".to_string();
        let mut tree = RenderTree::new();
        render_input_overlay(&mut tree, &app, 800.0, 600.0);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_board_no_columns() {
        let mut app = KanbanApp::new();
        app.active_board_mut().columns.clear();
        let mut tree = RenderTree::new();
        render_board_view(&mut tree, &app, 1200.0, 800.0, 40.0);
        assert!(!tree.is_empty()); // Shows "No columns" message
    }

    // ---- Event handling tests ----

    fn make_key(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: String::new(),
        }
    }

    fn make_char_key(ch: char) -> KeyEvent {
        KeyEvent {
            key: Key::Unknown(ch as u32),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        }
    }

    #[test]
    fn test_key_escape_from_detail() {
        let mut app = KanbanApp::new();
        app.view = View::CardDetail;
        let handled = handle_key_event(&mut app, &make_key(Key::Escape, Modifiers::NONE));
        assert!(handled);
        assert_eq!(app.view, View::Board);
    }

    #[test]
    fn test_key_escape_from_archive() {
        let mut app = KanbanApp::new();
        app.view = View::Archive;
        let handled = handle_key_event(&mut app, &make_key(Key::Escape, Modifiers::NONE));
        assert!(handled);
        assert_eq!(app.view, View::Board);
    }

    #[test]
    fn test_key_n_starts_new_card() {
        let mut app = KanbanApp::new();
        let handled = handle_key_event(&mut app, &make_key(Key::N, Modifiers::NONE));
        assert!(handled);
        assert_eq!(app.input_mode, InputMode::NewCardTitle);
    }

    #[test]
    fn test_key_shift_c_new_column() {
        let mut app = KanbanApp::new();
        let handled = handle_key_event(&mut app, &make_key(Key::C, Modifiers::shift()));
        assert!(handled);
        assert_eq!(app.input_mode, InputMode::NewColumnName);
    }

    #[test]
    fn test_key_ctrl_f_toggle_filter() {
        let mut app = KanbanApp::new();
        assert!(!app.show_filter_bar);
        handle_key_event(&mut app, &make_key(Key::F, Modifiers::ctrl()));
        assert!(app.show_filter_bar);
        handle_key_event(&mut app, &make_key(Key::F, Modifiers::ctrl()));
        assert!(!app.show_filter_bar);
    }

    #[test]
    fn test_key_left_right_navigation() {
        let mut app = KanbanApp::new();
        assert_eq!(app.selected_column, 0);
        handle_key_event(&mut app, &make_key(Key::Right, Modifiers::NONE));
        assert_eq!(app.selected_column, 1);
        handle_key_event(&mut app, &make_key(Key::Left, Modifiers::NONE));
        assert_eq!(app.selected_column, 0);
    }

    #[test]
    fn test_key_left_at_zero() {
        let mut app = KanbanApp::new();
        handle_key_event(&mut app, &make_key(Key::Left, Modifiers::NONE));
        assert_eq!(app.selected_column, 0);
    }

    #[test]
    fn test_key_priority_cycle() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Test", 0).unwrap();
        app.selected_card = Some(id);
        // Default is Medium
        handle_key_event(&mut app, &make_key(Key::P, Modifiers::NONE));
        assert_eq!(
            app.active_board().cards.get(&id).unwrap().priority,
            Priority::High
        );
    }

    #[test]
    fn test_key_move_card_right() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Test", 0).unwrap();
        app.selected_card = Some(id);
        handle_key_event(&mut app, &make_key(Key::M, Modifiers::NONE));
        assert_eq!(app.active_board().find_card_column(id), Some(1));
    }

    #[test]
    fn test_key_move_card_left() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Test", 1).unwrap();
        app.selected_card = Some(id);
        handle_key_event(&mut app, &make_key(Key::B, Modifiers::NONE));
        assert_eq!(app.active_board().find_card_column(id), Some(0));
    }

    #[test]
    fn test_key_sort_column() {
        let mut app = KanbanApp::new();
        app.add_card("B", 0);
        app.add_card("A", 0);
        handle_key_event(&mut app, &make_key(Key::T, Modifiers::NONE));
        // Sort happened (by priority, which are both Medium, so stable)
        assert_eq!(
            app.active_board().columns.first().unwrap().card_ids.len(),
            2
        );
    }

    #[test]
    fn test_key_ctrl_d_delete() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Test", 0).unwrap();
        app.selected_card = Some(id);
        handle_key_event(&mut app, &make_key(Key::D, Modifiers::ctrl()));
        assert!(!app.active_board().cards.contains_key(&id));
        assert!(app.selected_card.is_none());
    }

    #[test]
    fn test_key_ctrl_a_archive() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Test", 0).unwrap();
        app.selected_card = Some(id);
        handle_key_event(&mut app, &make_key(Key::A, Modifiers::ctrl()));
        assert!(app.active_board().cards.get(&id).unwrap().archived);
    }

    #[test]
    fn test_key_alt_number_views() {
        let mut app = KanbanApp::new();
        handle_key_event(&mut app, &make_key(Key::Num2, Modifiers::alt()));
        assert_eq!(app.view, View::Statistics);
        handle_key_event(&mut app, &make_key(Key::Num3, Modifiers::alt()));
        assert_eq!(app.view, View::Archive);
        handle_key_event(&mut app, &make_key(Key::Num4, Modifiers::alt()));
        assert_eq!(app.view, View::BoardList);
        handle_key_event(&mut app, &make_key(Key::Num1, Modifiers::alt()));
        assert_eq!(app.view, View::Board);
    }

    #[test]
    fn test_input_mode_escape() {
        let mut app = KanbanApp::new();
        app.input_mode = InputMode::NewCardTitle;
        app.input_buffer = "partial".to_string();
        handle_key_event(&mut app, &make_key(Key::Escape, Modifiers::NONE));
        assert_eq!(app.input_mode, InputMode::None);
        assert!(app.input_buffer.is_empty());
    }

    #[test]
    fn test_input_mode_enter_creates_card() {
        let mut app = KanbanApp::new();
        app.input_mode = InputMode::NewCardTitle;
        app.input_buffer = "New Task".to_string();
        app.selected_column = 0;
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.input_mode, InputMode::None);
        assert_eq!(
            app.active_board().columns.first().unwrap().card_ids.len(),
            1
        );
    }

    #[test]
    fn test_input_mode_enter_empty_noop() {
        let mut app = KanbanApp::new();
        app.input_mode = InputMode::NewCardTitle;
        app.input_buffer.clear();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.input_mode, InputMode::None);
        assert!(
            app.active_board()
                .columns
                .first()
                .unwrap()
                .card_ids
                .is_empty()
        );
    }

    #[test]
    fn test_input_mode_backspace() {
        let mut app = KanbanApp::new();
        app.input_mode = InputMode::NewCardTitle;
        app.input_buffer = "abc".to_string();
        handle_key_event(&mut app, &make_key(Key::Backspace, Modifiers::NONE));
        assert_eq!(app.input_buffer, "ab");
    }

    #[test]
    fn test_input_mode_char_append() {
        let mut app = KanbanApp::new();
        app.input_mode = InputMode::NewCardTitle;
        app.input_buffer = "he".to_string();
        handle_key_event(&mut app, &make_char_key('l'));
        assert_eq!(app.input_buffer, "hel");
    }

    #[test]
    fn test_input_mode_new_column() {
        let mut app = KanbanApp::new();
        let initial_cols = app.active_board().columns.len();
        app.input_mode = InputMode::NewColumnName;
        app.input_buffer = "Testing".to_string();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.active_board().columns.len(), initial_cols + 1);
    }

    #[test]
    fn test_input_mode_new_board() {
        let mut app = KanbanApp::new();
        app.input_mode = InputMode::NewBoardName;
        app.input_buffer = "Second Project".to_string();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.boards.len(), 2);
    }

    #[test]
    fn test_input_mode_add_comment() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Task", 0).unwrap();
        app.selected_card = Some(id);
        app.input_mode = InputMode::AddComment;
        app.input_buffer = "Great progress!".to_string();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.active_board().cards.get(&id).unwrap().comments.len(), 1);
    }

    #[test]
    fn test_input_mode_add_checklist() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Task", 0).unwrap();
        app.selected_card = Some(id);
        app.input_mode = InputMode::AddChecklistItem;
        app.input_buffer = "Write tests".to_string();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(
            app.active_board().cards.get(&id).unwrap().checklist.len(),
            1
        );
    }

    #[test]
    fn test_input_mode_edit_title() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Old Title", 0).unwrap();
        app.selected_card = Some(id);
        app.input_mode = InputMode::EditCardTitle;
        app.input_buffer = "New Title".to_string();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(
            app.active_board().cards.get(&id).unwrap().title,
            "New Title"
        );
    }

    #[test]
    fn test_input_mode_edit_description() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Task", 0).unwrap();
        app.selected_card = Some(id);
        app.input_mode = InputMode::CardDescription;
        app.input_buffer = "New description".to_string();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(
            app.active_board().cards.get(&id).unwrap().description,
            "New description"
        );
    }

    #[test]
    fn test_input_mode_rename_column() {
        let mut app = KanbanApp::new();
        app.selected_column = 0;
        app.input_mode = InputMode::RenameColumn;
        app.input_buffer = "Inbox".to_string();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.active_board().columns.first().unwrap().name, "Inbox");
    }

    #[test]
    fn test_key_not_pressed_ignored() {
        let mut app = KanbanApp::new();
        let key = KeyEvent {
            key: Key::N,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        let handled = handle_key_event(&mut app, &key);
        assert!(!handled);
    }

    #[test]
    fn test_enter_opens_card_detail() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Task", 0).unwrap();
        app.selected_card = Some(id);
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.view, View::CardDetail);
    }

    #[test]
    fn test_enter_no_card_noop() {
        let mut app = KanbanApp::new();
        let handled = handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert!(!handled);
        assert_eq!(app.view, View::Board);
    }

    // ---- Palette tests ----

    #[test]
    fn test_palette_colors_distinct() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let colors = [
            pal.base,
            pal.mantle,
            pal.crust,
            pal.surface0,
            pal.text,
            pal.blue,
            pal.red,
            pal.green,
        ];
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(
                    colors.get(i),
                    colors.get(j),
                    "colors at {} and {} should differ",
                    i,
                    j
                );
            }
        }
    }

    // ---- Widget integration tests ----

    #[test]
    fn test_widget_tree_render() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let root = Widget::container()
            .with_background(pal.crust)
            .with_flex_direction(FlexDirection::Column);
        let mut wt = WidgetTree::new(root, 1200.0, 800.0);
        wt.layout();
        let rt = wt.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_column_active_card_count() {
        let mut board = Board::default_board();
        board.add_card_to_column(Card::new("A"), 0);
        board.add_card_to_column(Card::new("B"), 0);
        let count = board
            .columns
            .first()
            .unwrap()
            .active_card_count(&board.cards);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_column_active_count_excludes_archived() {
        let mut board = Board::default_board();
        let c1 = Card::new("A");
        let c1_id = c1.id;
        board.add_card_to_column(c1, 0);
        board.add_card_to_column(Card::new("B"), 0);
        board.archive_card(c1_id);
        let count = board
            .columns
            .first()
            .unwrap()
            .active_card_count(&board.cards);
        assert_eq!(count, 1);
    }

    // ---- board-view scrolling ----

    /// Test window size for the board view. Tall enough to show several cards
    /// and short enough that a long column has to hide some.
    const TEST_W: f32 = 1200.0;
    const TEST_H: f32 = 600.0;

    /// An app whose first column holds `n` cards named `card0`..`card{n-1}`.
    fn app_with_cards(n: usize) -> KanbanApp {
        let mut app = KanbanApp::new();
        for i in 0..n {
            app.add_card(&format!("card{i}"), 0);
        }
        // The size these tests draw at, which is what `render` would have
        // recorded: the keys scroll against the window last drawn.
        app.win_width = TEST_W;
        app.win_height = TEST_H;
        app
    }

    /// The `cardN` titles the board view actually drew, in draw order.
    fn drawn_card_titles(app: &KanbanApp) -> Vec<String> {
        render_app(app, TEST_W, TEST_H)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } if text.starts_with("card") => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The lowest pixel any command reaches, so "did it draw past the window?"
    /// is answered from the output rather than from the code that produced it.
    fn lowest_pixel(app: &KanbanApp) -> f32 {
        render_app(app, TEST_W, TEST_H)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { y, height, .. }
                | RenderCommand::StrokeRect { y, height, .. } => Some(y + height),
                RenderCommand::Text { y, font_size, .. } => Some(y + font_size),
                _ => None,
            })
            // The full-window background is exactly the window; anything below
            // it is what this is looking for.
            .fold(0.0_f32, f32::max)
    }

    #[test]
    fn a_card_is_drawn_the_height_it_was_measured() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        // The column reserves `card_height` and `render_card` returns what it
        // drew. If they disagree the cards overlap, or the last one crosses the
        // column's bottom edge.
        let mut board = Board::default_board();
        let label = board.labels.first().map(|l| l.id);
        for (i, decorate) in [false, true].into_iter().enumerate() {
            let mut card = Card::new(&format!("c{i}"));
            if decorate {
                if let Some(l) = label {
                    card.labels.push(l);
                }
                card.assignee = "someone".to_string();
                card.comments.push(Comment::new("someone", "hi", 1));
            }
            let expected = card_height(&card);
            let mut tree = RenderTree::new();
            let drawn = render_card(&mut tree, &pal, &card, &board, 0.0, 0.0, 200.0, false);
            assert!(
                (drawn - expected).abs() < 0.001,
                "decorate={decorate}: drew {drawn}, measured {expected}"
            );
            board.add_card_to_column(card, 0);
        }
    }

    #[test]
    fn a_column_draws_only_the_cards_that_fit() {
        // The loop used to draw every card in the column, so a column with more
        // cards than the window is tall ran off the bottom and over whatever
        // was beneath it.
        let app = app_with_cards(60);
        let drawn = drawn_card_titles(&app);
        assert!(!drawn.is_empty(), "a 600px board should show some cards");
        assert!(drawn.len() < 60, "a 60-card column cannot fit in 600px");
        assert!(
            lowest_pixel(&app) <= TEST_H,
            "the board drew down to {}, past a {TEST_H}px window",
            lowest_pixel(&app)
        );
    }

    #[test]
    fn a_column_that_is_hiding_cards_says_so() {
        let app = app_with_cards(60);
        let shown = drawn_card_titles(&app).len();
        let note = format!("+{} more", 60 - shown);
        let texts: Vec<String> = render_app(&app, TEST_W, TEST_H)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&note),
            "expected {note:?} among the drawn text, got {texts:?}"
        );

        // A column showing everything makes no such claim.
        let short = app_with_cards(1);
        let short_texts: Vec<String> = render_app(&short, TEST_W, TEST_H)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            !short_texts
                .iter()
                .any(|t| t.starts_with('+') && t.ends_with(" more")),
            "a column that fits should not claim to be hiding cards: {short_texts:?}"
        );
    }

    #[test]
    fn scrolling_the_board_reaches_the_cards_below_the_fold() {
        // `scroll_offset` used to be an `f32` that nothing read and nothing
        // wrote, so the cards past the first screenful were unreachable.
        let mut app = app_with_cards(60);
        assert_eq!(
            drawn_card_titles(&app).first().map(String::as_str),
            Some("card0")
        );

        handle_key_event(&mut app, &make_key(Key::PageDown, Modifiers::NONE));
        assert_eq!(
            drawn_card_titles(&app).first().map(String::as_str),
            Some(&format!("card{BOARD_PAGE_STEP}")[..]),
            "PageDown should move by {BOARD_PAGE_STEP} cards"
        );

        // The last card is reachable, which is the whole point.
        for _ in 0..60 {
            handle_key_event(&mut app, &make_key(Key::PageDown, Modifiers::NONE));
        }
        assert_eq!(
            drawn_card_titles(&app).last().map(String::as_str),
            Some("card59"),
            "the end of a column must be reachable"
        );
        assert!(
            lowest_pixel(&app) <= TEST_H,
            "the last page drew past the window"
        );

        // ... and scrolling back up returns to the top without wrapping.
        for _ in 0..200 {
            handle_key_event(&mut app, &make_key(Key::PageUp, Modifiers::NONE));
        }
        assert_eq!(app.scroll_offset, 0, "the offset must not wrap round");
        assert_eq!(
            drawn_card_titles(&app).first().map(String::as_str),
            Some("card0")
        );
    }

    // == Choosing a card (2026-09-27) ============================================
    //
    // Nothing in the running program chose a card: Up and Down only scrolled,
    // and every test that pressed Enter, P, M, B, Ctrl+D or Ctrl+A first set
    // `selected_card` itself. So those keys all worked in the tests and did
    // nothing at all in the window.

    fn tap(app: &mut KanbanApp, key: Key) -> bool {
        handle_key_event(app, &make_key(key, Modifiers::NONE))
    }

    fn chosen_title(app: &KanbanApp) -> Option<String> {
        let id = app.selected_card?;
        app.active_board().cards.get(&id).map(|c| c.title.clone())
    }

    #[test]
    fn the_arrows_choose_a_card_and_enter_opens_it() {
        let mut app = app_with_cards(3);
        assert!(tap(&mut app, Key::Down), "Down chose nothing");
        assert_eq!(chosen_title(&app).as_deref(), Some("card0"));
        tap(&mut app, Key::Down);
        assert_eq!(chosen_title(&app).as_deref(), Some("card1"));
        tap(&mut app, Key::Up);
        assert_eq!(chosen_title(&app).as_deref(), Some("card0"));
        assert!(
            !tap(&mut app, Key::Up),
            "Up at the top answered as if it moved"
        );
        tap(&mut app, Key::Enter);
        assert_eq!(
            app.view,
            View::CardDetail,
            "Enter did not open the chosen card"
        );
    }

    #[test]
    fn up_with_nothing_chosen_chooses_the_last_card() {
        let mut app = app_with_cards(3);
        tap(&mut app, Key::Up);
        assert_eq!(chosen_title(&app).as_deref(), Some("card2"));
    }

    #[test]
    fn walking_down_a_long_column_keeps_the_chosen_card_drawn() {
        let mut app = app_with_cards(60);
        for i in 0..60 {
            tap(&mut app, Key::Down);
            let chosen = format!("card{i}");
            assert_eq!(chosen_title(&app).as_deref(), Some(chosen.as_str()));
            let drawn = drawn_card_titles(&app);
            assert!(
                drawn.contains(&chosen),
                "{chosen} is chosen and not drawn: {drawn:?}"
            );
        }
        // And back up to the top.
        for _ in 0..60 {
            tap(&mut app, Key::Up);
        }
        assert_eq!(chosen_title(&app).as_deref(), Some("card0"));
        assert_eq!(
            drawn_card_titles(&app).first().map(String::as_str),
            Some("card0")
        );
    }

    #[test]
    fn left_and_right_choose_the_card_level_with_it_in_the_next_column() {
        let mut app = KanbanApp::new();
        app.win_height = TEST_H;
        for i in 0..3 {
            app.add_card(&format!("left{i}"), 0);
            app.add_card(&format!("right{i}"), 1);
        }
        tap(&mut app, Key::Down);
        tap(&mut app, Key::Down);
        assert_eq!(chosen_title(&app).as_deref(), Some("left1"));
        assert!(tap(&mut app, Key::Right));
        assert_eq!(app.selected_column, 1);
        assert_eq!(chosen_title(&app).as_deref(), Some("right1"));
        tap(&mut app, Key::Right);
        assert_eq!(app.selected_column, 2);
        assert_eq!(
            chosen_title(&app),
            None,
            "an empty column has no card to choose"
        );
        tap(&mut app, Key::Left);
        assert_eq!(
            chosen_title(&app).as_deref(),
            Some("right0"),
            "from nothing, the top card"
        );
        tap(&mut app, Key::Left);
        assert!(
            !tap(&mut app, Key::Left),
            "Left at the first column answered as if it moved"
        );
    }

    /// The keys that act on a card act on the one the arrows chose, with no
    /// test reaching in to choose it for them.
    #[test]
    fn the_card_keys_work_on_a_card_the_arrows_chose() {
        let mut app = app_with_cards(3);
        tap(&mut app, Key::Down);
        tap(&mut app, Key::Down);
        assert_eq!(chosen_title(&app).as_deref(), Some("card1"));

        // M moves it on, and the choice goes with it, so M again moves it on.
        tap(&mut app, Key::M);
        assert_eq!(app.selected_column, 1);
        assert_eq!(chosen_title(&app).as_deref(), Some("card1"));
        tap(&mut app, Key::M);
        assert_eq!(app.selected_column, 2);
        tap(&mut app, Key::B);
        assert_eq!(app.selected_column, 1);

        // Archive: the card goes, and nothing in that column is left chosen
        // because nothing else is in it.
        handle_key_event(&mut app, &make_key(Key::A, Modifiers::ctrl()));
        assert_eq!(app.active_board().archived_card_ids.len(), 1);
        assert_eq!(chosen_title(&app), None);

        // Delete in a column with cards: the next card is chosen in its place.
        tap(&mut app, Key::Left);
        assert_eq!(chosen_title(&app).as_deref(), Some("card0"));
        handle_key_event(&mut app, &make_key(Key::D, Modifiers::ctrl()));
        assert_eq!(
            chosen_title(&app).as_deref(),
            Some("card2"),
            "the neighbour was not chosen"
        );
        handle_key_event(&mut app, &make_key(Key::D, Modifiers::ctrl()));
        assert_eq!(chosen_title(&app), None);
        assert!(app.active_board().columns[0].card_ids.is_empty());
    }

    #[test]
    fn a_new_card_is_chosen() {
        let mut app = app_with_cards(2);
        tap(&mut app, Key::N);
        for c in "fresh".chars() {
            handle_key_event(&mut app, &make_char_key(c));
        }
        tap(&mut app, Key::Enter);
        assert_eq!(chosen_title(&app).as_deref(), Some("fresh"));
    }

    #[test]
    fn a_new_board_starts_with_nothing_chosen() {
        let mut app = app_with_cards(2);
        tap(&mut app, Key::Down);
        app.selected_column = 3;
        app.add_board("Second");
        assert_eq!(app.selected_card, None);
        assert_eq!(app.selected_column, 0);
    }

    // == The open card's keys (2026-09-27) =======================================

    /// A board with one card, chosen and open.
    fn open_card(title: &str) -> (KanbanApp, Id) {
        let mut app = app_with_cards(0);
        let id = app.add_card(title, 0).expect("a card");
        tap(&mut app, Key::Down);
        tap(&mut app, Key::Enter);
        assert_eq!(app.view, View::CardDetail);
        (app, id)
    }

    fn type_text(app: &mut KanbanApp, text: &str) {
        for c in text.chars() {
            handle_key_event(app, &make_char_key(c));
        }
    }

    fn card_of(app: &KanbanApp, id: Id) -> &Card {
        app.active_board().cards.get(&id).expect("the card")
    }

    #[test]
    fn e_edits_the_title_starting_from_what_is_there() {
        let (mut app, id) = open_card("Draft");
        tap(&mut app, Key::E);
        assert_eq!(app.input_mode, InputMode::EditCardTitle);
        assert_eq!(
            app.input_buffer, "Draft",
            "an edit that starts empty is a retype"
        );
        type_text(&mut app, " two");
        tap(&mut app, Key::Enter);
        assert_eq!(card_of(&app, id).title, "Draft two");
        assert_eq!(app.view, View::CardDetail, "the card closed under the edit");
    }

    #[test]
    fn d_edits_the_description_and_emptying_it_clears_it() {
        let (mut app, id) = open_card("Card");
        tap(&mut app, Key::D);
        type_text(&mut app, "What it is for");
        tap(&mut app, Key::Enter);
        assert_eq!(card_of(&app, id).description, "What it is for");

        tap(&mut app, Key::D);
        assert_eq!(app.input_buffer, "What it is for");
        for _ in 0.."What it is for".len() {
            tap(&mut app, Key::Backspace);
        }
        tap(&mut app, Key::Enter);
        assert_eq!(
            card_of(&app, id).description,
            "",
            "an emptied description was kept"
        );
    }

    #[test]
    fn c_adds_a_comment_signed_by_whoever_is_writing() {
        let (mut app, id) = open_card("Card");
        app.author = "alice".to_string();
        tap(&mut app, Key::C);
        type_text(&mut app, "Looks right");
        tap(&mut app, Key::Enter);
        let comments = &card_of(&app, id).comments;
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "Looks right");
        assert_eq!(comments[0].author, "alice");
        // Shift+C on the board is still a new column, not a comment.
        let before = app.active_board().columns.len();
        tap(&mut app, Key::Escape);
        handle_key_event(
            &mut app,
            &make_key(
                Key::C,
                Modifiers {
                    shift: true,
                    ..Modifiers::NONE
                },
            ),
        );
        assert_eq!(app.input_mode, InputMode::NewColumnName);
        assert_eq!(app.active_board().columns.len(), before);
    }

    #[test]
    fn l_adds_a_checklist_item_and_tab_and_space_tick_it() {
        let (mut app, id) = open_card("Card");
        for item in ["first", "second"] {
            tap(&mut app, Key::L);
            type_text(&mut app, item);
            tap(&mut app, Key::Enter);
        }
        assert_eq!(card_of(&app, id).checklist.len(), 2);

        // Space with nothing chosen says how, and ticks nothing.
        assert!(tap(&mut app, Key::Space));
        assert!(app.note.is_some());
        assert_eq!(card_of(&app, id).checklist_progress(), (0, 2));

        assert!(tap(&mut app, Key::Tab));
        assert_eq!(app.checklist_focus, Some(0));
        tap(&mut app, Key::Tab);
        assert_eq!(app.checklist_focus, Some(1));
        assert!(
            !tap(&mut app, Key::Tab),
            "Tab past the last item answered as if it moved"
        );
        tap(&mut app, Key::Space);
        assert!(card_of(&app, id).checklist[1].done);
        assert!(!card_of(&app, id).checklist[0].done);
        handle_key_event(
            &mut app,
            &make_key(
                Key::Tab,
                Modifiers {
                    shift: true,
                    ..Modifiers::NONE
                },
            ),
        );
        assert_eq!(app.checklist_focus, Some(0));
        tap(&mut app, Key::Space);
        assert_eq!(card_of(&app, id).checklist_progress(), (2, 2));
        tap(&mut app, Key::Space);
        assert_eq!(
            card_of(&app, id).checklist_progress(),
            (1, 2),
            "Space again unticks"
        );
    }

    #[test]
    fn tab_brings_a_checklist_item_below_the_fold_into_sight() {
        let (mut app, id) = open_card("Card");
        {
            let card = app.active_board_mut().cards.get_mut(&id).expect("card");
            card.description = "long enough to push the checklist down. ".repeat(60);
            for i in 0..30 {
                card.add_checklist_item(&format!("step {i}"));
            }
        }
        let body = DetailModal::for_window(app.win_width, app.win_height).body_height();
        for _ in 0..30 {
            tap(&mut app, Key::Tab);
            let focus = app.checklist_focus.expect("a focus");
            let board = app.active_board();
            let row = checklist_rows(&app, board, card_of(&app, id))[focus];
            assert!(
                row >= app.detail_scroll && row + CHECK_ROW_H <= app.detail_scroll + body + 0.01,
                "item {focus} at {row} is outside the body scrolled to {} (height {body})",
                app.detail_scroll
            );
        }
    }

    #[test]
    fn a_new_card_opens_with_no_checklist_focus_left_over() {
        let (mut app, id) = open_card("Card");
        app.active_board_mut()
            .cards
            .get_mut(&id)
            .expect("card")
            .add_checklist_item("x");
        tap(&mut app, Key::Tab);
        assert_eq!(app.checklist_focus, Some(0));
        tap(&mut app, Key::Escape);
        tap(&mut app, Key::Enter);
        assert_eq!(app.checklist_focus, None);
    }

    #[test]
    fn the_open_card_names_its_keys() {
        let (app, _) = open_card("Card");
        assert!(
            drawn(&app).contains(DETAIL_KEYS),
            "the card's keys are drawn nowhere"
        );
    }

    // == The archive, the columns and the filters (2026-09-27) ====================

    fn alt(k: Key) -> KeyEvent {
        make_key(
            k,
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        )
    }

    fn shift(k: Key) -> KeyEvent {
        make_key(
            k,
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        )
    }

    fn column_titles(app: &KanbanApp, col: usize) -> Vec<String> {
        let board = app.active_board();
        board.columns[col]
            .card_ids
            .iter()
            .filter_map(|id| board.cards.get(id))
            .map(|c| c.title.clone())
            .collect()
    }

    /// A card archived from a column goes back to that column.
    #[test]
    fn an_archived_card_is_restored_to_the_column_it_came_from() {
        let mut app = app_with_cards(0);
        app.add_card("keep", 2).expect("card");
        app.add_card("shelve", 2).expect("card");
        tap(&mut app, Key::Right);
        tap(&mut app, Key::Right);
        tap(&mut app, Key::Down);
        tap(&mut app, Key::Down);
        assert_eq!(chosen_title(&app).as_deref(), Some("shelve"));
        handle_key_event(&mut app, &make_key(Key::A, Modifiers::ctrl()));
        assert_eq!(column_titles(&app, 2), ["keep"]);

        handle_key_event(&mut app, &alt(Key::Num3));
        assert_eq!(app.view, View::Archive);
        assert!(
            drawn(&app).contains("back to "),
            "the archive does not say where Enter puts it"
        );
        assert!(tap(&mut app, Key::Enter));
        assert_eq!(column_titles(&app, 2), ["keep", "shelve"]);
        assert!(app.active_board().archived_card_ids.is_empty());
        let note = app.note.clone().unwrap_or_default();
        assert!(note.starts_with("Restored shelve to "), "{note}");
    }

    /// Its column gone, it goes to the first; with no columns, it stays put
    /// and the window says why.
    #[test]
    fn a_card_whose_column_is_gone_is_restored_to_the_first() {
        let mut board = Board::new("B");
        board.add_column("One");
        board.add_column("Two");
        let id = board.add_card_to_column(Card::new("x"), 1).expect("card");
        board.archive_card(id);
        board.remove_column(1);
        assert_eq!(board.restore_card(id), Some(0));
        assert_eq!(board.columns[0].card_ids, [id]);

        let mut bare = Board::new("Bare");
        bare.add_column("Only");
        let id = bare.add_card_to_column(Card::new("y"), 0).expect("card");
        bare.archive_card(id);
        bare.remove_column(0);
        assert_eq!(bare.restore_card(id), None);
        assert_eq!(
            bare.archived_card_ids,
            [id],
            "a failed restore lost the card"
        );
        assert!(bare.cards[&id].archived);
    }

    /// Restoring into a column that is not there changes nothing: it used to
    /// clear the archive first and leave the card in no column at all.
    #[test]
    fn unarchiving_into_no_column_leaves_the_card_archived() {
        let mut board = Board::new("B");
        board.add_column("One");
        let id = board.add_card_to_column(Card::new("x"), 0).expect("card");
        board.archive_card(id);
        assert!(!board.unarchive_card(id, 7));
        assert_eq!(board.archived_card_ids, [id]);
        assert!(board.cards[&id].archived);
    }

    #[test]
    fn the_archive_cursor_moves_and_ctrl_d_deletes_what_it_is_on() {
        let mut app = app_with_cards(0);
        for t in ["a", "b", "c"] {
            let id = app.add_card(t, 0).expect("card");
            app.active_board_mut().archive_card(id);
        }
        handle_key_event(&mut app, &alt(Key::Num3));
        assert!(!tap(&mut app, Key::Up));
        assert!(tap(&mut app, Key::Down));
        assert!(tap(&mut app, Key::Down));
        assert!(!tap(&mut app, Key::Down));
        assert_eq!(app.archive_cursor, 2);
        handle_key_event(&mut app, &make_key(Key::D, Modifiers::ctrl()));
        let left: Vec<String> = app
            .active_board()
            .archived_card_ids
            .iter()
            .map(|id| app.active_board().cards[id].title.clone())
            .collect();
        assert_eq!(left, ["a", "b"]);
        assert_eq!(app.archive_cursor, 1, "the cursor was left past the end");
    }

    /// The origin survives the boards file, and a JSON export.
    #[test]
    fn where_a_card_was_archived_from_is_kept() {
        let mut board = Board::default_board();
        let id = board.add_card_to_column(Card::new("x"), 3).expect("card");
        board.archive_card(id);
        let want = board.columns[3].id;
        assert_eq!(board.cards[&id].archived_from, Some(want));

        let (back, _) = parse_boards(&boards_text(&[board.clone()], 0)).expect("read back");
        assert_eq!(back[0].cards[&id].archived_from, Some(want));

        let json = JsonExporter::export_board(&board);
        let imported = JsonImporter::import_board(&json).expect("imported");
        assert_eq!(imported.cards[&id].archived_from, Some(want));
    }

    /// A boards file written before origins existed still reads.
    #[test]
    fn a_format_one_boards_file_still_reads() {
        let text = "slateos-kanban\t1\nactive\t0\nboard\t1\t0\tOld\ncard\t7\tmedium\t\t5\t1\tT\t\t\t\ncolumn\t2\t\tpriority\t0\tTodo\narchived\t7\n";
        let (boards, _) = parse_boards(text).expect("a format 1 file was refused");
        assert_eq!(boards[0].archived_card_ids.len(), 1);
        assert_eq!(
            boards[0].cards.values().next().expect("card").archived_from,
            None
        );
    }

    #[test]
    fn r_renames_the_chosen_column_starting_from_its_name() {
        let mut app = app_with_cards(0);
        tap(&mut app, Key::Right);
        let name = app.active_board().columns[1].name.clone();
        tap(&mut app, Key::R);
        assert_eq!(app.input_mode, InputMode::RenameColumn);
        assert_eq!(app.input_buffer, name);
        type_text(&mut app, "!");
        tap(&mut app, Key::Enter);
        assert_eq!(app.active_board().columns[1].name, format!("{name}!"));
    }

    #[test]
    fn z_collapses_a_column_and_its_cards_are_not_drawn_or_chosen() {
        let mut app = app_with_cards(3);
        tap(&mut app, Key::Down);
        assert!(chosen_title(&app).is_some());
        tap(&mut app, Key::Z);
        assert!(app.active_board().columns[0].collapsed);
        assert_eq!(chosen_title(&app), None, "a hidden card stayed chosen");
        assert!(
            drawn_card_titles(&app).is_empty(),
            "a collapsed column drew its cards"
        );
        assert!(drawn(&app).contains("3 cards hidden"));
        assert!(
            !tap(&mut app, Key::Down),
            "Down chose a card nobody can see"
        );
        tap(&mut app, Key::Z);
        assert!(!app.active_board().columns[0].collapsed);
        assert_eq!(drawn_card_titles(&app).len(), 3);
    }

    #[test]
    fn a_collapsed_column_is_narrow_and_the_others_take_the_room() {
        let mut columns = vec![Column::new("A"), Column::new("B"), Column::new("C")];
        let wide = column_spans(&columns, 1200.0);
        columns[1].collapsed = true;
        let narrow = column_spans(&columns, 1200.0);
        assert!((narrow[1].1 - COLLAPSED_W).abs() < 0.01);
        assert!(narrow[0].1 > wide[0].1, "the open columns did not widen");
        // Laid out left to right without overlapping.
        for pair in narrow.windows(2) {
            assert!(pair[0].0 + pair[0].1 <= pair[1].0);
        }
    }

    #[test]
    fn shift_t_sorts_by_the_next_order_and_says_which() {
        let mut app = app_with_cards(0);
        for t in ["b", "c", "a"] {
            app.add_card(t, 0);
        }
        assert_eq!(app.active_board().columns[0].sort_by, SortBy::Priority);
        // Priority, due date, created, title: three presses reach title.
        for _ in 0..3 {
            handle_key_event(&mut app, &shift(Key::T));
        }
        assert_eq!(app.active_board().columns[0].sort_by, SortBy::Title);
        assert_eq!(column_titles(&app, 0), ["a", "b", "c"]);
        assert_eq!(app.note.as_deref(), Some("Sorted by title"));
        handle_key_event(&mut app, &shift(Key::T));
        assert_eq!(
            app.active_board().columns[0].sort_by,
            SortBy::Priority,
            "no wrap"
        );
    }

    #[test]
    fn sorting_by_due_date_puts_undated_cards_last() {
        let mut board = Board::new("B");
        board.add_column("A");
        let undated = board
            .add_card_to_column(Card::new("none"), 0)
            .expect("card");
        let mut soon = Card::new("soon");
        soon.due_date = Some(SimpleDate {
            year: 2026,
            month: 10,
            day: 1,
        });
        let soon = board.add_card_to_column(soon, 0).expect("card");
        board.columns[0].sort_by = SortBy::DueDate;
        board.sort_column(0);
        assert_eq!(board.columns[0].card_ids, [soon, undated]);
    }

    #[test]
    fn shift_delete_removes_an_empty_column_and_refuses_one_with_cards() {
        let mut app = app_with_cards(2);
        let before = app.active_board().columns.len();
        assert!(handle_key_event(&mut app, &shift(Key::Delete)));
        assert_eq!(
            app.active_board().columns.len(),
            before,
            "a column with cards was removed"
        );
        let note = app.note.clone().unwrap_or_default();
        assert!(note.contains("2 cards") && note.contains("first"), "{note}");

        tap(&mut app, Key::Right);
        handle_key_event(&mut app, &shift(Key::Delete));
        assert_eq!(app.active_board().columns.len(), before - 1);
        assert!(app.selected_column < app.active_board().columns.len());
    }

    #[test]
    fn the_chosen_column_is_marked() {
        let mut app = app_with_cards(0);
        tap(&mut app, Key::Right);
        let spans = column_spans(&app.active_board().columns, TEST_W);
        let marked: Vec<f32> = render_app(&app, TEST_W, TEST_H)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::StrokeRect { x, color, .. } if *color == app.palette.accent => {
                    Some(*x)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            marked,
            [spans[1].0],
            "the chosen column is not the one marked"
        );
    }

    #[test]
    fn the_filter_bar_sets_priority_assignee_and_label() {
        let mut app = app_with_cards(0);
        let high = app.add_card("urgent", 0).expect("card");
        app.active_board_mut()
            .cards
            .get_mut(&high)
            .expect("card")
            .priority = Priority::High;
        let alices = app.add_card("hers", 0).expect("card");
        app.active_board_mut()
            .cards
            .get_mut(&alices)
            .expect("card")
            .assignee = "Alice".to_string();
        handle_key_event(&mut app, &make_key(Key::F, Modifiers::ctrl()));
        assert!(app.show_filter_bar);

        // Ctrl+P steps Low, Medium, High -- and a chosen card the filter hides
        // stops being chosen.
        tap(&mut app, Key::Down);
        assert_eq!(chosen_title(&app).as_deref(), Some("urgent"));
        handle_key_event(&mut app, &make_key(Key::P, Modifiers::ctrl()));
        assert_eq!(app.filter.priority_filter, Some(Priority::Low));
        assert_ne!(
            chosen_title(&app).as_deref(),
            Some("urgent"),
            "a hidden card is still chosen"
        );
        handle_key_event(&mut app, &make_key(Key::P, Modifiers::ctrl()));
        handle_key_event(&mut app, &make_key(Key::P, Modifiers::ctrl()));
        assert_eq!(app.filter.priority_filter, Some(Priority::High));
        handle_key_event(&mut app, &make_key(Key::P, Modifiers::ctrl()));
        handle_key_event(&mut app, &make_key(Key::P, Modifiers::ctrl()));
        assert_eq!(
            app.filter.priority_filter, None,
            "Ctrl+P does not come back to all"
        );

        // Ctrl+U types an assignee; an empty one clears it.
        handle_key_event(&mut app, &make_key(Key::U, Modifiers::ctrl()));
        assert_eq!(app.input_mode, InputMode::AssigneeFilter);
        type_text(&mut app, "ali");
        tap(&mut app, Key::Enter);
        assert_eq!(app.filtered_card_ids(0), [alices]);
        assert!(drawn(&app).contains("Assignee: ali"));
        handle_key_event(&mut app, &make_key(Key::U, Modifiers::ctrl()));
        for _ in 0..3 {
            tap(&mut app, Key::Backspace);
        }
        tap(&mut app, Key::Enter);
        assert!(
            app.filter.assignee_filter.is_empty(),
            "an empty assignee did not clear the filter"
        );

        // Ctrl+L steps through the board's labels and back to any.
        let labels: Vec<Id> = app.active_board().labels.iter().map(|l| l.id).collect();
        for want in labels.iter().map(|&id| Some(id)).chain([None]) {
            handle_key_event(&mut app, &make_key(Key::L, Modifiers::ctrl()));
            assert_eq!(app.filter.label_filter, want);
        }
    }

    #[test]
    fn a_column_that_shrinks_under_a_stale_offset_shows_its_last_page() {
        let mut app = app_with_cards(60);
        for _ in 0..10 {
            handle_key_event(&mut app, &make_key(Key::PageDown, Modifiers::NONE));
        }
        let deep = app.scroll_offset;
        assert!(deep > 0, "the test needs the board scrolled down");

        // A filter is applied and most of the cards go away, with nothing
        // resetting the scroll position.
        app.filter.search_text = "card1".to_string();
        let drawn = drawn_card_titles(&app);
        assert!(!drawn.is_empty(), "the column must not go blank");
        assert!(
            drawn.iter().all(|t| t.starts_with("card1")),
            "the filter was not applied: {drawn:?}"
        );
        assert_eq!(
            app.scroll_offset, deep,
            "the stored offset is not rewritten"
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

        // Named explicitly rather than relied on from the file's own imports.
        // The sixteen applications that declare their palette inside a
        // `mod mocha` block import `Color` *there*, so it is not in scope at
        // file level at all -- and once the module is emptied and removed, the
        // import goes with it.
        use guitk::Color;

        fn fills(app: &mut KanbanApp) -> Vec<Color> {
            // Fully qualified. Several applications also have an *inherent*
            // `render`, with different arguments, and an inherent method wins
            // resolution over a trait one -- so `app.render(w, h)` calls the
            // wrong function and fails to compile in a way that looks like the
            // trait is missing.
            oswindow::app::App::render(app, 1200.0, 800.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = KanbanApp::new();

        oswindow::app::App::theme_changed(&mut app, &theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        oswindow::app::App::theme_changed(&mut app, &theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        oswindow::app::App::theme_changed(
            &mut app,
            &theme(
                appearance::ThemeMode::Dark,
                Some(appearance::HighContrastScheme::WhiteOnBlack),
            ),
        );
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // ------------------------------------------------------------------
    // The boards file
    //
    // Nothing was kept: every card was gone when the window closed, and a
    // board's JSON export was the only way to keep one.
    // ------------------------------------------------------------------

    fn key_ev(key: Key, text: &str) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: text.to_owned(),
        })
    }

    fn type_in(app: &mut KanbanApp, text: &str) {
        for c in text.chars() {
            app.on_event(&key_ev(Key::Unknown(0), &c.to_string()));
        }
    }

    /// Text a card is full of and a line-based file mangles.
    const AWKWARD: [&str; 7] = [
        "plain",
        "tab\there",
        "new\nline",
        "cr\rhere",
        "back\\slash",
        "caf\u{e9} \u{1F4CB}",
        "",
    ];

    /// Two boards with one of everything the file has to carry.
    fn full_boards() -> Vec<Board> {
        let mut first = Board::default_board();
        first.swimlanes_enabled = true;
        first.swimlane_names = vec![AWKWARD[1].to_owned(), AWKWARD[6].to_owned()];
        first
            .labels
            .push(Label::new(AWKWARD[2], Color::rgba(1, 2, 3, 4)));
        let label = first.labels[0].id;
        for (i, awkward) in AWKWARD.iter().enumerate() {
            let mut card = Card::new(awkward)
                .with_description(awkward)
                .with_assignee(awkward)
                .with_priority(
                    [
                        Priority::Low,
                        Priority::Medium,
                        Priority::High,
                        Priority::Critical,
                    ][i % 4],
                )
                .with_created_at(1_000 + i as u64)
                .with_label(label);
            if i % 2 == 0 {
                card = card.with_due_date(SimpleDate::new(2026, 2, 30));
            }
            card.swimlane = (*awkward).to_owned();
            card.add_checklist_item(awkward);
            card.checklist[0].done = i % 3 == 0;
            card.add_comment(awkward, awkward, 5_000 + i as u64);
            first.add_card_to_column(card, i % first.columns.len());
        }
        let archived = *first.columns[0].card_ids.first().unwrap();
        assert!(first.archive_card(archived));
        // In no column and not archived: kept all the same.
        let loose = Card::new("loose");
        first.cards.insert(loose.id, loose);
        first.columns[1].wip_limit = Some(2);
        first.columns[2].sort_by = SortBy::Title;
        first.columns[3].collapsed = true;

        let mut second = Board::new(AWKWARD[3]);
        second.add_column(AWKWARD[4]);
        vec![first, second]
    }

    #[test]
    fn boards_written_and_read_again_are_the_same_boards() {
        let boards = full_boards();
        let text = boards_text(&boards, 1);
        let (back, active) = parse_boards(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(active, 1);
        assert_eq!(back, boards);
        // The same boards are the same text, whatever order the map is in.
        assert_eq!(boards_text(&back, 1), text);
        for line in text.lines().skip(1) {
            let kind = line.split('\t').next().unwrap();
            assert!(
                [
                    "active", "board", "lane", "label", "card", "tag", "item", "comment", "column",
                    "archived", "origin"
                ]
                .contains(&kind),
                "{line:?}"
            );
        }
    }

    #[test]
    fn boards_that_cannot_be_read_whole_are_refused_and_say_where() {
        let head = "slateos-kanban\t1";
        let board = "board\t1\t0\tWork";
        let card = |id: u64| format!("card\t{id}\tmedium\t\t5\t0\tTitle\t\t\t");
        let cases: [(String, &str); 16] = [
            (String::new(), "not a SlateOS kanban file"),
            (String::from("slateos-kanban\t3"), "a later format (3)"),
            // Format 1 had no origins; a file that says it is format 1 and
            // has one was not written by this program.
            (
                format!("{head}\n{board}\n{}\narchived\t7\norigin\t7\t2", card(7)),
                "line 5: it is not a line this version reads",
            ),
            (
                format!(
                    "slateos-kanban\t2\n{board}\n{}\ncolumn\t2\t\tpriority\t0\tA\t7\norigin\t7\t2",
                    card(7)
                ),
                "line 5: an origin names a card that is not archived",
            ),
            (String::from(head), "it holds no boards"),
            (
                format!("{head}\n{board}\n{board}"),
                "line 3: two boards have one number",
            ),
            (
                format!("{head}\n{board}\n{}\n{}", card(7), card(7)),
                "line 4: two cards on one board have one number",
            ),
            (
                format!("{head}\n{board}\ncolumn\t2\t\tpriority\t0\tTodo\t9"),
                "line 3: card 9 is not on the board",
            ),
            (
                format!(
                    "{head}\n{board}\n{}\ncolumn\t2\t\tpriority\t0\tA\t7\ncolumn\t3\t\tpriority\t0\tB\t7",
                    card(7)
                ),
                "line 5: card 7 is in two places",
            ),
            (
                format!(
                    "{head}\n{board}\n{}\ncolumn\t2\t\tpriority\t0\tA\t7\narchived\t7",
                    card(7)
                ),
                "line 5: card 7 is in two places",
            ),
            (
                format!("{head}\n{board}\ncard\t7\turgent\t\t5\t0\tT\t\t\t"),
                "line 3: a priority this version does not know",
            ),
            (
                format!("{head}\n{board}\ncolumn\t2\t\tchaos\t0\tTodo"),
                "line 3: a sort order this version does not know",
            ),
            (
                format!("{head}\n{board}\nlabel\t3\tred\tBug"),
                "line 3: a colour is not one",
            ),
            (
                format!("{head}\nactive\t4\n{board}"),
                "line 2: the open board is not one of them",
            ),
            (
                format!("{head}\n{board}\ntag\t3"),
                "line 3: a card's label comes before any card",
            ),
            (
                format!("{head}\n{board}\nsticker\t1"),
                "line 3: it is not a line this version reads",
            ),
        ];
        for (text, want) in &cases {
            match parse_boards(text) {
                Ok(_) => panic!("read {text:?}"),
                Err(why) => assert!(why.contains(want), "{text:?}: said {why:?}, not {want:?}"),
            }
        }
        // The control: the pieces above make boards that read.
        let good = format!(
            "{head}\n{board}\n{}\ncolumn\t2\t\tpriority\t0\tTodo\t7\narchived\n",
            card(7)
        );
        let (boards, active) = parse_boards(&good).unwrap();
        assert_eq!((boards.len(), active), (1, 0));
        assert_eq!(boards[0].columns[0].card_ids, [Id(7)]);
    }

    #[test]
    fn what_is_put_on_a_board_is_there_next_time() {
        settingsfile::testing::with_scratch_config("kanban-kept", |_| {
            let mut app = KanbanApp::from_settings();
            assert!(app.store_error.is_none(), "{:?}", app.store_error);
            // An event that changes nothing writes nothing either.
            app.on_event(&key_ev(Key::Unknown(0), ""));
            assert!(
                !boards_path().unwrap().exists(),
                "a first run wrote a board nobody had touched"
            );
            app.on_event(&key_ev(Key::N, "n"));
            type_in(&mut app, "Write the report");
            app.on_event(&key_ev(Key::Enter, ""));
            assert_eq!(app.active_board().cards.len(), 1, "no card was made");
            app.on_event(&key_ev(Key::P, "p"));

            let again = KanbanApp::from_settings();
            assert!(again.store_error.is_none(), "{:?}", again.store_error);
            assert_eq!(again.boards, app.boards);
            let card = again.active_board().cards.values().next().unwrap();
            assert_eq!(card.title, "Write the report");
            assert!(card.created_at > 1_000_000, "the card was not given a time");
            let kept = card.id;

            // A card made after the restart takes no kept card's number.
            let mut again = again;
            let fresh = again.add_card("Next", 0).unwrap();
            assert_ne!(fresh, kept);
            assert_eq!(again.active_board().cards.len(), 2);
        });
    }

    #[test]
    fn a_window_made_by_new_keeps_nothing() {
        settingsfile::testing::with_scratch_config("kanban-quiet", |dir| {
            let mut app = KanbanApp::new();
            app.on_event(&key_ev(Key::N, "n"));
            type_in(&mut app, "Scratch");
            app.on_event(&key_ev(Key::Enter, ""));
            assert!(!dir.join("slateos").join("kanban").exists());
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::Exit
            ));
        });
    }

    #[test]
    fn a_file_that_cannot_be_read_is_left_as_it_is() {
        settingsfile::testing::with_scratch_config("kanban-broken", |_| {
            let path = boards_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let broken = "slateos-kanban\t1\nboard\t1\t0\tWork\ncard\t7\turgent\t\t5\t0\tT\t\t\t\n";
            std::fs::write(&path, broken).unwrap();
            let mut app = KanbanApp::from_settings();
            let error = app
                .store_error
                .clone()
                .expect("an unreadable file was taken without a word");
            assert!(error.contains("line 3"), "{error}");
            let texts: Vec<String> = render_app(&app, TEST_W, TEST_H)
                .commands
                .into_iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text),
                    _ => None,
                })
                .collect();
            assert!(texts.contains(&error), "the refusal is not on screen");
            app.on_event(&key_ev(Key::N, "n"));
            type_in(&mut app, "New");
            app.on_event(&key_ev(Key::Enter, ""));
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                broken,
                "the unreadable file was saved over"
            );
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::Exit
            ));
        });
    }

    #[test]
    fn a_file_too_big_to_read_whole_is_refused() {
        settingsfile::testing::with_scratch_config("kanban-big", |_| {
            let text = boards_text(&full_boards(), 0);
            let path = boards_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
            let mut app = KanbanApp::new();
            app.persist = true;
            app.load_boards_within(&path, text.len() - 1);
            assert!(app.store_error.clone().unwrap().contains("larger than"));
            assert!(!app.persist, "a file read in part would be saved over");
            let mut whole = KanbanApp::new();
            whole.persist = true;
            whole.load_boards_within(&path, text.len());
            assert!(whole.store_error.is_none(), "{:?}", whole.store_error);
            assert_eq!(
                whole.boards,
                full_boards_ids_of(&text),
                "control: the whole file reads"
            );
        });
    }

    /// The boards a text holds, for comparing with a load of it.
    fn full_boards_ids_of(text: &str) -> Vec<Board> {
        parse_boards(text).unwrap().0
    }

    #[test]
    fn closing_while_a_save_fails_asks_first() {
        settingsfile::testing::with_scratch_config("kanban-failing", |_| {
            let mut app = KanbanApp::from_settings();
            let path = boards_path().unwrap();
            std::fs::create_dir_all(&path).unwrap();
            app.on_event(&key_ev(Key::N, "n"));
            type_in(&mut app, "Unkept");
            app.on_event(&key_ev(Key::Enter, ""));
            let error = app.store_error.clone().expect("a failed save said nothing");
            assert!(error.starts_with("Not saved to "), "{error}");
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::KeepOpen
            ));
            // A key under the question reaches nothing.
            let cards = app.active_board().cards.len();
            app.on_event(&key_ev(Key::N, "n"));
            assert_eq!(app.input_mode, InputMode::None, "a key reached the board");
            assert_eq!(app.active_board().cards.len(), cards);
            // Save while it still fails: the window stays.
            assert!(matches!(
                app.on_event(&key_ev(Key::S, "s")),
                Response::Redraw
            ));
            assert!(!app.quit);
            // Put right while the question is up, then Save: it goes.
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::KeepOpen
            ));
            std::fs::remove_dir(&path).unwrap();
            assert!(matches!(app.on_event(&key_ev(Key::S, "s")), Response::Exit));
            assert_eq!(KanbanApp::from_settings().boards, app.boards);
            // And Don't save leaves without it.
            let mut other = KanbanApp::from_settings();
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir_all(&path).unwrap();
            other.on_event(&key_ev(Key::N, "n"));
            type_in(&mut other, "Lost");
            other.on_event(&key_ev(Key::Enter, ""));
            other.on_event(&Event::CloseRequested);
            assert!(matches!(
                other.on_event(&key_ev(Key::D, "d")),
                Response::Exit
            ));
        });
    }

    /// An import kept the ids the file carried and did not move the counter
    /// past them, so the next card made could be given the id of one just
    /// read -- and cards are kept in a map by id, so the new card replaced it.
    #[test]
    fn a_card_made_after_an_import_replaces_nothing() {
        // The counter moves past every id read, so nothing made later can be
        // given one. Checked directly, because other tests take ids from the
        // same counter at the same time: an id handed out after the read is
        // later than it whatever else was handed out meanwhile.
        let read = NEXT_ID.load(Ordering::Relaxed).saturating_add(10);
        let _ = Id::from_stored(read);
        assert!(
            Id::new().0 > read,
            "an id was handed out that a file already used"
        );

        // And through the importer, with the imported card just ahead of the
        // counter, where the next cards made would have landed on it.
        let far = NEXT_ID.load(Ordering::Relaxed).saturating_add(2);
        let json = format!(
            "{{\"name\":\"Imported\",\"labels\":[],\"cards\":[{{\"id\":{far},\"title\":\"Theirs\"}}],\
             \"columns\":[{{\"id\":{},\"name\":\"Todo\",\"card_ids\":[{far}]}}]}}",
            far + 1
        );
        let mut app = KanbanApp::new();
        let board = JsonImporter::import_board(&json).unwrap();
        app.boards.push(board);
        app.active_board_idx = app.boards.len() - 1;
        for _ in 0..5 {
            app.add_card("Mine", 0);
        }
        assert!(
            app.active_board()
                .cards
                .values()
                .any(|c| c.title == "Theirs"),
            "an imported card was replaced by a new one"
        );
        assert_eq!(app.active_board().cards.len(), 6);
    }

    /// A board file may name a card twice, or one it does not have; the board
    /// imported from it may not.
    #[test]
    fn an_import_leaves_a_board_that_can_be_kept() {
        let json = "{\"name\":\"Messy\",\"labels\":[],\
                    \"cards\":[{\"id\":900001,\"title\":\"A\"},{\"id\":900002,\"title\":\"B\"}],\
                    \"columns\":[{\"id\":900010,\"name\":\"X\",\"card_ids\":[900001,900001,900099]},\
                                 {\"id\":900011,\"name\":\"Y\",\"card_ids\":[900001,900002]}]}";
        let board = JsonImporter::import_board(json).unwrap();
        assert_eq!(board.columns[0].card_ids, [Id(900_001)]);
        assert_eq!(board.columns[1].card_ids, [Id(900_002)]);
        let text = boards_text(&[board], 0);
        assert!(parse_boards(&text).is_ok(), "{text}");
    }

    /// What an import or an export did was recorded and drawn nowhere.
    #[test]
    fn what_an_import_did_is_on_the_status_line() {
        let mut app = KanbanApp::new();
        app.add_card("One", 0);
        let said = app.read_board(std::path::Path::new("/nowhere/board.json"));
        app.last_file_action = Some(said.clone());
        assert!(said.starts_with("Could not read"), "{said}");
        let drawn = render_app(&app, TEST_W, TEST_H)
            .commands
            .into_iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == said));
        assert!(drawn, "what the import did is not on screen");
    }

    #[test]
    fn stamps_are_the_clocks_and_always_later() {
        let mut app = KanbanApp::new();
        let before = clock_ms();
        let first = app.next_timestamp();
        let second = app.next_timestamp();
        assert!(first >= before && second > first);
        let late = 4_000_000_000_000_u64;
        let text = format!(
            "slateos-kanban\t1\nboard\t1\t0\tW\ncard\t2\tmedium\t\t{late}\t0\tT\t\t\t\ncolumn\t3\t\tpriority\t0\tC\t2\narchived\n"
        );
        settingsfile::testing::with_scratch_config("kanban-stamps", |_| {
            let path = boards_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
            let mut app = KanbanApp::from_settings();
            assert!(
                app.next_timestamp() > late,
                "a stamp after a restart is earlier than a kept one"
            );
        });
    }
}
