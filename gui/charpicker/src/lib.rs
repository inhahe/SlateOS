//! A character picker -- [`CharPicker`]: every emoji, and the symbols, maths,
//! arrows, currency signs and Latin, Greek and Cyrillic letters a keyboard
//! does not have, found by category or by search, for a program to insert.
//!
//! # What it is for
//!
//! `roadmap-detailed.md` asks for one "Unicode selection dialog" that a
//! shortcut's binding, the tray's emoji entry and programs all open, "so users
//! learn one dialog". This is it. The names, keywords and order it shows are
//! [`charnames`]'s, generated from the Unicode Consortium's own data.
//!
//! # The parts
//!
//! A search field over everything; the categories down the left -- what was
//! picked lately, the emoji groups, then the other characters; a grid of the
//! category chosen or of what the search found; and under it a line naming
//! the character under the pointer, or under the keyboard's cursor, with its
//! code points, beside the six skin tones.
//!
//! # Picking
//!
//! A click on a cell, or Enter on the keyboard's, picks it:
//! [`CharPickerEvent::Picked`] carries the text to insert -- an emoji in the
//! skin tone chosen. The picker stays open, so a host that inserts one
//! character closes it and a host that lets the user insert several does not.
//! Escape is [`CharPickerEvent::Cancelled`].
//!
//! # Skin tones
//!
//! Each emoji is listed once and drawn in the tone chosen ([`charnames`]'s
//! "Skin tones") -- the recent ones too, which are remembered untoned for that
//! reason: a hand picked before the tone was changed is drawn in the new one.
//!
//! # What a host keeps
//!
//! The tone and the recent picks are the user's. A host that keeps them
//! between openings reads [`CharPicker::tone`] and [`CharPicker::recent`] when
//! the picker closes, and hands them back with [`CharPicker::with_tone`] and
//! [`CharPicker::with_recent`].
//!
//! # The keyboard
//!
//! Typing searches, from anywhere but the tones. Down from the search field
//! goes into the grid, where the arrows move the cursor, Page Up and Page Down
//! move it a gridful, Home and End to the ends, and Enter or Space picks; Up
//! from the grid's top row goes back to the field, Left from a row's first
//! cell to the categories, and Right from the categories into the grid. Enter
//! in the search field picks what the cursor is on -- after a search, the best
//! match, so a name typed and Enter inserts it. Among the categories Up, Down,
//! Home, End and the page keys choose as they move; among the tones Left and
//! Right do. Tab and Shift+Tab go round the four; Escape turns the picker down
//! from anywhere.

use charnames::Found;
use guitk::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::listview::{ListKey, ListViewport};
use guitk::palette::Palette;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text::{self, scaled};
use guitk::textedit::{self, SingleLine};
use guitk::textinput::{KeyEdit, TextInput};
use guitk::{field, grid, scroll_window, scrollbar, wheel};

pub use charnames::SkinTone;

/// How many recent picks the picker keeps.
pub const MAX_RECENT: usize = 32;

/// What a text field's menu calls the row that opens the picker -- the same
/// words wherever a field offers it, so users learn one row as one dialog.
pub const MENU_LABEL: &str = "Emoji & Symbols\u{2026}";

/// The chord that opens the picker over a text field, as a menu writes it
/// beside [`MENU_LABEL`]. See [`is_shortcut`].
pub const SHORTCUT_LABEL: &str = "Ctrl+.";

/// Whether `key` is the chord that opens the picker over the text field
/// that has the keyboard: Ctrl and the full stop -- the toolkits' own (GTK's
/// emoji chooser opens on it), and free in every field here, where Ctrl+.
/// types nothing and edits nothing.
#[must_use]
pub fn is_shortcut(key: &KeyEvent) -> bool {
    key.pressed && key.key == Key::Period && key.modifiers.is_ctrl_chord() && !key.modifiers.shift
}

/// The dialog's title.
const TITLE: &str = "Emoji & Symbols";

// The layout, in pixels at the default text size: each is drawn through
// `text::scaled`, so a larger text size makes a larger picker.
const PADDING: f32 = 12.0;
const TITLE_HEIGHT: f32 = 32.0;
const TITLE_SIZE: f32 = 13.0;
const FIELD_HEIGHT: f32 = 28.0;
const FIELD_PADDING: f32 = 8.0;
const FIELD_TEXT_SIZE: f32 = 13.0;
const GAP: f32 = 12.0;
const SIDEBAR_WIDTH: f32 = 160.0;
const SIDE_ROW: f32 = 24.0;
const SIDE_SIZE: f32 = 13.0;
const HEADING_SIZE: f32 = 11.0;
const ROW_PADDING: f32 = 8.0;
const CELL: f32 = 40.0;
const GLYPH_SIZE: f32 = 24.0;
const STATUS_HEIGHT: f32 = 48.0;
const STATUS_GLYPH: f32 = 30.0;
const NAME_SIZE: f32 = 13.0;
const CODE_SIZE: f32 = 11.0;
const TONE_CELL: f32 = 30.0;
const TONE_GLYPH: f32 = 18.0;
const BAR_WIDTH: f32 = 12.0;
const RADIUS: f32 = 6.0;
const WELL_RADIUS: f32 = 4.0;
const DIALOG_WIDTH: f32 = 560.0;
const DIALOG_HEIGHT: f32 = 460.0;

/// What the picker says happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CharPickerEvent {
    /// The user picked this text: one character, or an emoji's sequence in
    /// the skin tone chosen. Insert it; the picker stays open.
    Picked(String),
    /// The user turned the picker down -- Escape.
    Cancelled,
}

/// A category the grid can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// What was picked lately, most recent first.
    Recent,
    /// An emoji group, by its place in [`charnames::groups`].
    Emoji(usize),
    /// A category of other characters, by its name in
    /// [`charnames::categories`].
    Characters(&'static str),
}

impl Category {
    /// Its name in the sidebar.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Recent => "Recent",
            Self::Emoji(group) => charnames::groups().get(group).copied().unwrap_or(""),
            Self::Characters(name) => name,
        }
    }
}

/// A row of the sidebar: a heading over a run of categories, or a category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SideRow {
    Heading(&'static str),
    Category(Category),
}

/// The sidebar, top to bottom: the recent picks; the emoji groups under a
/// heading -- all but "Component", whose skin-tone swatches and hair styles
/// are parts of other emoji rather than emoji to insert -- and then the other
/// characters under theirs.
fn sidebar() -> Vec<SideRow> {
    let mut rows = vec![
        SideRow::Category(Category::Recent),
        SideRow::Heading("Emoji"),
    ];
    rows.extend(
        charnames::groups()
            .iter()
            .enumerate()
            .filter(|(_, name)| **name != "Component")
            .map(|(group, _)| SideRow::Category(Category::Emoji(group))),
    );
    rows.push(SideRow::Heading("Characters"));
    rows.extend(charnames::categories().map(|name| SideRow::Category(Category::Characters(name))));
    rows
}

/// What a cell of the grid holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    /// An emoji or a named character.
    Known(Found),
    /// A character a code point typed in the search names, which has no name
    /// here -- U+4E00, say: the search still offers what was asked for.
    Bare(char),
}

impl Cell {
    /// What it is in skin tone `tone`: itself, for all but an emoji that
    /// comes in tones.
    fn toned(self, tone: Option<SkinTone>) -> Self {
        match (self, tone) {
            (Self::Known(found), Some(tone)) => Self::Known(found.in_tone(tone)),
            _ => self,
        }
    }

    /// The text it inserts and is drawn as.
    fn text(self) -> String {
        match self {
            Self::Known(found) => found.text.to_string(),
            Self::Bare(c) => c.to_string(),
        }
    }

    /// Its name, if it has one here.
    fn name(self) -> Option<&'static str> {
        match self {
            Self::Known(found) => Some(found.name),
            Self::Bare(_) => None,
        }
    }
}

/// What `text` -- a recent pick -- is as a cell, if the picker can show it.
fn cell_of(text: &str) -> Option<Cell> {
    if let Some(found) = charnames::lookup(text) {
        return Some(Cell::Known(found));
    }
    let mut chars = text.chars();
    let c = chars.next()?;
    (chars.next().is_none() && !c.is_control() && !c.is_whitespace()).then_some(Cell::Bare(c))
}

/// `text`'s code points, as the status line writes them: "U+1F44B U+1F3FD".
fn code_points(text: &str) -> String {
    text.chars()
        .map(|c| format!("U+{:04X}", u32::from(c)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The parts of the picker that take the keyboard, in Tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The search field.
    Search,
    /// The categories down the left.
    Categories,
    /// The grid.
    Grid,
    /// The skin tones.
    Tones,
}

impl Part {
    const ORDER: [Self; 4] = [Self::Search, Self::Categories, Self::Grid, Self::Tones];

    /// The part after this one in Tab order, or before it, round the ends.
    fn step(self, backwards: bool) -> Self {
        let at = Self::ORDER.iter().position(|&p| p == self).unwrap_or(0);
        let to = if backwards {
            at.checked_sub(1)
        } else {
            at.checked_add(1)
        };
        to.and_then(|i| Self::ORDER.get(i).copied())
            .unwrap_or(if backwards { Self::Tones } else { Self::Search })
    }
}

/// One of the picker's two scrolling lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    /// The categories.
    Categories,
    /// The grid.
    Grid,
}

/// What a point in the picker reaches: the hit boxes its frame records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The picker itself, wherever it is nothing else -- so a host can tell
    /// a click on the picker from one past its edge.
    Picker,
    /// The search field.
    Search,
    /// The categories' well, between and below their rows.
    Sidebar,
    /// A category's row, by its place in the sidebar.
    Category(usize),
    /// The grid's well, between and below its cells.
    Grid,
    /// A cell, by its place among those shown.
    Cell(usize),
    /// A skin tone's swatch: `None` is no tone.
    Tone(Option<SkinTone>),
    /// A list's scrollbar column, outside its thumb.
    Track(List),
    /// A list's scrollbar thumb.
    Thumb(List),
}

/// The skin tones in the order their swatches stand: none first.
const TONES: [Option<SkinTone>; 6] = [
    None,
    Some(SkinTone::Light),
    Some(SkinTone::MediumLight),
    Some(SkinTone::Medium),
    Some(SkinTone::MediumDark),
    Some(SkinTone::Dark),
];

/// A scrollbar thumb held by the pointer: which list's, and how far below
/// the thumb's top it was taken hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ThumbDrag {
    list: List,
    grab: f32,
}

/// Where everything is in a picker `width` by `height`: the one computation
/// the drawing and the keyboard both read.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    title: Rect,
    search: Rect,
    sidebar: Rect,
    grid: Rect,
    status: Rect,
    tones: Rect,
}

impl Layout {
    fn new(width: f32, height: f32) -> Self {
        let pad = scaled(PADDING);
        let inner_w = (width - 2.0 * pad).max(0.0);
        let title = Rect::new(0.0, 0.0, width.max(0.0), scaled(TITLE_HEIGHT));
        let search = Rect::new(pad, title.bottom() + pad, inner_w, scaled(FIELD_HEIGHT));
        let status_h = scaled(STATUS_HEIGHT);
        // A picker too short for its parts gives the lists no rows, not a
        // negative height: a box whose bottom is above its top.
        let status = Rect::new(
            pad,
            (height - pad - status_h).max(search.bottom()),
            inner_w,
            status_h,
        );
        let body_y = search.bottom() + pad;
        let body_h = (status.y - pad - body_y).max(0.0);
        let sidebar = Rect::new(pad, body_y, scaled(SIDEBAR_WIDTH).min(inner_w), body_h);
        let grid_x = (sidebar.right() + scaled(GAP)).min(pad + inner_w);
        let grid = Rect::new(grid_x, body_y, (pad + inner_w - grid_x).max(0.0), body_h);
        let tone = scaled(TONE_CELL);
        let tones_w = (tone * 6.0).min(status.w);
        let tones = Rect::new(
            status.right() - tones_w,
            status.y + (status.h - tone) / 2.0,
            tones_w,
            tone,
        );
        Self {
            title,
            search,
            sidebar,
            grid,
            status,
            tones,
        }
    }

    /// How many sidebar rows show.
    fn side_rows(&self) -> usize {
        rows_in(self.sidebar.h, scaled(SIDE_ROW))
    }

    /// Where the cells go.
    fn cells(&self) -> Cells {
        Cells::in_grid(self.grid)
    }
}

/// Where the grid's cells go: how many across, how wide each is, and how
/// many rows show.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cells {
    /// The grid's well less its scrollbar's column.
    area: Rect,
    /// How many cells a row holds: never none.
    columns: usize,
    /// How wide each cell is: the area shared among them.
    width: f32,
    /// How tall each is.
    height: f32,
    /// How many rows show.
    rows_shown: usize,
}

impl Cells {
    fn in_grid(grid: Rect) -> Self {
        // The scrollbar's column is kept whether a bar is drawn in it or not,
        // so the columns do not change when a search grows long enough to
        // need one.
        let bar = scaled(BAR_WIDTH).min(grid.w);
        let area = Rect::new(grid.x, grid.y, (grid.w - bar).max(0.0), grid.h);
        let height = scaled(CELL);
        let columns = grid::columns_across(area.w, height, 0.0).get();
        Self {
            area,
            columns,
            width: area.w / count_f32(columns),
            height,
            rows_shown: rows_in(area.h, height),
        }
    }

    /// How many rows `len` cells make.
    fn rows(&self, len: usize) -> usize {
        len.div_ceil(self.columns)
    }

    /// The row cell `i` is in.
    fn row_of(&self, i: usize) -> usize {
        i.checked_div(self.columns).unwrap_or(0)
    }

    /// The column cell `i` is in.
    fn column_of(&self, i: usize) -> usize {
        i.checked_rem(self.columns).unwrap_or(0)
    }

    /// Cell `i`'s box, with row `first` at the top of the area.
    fn rect(&self, i: usize, first: usize) -> Rect {
        let row = self.row_of(i).saturating_sub(first);
        Rect::new(
            self.area.x + count_f32(self.column_of(i)) * self.width,
            self.area.y + count_f32(row) * self.height,
            self.width,
            self.height,
        )
    }
}

/// A count of rows or cells on screen as a length's multiplier.
#[allow(
    clippy::cast_precision_loss,
    reason = "a count of rows or cells on screen: a few thousand at most, far inside f32's range"
)]
fn count_f32(n: usize) -> f32 {
    n as f32
}

/// How many whole rows `row` tall fit in `height`: none for a height or a row
/// that is not a positive number.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a positive, finite quotient of two lengths on a screen, floored"
)]
fn rows_in(height: f32, row: f32) -> usize {
    if row > 0.0 && height > 0.0 && height.is_finite() {
        (height / row).floor() as usize
    } else {
        0
    }
}

/// The top of a line of text `size` pixels at `weight` centred in `rect`.
fn centred(rect: Rect, size: f32, weight: FontWeightHint) -> f32 {
    rect.y + (rect.h - text::line_height(size, weight)) / 2.0
}

/// Whether `key` types something into a field: text that is no control
/// character, without a Ctrl chord.
fn types(key: &KeyEvent) -> bool {
    !key.text.is_empty()
        && !key.modifiers.is_ctrl_chord()
        && !key.text.chars().any(char::is_control)
}

/// The character picker. Shown in a window or a popup of its host's: the
/// host forwards every key and mouse event, draws [`render`](Self::render) at
/// the size it passes to the handlers, and acts on the events they return.
#[derive(Clone, Debug)]
pub struct CharPicker {
    /// What has been typed in the search field.
    search: TextInput,
    /// The sidebar's rows, built once.
    side: Vec<SideRow>,
    /// The sidebar's scroll, and the category chosen as a row of `side`.
    side_view: ListViewport,
    /// The category the grid shows while the search is empty.
    category: Category,
    /// The cells the grid shows: the category's, or what the search found.
    shown: Vec<Cell>,
    /// The keyboard's cell, as a place in `shown`.
    cursor: Option<usize>,
    /// The grid's first row on screen.
    first_row: usize,
    /// Which part has the keyboard.
    focus: Part,
    /// What the pointer is over.
    hovered: Option<Target>,
    /// A scrollbar thumb the pointer holds.
    drag: Option<ThumbDrag>,
    /// The scroll wheel's part-rows, between notches.
    wheel: wheel::Accumulator,
    /// The skin tone emoji are drawn and picked in.
    tone: Option<SkinTone>,
    /// The recent picks, most recent first, untoned.
    recent: Vec<String>,
    /// The raised hand the tone swatches are drawn as.
    hand: Option<Found>,
    /// The user's focus-ring width.
    focus_ring: f32,
    /// The user's caret width.
    caret_width: f32,
}

impl Default for CharPicker {
    fn default() -> Self {
        Self::new()
    }
}

impl CharPicker {
    /// A picker with nothing picked yet, on the first emoji group, its
    /// emoji in no skin tone.
    #[must_use]
    pub fn new() -> Self {
        let side = sidebar();
        let first = side
            .iter()
            .position(|row| matches!(row, SideRow::Category(Category::Emoji(_))))
            .unwrap_or(0);
        let mut picker = Self {
            search: TextInput::new(),
            side,
            side_view: ListViewport::new(0),
            category: Category::Emoji(0),
            shown: Vec::new(),
            cursor: None,
            first_row: 0,
            focus: Part::Search,
            hovered: None,
            drag: None,
            wheel: wheel::Accumulator::default(),
            tone: None,
            recent: Vec::new(),
            hand: charnames::lookup("\u{270B}"),
            focus_ring: guitk::style::FOCUS_RING_WIDTH,
            caret_width: textedit::CARET_WIDTH,
        };
        picker.choose(first);
        // Back to the top: choosing in a list with no rows on screen yet
        // scrolls the choice to the top row, which would open the sidebar
        // with Recent and the "Emoji" heading hidden above it. From the top,
        // the first height the list is given scrolls only as far as the
        // choice needs.
        picker.side_view.scroll_to(0, picker.side.len());
        picker
    }

    /// Start with these recent picks, most recent first -- what
    /// [`recent`](Self::recent) said when the picker last closed -- and on
    /// them, if there are any. Text the picker cannot show is dropped, and so
    /// is any past the first [`MAX_RECENT`].
    #[must_use]
    pub fn with_recent(mut self, recent: Vec<String>) -> Self {
        let mut kept: Vec<String> = Vec::new();
        for text in recent {
            if kept.len() == MAX_RECENT {
                break;
            }
            if cell_of(&text).is_some() && !kept.contains(&text) {
                kept.push(text);
            }
        }
        self.recent = kept;
        if !self.recent.is_empty() {
            self.choose(0);
        }
        self
    }

    /// Draw and pick emoji in skin tone `tone` -- what [`tone`](Self::tone)
    /// said when the picker last closed.
    #[must_use]
    pub fn with_tone(mut self, tone: Option<SkinTone>) -> Self {
        self.tone = tone;
        self
    }

    /// Draw the focus ring this wide: the user's setting.
    #[must_use]
    pub fn with_focus_ring(mut self, width: f32) -> Self {
        if width.is_finite() && width > 0.0 {
            self.focus_ring = width;
        }
        self
    }

    /// Draw the search field's caret this wide: the user's setting.
    #[must_use]
    pub fn with_caret_width(mut self, width: f32) -> Self {
        if width.is_finite() && width > 0.0 {
            self.caret_width = width;
        }
        self
    }

    /// The size the picker is laid out for at the user's text size, for a
    /// host that sizes its window or popup to it.
    #[must_use]
    pub fn preferred_size() -> (f32, f32) {
        (scaled(DIALOG_WIDTH), scaled(DIALOG_HEIGHT))
    }

    /// The skin tone emoji are drawn and picked in.
    #[must_use]
    pub fn tone(&self) -> Option<SkinTone> {
        self.tone
    }

    /// The recent picks, most recent first, untoned -- for a host to keep.
    #[must_use]
    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// The category the grid shows while the search is empty.
    #[must_use]
    pub fn category(&self) -> Category {
        self.category
    }

    /// Which part has the keyboard.
    #[must_use]
    pub fn focus(&self) -> Part {
        self.focus
    }

    /// What has been typed in the search field.
    #[must_use]
    pub fn search_text(&self) -> &str {
        self.search.text()
    }

    /// The grid's cells, as drawn: each the text it inserts.
    pub fn shown(&self) -> impl Iterator<Item = String> + '_ {
        self.shown.iter().map(|cell| cell.toned(self.tone).text())
    }

    /// The keyboard's cell, as a place among [`shown`](Self::shown).
    #[must_use]
    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    // ---------------------------------------------------------------------
    // Choosing
    // ---------------------------------------------------------------------

    /// The sidebar's row `row` is the category shown, if it is a category;
    /// choosing one empties the search, whose results it replaces.
    fn choose(&mut self, row: usize) {
        let Some(SideRow::Category(category)) = self.side.get(row).copied() else {
            return;
        };
        self.side_view.select(Some(row), self.side.len());
        self.category = category;
        self.search.clear();
        self.refresh();
    }

    /// The cells of `category`.
    fn cells_of(&self, category: Category) -> Vec<Cell> {
        match category {
            Category::Recent => self.recent.iter().filter_map(|t| cell_of(t)).collect(),
            Category::Emoji(group) => charnames::emoji_in(group)
                .map(|e| Cell::Known(e.found()))
                .collect(),
            Category::Characters(name) => charnames::characters_in(name).map(Cell::Known).collect(),
        }
    }

    /// After the search or the category changed: what the grid shows -- the
    /// search's results while there is a search, else the category -- from
    /// its top, with the cursor on the best match of a search.
    fn refresh(&mut self) {
        let query = self.search.text().trim();
        if query.is_empty() {
            self.shown = self.cells_of(self.category);
            self.cursor = None;
        } else {
            let mut found: Vec<Cell> = charnames::search(query)
                .into_iter()
                .map(Cell::Known)
                .collect();
            if found.is_empty() {
                found.extend(charnames::code_point(query).map(Cell::Bare));
            }
            self.shown = found;
            self.cursor = (!self.shown.is_empty()).then_some(0);
        }
        self.first_row = 0;
        // A cell under the pointer is another cell now.
        if matches!(self.hovered, Some(Target::Cell(_))) {
            self.hovered = None;
        }
    }

    /// Pick cell `i`: remember it, and say what to insert.
    fn pick(&mut self, i: usize) -> Option<CharPickerEvent> {
        let cell = *self.shown.get(i)?;
        self.cursor = Some(i);
        let remembered = cell.text();
        self.recent.retain(|t| *t != remembered);
        self.recent.insert(0, remembered);
        self.recent.truncate(MAX_RECENT);
        Some(CharPickerEvent::Picked(cell.toned(self.tone).text()))
    }

    /// The viewports learn how many rows `layout` gives them, and the grid's
    /// scroll is kept inside its rows -- only clamped, never moved to the
    /// cursor, so a wheel scroll survives the next pointer move.
    fn fit(&mut self, layout: &Layout) {
        let side = layout.side_rows();
        if self.side_view.height() != side {
            self.side_view.set_height(side, self.side.len());
        }
        let cells = layout.cells();
        let last_first = cells
            .rows(self.shown.len())
            .saturating_sub(cells.rows_shown);
        self.first_row = self.first_row.min(last_first);
    }

    /// Bring the cursor's row on screen.
    fn reveal(&mut self, cells: &Cells) {
        let Some(cursor) = self.cursor else {
            return;
        };
        let row = cells.row_of(cursor);
        if row < self.first_row {
            self.first_row = row;
        } else if cells.rows_shown > 0 && row >= self.first_row.saturating_add(cells.rows_shown) {
            self.first_row = row.saturating_add(1).saturating_sub(cells.rows_shown);
        }
    }

    /// Move the cursor to cell `to`, and the grid with it.
    fn move_cursor(&mut self, to: usize, cells: &Cells) {
        if let Some(last) = self.shown.len().checked_sub(1) {
            self.cursor = Some(to.min(last));
            self.reveal(cells);
        }
    }

    // ---------------------------------------------------------------------
    // The keyboard
    // ---------------------------------------------------------------------

    /// Handle a key for a picker drawn `width` by `height` -- the size, so
    /// the arrows and the page keys move by the grid on screen.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        width: f32,
        height: f32,
    ) -> Option<CharPickerEvent> {
        if !key.pressed {
            return None;
        }
        let layout = Layout::new(width, height);
        self.fit(&layout);
        let cells = layout.cells();
        match key.key {
            Key::Escape => return Some(CharPickerEvent::Cancelled),
            Key::Tab if !key.modifiers.ctrl && !key.modifiers.alt => {
                self.focus = self.focus.step(key.modifiers.shift);
                match self.focus {
                    // What is typed after Tab replaces the search, as in
                    // every field the keyboard comes to.
                    Part::Search => self.search.select_all(),
                    Part::Grid => self.enter_grid(&cells),
                    Part::Categories | Part::Tones => {}
                }
                return None;
            }
            _ => {}
        }
        match self.focus {
            Part::Search => self.search_key(key, &cells),
            Part::Categories => self.categories_key(key, &cells),
            Part::Grid => self.grid_key(key, &cells),
            Part::Tones => {
                self.tones_key(key);
                None
            }
        }
    }

    /// The keyboard comes to the grid, onto the cell it was on or the first
    /// one on screen.
    fn enter_grid(&mut self, cells: &Cells) {
        self.focus = Part::Grid;
        if self.cursor.is_none() && !self.shown.is_empty() {
            let first_on_screen = self.first_row.saturating_mul(cells.columns);
            self.move_cursor(first_on_screen, cells);
        }
        self.reveal(cells);
    }

    fn search_key(&mut self, key: &KeyEvent, cells: &Cells) -> Option<CharPickerEvent> {
        let plain = !key.modifiers.alt && !key.modifiers.super_key && !key.modifiers.ctrl;
        match key.key {
            Key::Enter => return self.cursor.and_then(|i| self.pick(i)),
            // Down the screen from the field is into the grid; Up, Left,
            // Right, Home and End stay the field's.
            Key::Down | Key::PageDown if plain => {
                self.enter_grid(cells);
                return None;
            }
            _ => {}
        }
        self.edit_search(key)
    }

    /// `key` edits the search; the grid follows what it says.
    fn edit_search(&mut self, key: &KeyEvent) -> Option<CharPickerEvent> {
        match self
            .search
            .edit_key(key, scaled(FIELD_TEXT_SIZE), FontWeightHint::Regular)
        {
            KeyEdit::Changed => self.refresh(),
            KeyEdit::Handled | KeyEdit::Unhandled => {}
        }
        None
    }

    /// From the categories or the grid: a key that types -- or a Backspace,
    /// while there is a search to take a character from -- goes to the end of
    /// the search, and the keyboard with it, as in any list a user can type
    /// to find in.
    fn typed_elsewhere(&mut self, key: &KeyEvent) -> Option<CharPickerEvent> {
        let backspace = key.key == Key::Backspace && !self.search.text().is_empty();
        if !types(key) && !backspace {
            return None;
        }
        self.focus = Part::Search;
        let query = self.search.text().to_string();
        self.search.set_text(&query);
        self.edit_search(key)
    }

    fn categories_key(&mut self, key: &KeyEvent, cells: &Cells) -> Option<CharPickerEvent> {
        if key.key == Key::Right && !key.modifiers.alt && !key.modifiers.super_key {
            self.enter_grid(cells);
            return None;
        }
        if let Some(list_key) = ListKey::of(key) {
            let side = &self.side;
            let to = list_key.target_where(
                self.side_view.selected(),
                side.len(),
                self.side_view.height(),
                |i| matches!(side.get(i), Some(SideRow::Category(_))),
            )?;
            self.choose(to);
            return None;
        }
        self.typed_elsewhere(key)
    }

    fn grid_key(&mut self, key: &KeyEvent, cells: &Cells) -> Option<CharPickerEvent> {
        if key.modifiers.alt || key.modifiers.super_key {
            return None;
        }
        let columns = cells.columns;
        let page = cells.rows_shown.max(1).saturating_mul(columns);
        let last = self.shown.len().checked_sub(1);
        let at = self.cursor;
        match key.key {
            Key::Enter | Key::Space => return at.and_then(|i| self.pick(i)),
            Key::Left => match at {
                Some(i) if cells.column_of(i) > 0 => self.move_cursor(i.saturating_sub(1), cells),
                // From a row's first cell, the categories are to the left.
                _ => self.focus = Part::Categories,
            },
            Key::Right => {
                let to = at.map_or(0, |i| i.saturating_add(1));
                self.move_cursor(to, cells);
            }
            Key::Up => match at {
                Some(i) if i >= columns => self.move_cursor(i.saturating_sub(columns), cells),
                // From the top row, the search field is above.
                _ => self.focus = Part::Search,
            },
            Key::Down => {
                let (Some(i), Some(last)) = (at, last) else {
                    self.move_cursor(0, cells);
                    return None;
                };
                // Down into a shorter last row lands on its last cell; from
                // the last row, nowhere.
                if cells.row_of(i) < cells.row_of(last) {
                    self.move_cursor(i.saturating_add(columns), cells);
                }
            }
            Key::PageUp => {
                let to = at.map_or(0, |i| i.saturating_sub(page));
                self.move_cursor(to, cells);
            }
            Key::PageDown => {
                let to = at.map_or(0, |i| i.saturating_add(page));
                self.move_cursor(to, cells);
            }
            Key::Home => self.move_cursor(0, cells),
            Key::End => {
                if let Some(last) = last {
                    self.move_cursor(last, cells);
                }
            }
            _ => return self.typed_elsewhere(key),
        }
        None
    }

    fn tones_key(&mut self, key: &KeyEvent) {
        let at = TONES.iter().position(|&t| t == self.tone).unwrap_or(0);
        let to = match key.key {
            Key::Left | Key::Up => at.saturating_sub(1),
            Key::Right | Key::Down => at.saturating_add(1).min(TONES.len().saturating_sub(1)),
            Key::Home => 0,
            Key::End => TONES.len().saturating_sub(1),
            _ => return,
        };
        if let Some(&tone) = TONES.get(to) {
            self.tone = tone;
        }
    }

    // ---------------------------------------------------------------------
    // The pointer
    // ---------------------------------------------------------------------

    /// Handle a mouse event at coordinates relative to a picker drawn `width`
    /// by `height` -- the size the host draws it at, which the event is
    /// tested against. Forward every event while the picker is up, moves and
    /// releases too: a scrollbar is dragged by them.
    pub fn handle_mouse(
        &mut self,
        event: &MouseEvent,
        width: f32,
        height: f32,
    ) -> Option<CharPickerEvent> {
        let layout = Layout::new(width, height);
        self.fit(&layout);
        // Walked for its hit boxes: they do not depend on the colours, so
        // any palette answers alike.
        let frame = self.frame(&Palette::for_mode(false), width, height);
        let target = frame.hit_test(event.x, event.y);
        match event.kind {
            MouseEventKind::Move | MouseEventKind::Enter => {
                self.hovered = target;
                if let Some(drag) = self.drag {
                    self.drag_thumb(&frame, drag, event.y, &layout);
                }
                None
            }
            MouseEventKind::Leave => {
                self.hovered = None;
                None
            }
            MouseEventKind::Press(MouseButton::Left) => self.press(&frame, target, event.y, &layout),
            MouseEventKind::Release(MouseButton::Left) => {
                self.drag = None;
                None
            }
            MouseEventKind::Scroll { dy, .. } => {
                let rows = self.wheel.rows(dy);
                match target {
                    Some(
                        Target::Grid
                        | Target::Cell(_)
                        | Target::Track(List::Grid)
                        | Target::Thumb(List::Grid),
                    ) => self.scroll_grid(rows, &layout.cells()),
                    Some(
                        Target::Sidebar
                        | Target::Category(_)
                        | Target::Track(List::Categories)
                        | Target::Thumb(List::Categories),
                    ) => self.side_view.scroll_by(rows, self.side.len()),
                    _ => {}
                }
                None
            }
            _ => None,
        }
    }

    /// Scroll the grid `rows` rows, the cursor staying where it is.
    fn scroll_grid(&mut self, rows: isize, cells: &Cells) {
        let last_first = cells
            .rows(self.shown.len())
            .saturating_sub(cells.rows_shown);
        self.first_row = scroll_window::shift(self.first_row, rows).min(last_first);
    }

    fn press(
        &mut self,
        frame: &Frame<Target>,
        target: Option<Target>,
        y: f32,
        layout: &Layout,
    ) -> Option<CharPickerEvent> {
        match target? {
            Target::Search => {
                self.focus = Part::Search;
                None
            }
            Target::Category(row) => {
                self.focus = Part::Categories;
                self.choose(row);
                None
            }
            // A click picks without taking the keyboard from where it is: a
            // user typing a search goes on typing.
            Target::Cell(i) => self.pick(i),
            Target::Tone(tone) => {
                self.tone = tone;
                None
            }
            Target::Thumb(list) => {
                let thumb = frame.rect_of(|t| *t == Target::Thumb(list))?;
                self.drag = Some(ThumbDrag {
                    list,
                    grab: y - thumb.y,
                });
                None
            }
            Target::Track(list) => {
                // A press beside the thumb pages towards the press.
                let thumb = frame.rect_of(|t| *t == Target::Thumb(list))?;
                let cells = layout.cells();
                let page = match list {
                    List::Categories => self.side_view.height(),
                    List::Grid => cells.rows_shown,
                };
                let page = isize::try_from(page.max(1)).unwrap_or(isize::MAX);
                let delta = if y < thumb.y {
                    page.saturating_neg()
                } else {
                    page
                };
                match list {
                    List::Categories => self.side_view.scroll_by(delta, self.side.len()),
                    List::Grid => self.scroll_grid(delta, &cells),
                }
                None
            }
            Target::Picker | Target::Sidebar | Target::Grid => None,
        }
    }

    fn drag_thumb(&mut self, frame: &Frame<Target>, drag: ThumbDrag, y: f32, layout: &Layout) {
        let (Some(track), Some(thumb)) = (
            frame.rect_of(|t| *t == Target::Track(drag.list)),
            frame.rect_of(|t| *t == Target::Thumb(drag.list)),
        ) else {
            return;
        };
        match drag.list {
            List::Categories => {
                let len = self.side.len();
                let capacity = self.side_view.height();
                if let Some(first) =
                    scrollbar::first_from_drag(track, thumb.h, drag.grab, y, len, capacity)
                {
                    self.side_view.scroll_to(first, len);
                }
            }
            List::Grid => {
                let cells = layout.cells();
                let rows = cells.rows(self.shown.len());
                if let Some(first) =
                    scrollbar::first_from_drag(track, thumb.h, drag.grab, y, rows, cells.rows_shown)
                {
                    self.first_row = first;
                }
            }
        }
    }

    // ---------------------------------------------------------------------
    // Drawing
    // ---------------------------------------------------------------------

    /// Draw the picker `width` by `height`.
    #[must_use]
    pub fn render(&self, palette: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {
        self.frame(palette, width, height).into_tree().commands
    }

    /// Draw the picker `width` by `height`, recording what each part of it
    /// is clicked to reach -- the boxes the handlers test the pointer
    /// against, so nothing is drawn in one place and clicked in another.
    #[must_use]
    pub fn frame(&self, palette: &Palette, width: f32, height: f32) -> Frame<Target> {
        let layout = Layout::new(width, height);
        let mut frame = Frame::new(width, height);
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: palette.base,
            corner_radii: CornerRadii::all(scaled(RADIUS)),
        });
        frame.hit(Target::Picker, Rect::new(0.0, 0.0, width, height));
        Self::draw_title(palette, &mut frame, layout.title);
        self.draw_search(palette, &mut frame, layout.search);
        self.draw_sidebar(palette, &mut frame, layout.sidebar);
        self.draw_grid(palette, &mut frame, layout.grid);
        self.draw_status(palette, &mut frame, &layout);
        frame
    }

    fn draw_title(palette: &Palette, frame: &mut Frame<Target>, bar: Rect) {
        let r = scaled(RADIUS);
        frame.push(RenderCommand::FillRect {
            x: bar.x,
            y: bar.y,
            width: bar.w,
            height: bar.h,
            color: palette.surface0,
            corner_radii: CornerRadii {
                top_left: r,
                top_right: r,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
        });
        let size = scaled(TITLE_SIZE);
        let pad = scaled(PADDING);
        frame.push(RenderCommand::Text {
            x: bar.x + pad,
            y: centred(bar, size, FontWeightHint::Bold),
            text: TITLE.to_string(),
            color: palette.ink(palette.text),
            font_size: size,
            font_weight: FontWeightHint::Bold,
            max_width: Some((bar.w - 2.0 * pad).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn draw_search(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect) {
        let focused = self.focus == Part::Search;
        field::draw(
            frame,
            palette,
            rect,
            field::State {
                hovered: self.hovered == Some(Target::Search),
                focused,
                disabled: false,
                invalid: false,
            },
            self.focus_ring,
        );
        frame.hit(Target::Search, rect);
        let size = scaled(FIELD_TEXT_SIZE);
        let pad = scaled(FIELD_PADDING);
        let line = text::line_height(size, FontWeightHint::Regular);
        let inner = Rect::new(
            rect.x + pad,
            rect.y + (rect.h - line) / 2.0,
            (rect.w - 2.0 * pad).max(0.0),
            line,
        );
        if self.search.text().is_empty() {
            frame.push(RenderCommand::Text {
                x: inner.x,
                y: inner.y,
                text: "Search by name, keyword or U+code".to_string(),
                color: palette.ink(palette.subtext0),
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some(inner.w),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let mut tree = RenderTree::new();
        textedit::draw(
            &mut tree,
            &SingleLine {
                text: self.search.text(),
                cursor: self.search.cursor(),
                selection_anchor: self.search.selection_anchor(),
                focused,
                x: inner.x,
                y: inner.y,
                width: inner.w,
                line_height: line,
                font_size: size,
                weight: FontWeightHint::Regular,
                color: palette.ink(palette.text),
                selection_bg: palette.accent,
                selection_fg: guitk::palette::readable_on(palette.accent),
                caret_width: self.caret_width,
            },
        );
        for command in tree.commands {
            frame.push(command);
        }
    }

    /// A list's well, and the focus ring round it when it has the keyboard.
    fn draw_well(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect, focused: bool) {
        let radii = CornerRadii::all(scaled(WELL_RADIUS));
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: palette.crust,
            corner_radii: radii,
        });
        let (edge, width) = if focused {
            (palette.accent, self.focus_ring)
        } else {
            (palette.surface1, 1.0)
        };
        frame.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: edge,
            line_width: width,
            corner_radii: radii,
        });
    }

    /// A ground under a row or a cell: the selection's, the pointer's, or
    /// none.
    fn draw_ground(palette: &Palette, frame: &mut Frame<Target>, rect: Rect, selected: bool, hovered: bool) {
        let color = if selected {
            palette.selection_fill()
        } else if hovered {
            palette.surface0
        } else {
            return;
        };
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color,
            corner_radii: CornerRadii::all(scaled(WELL_RADIUS)),
        });
    }

    /// A list's scrollbar, if it needs one, in a column at the right of
    /// `rect`.
    fn draw_bar(
        &self,
        palette: &Palette,
        frame: &mut Frame<Target>,
        rect: Rect,
        list: List,
        (len, capacity, first): (usize, usize, usize),
    ) {
        if !scrollbar::needed(len, capacity) {
            return;
        }
        let w = scaled(BAR_WIDTH).min(rect.w);
        let track = Rect::new(rect.right() - w, rect.y, w, rect.h);
        let thumb = scrollbar::thumb(track, len, capacity, first);
        scrollbar::draw(
            frame,
            palette,
            track,
            thumb,
            scrollbar::BarState {
                hovered: matches!(self.hovered, Some(Target::Track(l) | Target::Thumb(l)) if l == list),
                dragging: self.drag.is_some_and(|d| d.list == list),
            },
        );
        frame.hit(Target::Track(list), track);
        frame.hit(Target::Thumb(list), thumb);
    }

    fn draw_sidebar(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect) {
        self.draw_well(palette, frame, rect, self.focus == Part::Categories);
        frame.hit(Target::Sidebar, rect);
        let row_h = scaled(SIDE_ROW);
        let len = self.side.len();
        let capacity = rows_in(rect.h, row_h);
        let mut view = self.side_view;
        if view.height() != capacity {
            view.set_height(capacity, len);
        }
        let rows = view.visible_range(len);
        self.draw_bar(palette, frame, rect, List::Categories, (len, capacity, rows.start));
        let bar = if scrollbar::needed(len, capacity) {
            scaled(BAR_WIDTH).min(rect.w)
        } else {
            0.0
        };
        let pad = scaled(ROW_PADDING);
        // While a search is shown, no category is: none is drawn chosen.
        let searching = !self.search.text().trim().is_empty();
        frame.clip(rect);
        for (slot, i) in rows.enumerate() {
            let Some(&row) = self.side.get(i) else {
                continue;
            };
            let at = Rect::new(
                rect.x,
                rect.y + count_f32(slot) * row_h,
                (rect.w - bar).max(0.0),
                row_h,
            );
            let (label, size, weight, color) = match row {
                SideRow::Heading(name) => (
                    name,
                    scaled(HEADING_SIZE),
                    FontWeightHint::Bold,
                    palette.ink(palette.subtext0),
                ),
                SideRow::Category(category) => {
                    Self::draw_ground(
                        palette,
                        frame,
                        at,
                        !searching && view.selected() == Some(i),
                        self.hovered == Some(Target::Category(i)),
                    );
                    frame.hit(Target::Category(i), at);
                    (
                        category.name(),
                        scaled(SIDE_SIZE),
                        FontWeightHint::Regular,
                        palette.ink(palette.text),
                    )
                }
            };
            frame.push(RenderCommand::Text {
                x: at.x + pad,
                y: centred(at, size, weight),
                text: label.to_string(),
                color,
                font_size: size,
                font_weight: weight,
                max_width: Some((at.w - 2.0 * pad).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        frame.unclip();
    }

    fn draw_grid(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect) {
        self.draw_well(palette, frame, rect, self.focus == Part::Grid);
        frame.hit(Target::Grid, rect);
        let cells = Cells::in_grid(rect);
        let len = self.shown.len();
        if len == 0 {
            let size = scaled(SIDE_SIZE);
            let pad = scaled(ROW_PADDING);
            let message = if !self.search.text().trim().is_empty() {
                "Nothing matches"
            } else if self.category == Category::Recent {
                "Nothing picked yet"
            } else {
                "Nothing here"
            };
            frame.push(RenderCommand::Text {
                x: rect.x + pad,
                y: rect.y + pad,
                text: message.to_string(),
                color: palette.ink(palette.subtext0),
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some((rect.w - 2.0 * pad).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }
        let rows = cells.rows(len);
        let first = self.first_row.min(rows.saturating_sub(cells.rows_shown));
        self.draw_bar(palette, frame, rect, List::Grid, (rows, cells.rows_shown, first));
        let start = first.saturating_mul(cells.columns).min(len);
        let end = first
            .saturating_add(cells.rows_shown)
            .saturating_mul(cells.columns)
            .min(len);
        // The cursor is drawn as chosen while the keyboard is where it acts
        // -- the grid, or the search field whose Enter picks it.
        let cursor_live = matches!(self.focus, Part::Grid | Part::Search);
        let size = scaled(GLYPH_SIZE);
        frame.clip(cells.area);
        for i in start..end {
            let Some(cell) = self.shown.get(i) else {
                continue;
            };
            let at = cells.rect(i, first);
            Self::draw_ground(
                palette,
                frame,
                at,
                cursor_live && self.cursor == Some(i),
                self.hovered == Some(Target::Cell(i)),
            );
            Self::draw_glyph(palette, frame, at, &cell.toned(self.tone).text(), size);
            frame.hit(Target::Cell(i), at);
        }
        frame.unclip();
    }

    /// `glyph` at `size`, centred in `rect` and cut at it: a wide sequence
    /// drawn by a face with no ligature for it must not spill into the cell
    /// beside it.
    fn draw_glyph(palette: &Palette, frame: &mut Frame<Target>, rect: Rect, glyph: &str, size: f32) {
        let width = text::measure(glyph, size, FontWeightHint::Regular);
        frame.clip(rect);
        frame.push(RenderCommand::Text {
            x: rect.x + ((rect.w - width) / 2.0).max(0.0),
            y: centred(rect, size, FontWeightHint::Regular),
            text: glyph.to_string(),
            color: palette.ink(palette.text),
            font_size: size,
            font_weight: FontWeightHint::Regular,
            max_width: Some(rect.w),
            overflow: TextOverflow::Clip,
        });
        frame.unclip();
    }

    /// The cell the status line names: the one under the pointer, else the
    /// keyboard's.
    fn subject(&self) -> Option<Cell> {
        let i = match self.hovered {
            Some(Target::Cell(i)) => Some(i),
            _ => self.cursor,
        }?;
        self.shown.get(i).map(|cell| cell.toned(self.tone))
    }

    fn draw_status(&self, palette: &Palette, frame: &mut Frame<Target>, layout: &Layout) {
        let status = layout.status;
        frame.push(RenderCommand::FillRect {
            x: status.x,
            y: status.y,
            width: status.w,
            height: 1.0,
            color: palette.surface1,
            corner_radii: CornerRadii::ZERO,
        });
        let gap = scaled(GAP);
        let text_right = (layout.tones.x - gap).max(status.x);
        if let Some(cell) = self.subject() {
            let glyph_box = Rect::new(status.x, status.y, status.h.min(text_right - status.x).max(0.0), status.h);
            let glyph = cell.text();
            Self::draw_glyph(palette, frame, glyph_box, &glyph, scaled(STATUS_GLYPH));
            let x = glyph_box.right() + scaled(ROW_PADDING);
            let w = (text_right - x).max(0.0);
            let half = Rect::new(x, status.y, w, status.h / 2.0);
            let name = cell.name().unwrap_or("No name known here");
            let name_size = scaled(NAME_SIZE);
            frame.push(RenderCommand::Text {
                x,
                y: half.bottom() - text::line_height(name_size, FontWeightHint::Bold),
                text: name.to_string(),
                color: palette.ink(palette.text),
                font_size: name_size,
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
            frame.push(RenderCommand::Text {
                x,
                y: half.bottom(),
                text: code_points(&glyph),
                color: palette.ink(palette.subtext0),
                font_size: scaled(CODE_SIZE),
                font_weight: FontWeightHint::Regular,
                max_width: Some(w),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            let size = scaled(NAME_SIZE);
            frame.push(RenderCommand::Text {
                x: status.x,
                y: centred(status, size, FontWeightHint::Regular),
                text: "Click a character to insert it".to_string(),
                color: palette.ink(palette.subtext0),
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some((text_right - status.x).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        self.draw_tones(palette, frame, layout.tones);
    }

    /// The six skin tones, each a raised hand in that tone.
    fn draw_tones(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect) {
        let w = rect.w / count_f32(TONES.len());
        let size = scaled(TONE_GLYPH);
        for (slot, &tone) in TONES.iter().enumerate() {
            let at = Rect::new(rect.x + count_f32(slot) * w, rect.y, w, rect.h);
            let chosen = tone == self.tone;
            Self::draw_ground(palette, frame, at, chosen, self.hovered == Some(Target::Tone(tone)));
            if chosen && self.focus == Part::Tones {
                frame.push(RenderCommand::StrokeRect {
                    x: at.x,
                    y: at.y,
                    width: at.w,
                    height: at.h,
                    color: palette.accent,
                    line_width: self.focus_ring,
                    corner_radii: CornerRadii::all(scaled(WELL_RADIUS)),
                });
            }
            let glyph = match (self.hand, tone) {
                (Some(hand), Some(tone)) => hand.in_tone(tone).text.to_string(),
                (Some(hand), None) => hand.text.to_string(),
                // No hand in the tables: the modifier alone, which an emoji
                // face draws as a swatch of its tone.
                (None, Some(tone)) => tone.modifier().to_string(),
                (None, None) => "\u{270B}".to_string(),
            };
            Self::draw_glyph(palette, frame, at, &glyph, size);
            frame.hit(Target::Tone(tone), at);
        }
    }
}

#[cfg(test)]
mod tests;
