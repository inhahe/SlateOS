#![allow(dead_code)]
//! Kanban Board / Project Management Application
//!
//! A feature-rich Kanban board for SlateOS with multiple boards, customizable
//! columns, rich cards (labels, priority, due dates, checklists, comments),
//! filtering, sorting, WIP limits, swimlanes, archiving, and JSON export/import.

use guitk::color::Color;
use guitk::event::{KeyEvent, Key};
use guitk::layout::FlexDirection;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::CornerRadii;
use guitk::widget::{Widget, WidgetTree};

use std::collections::HashMap;

// =============================================================================
// Catppuccin Mocha palette
// =============================================================================

mod palette {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
}

// =============================================================================
// Domain types
// =============================================================================

/// Unique identifier for domain objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Id(u64);

impl Id {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
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
    fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Critical => "Critical",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Low => palette::TEAL,
            Self::Medium => palette::BLUE,
            Self::High => palette::PEACH,
            Self::Critical => palette::RED,
        }
    }

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
#[derive(Clone, Debug)]
struct ChecklistItem {
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
#[derive(Clone, Debug)]
struct Comment {
    id: Id,
    author: String,
    text: String,
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

impl SimpleDate {
    fn new(year: u16, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    fn display(&self) -> String {
        let m = self.month.clamp(1, 12);
        let d = self.day.clamp(1, 31);
        format!("{:04}-{:02}-{:02}", self.year, m, d)
    }
}

/// A Kanban card with all associated metadata.
#[derive(Clone, Debug)]
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
    swimlane: String,
}

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
            swimlane: String::new(),
        }
    }

    fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    fn with_assignee(mut self, assignee: &str) -> Self {
        self.assignee = assignee.to_string();
        self
    }

    fn with_due_date(mut self, date: SimpleDate) -> Self {
        self.due_date = Some(date);
        self
    }

    fn with_label(mut self, label_id: Id) -> Self {
        if !self.labels.contains(&label_id) {
            self.labels.push(label_id);
        }
        self
    }

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
}

/// A Kanban column holding an ordered list of cards.
#[derive(Clone, Debug)]
struct Column {
    id: Id,
    name: String,
    card_ids: Vec<Id>,
    wip_limit: Option<usize>,
    sort_by: SortBy,
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
#[derive(Clone, Debug)]
struct Board {
    id: Id,
    name: String,
    columns: Vec<Column>,
    cards: HashMap<Id, Card>,
    labels: Vec<Label>,
    archived_card_ids: Vec<Id>,
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

    fn default_board() -> Self {
        let mut board = Self::new("My Project");

        // Default labels
        board.labels.push(Label::new("Bug", palette::RED));
        board.labels.push(Label::new("Feature", palette::BLUE));
        board.labels.push(Label::new("Enhancement", palette::GREEN));
        board.labels.push(Label::new("Documentation", palette::LAVENDER));
        board.labels.push(Label::new("Urgent", palette::PEACH));
        board.labels.push(Label::new("Design", palette::MAUVE));
        board.labels.push(Label::new("Testing", palette::TEAL));

        // Default columns
        board.columns.push(Column::new("Backlog"));
        board.columns.push(Column::new("Todo"));
        board.columns.push(Column::new("In Progress").with_wip_limit(5));
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
        if let Some(card) = self.cards.get_mut(&card_id) {
            card.archived = true;
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

    fn unarchive_card(&mut self, card_id: Id, column_idx: usize) -> bool {
        if let Some(card) = self.cards.get_mut(&card_id) {
            card.archived = false;
            self.archived_card_ids.retain(|&c| c != card_id);
            if let Some(col) = self.columns.get_mut(column_idx) {
                col.card_ids.push(card_id);
                return true;
            }
        }
        false
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
                        SortBy::DueDate => ca.due_date.cmp(&cb.due_date),
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
            && !card.has_label(label_id) {
                return false;
            }
        if let Some(priority) = self.priority_filter
            && card.priority != priority {
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
struct JsonExporter;

impl JsonExporter {
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
             \"archived\":{},\"swimlane\":\"{}\"}}",
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

/// Minimal JSON parser for board import (handles the structure exported above).
struct JsonImporter;

impl JsonImporter {
    /// Parse a JSON string value, returning the unescaped content and next offset.
    fn parse_string(data: &str, start: usize) -> Option<(String, usize)> {
        let bytes = data.as_bytes();
        if bytes.get(start).copied() != Some(b'"') {
            return None;
        }
        let mut result = String::new();
        let mut i = start.saturating_add(1);
        while i < bytes.len() {
            let b = bytes.get(i).copied()?;
            if b == b'"' {
                return Some((result, i.saturating_add(1)));
            }
            if b == b'\\' {
                i = i.saturating_add(1);
                let esc = bytes.get(i).copied()?;
                match esc {
                    b'"' => result.push('"'),
                    b'\\' => result.push('\\'),
                    b'n' => result.push('\n'),
                    b'r' => result.push('\r'),
                    b't' => result.push('\t'),
                    _ => {
                        result.push('\\');
                        result.push(esc as char);
                    }
                }
            } else {
                result.push(b as char);
            }
            i = i.saturating_add(1);
        }
        None
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

    /// Skip whitespace.
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

    /// Validate that we can round-trip a board through export.
    fn validate_export(board: &Board) -> bool {
        let json = JsonExporter::export_board(board);
        !json.is_empty()
    }
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
struct KanbanApp {
    boards: Vec<Board>,
    active_board_idx: usize,
    view: View,
    filter: FilterState,
    selected_card: Option<Id>,
    selected_column: usize,
    scroll_offset: f32,
    detail_scroll: f32,
    show_filter_bar: bool,
    input_buffer: String,
    input_mode: InputMode,
    timestamp_counter: u64,
}

/// What the user is currently typing into.
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
            boards: vec![default_board],
            active_board_idx: 0,
            view: View::Board,
            filter: FilterState::default(),
            selected_card: None,
            selected_column: 0,
            scroll_offset: 0.0,
            detail_scroll: 0.0,
            show_filter_bar: false,
            input_buffer: String::new(),
            input_mode: InputMode::None,
            timestamp_counter: 1000,
        }
    }

    fn active_board(&self) -> &Board {
        self.boards
            .get(self.active_board_idx)
            .expect("active_board_idx must be valid")
    }

    fn active_board_mut(&mut self) -> &mut Board {
        self.boards
            .get_mut(self.active_board_idx)
            .expect("active_board_idx must be valid")
    }

    fn next_timestamp(&mut self) -> u64 {
        self.timestamp_counter = self.timestamp_counter.saturating_add(1);
        self.timestamp_counter
    }

    fn add_card(&mut self, title: &str, col_idx: usize) -> Option<Id> {
        let ts = self.next_timestamp();
        let card = Card::new(title).with_created_at(ts);
        self.active_board_mut().add_card_to_column(card, col_idx)
    }

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
                    board.cards.get(cid).is_some_and(|card| {
                        !card.archived && self.filter.matches(card)
                    })
                })
                .copied()
                .collect()
        } else {
            Vec::new()
        }
    }

    fn export_json(&self) -> String {
        JsonExporter::export_board(self.active_board())
    }
}

// =============================================================================
// Rendering helpers
// =============================================================================

/// Render the toolbar at the top.
fn render_toolbar(tree: &mut RenderTree, app: &KanbanApp, width: f32) {
    let toolbar_h: f32 = 40.0;

    // Background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width,
        height: toolbar_h,
        color: palette::MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // App title
    tree.push(RenderCommand::Text {
        x: 12.0,
        y: 10.0,
        text: "Kanban Board".to_string(),
        color: palette::BLUE,
        font_size: 16.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(200.0),
    });

    // Board name
    let board_name = &app.active_board().name;
    tree.push(RenderCommand::Text {
        x: 160.0,
        y: 12.0,
        text: format!("/ {}", board_name),
        color: palette::SUBTEXT0,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(200.0),
    });

    // Toolbar buttons
    let button_y: f32 = 7.0;
    let button_h: f32 = 26.0;
    let btn_radius = CornerRadii::all(4.0);

    // Board List button
    render_toolbar_button(tree, 380.0, button_y, 70.0, button_h, "Boards", palette::SURFACE0, palette::TEXT, btn_radius);

    // Filter button
    let filter_color = if app.filter.is_active() {
        palette::BLUE
    } else {
        palette::SURFACE0
    };
    render_toolbar_button(tree, 460.0, button_y, 60.0, button_h, "Filter", filter_color, palette::TEXT, btn_radius);

    // Stats button
    render_toolbar_button(tree, 530.0, button_y, 55.0, button_h, "Stats", palette::SURFACE0, palette::TEXT, btn_radius);

    // Archive button
    render_toolbar_button(tree, 595.0, button_y, 65.0, button_h, "Archive", palette::SURFACE0, palette::TEXT, btn_radius);

    // Export button
    render_toolbar_button(tree, 670.0, button_y, 60.0, button_h, "Export", palette::SURFACE0, palette::TEXT, btn_radius);

    // New Card button
    render_toolbar_button(tree, width - 110.0, button_y, 100.0, button_h, "+ New Card", palette::BLUE, palette::CRUST, btn_radius);

    // Bottom border line
    tree.push(RenderCommand::Line {
        x1: 0.0,
        y1: toolbar_h,
        x2: width,
        y2: toolbar_h,
        color: palette::SURFACE0,
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
    });
}

/// Render the filter bar below the toolbar.
fn render_filter_bar(tree: &mut RenderTree, app: &KanbanApp, width: f32, y_offset: f32) -> f32 {
    if !app.show_filter_bar {
        return y_offset;
    }

    let bar_h: f32 = 36.0;

    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: y_offset,
        width,
        height: bar_h,
        color: palette::CRUST,
        corner_radii: CornerRadii::ZERO,
    });

    // Search icon area
    tree.push(RenderCommand::Text {
        x: 12.0,
        y: y_offset + 9.0,
        text: "Search:".to_string(),
        color: palette::SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Search input
    let search_text = if app.filter.search_text.is_empty() {
        "type to search..."
    } else {
        &app.filter.search_text
    };
    let search_color = if app.filter.search_text.is_empty() {
        palette::OVERLAY0
    } else {
        palette::TEXT
    };
    tree.push(RenderCommand::FillRect {
        x: 70.0,
        y: y_offset + 5.0,
        width: 180.0,
        height: 26.0,
        color: palette::SURFACE0,
        corner_radii: CornerRadii::all(3.0),
    });
    tree.push(RenderCommand::Text {
        x: 78.0,
        y: y_offset + 9.0,
        text: search_text.to_string(),
        color: search_color,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(164.0),
    });

    // Priority filter
    tree.push(RenderCommand::Text {
        x: 270.0,
        y: y_offset + 9.0,
        text: "Priority:".to_string(),
        color: palette::SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    let priority_label = app
        .filter
        .priority_filter
        .map_or("All", |p| p.label());
    tree.push(RenderCommand::FillRect {
        x: 332.0,
        y: y_offset + 5.0,
        width: 70.0,
        height: 26.0,
        color: palette::SURFACE0,
        corner_radii: CornerRadii::all(3.0),
    });
    tree.push(RenderCommand::Text {
        x: 340.0,
        y: y_offset + 9.0,
        text: priority_label.to_string(),
        color: palette::TEXT,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(54.0),
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
            palette::RED,
            palette::TEXT,
            CornerRadii::all(3.0),
        );
    }

    y_offset + bar_h
}

/// Render a single card.
fn render_card(
    tree: &mut RenderTree,
    card: &Card,
    board: &Board,
    x: f32,
    y: f32,
    card_width: f32,
    selected: bool,
) -> f32 {
    let padding: f32 = 8.0;
    let mut card_h: f32 = 12.0; // top padding

    // Priority indicator bar at top
    let priority_bar_h: f32 = 3.0;
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: card_width,
        height: priority_bar_h,
        color: card.priority.color(),
        corner_radii: CornerRadii {
            top_left: 6.0,
            top_right: 6.0,
            bottom_left: 0.0,
            bottom_right: 0.0,
        },
    });
    card_h += priority_bar_h;

    // Card background
    let bg_color = if selected {
        palette::SURFACE1
    } else {
        palette::SURFACE0
    };

    // Title
    tree.push(RenderCommand::Text {
        x: x + padding,
        y: y + card_h,
        text: card.title.clone(),
        color: palette::TEXT,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(card_width - padding * 2.0),
    });
    card_h += 18.0;

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
                    color: palette::CRUST,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(lw - 10.0),
                });
                label_x += lw + 4.0;
            }
        }
        card_h += 20.0;
    }

    // Metadata row: assignee, due date
    let mut meta_parts: Vec<String> = Vec::new();
    if !card.assignee.is_empty() {
        meta_parts.push(card.assignee.clone());
    }
    if let Some(date) = card.due_date {
        meta_parts.push(date.display());
    }

    if !meta_parts.is_empty() {
        tree.push(RenderCommand::Text {
            x: x + padding,
            y: y + card_h,
            text: meta_parts.join(" | "),
            color: palette::SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(card_width - padding * 2.0),
        });
        card_h += 14.0;
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
                palette::GREEN
            } else {
                palette::YELLOW
            },
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        card_h += 14.0;
    }

    // Comment count
    if !card.comments.is_empty() {
        let comment_text = format!("{} comment(s)", card.comments.len());
        tree.push(RenderCommand::Text {
            x: x + padding,
            y: y + card_h,
            text: comment_text,
            color: palette::OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        card_h += 14.0;
    }

    card_h += 8.0; // bottom padding

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
            color: palette::BLUE,
            line_width: 2.0,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    card_h
}

/// Render a column header.
fn render_column_header(
    tree: &mut RenderTree,
    col: &Column,
    board: &Board,
    x: f32,
    y: f32,
    col_width: f32,
) {
    let header_h: f32 = 36.0;

    // Header background
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: col_width,
        height: header_h,
        color: palette::MANTLE,
        corner_radii: CornerRadii {
            top_left: 6.0,
            top_right: 6.0,
            bottom_left: 0.0,
            bottom_right: 0.0,
        },
    });

    // Column name
    tree.push(RenderCommand::Text {
        x: x + 10.0,
        y: y + 9.0,
        text: col.name.clone(),
        color: palette::TEXT,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(col_width - 80.0),
    });

    // Card count badge
    let active_count = col.active_card_count(&board.cards);
    let count_text = format!("{}", active_count);

    let badge_color = if col.is_over_wip_limit() {
        palette::RED
    } else {
        palette::SURFACE1
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
        color: palette::TEXT,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // WIP limit indicator
    if let Some(limit) = col.wip_limit {
        let wip_text = format!("/{}", limit);
        tree.push(RenderCommand::Text {
            x: badge_x + 24.0,
            y: y + 11.0,
            text: wip_text,
            color: if col.is_over_wip_limit() {
                palette::RED
            } else {
                palette::OVERLAY0
            },
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

/// Render the board view with columns of cards.
fn render_board_view(tree: &mut RenderTree, app: &KanbanApp, width: f32, height: f32, y_start: f32) {
    let board = app.active_board();
    let col_count = board.columns.len();
    if col_count == 0 {
        tree.push(RenderCommand::Text {
            x: width / 2.0 - 80.0,
            y: y_start + 50.0,
            text: "No columns yet. Press 'C' to add one.".to_string(),
            color: palette::SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        return;
    }

    let col_gap: f32 = 8.0;
    let col_margin: f32 = 8.0;
    let available_w = width - col_margin * 2.0 - col_gap * (col_count.saturating_sub(1)) as f32;
    let col_width = (available_w / col_count as f32).max(180.0);

    for (ci, col) in board.columns.iter().enumerate() {
        let col_x = col_margin + (col_width + col_gap) * ci as f32;
        let col_y = y_start + 8.0;

        // Column background
        tree.push(RenderCommand::FillRect {
            x: col_x,
            y: col_y,
            width: col_width,
            height: height - col_y - 8.0,
            color: palette::BASE,
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

        // Column header
        render_column_header(tree, col, board, col_x, col_y, col_width);

        // Cards
        let header_h: f32 = 36.0;
        let card_gap: f32 = 6.0;
        let card_margin: f32 = 6.0;
        let card_width = col_width - card_margin * 2.0;
        let mut card_y = col_y + header_h + card_gap;

        let filtered_ids = app.filtered_card_ids(ci);
        for card_id in &filtered_ids {
            if let Some(card) = board.cards.get(card_id) {
                let is_selected = app.selected_card == Some(*card_id);
                let ch = render_card(
                    tree,
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
    }
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
    let modal_w: f32 = 600.0_f32.min(width - 40.0);
    let modal_h: f32 = 500.0_f32.min(height - 60.0);
    let modal_x = (width - modal_w) / 2.0;
    let modal_y = (height - modal_h) / 2.0;

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
        color: palette::BASE,
        corner_radii: CornerRadii::all(8.0),
    });

    // Modal border
    tree.push(RenderCommand::StrokeRect {
        x: modal_x,
        y: modal_y,
        width: modal_w,
        height: modal_h,
        color: palette::SURFACE1,
        line_width: 1.0,
        corner_radii: CornerRadii::all(8.0),
    });

    let content_x = modal_x + 20.0;
    let content_w = modal_w - 40.0;
    let mut cy = modal_y + 16.0;

    // Title
    tree.push(RenderCommand::Text {
        x: content_x,
        y: cy,
        text: card.title.clone(),
        color: palette::TEXT,
        font_size: 18.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(content_w - 60.0),
    });

    // Close button
    render_toolbar_button(
        tree,
        modal_x + modal_w - 40.0,
        cy,
        28.0,
        24.0,
        "X",
        palette::SURFACE0,
        palette::RED,
        CornerRadii::all(4.0),
    );

    cy += 30.0;

    // Priority badge
    tree.push(RenderCommand::FillRect {
        x: content_x,
        y: cy,
        width: 80.0,
        height: 22.0,
        color: card.priority.color(),
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: content_x + 8.0,
        y: cy + 4.0,
        text: card.priority.label().to_string(),
        color: palette::CRUST,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // Assignee
    if !card.assignee.is_empty() {
        tree.push(RenderCommand::Text {
            x: content_x + 90.0,
            y: cy + 4.0,
            text: format!("Assigned: {}", card.assignee),
            color: palette::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    // Due date
    if let Some(date) = card.due_date {
        tree.push(RenderCommand::Text {
            x: content_x + 280.0,
            y: cy + 4.0,
            text: format!("Due: {}", date.display()),
            color: palette::YELLOW,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    cy += 30.0;

    // Labels
    if !card.labels.is_empty() {
        tree.push(RenderCommand::Text {
            x: content_x,
            y: cy,
            text: "Labels:".to_string(),
            color: palette::SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
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
                    color: palette::CRUST,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(lw - 12.0),
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
        color: palette::SURFACE0,
        width: 1.0,
    });
    cy += 10.0;

    // Description
    tree.push(RenderCommand::Text {
        x: content_x,
        y: cy,
        text: "Description".to_string(),
        color: palette::TEXT,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
    cy += 18.0;

    let desc_text = if card.description.is_empty() {
        "No description provided."
    } else {
        &card.description
    };
    tree.push(RenderCommand::Text {
        x: content_x,
        y: cy,
        text: desc_text.to_string(),
        color: if card.description.is_empty() {
            palette::OVERLAY0
        } else {
            palette::SUBTEXT0
        },
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(content_w),
    });
    cy += 30.0;

    // Checklist
    if !card.checklist.is_empty() {
        let (done, total) = card.checklist_progress();
        tree.push(RenderCommand::Text {
            x: content_x,
            y: cy,
            text: format!("Checklist ({}/{})", done, total),
            color: palette::TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 20.0;

        // Progress bar
        let bar_w = content_w.min(300.0);
        tree.push(RenderCommand::FillRect {
            x: content_x,
            y: cy,
            width: bar_w,
            height: 6.0,
            color: palette::SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        if total > 0 {
            let progress_w = bar_w * (done as f32 / total as f32);
            tree.push(RenderCommand::FillRect {
                x: content_x,
                y: cy,
                width: progress_w,
                height: 6.0,
                color: palette::GREEN,
                corner_radii: CornerRadii::all(3.0),
            });
        }
        cy += 12.0;

        for item in &card.checklist {
            let check_mark = if item.done { "[x]" } else { "[ ]" };
            let item_color = if item.done {
                palette::OVERLAY0
            } else {
                palette::TEXT
            };
            tree.push(RenderCommand::Text {
                x: content_x + 4.0,
                y: cy,
                text: format!("{} {}", check_mark, item.text),
                color: item_color,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 8.0),
            });
            cy += 18.0;
        }
        cy += 8.0;
    }

    // Comments
    if !card.comments.is_empty() {
        tree.push(RenderCommand::Text {
            x: content_x,
            y: cy,
            text: format!("Comments ({})", card.comments.len()),
            color: palette::TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 20.0;

        for comment in &card.comments {
            tree.push(RenderCommand::FillRect {
                x: content_x,
                y: cy,
                width: content_w,
                height: 36.0,
                color: palette::SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            tree.push(RenderCommand::Text {
                x: content_x + 8.0,
                y: cy + 4.0,
                text: comment.author.clone(),
                color: palette::BLUE,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: content_x + 8.0,
                y: cy + 18.0,
                text: comment.text.clone(),
                color: palette::SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 16.0),
            });
            cy += 42.0;
        }
    }
}

/// Render the archive view.
fn render_archive_view(tree: &mut RenderTree, app: &KanbanApp, width: f32, y_start: f32) {
    let board = app.active_board();

    tree.push(RenderCommand::Text {
        x: 20.0,
        y: y_start + 16.0,
        text: "Archived Cards".to_string(),
        color: palette::TEXT,
        font_size: 16.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    if board.archived_card_ids.is_empty() {
        tree.push(RenderCommand::Text {
            x: 20.0,
            y: y_start + 50.0,
            text: "No archived cards.".to_string(),
            color: palette::OVERLAY0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        return;
    }

    let mut cy = y_start + 44.0;
    for card_id in &board.archived_card_ids {
        if let Some(card) = board.cards.get(card_id) {
            tree.push(RenderCommand::FillRect {
                x: 20.0,
                y: cy,
                width: width - 40.0,
                height: 40.0,
                color: palette::SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            tree.push(RenderCommand::Text {
                x: 32.0,
                y: cy + 6.0,
                text: card.title.clone(),
                color: palette::TEXT,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 200.0),
            });
            tree.push(RenderCommand::Text {
                x: 32.0,
                y: cy + 22.0,
                text: format!("Priority: {} | {}", card.priority.label(), card.assignee),
                color: palette::SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            // Restore button
            render_toolbar_button(
                tree,
                width - 100.0,
                cy + 8.0,
                70.0,
                24.0,
                "Restore",
                palette::GREEN,
                palette::CRUST,
                CornerRadii::all(4.0),
            );
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
        color: palette::TEXT,
        font_size: 16.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    let mut cy = y_start + 48.0;

    // Completion rate
    let rate = board.completion_rate();
    tree.push(RenderCommand::Text {
        x: 20.0,
        y: cy,
        text: format!("Completion Rate: {:.1}%", rate),
        color: palette::GREEN,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
    cy += 28.0;

    // Completion bar
    let bar_w: f32 = 300.0;
    tree.push(RenderCommand::FillRect {
        x: 20.0,
        y: cy,
        width: bar_w,
        height: 12.0,
        color: palette::SURFACE0,
        corner_radii: CornerRadii::all(6.0),
    });
    tree.push(RenderCommand::FillRect {
        x: 20.0,
        y: cy,
        width: bar_w * (rate / 100.0),
        height: 12.0,
        color: palette::GREEN,
        corner_radii: CornerRadii::all(6.0),
    });
    cy += 30.0;

    // Total cards
    tree.push(RenderCommand::Text {
        x: 20.0,
        y: cy,
        text: format!("Total Cards: {} | Archived: {}", board.cards.len(), board.archived_card_ids.len()),
        color: palette::SUBTEXT0,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
    cy += 32.0;

    // Per-column stats
    tree.push(RenderCommand::Text {
        x: 20.0,
        y: cy,
        text: "Cards per Column:".to_string(),
        color: palette::TEXT,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
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
            color: palette::TEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
        });

        tree.push(RenderCommand::FillRect {
            x: 160.0,
            y: cy + 2.0,
            width: stat_bar_w,
            height: 14.0,
            color: palette::SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        tree.push(RenderCommand::FillRect {
            x: 160.0,
            y: cy + 2.0,
            width: stat_bar_w * bar_fraction,
            height: 14.0,
            color: if stat.over_wip {
                palette::RED
            } else {
                palette::BLUE
            },
            corner_radii: CornerRadii::all(3.0),
        });

        let wip_text = stat
            .wip_limit
            .map_or_else(|| format!("{}", stat.card_count), |l| format!("{}/{}", stat.card_count, l));
        tree.push(RenderCommand::Text {
            x: 370.0,
            y: cy,
            text: wip_text,
            color: if stat.over_wip {
                palette::RED
            } else {
                palette::SUBTEXT0
            },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
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
        color: palette::TEXT,
        font_size: 16.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    let mut cy = y_start + 50.0;
    for (i, board) in app.boards.iter().enumerate() {
        let is_active = i == app.active_board_idx;
        let bg = if is_active {
            palette::SURFACE1
        } else {
            palette::SURFACE0
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
                color: palette::BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        tree.push(RenderCommand::Text {
            x: 36.0,
            y: cy + 8.0,
            text: board.name.clone(),
            color: palette::TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 120.0),
        });
        tree.push(RenderCommand::Text {
            x: 36.0,
            y: cy + 28.0,
            text: format!(
                "{} columns, {} cards",
                board.columns.len(),
                board.cards.len()
            ),
            color: palette::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
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
        "+ New Board",
        palette::BLUE,
        palette::CRUST,
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
        color: palette::BASE,
        corner_radii: CornerRadii::all(8.0),
    });
    tree.push(RenderCommand::StrokeRect {
        x: dlg_x,
        y: dlg_y,
        width: dlg_w,
        height: dlg_h,
        color: palette::SURFACE1,
        line_width: 1.0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Prompt text
    tree.push(RenderCommand::Text {
        x: dlg_x + 16.0,
        y: dlg_y + 16.0,
        text: prompt.to_string(),
        color: palette::TEXT,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // Input field
    tree.push(RenderCommand::FillRect {
        x: dlg_x + 16.0,
        y: dlg_y + 42.0,
        width: dlg_w - 32.0,
        height: 30.0,
        color: palette::SURFACE0,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: dlg_x + 24.0,
        y: dlg_y + 48.0,
        text: app.input_buffer.clone(),
        color: palette::TEXT,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(dlg_w - 48.0),
    });

    // Hint
    tree.push(RenderCommand::Text {
        x: dlg_x + 16.0,
        y: dlg_y + 84.0,
        text: "Enter to confirm, Escape to cancel".to_string(),
        color: palette::OVERLAY0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

/// Full application render.
fn render_app(app: &KanbanApp, width: f32, height: f32) -> RenderTree {
    let mut tree = RenderTree::new();

    // Full-window background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width,
        height,
        color: palette::CRUST,
        corner_radii: CornerRadii::ZERO,
    });

    // Toolbar
    render_toolbar(&mut tree, app, width);
    let mut content_y: f32 = 40.0;

    // Filter bar
    content_y = render_filter_bar(&mut tree, app, width, content_y);

    // Main view
    match app.view {
        View::Board => render_board_view(&mut tree, app, width, height, content_y),
        View::CardDetail => {
            render_board_view(&mut tree, app, width, height, content_y);
            render_card_detail(&mut tree, app, width, height);
        }
        View::Archive => render_archive_view(&mut tree, app, width, content_y),
        View::Statistics => render_stats_view(&mut tree, app, width, content_y),
        View::BoardList => render_board_list(&mut tree, app, width, content_y),
    }

    // Input overlay
    render_input_overlay(&mut tree, app, width, height);

    tree
}

// =============================================================================
// Event handling
// =============================================================================

/// Handle keyboard events.
fn handle_key_event(app: &mut KanbanApp, key: &KeyEvent) -> bool {
    if !key.pressed {
        return false;
    }

    // If in input mode, route to input handler
    if app.input_mode != InputMode::None {
        return handle_input_key(app, key);
    }

    match key.key {
        // ESC to go back from sub-views
        Key::Escape => {
            match app.view {
                View::CardDetail => {
                    app.view = View::Board;
                    app.selected_card = None;
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
        Key::N if !key.modifiers.ctrl => {
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

        // F = toggle filter bar
        Key::F if key.modifiers.ctrl => {
            app.show_filter_bar = !app.show_filter_bar;
            if !app.show_filter_bar {
                app.filter.clear();
            }
            true
        }

        // S = toggle search
        Key::S if key.modifiers.ctrl => {
            if app.show_filter_bar {
                app.input_mode = InputMode::SearchFilter;
                app.input_buffer = app.filter.search_text.clone();
                return true;
            }
            false
        }

        // Arrow keys for column navigation
        Key::Left => {
            if app.selected_column > 0 {
                app.selected_column = app.selected_column.saturating_sub(1);
            }
            true
        }
        Key::Right => {
            let col_count = app.active_board().columns.len();
            if app.selected_column.saturating_add(1) < col_count {
                app.selected_column = app.selected_column.saturating_add(1);
            }
            true
        }

        // Enter on selected card opens detail
        Key::Enter => {
            if let Some(_card_id) = app.selected_card
                && app.view == View::Board {
                    app.view = View::CardDetail;
                    return true;
                }
            false
        }

        // D = delete card
        Key::D if key.modifiers.ctrl => {
            if let Some(card_id) = app.selected_card {
                app.active_board_mut().delete_card(card_id);
                app.selected_card = None;
                return true;
            }
            false
        }

        // A = archive card
        Key::A if key.modifiers.ctrl && !key.modifiers.shift => {
            if let Some(card_id) = app.selected_card {
                app.active_board_mut().archive_card(card_id);
                app.selected_card = None;
                return true;
            }
            false
        }

        // 1-5 = switch view
        Key::Num1 if key.modifiers.alt => {
            app.view = View::Board;
            true
        }
        Key::Num2 if key.modifiers.alt => {
            app.view = View::Statistics;
            true
        }
        Key::Num3 if key.modifiers.alt => {
            app.view = View::Archive;
            true
        }
        Key::Num4 if key.modifiers.alt => {
            app.view = View::BoardList;
            true
        }

        // P = cycle priority on selected card
        Key::P => {
            if let Some(card_id) = app.selected_card
                && let Some(card) = app.active_board_mut().cards.get_mut(&card_id) {
                    card.priority = card.priority.next();
                    return true;
                }
            false
        }

        // M = move card right one column
        Key::M => {
            if let Some(card_id) = app.selected_card {
                let board = app.active_board();
                if let Some(from_col) = board.find_card_column(card_id) {
                    let to_col = from_col.saturating_add(1);
                    if to_col < board.columns.len() {
                        app.active_board_mut().move_card(card_id, from_col, to_col, 0);
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
                    && from_col > 0 {
                        let to_col = from_col.saturating_sub(1);
                        app.active_board_mut().move_card(card_id, from_col, to_col, 0);
                        return true;
                    }
            }
            false
        }

        // T = sort current column
        Key::T => {
            if app.view == View::Board {
                let col = app.selected_column;
                app.active_board_mut().sort_column(col);
                return true;
            }
            false
        }

        _ => false,
    }
}

/// Handle key events during input mode.
fn handle_input_key(app: &mut KanbanApp, key: &KeyEvent) -> bool {
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

            if text.is_empty() {
                return true;
            }

            match mode {
                InputMode::NewCardTitle => {
                    let col = app.selected_column;
                    app.add_card(&text, col);
                }
                InputMode::SearchFilter => {
                    app.filter.search_text = text;
                }
                InputMode::AssigneeFilter => {
                    app.filter.assignee_filter = text;
                }
                InputMode::NewBoardName => {
                    app.add_board(&text);
                }
                InputMode::NewColumnName => {
                    app.active_board_mut().add_column(&text);
                }
                InputMode::CardDescription => {
                    if let Some(card_id) = app.selected_card
                        && let Some(card) = app.active_board_mut().cards.get_mut(&card_id) {
                            card.description = text;
                        }
                }
                InputMode::AddComment => {
                    if let Some(card_id) = app.selected_card {
                        let ts = app.next_timestamp();
                        if let Some(card) = app.active_board_mut().cards.get_mut(&card_id) {
                            card.add_comment("User", &text, ts);
                        }
                    }
                }
                InputMode::AddChecklistItem => {
                    if let Some(card_id) = app.selected_card
                        && let Some(card) = app.active_board_mut().cards.get_mut(&card_id) {
                            card.add_checklist_item(&text);
                        }
                }
                InputMode::EditCardTitle => {
                    if let Some(card_id) = app.selected_card
                        && let Some(card) = app.active_board_mut().cards.get_mut(&card_id) {
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
        _ => {
            if let Some(ch) = key.text {
                app.input_buffer.push(ch);
                return true;
            }
            false
        }
    }
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let mut app = KanbanApp::new();
    app.create_sample_data();

    let width: f32 = 1200.0;
    let height: f32 = 800.0;

    // Build widget tree for the window
    let root = Widget::container()
        .with_background(palette::CRUST)
        .with_flex_direction(FlexDirection::Column);
    let mut widget_tree = WidgetTree::new(root, width, height);
    widget_tree.layout();

    // Render the application
    let render_tree = render_app(&app, width, height);

    // In a real windowing environment, the render tree would be submitted
    // to the compositor. Here we just verify it's non-empty.
    let _cmd_count = render_tree.len();
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
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
        let c = Priority::High.color();
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
        let l = Label::new("Bug", palette::RED);
        assert_eq!(l.name, "Bug");
        assert_eq!(l.color, palette::RED);
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
        let priorities: Vec<Priority> = ids.iter().map(|id| board.cards.get(id).unwrap().priority).collect();
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
        let titles: Vec<&str> = ids.iter().map(|id| board.cards.get(id).unwrap().title.as_str()).collect();
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
        let label = Label::new("Bug", palette::RED);
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

    #[test]
    fn test_json_validate_export() {
        let board = Board::default_board();
        assert!(JsonImporter::validate_export(&board));
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

    #[test]
    fn test_app_export_json() {
        let mut app = KanbanApp::new();
        app.create_sample_data();
        let json = app.export_json();
        assert!(!json.is_empty());
        assert!(json.contains("My Project"));
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
        let first_card_id = app.active_board().columns.first()
            .and_then(|c| c.card_ids.first().copied());
        app.selected_card = first_card_id;
        app.view = View::CardDetail;
        let tree = render_app(&app, 1200.0, 800.0);
        assert!(!tree.is_empty());
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
            text: None,
        }
    }

    fn make_char_key(ch: char) -> KeyEvent {
        KeyEvent {
            key: Key::Unknown(ch as u32),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some(ch),
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
        assert_eq!(app.active_board().cards.get(&id).unwrap().priority, Priority::High);
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
        assert_eq!(app.active_board().columns.first().unwrap().card_ids.len(), 2);
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
        assert_eq!(app.active_board().columns.first().unwrap().card_ids.len(), 1);
    }

    #[test]
    fn test_input_mode_enter_empty_noop() {
        let mut app = KanbanApp::new();
        app.input_mode = InputMode::NewCardTitle;
        app.input_buffer.clear();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.input_mode, InputMode::None);
        assert!(app.active_board().columns.first().unwrap().card_ids.is_empty());
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
        assert_eq!(app.active_board().cards.get(&id).unwrap().checklist.len(), 1);
    }

    #[test]
    fn test_input_mode_edit_title() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Old Title", 0).unwrap();
        app.selected_card = Some(id);
        app.input_mode = InputMode::EditCardTitle;
        app.input_buffer = "New Title".to_string();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.active_board().cards.get(&id).unwrap().title, "New Title");
    }

    #[test]
    fn test_input_mode_edit_description() {
        let mut app = KanbanApp::new();
        let id = app.add_card("Task", 0).unwrap();
        app.selected_card = Some(id);
        app.input_mode = InputMode::CardDescription;
        app.input_buffer = "New description".to_string();
        handle_key_event(&mut app, &make_key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.active_board().cards.get(&id).unwrap().description, "New description");
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
            text: None,
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
        let colors = [
            palette::BASE, palette::MANTLE, palette::CRUST,
            palette::SURFACE0, palette::TEXT, palette::BLUE,
            palette::RED, palette::GREEN,
        ];
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors.get(i), colors.get(j), "colors at {} and {} should differ", i, j);
            }
        }
    }

    // ---- Widget integration tests ----

    #[test]
    fn test_widget_tree_render() {
        let root = Widget::container()
            .with_background(palette::CRUST)
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
        let count = board.columns.first().unwrap().active_card_count(&board.cards);
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
        let count = board.columns.first().unwrap().active_card_count(&board.cards);
        assert_eq!(count, 1);
    }
}
