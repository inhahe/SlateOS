//! `Slate OS` `SQLite` Database Viewer / Browser
//!
//! A database viewer/browser tool (like DB Browser for `SQLite`) with:
//! - SQL parser: basic SELECT, INSERT, UPDATE, DELETE, CREATE TABLE, DROP TABLE
//! - Data types: INTEGER, REAL, TEXT, BLOB, NULL
//! - In-memory table storage (simulated `SQLite` engine)
//! - Table schema viewer with column names, types, constraints
//! - Paginated data browser with column sorting
//! - SQL query editor with syntax highlighting hints
//! - Query history with favorites
//! - Multiple database connections (tabs)
//! - Object tree sidebar (tables, indexes, views, triggers)
//! - Export: CSV, JSON, SQL INSERT statements
//! - Import: CSV with header detection
//! - Row editing: insert/update/delete individual rows
//! - WHERE clause builder (column, operator, value)
//! - Aggregate functions: COUNT, SUM, AVG, MIN, MAX
//! - Schema diagram: FK relationship visualization
//! - Multi-panel UI: sidebar, data grid, SQL editor, results
//!
//! Uses the guitk library for UI rendering.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
//
// Twelve crate-level `#![allow]`s stood here and were removed: not one of them
// silences anything the crate still trips. They were added when the drawing
// pass was a wall of hand-computed offsets -- `too_many_lines`,
// `similar_names`, `cast_precision_loss`, `wildcard_imports` -- and kept
// afterwards out of habit, so the crate carried a blanket permission for faults
// it had stopped committing, and would have carried it silently through the
// next one.

use guitk::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
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

/// The size the window opens at, and the size the probe draws at.
const WINDOW_WIDTH: f32 = 1200.0;
/// The height the window opens at.
const WINDOW_HEIGHT: f32 = 800.0;

const TOOLBAR_HEIGHT: f32 = 36.0;
const STATUS_BAR_HEIGHT: f32 = 22.0;
/// The widest the object-tree sidebar is ever drawn.
///
/// This used to be the *only* width the sidebar was drawn at, subtracted from
/// the window whatever the window measured, so a 200-point window handed the
/// data grid a width of -20.
const SIDEBAR_WIDTH: f32 = 220.0;
const TAB_HEIGHT: f32 = 30.0;
const ROW_HEIGHT: f32 = 26.0;
const HEADER_HEIGHT: f32 = 28.0;
const EDITOR_HEIGHT: f32 = 140.0;
const CORNER_RADIUS: f32 = 4.0;
const CELL_PADDING: f32 = 8.0;
const PAGE_SIZE: usize = 50;
const DEFAULT_COL_WIDTH: f32 = 140.0;

/// The narrowest a grid column is squeezed to before columns start being left
/// out of the picture altogether.
///
/// A column narrower than this is an ellipsis with a header over it. The grid
/// does not scroll sideways, so the choice is between columns nobody can read
/// and columns nobody can see; the pagination bar says how many were left out,
/// which is the one thing the old pass could not do -- it drew all of them at a
/// width of its own choosing and let the clip swallow whatever ran off the end,
/// leaving a table that looked complete and was not.
const MIN_COL_WIDTH: f32 = 60.0;

/// Height of the strip of tabs that names the four bottom panels.
const PANEL_TAB_HEIGHT: f32 = 26.0;
/// Height of the bar under the grid holding the page count and the arrows.
const PAGE_BAR_HEIGHT: f32 = 22.0;
/// Height of one row of the object tree, and of one filter chip under it.
const TREE_ROW_HEIGHT: f32 = 22.0;
/// Height of one line of the query history under the SQL editor.
const HISTORY_ROW_HEIGHT: f32 = 18.0;

/// The narrowest object-tree sidebar worth drawing.
///
/// Below this the tree is an ellipsis where a table name should be, and it has
/// been paid for out of the grid's width.
const MIN_SIDEBAR_WIDTH: f32 = 120.0;

/// The narrowest data grid worth having a sidebar beside.
///
/// A window this narrow is showing a database and no data, which is not a
/// database browser. The sidebar goes first.
const MIN_GRID_WIDTH: f32 = 260.0;

/// The shortest data grid worth taking a bottom panel out of: a header row, one
/// row of data, and the pagination bar that says which page it is on.
const MIN_GRID_HEIGHT: f32 = HEADER_HEIGHT + ROW_HEIGHT + PAGE_BAR_HEIGHT;

// ============================================================================
// Geometry
// ============================================================================

/// Every rectangle the picture is built from, solved from the live window size
/// on every frame.
///
/// Nothing here is a constant offset into a window nobody measured. The old
/// drawing pass wrote `SIDEBAR_WIDTH` and bare subtractions straight into the
/// commands -- `width - SIDEBAR_WIDTH` for the grid, `height - content_y -
/// STATUS_BAR_HEIGHT` for the content, `content_height - EDITOR_HEIGHT` for the
/// grid's height -- none of which was ever compared against zero. At 200 points
/// across the grid was 20 points *wide in the negative*, and at 150 points tall
/// it was 128 points tall in the negative, so the whole picture was drawn
/// upwards out of the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The whole window.
    pub window: Rect,
    /// The toolbar across the top: Execute, New Tab, the exports, Import.
    pub toolbar: Rect,
    /// The strip of database-connection tabs under the toolbar.
    pub tabs: Rect,
    /// The object-tree sidebar. Zero-width when the window is too narrow.
    pub sidebar: Rect,
    /// The data grid, including its header row and its pagination bar.
    pub grid: Rect,
    /// The bottom panel. Zero-height when the window is too short for it.
    pub panel: Rect,
    /// The strip along the bottom naming the database and the last action.
    pub status: Rect,
}

impl Layout {
    /// Solve the whole picture for a window of `w` by `h`.
    #[must_use]
    pub fn solve(w: f32, h: f32) -> Self {
        // A size that is not a number is not a size. NaN survives every `max`,
        // `min` and `clamp` below and comes out as a rectangle that contains no
        // point and clips everything away -- a blank window that answers no
        // press, with nothing in the picture to say why.
        let w = if w.is_finite() { w.max(0.0) } else { 0.0 };
        let h = if h.is_finite() { h.max(0.0) } else { 0.0 };
        let window = Rect::new(0.0, 0.0, w, h);

        // The status strip is given room first and everything above stops at
        // its top edge, so nothing can be painted under it and then answer a
        // press through it. Written the other way round -- the status pushed
        // down to clear a full-height toolbar -- a window thirty points tall
        // put the status bar at y=36 and drew it below the window's own bottom.
        let status_h = STATUS_BAR_HEIGHT.min(h);
        let status = Rect::new(0.0, (h - status_h).max(0.0), w, status_h);
        let bottom = status.y;

        let toolbar_h = TOOLBAR_HEIGHT.min(bottom);
        let toolbar = Rect::new(0.0, 0.0, w, toolbar_h);

        // The row of database tabs is given up before the grid is: a window
        // showing which databases are open and none of their contents is not a
        // database browser.
        let tabs_h = if toolbar_h + TAB_HEIGHT + MIN_GRID_HEIGHT <= bottom {
            TAB_HEIGHT
        } else {
            0.0
        };
        let tabs = Rect::new(0.0, toolbar_h, w, tabs_h);

        let content_y = toolbar_h + tabs_h;
        let content_h = (bottom - content_y).max(0.0);

        // The sidebar takes at most two fifths of the window and never more
        // than `SIDEBAR_WIDTH`, and goes altogether when what would be left is
        // too narrow to read a table in. The grid is the program.
        let wanted_side = (w * 0.4).min(SIDEBAR_WIDTH);
        let side_w = if wanted_side >= MIN_SIDEBAR_WIDTH && w - wanted_side >= MIN_GRID_WIDTH {
            wanted_side
        } else {
            0.0
        };
        let sidebar = Rect::new(0.0, content_y, side_w, content_h);

        let main_x = side_w;
        let main_w = (w - side_w).max(0.0);

        // The bottom panel is taken only if the grid still keeps a header row,
        // one row of data and its pagination bar afterwards.
        let panel_h = if content_h - EDITOR_HEIGHT >= MIN_GRID_HEIGHT {
            EDITOR_HEIGHT
        } else {
            0.0
        };
        let grid_h = (content_h - panel_h).max(0.0);
        let grid = Rect::new(main_x, content_y, main_w, grid_h);
        let panel = Rect::new(main_x, content_y + grid_h, main_w, panel_h);

        Self {
            window,
            toolbar,
            tabs,
            sidebar,
            grid,
            panel,
            status,
        }
    }

    /// The grid's column-header row.
    #[must_use]
    fn grid_header(&self) -> Rect {
        Rect::new(
            self.grid.x,
            self.grid.y,
            self.grid.w,
            HEADER_HEIGHT.min(self.grid.h),
        )
    }

    /// The bar along the bottom of the grid: the page count and the arrows.
    ///
    /// It is measured from what is left under the header rather than taken off
    /// the bottom unconditionally, so in a grid shorter than the two of them
    /// together the bar cannot end up above its own header.
    #[must_use]
    fn page_bar(&self) -> Rect {
        let h = PAGE_BAR_HEIGHT.min((self.grid.h - HEADER_HEIGHT).max(0.0));
        Rect::new(self.grid.x, self.grid.bottom() - h, self.grid.w, h)
    }

    /// The rows of the grid, between the header row and the pagination bar.
    #[must_use]
    fn grid_rows(&self) -> Rect {
        let top = self.grid_header().bottom();
        let bottom = self.page_bar().y;
        Rect::new(self.grid.x, top, self.grid.w, (bottom - top).max(0.0))
    }

    /// The strip of tabs naming the four bottom panels.
    #[must_use]
    fn panel_tabs(&self) -> Rect {
        Rect::new(
            self.panel.x,
            self.panel.y,
            self.panel.w,
            PANEL_TAB_HEIGHT.min(self.panel.h),
        )
    }

    /// What is left of the bottom panel under its tabs.
    #[must_use]
    fn panel_body(&self) -> Rect {
        let top = self.panel_tabs().bottom();
        Rect::new(
            self.panel.x,
            top,
            self.panel.w,
            (self.panel.bottom() - top).max(0.0),
        )
    }
}

// ============================================================================
// What a press can land on
// ============================================================================

/// Everything in the window a press can mean.
///
/// Every one of these was a painted rectangle before the window existed. The
/// list is the answer to "what can this program be asked to do?", and writing
/// it down is what turned up the actions that had no control at all: nothing
/// deleted a row, nothing removed a filter, nothing chose an export format,
/// and nothing but `DbTab::new` ever selected a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// Run what the SQL editor holds.
    Execute,
    /// Open another database connection.
    NewTab,
    /// Write the selected table out in one of the three formats.
    Export(ExportFormat),
    /// Read the SQL editor's text as CSV into a new table.
    Import,
    /// Make the `i`th database tab the active one.
    SelectTab(usize),
    /// Close the `i`th database tab.
    CloseTab(usize),
    /// The `+` at the end of the tab strip.
    AddTab,
    /// The `i`th row of the object tree.
    TreeNode(usize),
    /// Show or hide the filter builder in the sidebar.
    ToggleFilterBuilder,
    /// Step the filter builder's column to the next one.
    FilterColumn,
    /// Step the filter builder's comparison to the next one.
    FilterOp,
    /// Give the keyboard to the filter builder's value box.
    FilterValue,
    /// Add the filter the builder is showing.
    AddFilter,
    /// Drop the `i`th filter in force.
    RemoveFilter(usize),
    /// Sort by the `i`th column, or reverse a sort already on it.
    SortColumn(usize),
    /// Delete the row at index `i` *of the table*, not of the page.
    DeleteRow(usize),
    /// The pagination bar's left arrow.
    PrevPage,
    /// The pagination bar's right arrow.
    NextPage,
    /// One of the four tabs naming the bottom panels.
    ShowPanel(BottomPanel),
    /// Give the keyboard to the SQL editor.
    SqlEditor,
    /// Put the `i`th query in the history back in the editor.
    HistoryEntry(usize),
    /// Star or unstar the `i`th query in the history. Its own box rather than a
    /// second meaning for `HistoryEntry`: `toggle_favorite` existed, drew a
    /// `[*]`, and had no caller outside the tests, and a row cannot mean two
    /// things at once.
    FavoriteEntry(usize),
}

/// The toolbar's buttons, in the order they are drawn.
///
/// A list rather than six inline pushes, because the toolbar used to start at a
/// hard-coded `x = 130` and walk right without ever asking how wide the window
/// was: at 400 points across, Import was drawn past the right-hand edge, and a
/// button drawn off the edge that answers a press is worse than one that is not
/// drawn at all.
const TOOLBAR_BUTTONS: &[(&str, Target, Color)] = &[
    ("Execute", Target::Execute, GREEN),
    ("New Tab", Target::NewTab, BLUE),
    ("Export CSV", Target::Export(ExportFormat::Csv), PEACH),
    ("Export JSON", Target::Export(ExportFormat::Json), PEACH),
    (
        "Export SQL",
        Target::Export(ExportFormat::SqlInserts),
        PEACH,
    ),
    ("Import", Target::Import, TEAL),
    // `show_filter_builder` was a field with no switch: it was set in `new` and
    // read by the drawing pass, and nothing between the two could change it.
    ("Filters", Target::ToggleFilterBuilder, YELLOW),
];

/// What the keyboard is reaching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// Nothing; typing does nothing.
    None,
    /// The SQL editor.
    Editor,
    /// The filter builder's value box.
    FilterValue,
}

// ============================================================================
// Drawing helpers
// ============================================================================

/// A filled rectangle.
fn fill(r: Rect, color: Color, radius: f32) -> RenderCommand {
    RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::all(radius),
    }
}

/// An outlined rectangle.
fn stroke(r: Rect, color: Color, radius: f32) -> RenderCommand {
    RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width: 1.0,
        corner_radii: CornerRadii::all(radius),
    }
}

/// A horizontal rule across `r` at `y`.
fn hline(r: Rect, y: f32, color: Color) -> RenderCommand {
    RenderCommand::Line {
        x1: r.x,
        y1: y,
        x2: r.right(),
        y2: y,
        color,
        width: 1.0,
    }
}

/// `r` shrunk by `pad` on every side, never below nothing.
fn inset(r: Rect, pad: f32) -> Rect {
    Rect::new(
        r.x + pad,
        r.y + pad,
        (r.w - pad * 2.0).max(0.0),
        (r.h - pad * 2.0).max(0.0),
    )
}

/// `r` shrunk by `pad` on the left and right only.
fn inset_x(r: Rect, pad: f32) -> Rect {
    Rect::new(r.x + pad, r.y, (r.w - pad * 2.0).max(0.0), r.h)
}

/// The box a run of text actually inks, given the box it was asked to sit in
/// and the point size it was asked for.
///
/// A run taller than its box sticks out of *both* ends of it once it is
/// centred, and no caller can prevent that from where it stands. The size is
/// clamped rather than the box grown, because the box is the promise.
fn ink_box(r: Rect, size: f32) -> Rect {
    let size = size.min(r.h);
    Rect::new(r.x, r.y + (r.h - size) / 2.0, r.w, size)
}

/// A run of text bounded by the box it is drawn in, on both axes.
fn text_in(r: Rect, s: &str, size: f32, color: Color, weight: FontWeightHint) -> RenderCommand {
    let ink = ink_box(r, size);
    RenderCommand::Text {
        x: ink.x,
        y: ink.y,
        text: s.to_owned(),
        color,
        font_size: ink.h,
        font_weight: weight,
        max_width: Some(ink.w),
        overflow: TextOverflow::Ellipsis,
    }
}

/// Draw a run of text in a box, unless the box has no room to read it in.
///
/// Every text run in this program goes through here. A box with no width bounds
/// its run to nothing and a box with no height clamps its point size to
/// nothing: either way the run is invisible, so emitting it puts a command in
/// the frame that draws no pixel and reads, to anything inspecting the picture,
/// as a label that is present.
///
/// The clip in force is asked for the same reason. A cursor that runs down a
/// panel drawing row after row does not stop at the bottom of the panel by
/// itself; the clip hides what it draws past that point, but hiding is not the
/// same as not drawing, and a picture that claims to have drawn a column name
/// three hundred points below the panel is a picture that cannot be asked what
/// it is showing.
fn put_text(
    f: &mut Frame<Target>,
    r: Rect,
    s: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
) {
    let ink = ink_box(r, size);
    if ink.is_empty() || s.is_empty() || !f.is_visible(ink) {
        return;
    }
    f.push(text_in(r, s, size, color, weight));
}

/// The colour a cell of each type is drawn in.
fn cell_color(cell: &CellValue) -> Color {
    match cell {
        CellValue::Null => OVERLAY0,
        CellValue::Integer(_) => BLUE,
        CellValue::Real(_) => PEACH,
        CellValue::Text(_) => TEXT,
        CellValue::Blob(_) => MAUVE,
    }
}

/// The text a SQL token is drawn as, and the colour and weight it is drawn in.
///
/// The *text* is part of the answer, not just the colour: a string literal is
/// tokenized without its quotes and drawn with them, so the caller cannot
/// measure the token by looking at the token.
fn token_ink(token: &SqlToken) -> (String, Color, FontWeightHint) {
    match token {
        SqlToken::Keyword(k) => (k.clone(), MAUVE, FontWeightHint::Bold),
        SqlToken::Identifier(id) => (id.clone(), TEXT, FontWeightHint::Regular),
        SqlToken::StringLiteral(s) => (format!("'{s}'"), GREEN, FontWeightHint::Regular),
        SqlToken::NumberLiteral(n) => (n.clone(), PEACH, FontWeightHint::Regular),
        SqlToken::Operator(op) => (op.clone(), RED, FontWeightHint::Regular),
        SqlToken::Comma => (",".to_owned(), TEXT, FontWeightHint::Regular),
        SqlToken::Semicolon => (";".to_owned(), TEXT, FontWeightHint::Regular),
        SqlToken::LeftParen => ("(".to_owned(), YELLOW, FontWeightHint::Regular),
        SqlToken::RightParen => (")".to_owned(), YELLOW, FontWeightHint::Regular),
        SqlToken::Star => ("*".to_owned(), PEACH, FontWeightHint::Bold),
        SqlToken::Dot => (".".to_owned(), TEXT, FontWeightHint::Regular),
        SqlToken::Whitespace => (" ".to_owned(), TEXT, FontWeightHint::Regular),
    }
}

/// Point size of the query-result message above the results table.
const RESULT_MSG_FONT_SIZE: f32 = 11.0;
/// Line-to-line spacing of the query-result message, which is wrapped.
const RESULT_MSG_LINE_HEIGHT: f32 = 14.0;
/// Gap between the last line of the message and the results table below it.
/// Chosen so a one-line message leaves the table exactly where it has always
/// been (`y + 22`), which is the overwhelmingly common case.
const RESULT_MSG_GAP: f32 = 4.0;
/// Lines the message may take when a results table follows it. A message that
/// accompanies rows is generated ("10 rows returned in 4 ms") and short; the
/// cap is there so a pathological one cannot crowd out the results it is
/// describing. A message with no table under it — an error — is free to use
/// the whole pane, because there is nothing else to show.
const RESULT_MSG_MAX_LINES_WITH_TABLE: usize = 3;
/// Rows of a query result the pane will draw. The pane does not scroll, so a
/// result larger than this is reported by its message and read a page at a time
/// from the data grid instead.
const RESULT_ROWS_SHOWN: usize = 20;

/// The widths the schema pane's three columns are drawn at when the pane is
/// wide enough for all of them. Below that they are scaled down together, so
/// the proportions hold and no column is pushed outside the pane.
const SCHEMA_COL_WIDTHS: [f32; 3] = [180.0, 100.0, 200.0];
/// Line-to-line spacing of a schema row.
const SCHEMA_ROW_HEIGHT: f32 = 18.0;

/// Width of a table box in the schema diagram, or the width of the diagram
/// itself when that is narrower.
const DIAGRAM_BOX_WIDTH: f32 = 160.0;
/// Gap between two table boxes side by side.
const DIAGRAM_BOX_SPACING: f32 = 40.0;
/// Gap between two rows of table boxes.
const DIAGRAM_ROW_GAP: f32 = 16.0;
/// Height of the coloured band carrying a table box's name. A box shorter than
/// this cannot say which table it is, so it is not drawn at all.
const DIAGRAM_HEADER_HEIGHT: f32 = 20.0;
/// Line-to-line spacing of the column list inside a table box.
const DIAGRAM_COL_STEP: f32 = 14.0;

// ============================================================================
// SQL keywords for syntax highlighting
// ============================================================================

const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "DROP",
    "ALTER",
    "ADD",
    "COLUMN",
    "INDEX",
    "VIEW",
    "TRIGGER",
    "PRIMARY",
    "KEY",
    "NOT",
    "NULL",
    "UNIQUE",
    "DEFAULT",
    "CHECK",
    "FOREIGN",
    "REFERENCES",
    "ON",
    "CASCADE",
    "RESTRICT",
    "AND",
    "OR",
    "IN",
    "BETWEEN",
    "LIKE",
    "IS",
    "AS",
    "ORDER",
    "BY",
    "ASC",
    "DESC",
    "LIMIT",
    "OFFSET",
    "GROUP",
    "HAVING",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "OUTER",
    "CROSS",
    "DISTINCT",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "INTEGER",
    "REAL",
    "TEXT",
    "BLOB",
    "IF",
    "EXISTS",
];

// ============================================================================
// Data types
// ============================================================================

/// SQLite-compatible data types.
#[derive(Clone, Debug, PartialEq)]
pub enum DataType {
    Integer,
    Real,
    Text,
    Blob,
    Null,
}

impl DataType {
    fn label(&self) -> &'static str {
        match self {
            Self::Integer => "INTEGER",
            Self::Real => "REAL",
            Self::Text => "TEXT",
            Self::Blob => "BLOB",
            Self::Null => "NULL",
        }
    }

    fn from_str_loose(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" => Self::Integer,
            "REAL" | "FLOAT" | "DOUBLE" | "NUMERIC" | "DECIMAL" => Self::Real,
            "TEXT" | "VARCHAR" | "CHAR" | "STRING" | "CLOB" => Self::Text,
            "BLOB" | "BINARY" | "VARBINARY" => Self::Blob,
            "NULL" => Self::Null,
            _ => Self::Text,
        }
    }

    fn color(&self) -> Color {
        match self {
            Self::Integer => BLUE,
            Self::Real => PEACH,
            Self::Text => GREEN,
            Self::Blob => MAUVE,
            Self::Null => OVERLAY0,
        }
    }
}

// ============================================================================
// Cell value
// ============================================================================

/// A single cell value in the database.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Null,
}

impl CellValue {
    fn display(&self) -> String {
        match self {
            Self::Integer(v) => v.to_string(),
            Self::Real(v) => format!("{v:.6}"),
            Self::Text(s) => s.clone(),
            Self::Blob(b) => format!("<BLOB {} bytes>", b.len()),
            Self::Null => "NULL".to_owned(),
        }
    }

    fn as_sort_key(&self) -> SortKey<'_> {
        match self {
            Self::Null => SortKey::Null,
            Self::Integer(v) => SortKey::Int(*v),
            Self::Real(v) => SortKey::Float(*v),
            Self::Text(s) => SortKey::Str(s.as_str()),
            Self::Blob(b) => SortKey::Bytes(b.as_slice()),
        }
    }

    /// Parse a string into a `CellValue` given a target data type.
    fn parse_as(s: &str, dtype: &DataType) -> Self {
        if s.eq_ignore_ascii_case("null") || s.is_empty() {
            return Self::Null;
        }
        match dtype {
            DataType::Integer => s
                .parse::<i64>()
                .map_or(Self::Text(s.to_owned()), Self::Integer),
            DataType::Real => s
                .parse::<f64>()
                .map_or(Self::Text(s.to_owned()), Self::Real),
            DataType::Text => Self::Text(s.to_owned()),
            DataType::Blob => Self::Blob(s.as_bytes().to_vec()),
            DataType::Null => Self::Null,
        }
    }
}

/// Sort key for ordering cell values.
#[derive(Debug)]
enum SortKey<'a> {
    Null,
    Int(i64),
    Float(f64),
    Str(&'a str),
    Bytes(&'a [u8]),
}

impl PartialEq for SortKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp_value(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for SortKey<'_> {}

impl PartialOrd for SortKey<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortKey<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp_value(other)
    }
}

impl SortKey<'_> {
    fn cmp_value(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Null, _) => Ordering::Less,
            (_, Self::Null) => Ordering::Greater,
            (Self::Int(a), Self::Int(b)) => a.cmp(b),
            (Self::Float(a), Self::Float(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Self::Str(a), Self::Str(b)) => a.cmp(b),
            (Self::Bytes(a), Self::Bytes(b)) => a.cmp(b),
            (Self::Int(a), Self::Float(b)) => (*a as f64).partial_cmp(b).unwrap_or(Ordering::Equal),
            (Self::Float(a), Self::Int(b)) => {
                a.partial_cmp(&(*b as f64)).unwrap_or(Ordering::Equal)
            }
            _ => Ordering::Equal,
        }
    }
}

// ============================================================================
// Column constraints
// ============================================================================

/// Column constraint flags.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColumnConstraints {
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default_value: Option<String>,
    pub auto_increment: bool,
}

impl ColumnConstraints {
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.primary_key {
            parts.push("PK");
        }
        if self.auto_increment {
            parts.push("AI");
        }
        if self.not_null {
            parts.push("NN");
        }
        if self.unique {
            parts.push("UQ");
        }
        if let Some(ref def) = self.default_value {
            // Append "DEF={def}" as a final part using the already-built prefix.
            return format!("{} DEF={def}", parts.join(" ")).trim().to_owned();
        }
        parts.join(" ")
    }
}

// ============================================================================
// Column definition
// ============================================================================

/// A column in a table schema.
#[derive(Clone, Debug)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub constraints: ColumnConstraints,
}

impl ColumnDef {
    fn new(name: &str, data_type: DataType) -> Self {
        Self {
            name: name.to_owned(),
            data_type,
            constraints: ColumnConstraints::default(),
        }
    }

    fn with_primary_key(mut self) -> Self {
        self.constraints.primary_key = true;
        self.constraints.not_null = true;
        self
    }

    fn with_not_null(mut self) -> Self {
        self.constraints.not_null = true;
        self
    }

    fn with_unique(mut self) -> Self {
        self.constraints.unique = true;
        self
    }

    fn with_default(mut self, default: &str) -> Self {
        self.constraints.default_value = Some(default.to_owned());
        self
    }

    fn with_auto_increment(mut self) -> Self {
        self.constraints.auto_increment = true;
        self
    }
}

// ============================================================================
// Foreign key reference
// ============================================================================

/// A foreign key constraint between tables.
#[derive(Clone, Debug, PartialEq)]
pub struct ForeignKey {
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
}

// ============================================================================
// Index definition
// ============================================================================

/// An index on a table.
#[derive(Clone, Debug)]
pub struct IndexDef {
    pub name: String,
    pub table_name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

// ============================================================================
// View definition
// ============================================================================

/// A view (stored query).
#[derive(Clone, Debug)]
pub struct ViewDef {
    pub name: String,
    pub sql: String,
}

// ============================================================================
// Trigger definition
// ============================================================================

/// A trigger on a table.
#[derive(Clone, Debug)]
pub struct TriggerDef {
    pub name: String,
    pub table_name: String,
    pub event: String,
    pub sql: String,
}

// ============================================================================
// Table
// ============================================================================

/// An in-memory database table.
#[derive(Clone, Debug)]
pub struct Table {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub rows: Vec<Vec<CellValue>>,
    pub auto_increment_counter: i64,
}

impl Table {
    fn new(name: &str, columns: Vec<ColumnDef>) -> Self {
        Self {
            name: name.to_owned(),
            columns,
            rows: Vec::new(),
            auto_increment_counter: 0,
        }
    }

    /// Insert a row, filling in auto-increment values and defaults.
    fn insert_row(&mut self, values: Vec<CellValue>) -> Result<(), String> {
        if values.len() != self.columns.len() {
            return Err(format!(
                "Column count mismatch: expected {}, got {}",
                self.columns.len(),
                values.len()
            ));
        }

        // Validate NOT NULL constraints
        for (i, val) in values.iter().enumerate() {
            if let Some(col) = self.columns.get(i)
                && col.constraints.not_null
                && *val == CellValue::Null
                && !col.constraints.auto_increment
            {
                return Err(format!("Column '{}' cannot be NULL", col.name));
            }
        }

        // Handle auto-increment
        let mut final_values = values;
        for (i, col) in self.columns.iter().enumerate() {
            if col.constraints.auto_increment
                && let Some(v) = final_values.get(i)
                && *v == CellValue::Null
            {
                self.auto_increment_counter = self.auto_increment_counter.saturating_add(1);
                if let Some(cell) = final_values.get_mut(i) {
                    *cell = CellValue::Integer(self.auto_increment_counter);
                }
            }
        }

        // Check UNIQUE constraints
        for (i, col) in self.columns.iter().enumerate() {
            if (col.constraints.unique || col.constraints.primary_key)
                && let Some(new_val) = final_values.get(i)
            {
                for existing_row in &self.rows {
                    if let Some(existing_val) = existing_row.get(i)
                        && *existing_val != CellValue::Null
                        && *existing_val == *new_val
                    {
                        return Err(format!("UNIQUE constraint failed: column '{}'", col.name));
                    }
                }
            }
        }

        self.rows.push(final_values);
        Ok(())
    }

    /// Find column index by name (case-insensitive).
    fn column_index(&self, name: &str) -> Option<usize> {
        let name_upper = name.to_uppercase();
        self.columns
            .iter()
            .position(|c| c.name.to_uppercase() == name_upper)
    }

    /// Delete rows matching a predicate on a specific column.
    fn delete_where(&mut self, col_idx: usize, op: &FilterOp, value: &CellValue) -> usize {
        let before = self.rows.len();
        self.rows.retain(|row| {
            row.get(col_idx)
                .is_none_or(|cell| !matches_filter(cell, op, value))
        });
        before.saturating_sub(self.rows.len())
    }

    /// Update rows matching a predicate.
    fn update_where(
        &mut self,
        set_col: usize,
        set_value: &CellValue,
        where_col: usize,
        op: &FilterOp,
        where_value: &CellValue,
    ) -> usize {
        let mut count = 0usize;
        for row in &mut self.rows {
            let matches = row
                .get(where_col)
                .is_some_and(|cell| matches_filter(cell, op, where_value));
            if matches && let Some(cell) = row.get_mut(set_col) {
                *cell = set_value.clone();
                count = count.saturating_add(1);
            }
        }
        count
    }

    /// Get column count.
    fn col_count(&self) -> usize {
        self.columns.len()
    }

    /// Get row count.
    fn row_count(&self) -> usize {
        self.rows.len()
    }
}

// ============================================================================
// Database
// ============================================================================

/// An in-memory database containing tables, indexes, views, and triggers.
#[derive(Clone, Debug)]
pub struct Database {
    pub name: String,
    pub tables: Vec<Table>,
    pub indexes: Vec<IndexDef>,
    pub views: Vec<ViewDef>,
    pub triggers: Vec<TriggerDef>,
    pub foreign_keys: Vec<ForeignKey>,
}

impl Database {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            tables: Vec::new(),
            indexes: Vec::new(),
            views: Vec::new(),
            triggers: Vec::new(),
            foreign_keys: Vec::new(),
        }
    }

    fn find_table(&self, name: &str) -> Option<&Table> {
        let name_upper = name.to_uppercase();
        self.tables
            .iter()
            .find(|t| t.name.to_uppercase() == name_upper)
    }

    fn find_table_mut(&mut self, name: &str) -> Option<&mut Table> {
        let name_upper = name.to_uppercase();
        self.tables
            .iter_mut()
            .find(|t| t.name.to_uppercase() == name_upper)
    }

    fn create_table(&mut self, table: Table) -> Result<(), String> {
        if self.find_table(&table.name).is_some() {
            return Err(format!("Table '{}' already exists", table.name));
        }
        self.tables.push(table);
        Ok(())
    }

    fn drop_table(&mut self, name: &str) -> Result<(), String> {
        let name_upper = name.to_uppercase();
        let idx = self
            .tables
            .iter()
            .position(|t| t.name.to_uppercase() == name_upper)
            .ok_or_else(|| format!("Table '{name}' not found"))?;
        self.tables.remove(idx);
        // Also remove related indexes, triggers, foreign keys
        self.indexes
            .retain(|i| i.table_name.to_uppercase() != name_upper);
        self.triggers
            .retain(|t| t.table_name.to_uppercase() != name_upper);
        self.foreign_keys.retain(|fk| {
            fk.from_table.to_uppercase() != name_upper && fk.to_table.to_uppercase() != name_upper
        });
        Ok(())
    }

    fn table_names(&self) -> Vec<String> {
        self.tables.iter().map(|t| t.name.clone()).collect()
    }

    /// Create a sample database for demonstration.
    fn sample() -> Self {
        let mut db = Self::new("sample.db");

        // Users table
        let users = Table::new(
            "users",
            vec![
                ColumnDef::new("id", DataType::Integer)
                    .with_primary_key()
                    .with_auto_increment(),
                ColumnDef::new("name", DataType::Text).with_not_null(),
                ColumnDef::new("email", DataType::Text)
                    .with_not_null()
                    .with_unique(),
                ColumnDef::new("age", DataType::Integer),
                ColumnDef::new("score", DataType::Real).with_default("0.0"),
            ],
        );
        let _ = db.create_table(users);

        // Insert sample users
        if let Some(table) = db.find_table_mut("users") {
            let sample_users: &[(&str, &str, i64, f64)] = &[
                ("Alice", "alice@example.com", 30, 95.5),
                ("Bob", "bob@example.com", 25, 82.3),
                ("Charlie", "charlie@example.com", 35, 91.0),
                ("Diana", "diana@example.com", 28, 88.7),
                ("Eve", "eve@example.com", 32, 76.2),
                ("Frank", "frank@example.com", 45, 99.1),
                ("Grace", "grace@example.com", 22, 67.8),
                ("Hank", "hank@example.com", 38, 84.5),
                ("Ivy", "ivy@example.com", 29, 92.3),
                ("Jack", "jack@example.com", 41, 71.6),
            ];
            for (name, email, age, score) in sample_users {
                let _ = table.insert_row(vec![
                    CellValue::Null, // auto-increment id
                    CellValue::Text((*name).to_owned()),
                    CellValue::Text((*email).to_owned()),
                    CellValue::Integer(*age),
                    CellValue::Real(*score),
                ]);
            }
        }

        // Products table
        let products = Table::new(
            "products",
            vec![
                ColumnDef::new("id", DataType::Integer)
                    .with_primary_key()
                    .with_auto_increment(),
                ColumnDef::new("name", DataType::Text).with_not_null(),
                ColumnDef::new("price", DataType::Real).with_not_null(),
                ColumnDef::new("category", DataType::Text),
                ColumnDef::new("stock", DataType::Integer).with_default("0"),
            ],
        );
        let _ = db.create_table(products);

        if let Some(table) = db.find_table_mut("products") {
            let sample_products: &[(&str, f64, &str, i64)] = &[
                ("Laptop", 999.99, "Electronics", 50),
                ("Keyboard", 79.99, "Electronics", 200),
                ("Mouse", 29.99, "Electronics", 300),
                ("Desk", 249.99, "Furniture", 30),
                ("Chair", 199.99, "Furniture", 45),
                ("Monitor", 399.99, "Electronics", 80),
                ("Headset", 59.99, "Electronics", 150),
                ("Webcam", 49.99, "Electronics", 100),
            ];
            for (name, price, category, stock) in sample_products {
                let _ = table.insert_row(vec![
                    CellValue::Null,
                    CellValue::Text((*name).to_owned()),
                    CellValue::Real(*price),
                    CellValue::Text((*category).to_owned()),
                    CellValue::Integer(*stock),
                ]);
            }
        }

        // Orders table with FK
        let orders = Table::new(
            "orders",
            vec![
                ColumnDef::new("id", DataType::Integer)
                    .with_primary_key()
                    .with_auto_increment(),
                ColumnDef::new("user_id", DataType::Integer).with_not_null(),
                ColumnDef::new("product_id", DataType::Integer).with_not_null(),
                ColumnDef::new("quantity", DataType::Integer).with_default("1"),
                ColumnDef::new("total", DataType::Real),
            ],
        );
        let _ = db.create_table(orders);

        if let Some(table) = db.find_table_mut("orders") {
            let sample_orders: &[(i64, i64, i64, f64)] = &[
                (1, 1, 1, 999.99),
                (1, 3, 2, 59.98),
                (2, 2, 1, 79.99),
                (3, 6, 1, 399.99),
                (4, 5, 2, 399.98),
                (5, 4, 1, 249.99),
            ];
            for (uid, pid, qty, total) in sample_orders {
                let _ = table.insert_row(vec![
                    CellValue::Null,
                    CellValue::Integer(*uid),
                    CellValue::Integer(*pid),
                    CellValue::Integer(*qty),
                    CellValue::Real(*total),
                ]);
            }
        }

        // Foreign keys
        db.foreign_keys.push(ForeignKey {
            from_table: "orders".to_owned(),
            from_column: "user_id".to_owned(),
            to_table: "users".to_owned(),
            to_column: "id".to_owned(),
        });
        db.foreign_keys.push(ForeignKey {
            from_table: "orders".to_owned(),
            from_column: "product_id".to_owned(),
            to_table: "products".to_owned(),
            to_column: "id".to_owned(),
        });

        // Indexes
        db.indexes.push(IndexDef {
            name: "idx_users_email".to_owned(),
            table_name: "users".to_owned(),
            columns: vec!["email".to_owned()],
            unique: true,
        });
        db.indexes.push(IndexDef {
            name: "idx_orders_user".to_owned(),
            table_name: "orders".to_owned(),
            columns: vec!["user_id".to_owned()],
            unique: false,
        });

        // Views
        db.views.push(ViewDef {
            name: "user_orders_view".to_owned(),
            sql: "SELECT u.name, p.name AS product, o.quantity, o.total FROM orders o JOIN users u ON o.user_id = u.id JOIN products p ON o.product_id = p.id".to_owned(),
        });

        // Triggers
        db.triggers.push(TriggerDef {
            name: "update_stock".to_owned(),
            table_name: "orders".to_owned(),
            event: "AFTER INSERT".to_owned(),
            sql: "UPDATE products SET stock = stock - NEW.quantity WHERE id = NEW.product_id"
                .to_owned(),
        });

        db
    }
}

// ============================================================================
// Filter operations
// ============================================================================

/// Comparison operators for WHERE clauses.
#[derive(Clone, Debug, PartialEq)]
pub enum FilterOp {
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
    LessOrEqual,
    GreaterOrEqual,
    Like,
    IsNull,
    IsNotNull,
}

impl FilterOp {
    fn label(&self) -> &'static str {
        match self {
            Self::Equal => "=",
            Self::NotEqual => "!=",
            Self::LessThan => "<",
            Self::GreaterThan => ">",
            Self::LessOrEqual => "<=",
            Self::GreaterOrEqual => ">=",
            Self::Like => "LIKE",
            Self::IsNull => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
        }
    }

    fn all() -> &'static [Self] {
        &[
            Self::Equal,
            Self::NotEqual,
            Self::LessThan,
            Self::GreaterThan,
            Self::LessOrEqual,
            Self::GreaterOrEqual,
            Self::Like,
            Self::IsNull,
            Self::IsNotNull,
        ]
    }
}

/// Check if a cell value matches a filter condition.
fn matches_filter(cell: &CellValue, op: &FilterOp, value: &CellValue) -> bool {
    match op {
        FilterOp::IsNull => *cell == CellValue::Null,
        FilterOp::IsNotNull => *cell != CellValue::Null,
        FilterOp::Equal => cell.as_sort_key() == value.as_sort_key(),
        FilterOp::NotEqual => cell.as_sort_key() != value.as_sort_key(),
        FilterOp::LessThan => cell.as_sort_key() < value.as_sort_key(),
        FilterOp::GreaterThan => cell.as_sort_key() > value.as_sort_key(),
        FilterOp::LessOrEqual => cell.as_sort_key() <= value.as_sort_key(),
        FilterOp::GreaterOrEqual => cell.as_sort_key() >= value.as_sort_key(),
        FilterOp::Like => {
            if let (CellValue::Text(cell_str), CellValue::Text(pattern)) = (cell, value) {
                simple_like_match(cell_str, pattern)
            } else {
                false
            }
        }
    }
}

/// Simple LIKE pattern matcher supporting % and _ wildcards.
///
/// Matches over `&[char]`, not `&[u8]`. SQL's `_` is defined as exactly one
/// *character*, and this used to walk bytes, so `_` matched one third of a
/// kanji: `LIKE '_'` was false for a one-character CJK cell while `LIKE '___'`
/// was true for it. Both inputs are `&str`, so decoding always succeeds.
fn simple_like_match(text: &str, pattern: &str) -> bool {
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    like_match_inner(&text, &pattern)
}

fn like_match_inner(text: &[char], pattern: &[char]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    if let Some(&first_p) = pattern.first() {
        if first_p == '%' {
            // % matches any sequence
            let rest_pattern = pattern.get(1..).unwrap_or_default();
            for i in 0..=text.len() {
                if like_match_inner(text.get(i..).unwrap_or_default(), rest_pattern) {
                    return true;
                }
            }
            false
        } else if first_p == '_' {
            // _ matches exactly one character
            if text.is_empty() {
                return false;
            }
            like_match_inner(
                text.get(1..).unwrap_or_default(),
                pattern.get(1..).unwrap_or_default(),
            )
        } else {
            // Literal match
            if text.is_empty() || text.first() != pattern.first() {
                return false;
            }
            like_match_inner(
                text.get(1..).unwrap_or_default(),
                pattern.get(1..).unwrap_or_default(),
            )
        }
    } else {
        text.is_empty()
    }
}

// ============================================================================
// Active filter
// ============================================================================

/// An active filter rule for the WHERE clause builder.
#[derive(Clone, Debug)]
pub struct ActiveFilter {
    pub column_idx: usize,
    pub op: FilterOp,
    pub value_str: String,
}

// ============================================================================
// Sort state
// ============================================================================

/// Sorting direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SortDir {
    Ascending,
    Descending,
}

/// Current sort state for a table view.
#[derive(Clone, Debug)]
pub struct SortState {
    pub column_idx: usize,
    pub direction: SortDir,
}

// ============================================================================
// Query result
// ============================================================================

/// Result of executing a SQL query.
#[derive(Clone, Debug)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<CellValue>>,
    pub message: String,
    pub affected_rows: usize,
    pub is_error: bool,
}

impl QueryResult {
    fn success(msg: &str) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            message: msg.to_owned(),
            affected_rows: 0,
            is_error: false,
        }
    }

    fn error(msg: &str) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            message: msg.to_owned(),
            affected_rows: 0,
            is_error: true,
        }
    }

    fn with_data(columns: Vec<String>, rows: Vec<Vec<CellValue>>) -> Self {
        let row_count = rows.len();
        Self {
            columns,
            rows,
            message: format!("{row_count} row(s) returned"),
            affected_rows: row_count,
            is_error: false,
        }
    }
}

// ============================================================================
// Query history entry
// ============================================================================

/// An entry in the query history.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub sql: String,
    pub success: bool,
    pub message: String,
    pub favorite: bool,
    pub timestamp_counter: u64,
}

// ============================================================================
// SQL parser — tokenizer
// ============================================================================

/// A SQL token.
#[derive(Clone, Debug, PartialEq)]
pub enum SqlToken {
    Keyword(String),
    Identifier(String),
    StringLiteral(String),
    NumberLiteral(String),
    Operator(String),
    Comma,
    Semicolon,
    LeftParen,
    RightParen,
    Star,
    Dot,
    Whitespace,
}

/// Tokenize a SQL string into tokens.
fn tokenize_sql(input: &str) -> Vec<SqlToken> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars.get(i).copied().unwrap_or(' ');

        // Skip whitespace
        if ch.is_whitespace() {
            tokens.push(SqlToken::Whitespace);
            while i < len && chars.get(i).is_some_and(|c| c.is_whitespace()) {
                i = i.saturating_add(1);
            }
            continue;
        }

        // String literals
        if ch == '\'' {
            let mut s = String::new();
            i = i.saturating_add(1);
            while i < len {
                let c = chars.get(i).copied().unwrap_or(' ');
                if c == '\'' {
                    // Check for escaped quote
                    if i.saturating_add(1) < len && chars.get(i.saturating_add(1)) == Some(&'\'') {
                        s.push('\'');
                        i = i.saturating_add(2);
                    } else {
                        i = i.saturating_add(1);
                        break;
                    }
                } else {
                    s.push(c);
                    i = i.saturating_add(1);
                }
            }
            tokens.push(SqlToken::StringLiteral(s));
            continue;
        }

        // Numbers
        if ch.is_ascii_digit()
            || (ch == '.'
                && i.saturating_add(1) < len
                && chars
                    .get(i.saturating_add(1))
                    .is_some_and(char::is_ascii_digit))
        {
            let mut num = String::new();
            while i < len
                && chars
                    .get(i)
                    .is_some_and(|c| c.is_ascii_digit() || *c == '.')
            {
                num.push(chars.get(i).copied().unwrap_or('0'));
                i = i.saturating_add(1);
            }
            tokens.push(SqlToken::NumberLiteral(num));
            continue;
        }

        // Identifiers / keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            let mut ident = String::new();
            while i < len
                && chars
                    .get(i)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
            {
                ident.push(chars.get(i).copied().unwrap_or('_'));
                i = i.saturating_add(1);
            }
            let upper = ident.to_uppercase();
            if SQL_KEYWORDS.contains(&upper.as_str()) {
                tokens.push(SqlToken::Keyword(upper));
            } else {
                tokens.push(SqlToken::Identifier(ident));
            }
            continue;
        }

        // Operators
        match ch {
            '=' => {
                tokens.push(SqlToken::Operator("=".to_owned()));
                i = i.saturating_add(1);
            }
            '!' if i.saturating_add(1) < len && chars.get(i.saturating_add(1)) == Some(&'=') => {
                tokens.push(SqlToken::Operator("!=".to_owned()));
                i = i.saturating_add(2);
            }
            '<' if i.saturating_add(1) < len && chars.get(i.saturating_add(1)) == Some(&'=') => {
                tokens.push(SqlToken::Operator("<=".to_owned()));
                i = i.saturating_add(2);
            }
            '<' if i.saturating_add(1) < len && chars.get(i.saturating_add(1)) == Some(&'>') => {
                tokens.push(SqlToken::Operator("<>".to_owned()));
                i = i.saturating_add(2);
            }
            '<' => {
                tokens.push(SqlToken::Operator("<".to_owned()));
                i = i.saturating_add(1);
            }
            '>' if i.saturating_add(1) < len && chars.get(i.saturating_add(1)) == Some(&'=') => {
                tokens.push(SqlToken::Operator(">=".to_owned()));
                i = i.saturating_add(2);
            }
            '>' => {
                tokens.push(SqlToken::Operator(">".to_owned()));
                i = i.saturating_add(1);
            }
            '(' => {
                tokens.push(SqlToken::LeftParen);
                i = i.saturating_add(1);
            }
            ')' => {
                tokens.push(SqlToken::RightParen);
                i = i.saturating_add(1);
            }
            ',' => {
                tokens.push(SqlToken::Comma);
                i = i.saturating_add(1);
            }
            ';' => {
                tokens.push(SqlToken::Semicolon);
                i = i.saturating_add(1);
            }
            '*' => {
                tokens.push(SqlToken::Star);
                i = i.saturating_add(1);
            }
            '.' => {
                tokens.push(SqlToken::Dot);
                i = i.saturating_add(1);
            }
            _ => {
                i = i.saturating_add(1);
            } // Skip unknown chars
        }
    }

    tokens
}

// ============================================================================
// SQL parser — statement types
// ============================================================================

/// Parsed SQL statement.
#[derive(Clone, Debug)]
pub enum SqlStatement {
    Select {
        columns: Vec<SelectColumn>,
        table: String,
        where_clause: Option<WhereClause>,
        order_by: Option<(String, SortDir)>,
        limit: Option<usize>,
        offset: Option<usize>,
        group_by: Option<String>,
    },
    Insert {
        table: String,
        columns: Vec<String>,
        values: Vec<Vec<String>>,
    },
    Update {
        table: String,
        set_clauses: Vec<(String, String)>,
        where_clause: Option<WhereClause>,
    },
    Delete {
        table: String,
        where_clause: Option<WhereClause>,
    },
    CreateTable {
        name: String,
        columns: Vec<ParsedColumnDef>,
        if_not_exists: bool,
    },
    DropTable {
        name: String,
        if_exists: bool,
    },
}

/// A column in a SELECT statement.
#[derive(Clone, Debug)]
pub enum SelectColumn {
    AllColumns,
    Named(String),
    Aggregate {
        func: AggFunc,
        column: String,
        alias: Option<String>,
    },
}

/// Aggregate functions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AggFunc {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl AggFunc {
    fn label(&self) -> &'static str {
        match self {
            Self::Count => "COUNT",
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }

    fn from_keyword(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "COUNT" => Some(Self::Count),
            "SUM" => Some(Self::Sum),
            "AVG" => Some(Self::Avg),
            "MIN" => Some(Self::Min),
            "MAX" => Some(Self::Max),
            _ => None,
        }
    }
}

/// A WHERE clause condition.
#[derive(Clone, Debug)]
pub struct WhereClause {
    pub column: String,
    pub op: FilterOp,
    pub value: String,
}

/// A column definition from a CREATE TABLE statement.
#[derive(Clone, Debug)]
pub struct ParsedColumnDef {
    pub name: String,
    pub data_type: String,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub auto_increment: bool,
    pub default_value: Option<String>,
}

// ============================================================================
// SQL parser — parse functions
// ============================================================================

/// Parse a SQL string into a statement.
fn parse_sql(input: &str) -> Result<SqlStatement, String> {
    let tokens: Vec<SqlToken> = tokenize_sql(input)
        .into_iter()
        .filter(|t| *t != SqlToken::Whitespace)
        .collect();

    if tokens.is_empty() {
        return Err("Empty query".to_owned());
    }

    let first = tokens.first().ok_or_else(|| "Empty query".to_owned())?;
    match first {
        SqlToken::Keyword(k) => match k.as_str() {
            "SELECT" => parse_select(&tokens),
            "INSERT" => parse_insert(&tokens),
            "UPDATE" => parse_update(&tokens),
            "DELETE" => parse_delete(&tokens),
            "CREATE" => parse_create_table(&tokens),
            "DROP" => parse_drop_table(&tokens),
            _ => Err(format!("Unsupported statement: {k}")),
        },
        _ => Err("Expected SQL keyword at start".to_owned()),
    }
}

fn parse_select(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip SELECT

    // Parse column list
    let mut columns = Vec::new();
    loop {
        if pos >= tokens.len() {
            return Err("Expected column list".to_owned());
        }
        let tok = tokens.get(pos).ok_or("Unexpected end of input")?;
        match tok {
            SqlToken::Star => {
                columns.push(SelectColumn::AllColumns);
                pos = pos.saturating_add(1);
            }
            SqlToken::Keyword(k) if AggFunc::from_keyword(k).is_some() => {
                let func = AggFunc::from_keyword(k).ok_or("Invalid aggregate")?;
                pos = pos.saturating_add(1);
                // Expect (column)
                expect_token(tokens, pos, &SqlToken::LeftParen)?;
                pos = pos.saturating_add(1);
                let col_name = match tokens.get(pos) {
                    Some(SqlToken::Identifier(name)) => name.clone(),
                    Some(SqlToken::Star) => "*".to_owned(),
                    _ => return Err("Expected column name in aggregate".to_owned()),
                };
                pos = pos.saturating_add(1);
                expect_token(tokens, pos, &SqlToken::RightParen)?;
                pos = pos.saturating_add(1);
                // Optional alias
                let alias = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "AS") {
                    pos = pos.saturating_add(1);
                    let a = extract_identifier(tokens, pos)?;
                    pos = pos.saturating_add(1);
                    Some(a)
                } else {
                    None
                };
                columns.push(SelectColumn::Aggregate {
                    func,
                    column: col_name,
                    alias,
                });
            }
            SqlToken::Identifier(name) => {
                columns.push(SelectColumn::Named(name.clone()));
                pos = pos.saturating_add(1);
            }
            _ => return Err("Expected column name or *".to_owned()),
        }
        if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
            pos = pos.saturating_add(1);
        } else {
            break;
        }
    }

    // FROM clause
    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "FROM") {
        return Err("Expected FROM".to_owned());
    }
    pos = pos.saturating_add(1);
    let table = extract_identifier(tokens, pos)?;
    pos = pos.saturating_add(1);

    // Optional WHERE
    let where_clause = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "WHERE") {
        pos = pos.saturating_add(1);
        let (wc, new_pos) = parse_where_clause(tokens, pos)?;
        pos = new_pos;
        Some(wc)
    } else {
        None
    };

    // Optional GROUP BY
    let group_by = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "GROUP") {
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "BY") {
            return Err("Expected BY after GROUP".to_owned());
        }
        pos = pos.saturating_add(1);
        let col = extract_identifier(tokens, pos)?;
        pos = pos.saturating_add(1);
        Some(col)
    } else {
        None
    };

    // Optional ORDER BY
    let order_by = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "ORDER") {
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "BY") {
            return Err("Expected BY after ORDER".to_owned());
        }
        pos = pos.saturating_add(1);
        let col = extract_identifier(tokens, pos)?;
        pos = pos.saturating_add(1);
        let dir = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "DESC") {
            pos = pos.saturating_add(1);
            SortDir::Descending
        } else {
            if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "ASC") {
                pos = pos.saturating_add(1);
            }
            SortDir::Ascending
        };
        Some((col, dir))
    } else {
        None
    };

    // Optional LIMIT
    let limit = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "LIMIT") {
        pos = pos.saturating_add(1);
        let n = extract_number(tokens, pos)?;
        pos = pos.saturating_add(1);
        Some(n)
    } else {
        None
    };

    // Optional OFFSET
    let offset = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "OFFSET") {
        pos = pos.saturating_add(1);
        let n = extract_number(tokens, pos)?;
        pos = pos.saturating_add(1);
        Some(n)
    } else {
        None
    };

    let _ = pos; // suppress "pos unused" - we've parsed everything we need

    Ok(SqlStatement::Select {
        columns,
        table,
        where_clause,
        order_by,
        limit,
        offset,
        group_by,
    })
}

fn parse_insert(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip INSERT

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "INTO") {
        return Err("Expected INTO after INSERT".to_owned());
    }
    pos = pos.saturating_add(1);

    let table = extract_identifier(tokens, pos)?;
    pos = pos.saturating_add(1);

    // Optional column list
    let mut col_names = Vec::new();
    if matches!(tokens.get(pos), Some(SqlToken::LeftParen)) {
        pos = pos.saturating_add(1);
        loop {
            let name = extract_identifier(tokens, pos)?;
            col_names.push(name);
            pos = pos.saturating_add(1);
            if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
                pos = pos.saturating_add(1);
            } else {
                break;
            }
        }
        expect_token(tokens, pos, &SqlToken::RightParen)?;
        pos = pos.saturating_add(1);
    }

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "VALUES") {
        return Err("Expected VALUES".to_owned());
    }
    pos = pos.saturating_add(1);

    // Parse value lists
    let mut all_values = Vec::new();
    loop {
        expect_token(tokens, pos, &SqlToken::LeftParen)?;
        pos = pos.saturating_add(1);
        let mut row_values = Vec::new();
        loop {
            let val = extract_value_str(tokens, pos)?;
            row_values.push(val);
            pos = pos.saturating_add(1);
            if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
                pos = pos.saturating_add(1);
            } else {
                break;
            }
        }
        expect_token(tokens, pos, &SqlToken::RightParen)?;
        pos = pos.saturating_add(1);
        all_values.push(row_values);

        if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
            pos = pos.saturating_add(1);
        } else {
            break;
        }
    }

    Ok(SqlStatement::Insert {
        table,
        columns: col_names,
        values: all_values,
    })
}

fn parse_update(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip UPDATE

    let table = extract_identifier(tokens, pos)?;
    pos = pos.saturating_add(1);

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "SET") {
        return Err("Expected SET".to_owned());
    }
    pos = pos.saturating_add(1);

    let mut set_clauses = Vec::new();
    loop {
        let col = extract_identifier(tokens, pos)?;
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Operator(op)) if op == "=") {
            return Err("Expected = in SET clause".to_owned());
        }
        pos = pos.saturating_add(1);
        let val = extract_value_str(tokens, pos)?;
        pos = pos.saturating_add(1);
        set_clauses.push((col, val));
        if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
            pos = pos.saturating_add(1);
        } else {
            break;
        }
    }

    let where_clause = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "WHERE") {
        pos = pos.saturating_add(1);
        let (wc, _new_pos) = parse_where_clause(tokens, pos)?;
        Some(wc)
    } else {
        None
    };

    Ok(SqlStatement::Update {
        table,
        set_clauses,
        where_clause,
    })
}

fn parse_delete(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip DELETE

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "FROM") {
        return Err("Expected FROM after DELETE".to_owned());
    }
    pos = pos.saturating_add(1);

    let table = extract_identifier(tokens, pos)?;
    pos = pos.saturating_add(1);

    let where_clause = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "WHERE") {
        pos = pos.saturating_add(1);
        let (wc, _new_pos) = parse_where_clause(tokens, pos)?;
        Some(wc)
    } else {
        None
    };

    Ok(SqlStatement::Delete {
        table,
        where_clause,
    })
}

fn parse_create_table(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip CREATE

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "TABLE") {
        return Err("Expected TABLE after CREATE".to_owned());
    }
    pos = pos.saturating_add(1);

    let if_not_exists = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "IF") {
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "NOT") {
            return Err("Expected NOT after IF".to_owned());
        }
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "EXISTS") {
            return Err("Expected EXISTS after NOT".to_owned());
        }
        pos = pos.saturating_add(1);
        true
    } else {
        false
    };

    let name = extract_identifier(tokens, pos)?;
    pos = pos.saturating_add(1);

    expect_token(tokens, pos, &SqlToken::LeftParen)?;
    pos = pos.saturating_add(1);

    let mut columns = Vec::new();
    loop {
        if matches!(tokens.get(pos), Some(SqlToken::RightParen)) {
            break;
        }
        let col_name = extract_identifier(tokens, pos)?;
        pos = pos.saturating_add(1);
        let col_type = extract_identifier_or_keyword(tokens, pos)?;
        pos = pos.saturating_add(1);

        let mut col = ParsedColumnDef {
            name: col_name,
            data_type: col_type,
            primary_key: false,
            not_null: false,
            unique: false,
            auto_increment: false,
            default_value: None,
        };

        // Parse column constraints
        loop {
            match tokens.get(pos) {
                Some(SqlToken::Keyword(k)) if k == "PRIMARY" => {
                    pos = pos.saturating_add(1);
                    if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "KEY") {
                        pos = pos.saturating_add(1);
                    }
                    col.primary_key = true;
                    col.not_null = true;
                }
                Some(SqlToken::Keyword(k)) if k == "NOT" => {
                    pos = pos.saturating_add(1);
                    if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "NULL") {
                        pos = pos.saturating_add(1);
                    }
                    col.not_null = true;
                }
                Some(SqlToken::Keyword(k)) if k == "UNIQUE" => {
                    pos = pos.saturating_add(1);
                    col.unique = true;
                }
                Some(SqlToken::Keyword(k)) if k == "DEFAULT" => {
                    pos = pos.saturating_add(1);
                    let val = extract_value_str(tokens, pos)?;
                    pos = pos.saturating_add(1);
                    col.default_value = Some(val);
                }
                Some(SqlToken::Identifier(s)) if s.to_uppercase() == "AUTOINCREMENT" => {
                    pos = pos.saturating_add(1);
                    col.auto_increment = true;
                }
                _ => break,
            }
        }

        columns.push(col);

        if matches!(tokens.get(pos), Some(SqlToken::Comma)) {
            pos = pos.saturating_add(1);
        } else {
            break;
        }
    }

    Ok(SqlStatement::CreateTable {
        name,
        columns,
        if_not_exists,
    })
}

fn parse_drop_table(tokens: &[SqlToken]) -> Result<SqlStatement, String> {
    let mut pos = 1; // Skip DROP

    if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "TABLE") {
        return Err("Expected TABLE after DROP".to_owned());
    }
    pos = pos.saturating_add(1);

    let if_exists = if matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "IF") {
        pos = pos.saturating_add(1);
        if !matches!(tokens.get(pos), Some(SqlToken::Keyword(k)) if k == "EXISTS") {
            return Err("Expected EXISTS after IF".to_owned());
        }
        pos = pos.saturating_add(1);
        true
    } else {
        false
    };

    let name = extract_identifier(tokens, pos)?;

    Ok(SqlStatement::DropTable { name, if_exists })
}

// ============================================================================
// Parser helpers
// ============================================================================

fn expect_token(tokens: &[SqlToken], pos: usize, expected: &SqlToken) -> Result<(), String> {
    match tokens.get(pos) {
        Some(tok) if std::mem::discriminant(tok) == std::mem::discriminant(expected) => Ok(()),
        Some(tok) => Err(format!("Expected {expected:?}, got {tok:?}")),
        None => Err(format!("Unexpected end of input, expected {expected:?}")),
    }
}

fn extract_identifier(tokens: &[SqlToken], pos: usize) -> Result<String, String> {
    match tokens.get(pos) {
        Some(SqlToken::Identifier(name)) => Ok(name.clone()),
        Some(tok) => Err(format!("Expected identifier, got {tok:?}")),
        None => Err("Unexpected end of input".to_owned()),
    }
}

fn extract_identifier_or_keyword(tokens: &[SqlToken], pos: usize) -> Result<String, String> {
    match tokens.get(pos) {
        Some(SqlToken::Identifier(name)) => Ok(name.clone()),
        Some(SqlToken::Keyword(name)) => Ok(name.clone()),
        Some(tok) => Err(format!("Expected identifier or keyword, got {tok:?}")),
        None => Err("Unexpected end of input".to_owned()),
    }
}

fn extract_value_str(tokens: &[SqlToken], pos: usize) -> Result<String, String> {
    match tokens.get(pos) {
        Some(SqlToken::StringLiteral(s)) => Ok(s.clone()),
        Some(SqlToken::NumberLiteral(s)) => Ok(s.clone()),
        Some(SqlToken::Identifier(s)) => Ok(s.clone()),
        Some(SqlToken::Keyword(k)) if k == "NULL" => Ok("NULL".to_owned()),
        Some(tok) => Err(format!("Expected value, got {tok:?}")),
        None => Err("Unexpected end of input".to_owned()),
    }
}

fn extract_number(tokens: &[SqlToken], pos: usize) -> Result<usize, String> {
    match tokens.get(pos) {
        Some(SqlToken::NumberLiteral(s)) => s
            .parse::<usize>()
            .map_err(|e| format!("Invalid number: {e}")),
        Some(tok) => Err(format!("Expected number, got {tok:?}")),
        None => Err("Unexpected end of input".to_owned()),
    }
}

fn parse_where_clause(tokens: &[SqlToken], pos: usize) -> Result<(WhereClause, usize), String> {
    let col = extract_identifier(tokens, pos)?;
    let mut p = pos.saturating_add(1);

    // Check for IS NULL / IS NOT NULL
    if matches!(tokens.get(p), Some(SqlToken::Keyword(k)) if k == "IS") {
        p = p.saturating_add(1);
        if matches!(tokens.get(p), Some(SqlToken::Keyword(k)) if k == "NOT") {
            p = p.saturating_add(1);
            if matches!(tokens.get(p), Some(SqlToken::Keyword(k)) if k == "NULL") {
                p = p.saturating_add(1);
            }
            return Ok((
                WhereClause {
                    column: col,
                    op: FilterOp::IsNotNull,
                    value: String::new(),
                },
                p,
            ));
        }
        if matches!(tokens.get(p), Some(SqlToken::Keyword(k)) if k == "NULL") {
            p = p.saturating_add(1);
        }
        return Ok((
            WhereClause {
                column: col,
                op: FilterOp::IsNull,
                value: String::new(),
            },
            p,
        ));
    }

    // Check for LIKE
    if matches!(tokens.get(p), Some(SqlToken::Keyword(k)) if k == "LIKE") {
        p = p.saturating_add(1);
        let val = extract_value_str(tokens, p)?;
        p = p.saturating_add(1);
        return Ok((
            WhereClause {
                column: col,
                op: FilterOp::Like,
                value: val,
            },
            p,
        ));
    }

    // Regular operator
    let op = match tokens.get(p) {
        Some(SqlToken::Operator(s)) => match s.as_str() {
            "=" => FilterOp::Equal,
            "!=" | "<>" => FilterOp::NotEqual,
            "<" => FilterOp::LessThan,
            ">" => FilterOp::GreaterThan,
            "<=" => FilterOp::LessOrEqual,
            ">=" => FilterOp::GreaterOrEqual,
            _ => return Err(format!("Unknown operator: {s}")),
        },
        _ => return Err("Expected operator in WHERE clause".to_owned()),
    };
    p = p.saturating_add(1);

    let val = extract_value_str(tokens, p)?;
    p = p.saturating_add(1);

    Ok((
        WhereClause {
            column: col,
            op,
            value: val,
        },
        p,
    ))
}

// ============================================================================
// SQL execution engine
// ============================================================================

/// Execute a parsed SQL statement against a database.
fn execute_sql(db: &mut Database, stmt: &SqlStatement) -> QueryResult {
    match stmt {
        SqlStatement::Select {
            columns,
            table,
            where_clause,
            order_by,
            limit,
            offset,
            group_by,
        } => execute_select(
            db,
            columns,
            table,
            where_clause.as_ref(),
            order_by.as_ref(),
            *limit,
            *offset,
            group_by.as_deref(),
        ),
        SqlStatement::Insert {
            table,
            columns,
            values,
        } => execute_insert(db, table, columns, values),
        SqlStatement::Update {
            table,
            set_clauses,
            where_clause,
        } => execute_update(db, table, set_clauses, where_clause.as_ref()),
        SqlStatement::Delete {
            table,
            where_clause,
        } => execute_delete(db, table, where_clause.as_ref()),
        SqlStatement::CreateTable {
            name,
            columns,
            if_not_exists,
        } => execute_create_table(db, name, columns, *if_not_exists),
        SqlStatement::DropTable { name, if_exists } => execute_drop_table(db, name, *if_exists),
    }
}

fn execute_select(
    db: &Database,
    columns: &[SelectColumn],
    table_name: &str,
    where_clause: Option<&WhereClause>,
    order_by: Option<&(String, SortDir)>,
    limit: Option<usize>,
    offset: Option<usize>,
    group_by: Option<&str>,
) -> QueryResult {
    let table = match db.find_table(table_name) {
        Some(t) => t,
        None => return QueryResult::error(&format!("Table '{table_name}' not found")),
    };

    // Apply WHERE filter
    let mut filtered_rows: Vec<&Vec<CellValue>> = table.rows.iter().collect();
    if let Some(wc) = where_clause {
        let col_idx = match table.column_index(&wc.column) {
            Some(idx) => idx,
            None => return QueryResult::error(&format!("Column '{}' not found", wc.column)),
        };
        let filter_value = if wc.value.is_empty() {
            CellValue::Null
        } else if let Some(col) = table.columns.get(col_idx) {
            CellValue::parse_as(&wc.value, &col.data_type)
        } else {
            CellValue::Text(wc.value.clone())
        };
        filtered_rows.retain(|row| {
            row.get(col_idx)
                .is_some_and(|cell| matches_filter(cell, &wc.op, &filter_value))
        });
    }

    // Handle GROUP BY with aggregates
    if let Some(group_col_name) = group_by {
        return execute_grouped_select(
            table,
            columns,
            &filtered_rows,
            group_col_name,
            order_by,
            limit,
            offset,
        );
    }

    // Check if we have aggregate functions without GROUP BY
    let has_aggregates = columns
        .iter()
        .any(|c| matches!(c, SelectColumn::Aggregate { .. }));
    if has_aggregates {
        return execute_aggregate_select(table, columns, &filtered_rows);
    }

    // Determine output columns
    let (out_col_names, col_indices) = resolve_columns(table, columns);

    // Build result rows
    let mut result_rows: Vec<Vec<CellValue>> = filtered_rows
        .iter()
        .map(|row| {
            col_indices
                .iter()
                .map(|&idx| row.get(idx).cloned().unwrap_or(CellValue::Null))
                .collect()
        })
        .collect();

    // ORDER BY
    if let Some((col_name, dir)) = order_by
        && let Some(sort_idx) = out_col_names
            .iter()
            .position(|n| n.to_uppercase() == col_name.to_uppercase())
    {
        result_rows.sort_by(|a, b| {
            let va = a.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
            let vb = b.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
            match dir {
                SortDir::Ascending => va.cmp(&vb),
                SortDir::Descending => vb.cmp(&va),
            }
        });
    }

    // OFFSET
    if let Some(off) = offset {
        if off < result_rows.len() {
            result_rows = result_rows.into_iter().skip(off).collect();
        } else {
            result_rows.clear();
        }
    }

    // LIMIT
    if let Some(lim) = limit {
        result_rows.truncate(lim);
    }

    QueryResult::with_data(out_col_names, result_rows)
}

fn resolve_columns(table: &Table, columns: &[SelectColumn]) -> (Vec<String>, Vec<usize>) {
    let mut names = Vec::new();
    let mut indices = Vec::new();

    for col in columns {
        match col {
            SelectColumn::AllColumns => {
                for (i, c) in table.columns.iter().enumerate() {
                    names.push(c.name.clone());
                    indices.push(i);
                }
            }
            SelectColumn::Named(name) => {
                if let Some(idx) = table.column_index(name) {
                    if let Some(c) = table.columns.get(idx) {
                        names.push(c.name.clone());
                    }
                    indices.push(idx);
                }
            }
            SelectColumn::Aggregate { .. } => {
                // Handled separately
            }
        }
    }

    (names, indices)
}

fn execute_aggregate_select(
    table: &Table,
    columns: &[SelectColumn],
    rows: &[&Vec<CellValue>],
) -> QueryResult {
    let mut out_names = Vec::new();
    let mut out_values = Vec::new();

    for col in columns {
        if let SelectColumn::Aggregate {
            func,
            column,
            alias,
        } = col
        {
            let name = alias
                .clone()
                .unwrap_or_else(|| format!("{}({})", func.label(), column));
            out_names.push(name);

            let col_idx = if column == "*" {
                None
            } else {
                table.column_index(column)
            };

            let value = compute_aggregate(*func, rows, col_idx);
            out_values.push(value);
        }
    }

    QueryResult::with_data(out_names, vec![out_values])
}

fn execute_grouped_select(
    table: &Table,
    columns: &[SelectColumn],
    rows: &[&Vec<CellValue>],
    group_col_name: &str,
    order_by: Option<&(String, SortDir)>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> QueryResult {
    let group_idx = match table.column_index(group_col_name) {
        Some(idx) => idx,
        None => return QueryResult::error(&format!("Column '{group_col_name}' not found")),
    };

    // Group rows by the group column value
    let mut groups: Vec<(CellValue, Vec<&Vec<CellValue>>)> = Vec::new();
    for row in rows {
        let key = row.get(group_idx).cloned().unwrap_or(CellValue::Null);
        let found = groups
            .iter_mut()
            .find(|(k, _)| k.as_sort_key() == key.as_sort_key());
        if let Some((_, group_rows)) = found {
            group_rows.push(row);
        } else {
            groups.push((key, vec![row]));
        }
    }

    // Build result
    let mut out_names = Vec::new();
    let mut result_rows = Vec::new();

    // Determine column names
    for col in columns {
        match col {
            SelectColumn::Named(name) => out_names.push(name.clone()),
            SelectColumn::Aggregate {
                func,
                column,
                alias,
            } => {
                out_names.push(
                    alias
                        .clone()
                        .unwrap_or_else(|| format!("{}({})", func.label(), column)),
                );
            }
            SelectColumn::AllColumns => {
                for c in &table.columns {
                    out_names.push(c.name.clone());
                }
            }
        }
    }

    for (group_key, group_rows) in &groups {
        let mut row_values = Vec::new();
        for col in columns {
            match col {
                SelectColumn::Named(name) => {
                    if name.to_uppercase() == group_col_name.to_uppercase() {
                        row_values.push(group_key.clone());
                    } else if let Some(idx) = table.column_index(name) {
                        row_values.push(
                            group_rows
                                .first()
                                .and_then(|r| r.get(idx))
                                .cloned()
                                .unwrap_or(CellValue::Null),
                        );
                    }
                }
                SelectColumn::Aggregate { func, column, .. } => {
                    let col_idx = if column == "*" {
                        None
                    } else {
                        table.column_index(column)
                    };
                    let group_row_refs: Vec<&Vec<CellValue>> = group_rows.clone();
                    let value = compute_aggregate(*func, &group_row_refs, col_idx);
                    row_values.push(value);
                }
                SelectColumn::AllColumns => {
                    if let Some(first) = group_rows.first() {
                        row_values.extend(first.iter().cloned());
                    }
                }
            }
        }
        result_rows.push(row_values);
    }

    // ORDER BY
    if let Some((col_name, dir)) = order_by
        && let Some(sort_idx) = out_names
            .iter()
            .position(|n| n.to_uppercase() == col_name.to_uppercase())
    {
        result_rows.sort_by(|a, b| {
            let va = a.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
            let vb = b.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
            match dir {
                SortDir::Ascending => va.cmp(&vb),
                SortDir::Descending => vb.cmp(&va),
            }
        });
    }

    // OFFSET
    if let Some(off) = offset {
        if off < result_rows.len() {
            result_rows = result_rows.into_iter().skip(off).collect();
        } else {
            result_rows.clear();
        }
    }

    // LIMIT
    if let Some(lim) = limit {
        result_rows.truncate(lim);
    }

    QueryResult::with_data(out_names, result_rows)
}

fn compute_aggregate(func: AggFunc, rows: &[&Vec<CellValue>], col_idx: Option<usize>) -> CellValue {
    match func {
        AggFunc::Count => {
            if let Some(idx) = col_idx {
                let count = rows
                    .iter()
                    .filter(|r| r.get(idx).is_some_and(|v| *v != CellValue::Null))
                    .count();
                CellValue::Integer(count as i64)
            } else {
                CellValue::Integer(rows.len() as i64)
            }
        }
        AggFunc::Sum => {
            let idx = col_idx.unwrap_or(0);
            let sum: f64 = rows
                .iter()
                .filter_map(|r| r.get(idx))
                .map(|v| match v {
                    CellValue::Integer(n) => *n as f64,
                    CellValue::Real(n) => *n,
                    _ => 0.0,
                })
                .sum();
            CellValue::Real(sum)
        }
        AggFunc::Avg => {
            let idx = col_idx.unwrap_or(0);
            let values: Vec<f64> = rows
                .iter()
                .filter_map(|r| r.get(idx))
                .filter_map(|v| match v {
                    CellValue::Integer(n) => Some(*n as f64),
                    CellValue::Real(n) => Some(*n),
                    _ => None,
                })
                .collect();
            if values.is_empty() {
                CellValue::Null
            } else {
                let sum: f64 = values.iter().sum();
                CellValue::Real(sum / values.len() as f64)
            }
        }
        AggFunc::Min => {
            let idx = col_idx.unwrap_or(0);
            rows.iter()
                .filter_map(|r| r.get(idx))
                .filter(|v| **v != CellValue::Null)
                .min_by(|a, b| a.as_sort_key().cmp(&b.as_sort_key()))
                .cloned()
                .unwrap_or(CellValue::Null)
        }
        AggFunc::Max => {
            let idx = col_idx.unwrap_or(0);
            rows.iter()
                .filter_map(|r| r.get(idx))
                .filter(|v| **v != CellValue::Null)
                .max_by(|a, b| a.as_sort_key().cmp(&b.as_sort_key()))
                .cloned()
                .unwrap_or(CellValue::Null)
        }
    }
}

fn execute_insert(
    db: &mut Database,
    table_name: &str,
    col_names: &[String],
    values: &[Vec<String>],
) -> QueryResult {
    let table = match db.find_table_mut(table_name) {
        Some(t) => t,
        None => return QueryResult::error(&format!("Table '{table_name}' not found")),
    };

    let col_count = table.col_count();
    let mut inserted = 0usize;

    // If column names provided, map values to correct positions
    let col_indices: Vec<usize> = if col_names.is_empty() {
        (0..col_count).collect()
    } else {
        let mut indices = Vec::new();
        for name in col_names {
            match table.column_index(name) {
                Some(idx) => indices.push(idx),
                None => return QueryResult::error(&format!("Column '{name}' not found")),
            }
        }
        indices
    };

    for val_row in values {
        let mut row = vec![CellValue::Null; col_count];
        for (vi, &ci) in col_indices.iter().enumerate() {
            if let Some(val_str) = val_row.get(vi)
                && let Some(col) = table.columns.get(ci)
                && let Some(cell) = row.get_mut(ci)
            {
                *cell = CellValue::parse_as(val_str, &col.data_type);
            }
        }

        // Fill defaults
        for (i, col) in table.columns.iter().enumerate() {
            if let Some(cell) = row.get(i)
                && *cell == CellValue::Null
                && let Some(ref def) = col.constraints.default_value
                && let Some(cell_mut) = row.get_mut(i)
            {
                *cell_mut = CellValue::parse_as(def, &col.data_type);
            }
        }

        match table.insert_row(row) {
            Ok(()) => inserted = inserted.saturating_add(1),
            Err(e) => return QueryResult::error(&format!("Insert failed: {e}")),
        }
    }

    let mut result = QueryResult::success(&format!("{inserted} row(s) inserted"));
    result.affected_rows = inserted;
    result
}

fn execute_update(
    db: &mut Database,
    table_name: &str,
    set_clauses: &[(String, String)],
    where_clause: Option<&WhereClause>,
) -> QueryResult {
    let table = match db.find_table_mut(table_name) {
        Some(t) => t,
        None => return QueryResult::error(&format!("Table '{table_name}' not found")),
    };

    let mut total_updated = 0usize;

    for (set_col_name, set_val_str) in set_clauses {
        let set_col_idx = match table.column_index(set_col_name) {
            Some(idx) => idx,
            None => return QueryResult::error(&format!("Column '{set_col_name}' not found")),
        };
        let set_value = if let Some(col) = table.columns.get(set_col_idx) {
            CellValue::parse_as(set_val_str, &col.data_type)
        } else {
            CellValue::Text(set_val_str.clone())
        };

        if let Some(wc) = where_clause {
            let where_col_idx = match table.column_index(&wc.column) {
                Some(idx) => idx,
                None => return QueryResult::error(&format!("Column '{}' not found", wc.column)),
            };
            let where_value = if let Some(col) = table.columns.get(where_col_idx) {
                CellValue::parse_as(&wc.value, &col.data_type)
            } else {
                CellValue::Text(wc.value.clone())
            };
            total_updated = total_updated.saturating_add(table.update_where(
                set_col_idx,
                &set_value,
                where_col_idx,
                &wc.op,
                &where_value,
            ));
        } else {
            // Update all rows
            for row in &mut table.rows {
                if let Some(cell) = row.get_mut(set_col_idx) {
                    *cell = set_value.clone();
                    total_updated = total_updated.saturating_add(1);
                }
            }
        }
    }

    let mut result = QueryResult::success(&format!("{total_updated} row(s) updated"));
    result.affected_rows = total_updated;
    result
}

fn execute_delete(
    db: &mut Database,
    table_name: &str,
    where_clause: Option<&WhereClause>,
) -> QueryResult {
    let table = match db.find_table_mut(table_name) {
        Some(t) => t,
        None => return QueryResult::error(&format!("Table '{table_name}' not found")),
    };

    let deleted = if let Some(wc) = where_clause {
        let col_idx = match table.column_index(&wc.column) {
            Some(idx) => idx,
            None => return QueryResult::error(&format!("Column '{}' not found", wc.column)),
        };
        let filter_value = if let Some(col) = table.columns.get(col_idx) {
            CellValue::parse_as(&wc.value, &col.data_type)
        } else {
            CellValue::Text(wc.value.clone())
        };
        table.delete_where(col_idx, &wc.op, &filter_value)
    } else {
        let count = table.rows.len();
        table.rows.clear();
        count
    };

    let mut result = QueryResult::success(&format!("{deleted} row(s) deleted"));
    result.affected_rows = deleted;
    result
}

fn execute_create_table(
    db: &mut Database,
    name: &str,
    columns: &[ParsedColumnDef],
    if_not_exists: bool,
) -> QueryResult {
    if if_not_exists && db.find_table(name).is_some() {
        return QueryResult::success("Table already exists (IF NOT EXISTS)");
    }

    let col_defs: Vec<ColumnDef> = columns
        .iter()
        .map(|pc| {
            let mut cd = ColumnDef::new(&pc.name, DataType::from_str_loose(&pc.data_type));
            cd.constraints.primary_key = pc.primary_key;
            cd.constraints.not_null = pc.not_null;
            cd.constraints.unique = pc.unique;
            cd.constraints.auto_increment = pc.auto_increment;
            cd.constraints.default_value = pc.default_value.clone();
            cd
        })
        .collect();

    let table = Table::new(name, col_defs);
    match db.create_table(table) {
        Ok(()) => QueryResult::success(&format!("Table '{name}' created")),
        Err(e) => QueryResult::error(&e),
    }
}

fn execute_drop_table(db: &mut Database, name: &str, if_exists: bool) -> QueryResult {
    if if_exists && db.find_table(name).is_none() {
        return QueryResult::success("Table does not exist (IF EXISTS)");
    }
    match db.drop_table(name) {
        Ok(()) => QueryResult::success(&format!("Table '{name}' dropped")),
        Err(e) => QueryResult::error(&e),
    }
}

// ============================================================================
// Export functions
// ============================================================================

/// Export table data as CSV.
pub fn export_csv(table: &Table) -> String {
    let mut out = String::new();
    // Header. Column names are as free-form as the cell values -- a table can
    // be created by `import_csv` from a file we did not write -- yet every
    // exporter in this module escaped its values and none escaped its names.
    let headers: Vec<String> = table
        .columns
        .iter()
        .map(|c| guitk::csv::field(&c.name))
        .collect();
    out.push_str(&headers.join(","));
    out.push('\n');

    // Data. Text is quoted unconditionally (rather than only when it needs
    // to be) so the export keeps the text/number distinction visible; that is
    // still conforming, and doubling the inner quotes makes commas, quotes
    // and newlines alike inert.
    for row in &table.rows {
        let vals: Vec<String> = row
            .iter()
            .map(|v| match v {
                CellValue::Text(s) => format!("\"{}\"", s.replace('"', "\"\"")),
                other => other.display(),
            })
            .collect();
        out.push_str(&vals.join(","));
        out.push('\n');
    }
    out
}

/// Export table data as JSON.
pub fn export_json(table: &Table) -> String {
    let mut out = String::from("[\n");
    for (ri, row) in table.rows.iter().enumerate() {
        out.push_str("  {");
        for (ci, val) in row.iter().enumerate() {
            if ci > 0 {
                out.push_str(", ");
            }
            let col_name =
                guitk::escape::json_string(table.columns.get(ci).map_or("?", |c| c.name.as_str()));
            match val {
                CellValue::Integer(n) => out.push_str(&format!("\"{col_name}\": {n}")),
                CellValue::Real(n) => out.push_str(&format!("\"{col_name}\": {n}")),
                CellValue::Text(s) => {
                    // The old `s.replace('"', "\\\"")` was worse than doing
                    // nothing for one input: a value ending in a backslash
                    // became `"...\"`, escaping the closing quote and
                    // truncating the whole document at that point.
                    out.push_str(&format!(
                        "\"{col_name}\": \"{}\"",
                        guitk::escape::json_string(s)
                    ));
                }
                CellValue::Blob(b) => {
                    out.push_str(&format!("\"{col_name}\": \"<blob:{}>\"", b.len()));
                }
                CellValue::Null => out.push_str(&format!("\"{col_name}\": null")),
            }
        }
        out.push('}');
        if ri < table.rows.len().saturating_sub(1) {
            out.push(',');
        }
        out.push('\n');
    }
    out.push(']');
    out
}

/// Export table data as SQL INSERT statements.
/// Quote a SQL identifier per the standard: wrap in double quotes and double
/// any embedded double quote.
///
/// Identifiers were previously interpolated bare, so a table or column name
/// containing a space, a keyword, or a `)` produced a script that either
/// failed to parse or -- worse -- parsed as something else entirely.
fn sql_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn export_sql_inserts(table: &Table) -> String {
    let mut out = String::new();
    let col_names: Vec<String> = table.columns.iter().map(|c| sql_ident(&c.name)).collect();
    let cols_str = col_names.join(", ");

    for row in &table.rows {
        let vals: Vec<String> = row
            .iter()
            .map(|v| match v {
                CellValue::Integer(n) => n.to_string(),
                CellValue::Real(n) => format!("{n}"),
                CellValue::Text(s) => format!("'{}'", s.replace('\'', "''")),
                CellValue::Blob(_) => "X''".to_owned(),
                CellValue::Null => "NULL".to_owned(),
            })
            .collect();
        out.push_str(&format!(
            "INSERT INTO {} ({}) VALUES ({});\n",
            sql_ident(&table.name),
            cols_str,
            vals.join(", ")
        ));
    }
    out
}

// ============================================================================
// Import CSV
// ============================================================================

/// Import CSV data into a table. Returns the parsed table.
pub fn import_csv(name: &str, csv_data: &str) -> Result<Table, String> {
    let mut records = guitk::csv::parse_records(csv_data).into_iter();

    // Detect header. Column names go through the same RFC 4180 decoding as
    // the values: a quoted header field may legitimately contain a comma, and
    // `export_csv` emits exactly that when a column name needs it.
    let headers = records.next().ok_or_else(|| "Empty CSV data".to_owned())?;

    if headers.is_empty() {
        return Err("No columns found in CSV header".to_owned());
    }

    let columns: Vec<ColumnDef> = headers
        .iter()
        .map(|h| ColumnDef::new(h.trimmed_if_bare(), DataType::Text))
        .collect();

    let mut table = Table::new(name, columns);

    for values in records {
        if values.iter().all(|v| v.text.trim().is_empty()) {
            continue;
        }
        // Pad short records so a ragged file still imports.
        let mut cells: Vec<CellValue> = values
            .iter()
            .take(headers.len())
            .map(|f| CellValue::Text(f.trimmed_if_bare().to_owned()))
            .collect();
        while cells.len() < headers.len() {
            cells.push(CellValue::Null);
        }
        if cells.len() == table.col_count() {
            let _ = table.insert_row(cells);
        }
    }

    // Attempt type inference on the data
    infer_column_types(&mut table);

    Ok(table)
}

/// Infer and convert column types based on data patterns.
fn infer_column_types(table: &mut Table) {
    for col_idx in 0..table.col_count() {
        let all_int = table.rows.iter().all(|row| {
            row.get(col_idx).is_none_or(|v| match v {
                CellValue::Text(s) => s.is_empty() || s.parse::<i64>().is_ok(),
                CellValue::Null => true,
                _ => false,
            })
        });

        if all_int && !table.rows.is_empty() {
            if let Some(col) = table.columns.get_mut(col_idx) {
                col.data_type = DataType::Integer;
            }
            for row in &mut table.rows {
                if let Some(cell) = row.get_mut(col_idx)
                    && let CellValue::Text(s) = cell
                    && let Ok(n) = s.parse::<i64>()
                {
                    *cell = CellValue::Integer(n);
                }
            }
            continue;
        }

        let all_real = table.rows.iter().all(|row| {
            row.get(col_idx).is_none_or(|v| match v {
                CellValue::Text(s) => s.is_empty() || s.parse::<f64>().is_ok(),
                CellValue::Null => true,
                _ => false,
            })
        });

        if all_real && !table.rows.is_empty() {
            if let Some(col) = table.columns.get_mut(col_idx) {
                col.data_type = DataType::Real;
            }
            for row in &mut table.rows {
                if let Some(cell) = row.get_mut(col_idx)
                    && let CellValue::Text(s) = cell
                    && let Ok(n) = s.parse::<f64>()
                {
                    *cell = CellValue::Real(n);
                }
            }
        }
    }
}

// ============================================================================
// Object tree sidebar items
// ============================================================================

/// Sidebar tree node types.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeNodeKind {
    TablesHeader,
    Table(String),
    IndexesHeader,
    Index(String),
    ViewsHeader,
    View(String),
    TriggersHeader,
    Trigger(String),
}

/// A node in the sidebar object tree.
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub kind: TreeNodeKind,
    pub expanded: bool,
    pub depth: usize,
}

fn build_tree_nodes(db: &Database) -> Vec<TreeNode> {
    let mut nodes = Vec::new();

    // Tables
    nodes.push(TreeNode {
        kind: TreeNodeKind::TablesHeader,
        expanded: true,
        depth: 0,
    });
    for name in db.table_names() {
        nodes.push(TreeNode {
            kind: TreeNodeKind::Table(name),
            expanded: false,
            depth: 1,
        });
    }

    // Indexes
    nodes.push(TreeNode {
        kind: TreeNodeKind::IndexesHeader,
        expanded: true,
        depth: 0,
    });
    for idx in &db.indexes {
        nodes.push(TreeNode {
            kind: TreeNodeKind::Index(idx.name.clone()),
            expanded: false,
            depth: 1,
        });
    }

    // Views
    nodes.push(TreeNode {
        kind: TreeNodeKind::ViewsHeader,
        expanded: true,
        depth: 0,
    });
    for view in &db.views {
        nodes.push(TreeNode {
            kind: TreeNodeKind::View(view.name.clone()),
            expanded: false,
            depth: 1,
        });
    }

    // Triggers
    nodes.push(TreeNode {
        kind: TreeNodeKind::TriggersHeader,
        expanded: true,
        depth: 0,
    });
    for trigger in &db.triggers {
        nodes.push(TreeNode {
            kind: TreeNodeKind::Trigger(trigger.name.clone()),
            expanded: false,
            depth: 1,
        });
    }

    nodes
}

// ============================================================================
// Active panels
// ============================================================================

/// Which bottom panel is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BottomPanel {
    SqlEditor,
    Results,
    Schema,
    Diagram,
}

impl BottomPanel {
    fn label(&self) -> &'static str {
        match self {
            Self::SqlEditor => "SQL Editor",
            Self::Results => "Results",
            Self::Schema => "Schema",
            Self::Diagram => "Diagram",
        }
    }

    fn all() -> &'static [Self] {
        &[Self::SqlEditor, Self::Results, Self::Schema, Self::Diagram]
    }
}

// ============================================================================
// Database tab (connection)
// ============================================================================

/// A row as the grid shows it: where it sits in the table it came from, and
/// its cells. The index travels with the cells because filtering and sorting
/// make the two orders differ.
pub type SourceRow = (usize, Vec<CellValue>);

/// What the grid draws: the column names, and the rows under them.
pub type TableView = (Vec<String>, Vec<SourceRow>);

/// A database connection tab.
#[derive(Clone, Debug)]
pub struct DbTab {
    pub db: Database,
    pub selected_table: Option<String>,
    pub sort_state: Option<SortState>,
    pub page: usize,
    pub filters: Vec<ActiveFilter>,
    pub tree_nodes: Vec<TreeNode>,
}

impl DbTab {
    fn new(db: Database) -> Self {
        let tree_nodes = build_tree_nodes(&db);
        let first_table = db.tables.first().map(|t| t.name.clone());
        Self {
            db,
            selected_table: first_table,
            sort_state: None,
            page: 0,
            filters: Vec::new(),
            tree_nodes,
        }
    }

    fn refresh_tree(&mut self) {
        self.tree_nodes = build_tree_nodes(&self.db);
    }

    /// The current table's columns, and its rows with sorting and filtering
    /// applied, each paired with its index in the table it came from.
    ///
    /// The index is carried because a row on the screen is not the same row in
    /// the table: filtering removes rows before it and sorting moves it. The
    /// grid's delete button names a row by where it sits in the picture, and
    /// `DbViewerApp::delete_row` removes one by where it sits in the table, so
    /// without the index between them the third row on a sorted screen deletes
    /// whatever happens to be third in insertion order -- silently, and with a
    /// success message.
    fn current_table_data(&self) -> Option<TableView> {
        let table_name = self.selected_table.as_ref()?;
        let table = self.db.find_table(table_name)?;

        let col_names: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
        let mut rows: Vec<SourceRow> = table.rows.iter().cloned().enumerate().collect();

        // Apply filters
        for filter in &self.filters {
            if let Some(col) = table.columns.get(filter.column_idx) {
                let filter_value = CellValue::parse_as(&filter.value_str, &col.data_type);
                rows.retain(|(_, row)| {
                    row.get(filter.column_idx)
                        .is_some_and(|cell| matches_filter(cell, &filter.op, &filter_value))
                });
            }
        }

        // Apply sorting
        if let Some(ref sort) = self.sort_state {
            let sort_idx = sort.column_idx;
            let dir = sort.direction;
            rows.sort_by(|(_, a), (_, b)| {
                let va = a.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
                let vb = b.get(sort_idx).map_or(SortKey::Null, |v| v.as_sort_key());
                match dir {
                    SortDir::Ascending => va.cmp(&vb),
                    SortDir::Descending => vb.cmp(&va),
                }
            });
        }

        Some((col_names, rows))
    }
}

// ============================================================================
// Application state
// ============================================================================

/// Main application state.
pub struct DbViewerApp {
    pub tabs: Vec<DbTab>,
    pub active_tab: usize,
    pub sql_input: String,
    pub query_result: Option<QueryResult>,
    pub history: Vec<HistoryEntry>,
    pub history_counter: u64,
    pub bottom_panel: BottomPanel,
    pub show_filter_builder: bool,
    pub filter_column_idx: usize,
    pub filter_op_idx: usize,
    pub filter_value: String,
    /// What the keyboard is reaching.
    pub focus: Focus,
    /// What the last press did, shown along the bottom.
    ///
    /// Several of this program's actions produce a string and nothing else --
    /// an export, a failed import, a delete -- and with no window there was
    /// nowhere for that string to go, so the functions that made it had no
    /// caller. This is where it goes.
    pub status: String,
    /// The size the last frame was drawn at.
    ///
    /// A press is answered against the frame the user actually saw, so this is
    /// remembered from `render` and from `Resize` both: a press can arrive
    /// after a resize and before the next frame, and answering that one against
    /// the old size hits whatever used to be under the pointer.
    window_width: f32,
    /// The height of the last frame drawn. See `window_width`.
    window_height: f32,
}

impl Default for DbViewerApp {
    fn default() -> Self {
        Self::new()
    }
}

impl DbViewerApp {
    pub fn new() -> Self {
        let sample_db = Database::sample();
        let tab = DbTab::new(sample_db);

        Self {
            tabs: vec![tab],
            active_tab: 0,
            sql_input: String::from("SELECT * FROM users"),
            query_result: None,
            history: Vec::new(),
            history_counter: 0,
            bottom_panel: BottomPanel::SqlEditor,
            show_filter_builder: false,
            filter_column_idx: 0,
            filter_op_idx: 0,
            filter_value: String::new(),
            focus: Focus::Editor,
            status: String::from("Ready"),
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
        }
    }

    /// Get the active tab.
    fn active_db_tab(&self) -> Option<&DbTab> {
        self.tabs.get(self.active_tab)
    }

    /// Get the active tab mutably.
    fn active_db_tab_mut(&mut self) -> Option<&mut DbTab> {
        self.tabs.get_mut(self.active_tab)
    }

    /// Execute the current SQL query.
    pub fn execute_query(&mut self) {
        let sql = self.sql_input.clone();
        if sql.trim().is_empty() {
            self.query_result = Some(QueryResult::error("Empty query"));
            return;
        }

        let result = match parse_sql(&sql) {
            Ok(stmt) => {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let result = execute_sql(&mut tab.db, &stmt);
                    tab.refresh_tree();
                    result
                } else {
                    QueryResult::error("No active database")
                }
            }
            Err(e) => QueryResult::error(&format!("Parse error: {e}")),
        };

        // Add to history
        self.history_counter = self.history_counter.saturating_add(1);
        self.history.push(HistoryEntry {
            sql: sql.clone(),
            success: !result.is_error,
            message: result.message.clone(),
            favorite: false,
            timestamp_counter: self.history_counter,
        });

        self.query_result = Some(result);
        self.bottom_panel = BottomPanel::Results;
    }

    /// Toggle a history entry's favorite status.
    pub fn toggle_favorite(&mut self, idx: usize) {
        if let Some(entry) = self.history.get_mut(idx) {
            entry.favorite = !entry.favorite;
        }
    }

    /// Add a new empty database tab.
    pub fn add_tab(&mut self, name: &str) {
        let db = Database::new(name);
        self.tabs.push(DbTab::new(db));
        self.active_tab = self.tabs.len().saturating_sub(1);
    }

    /// Close a tab.
    pub fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 {
            return; // Keep at least one tab
        }
        if idx < self.tabs.len() {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len().saturating_sub(1);
            }
        }
    }

    /// Select a table in the current tab's sidebar.
    pub fn select_table(&mut self, name: &str) {
        if let Some(tab) = self.active_db_tab_mut() {
            tab.selected_table = Some(name.to_owned());
            tab.page = 0;
            tab.sort_state = None;
            tab.filters.clear();
        }
    }

    /// Toggle sort on a column.
    pub fn toggle_sort(&mut self, col_idx: usize) {
        if let Some(tab) = self.active_db_tab_mut() {
            tab.sort_state = Some(match &tab.sort_state {
                Some(s) if s.column_idx == col_idx => SortState {
                    column_idx: col_idx,
                    direction: match s.direction {
                        SortDir::Ascending => SortDir::Descending,
                        SortDir::Descending => SortDir::Ascending,
                    },
                },
                _ => SortState {
                    column_idx: col_idx,
                    direction: SortDir::Ascending,
                },
            });
        }
    }

    /// Navigate to next page.
    pub fn next_page(&mut self) {
        if let Some(tab) = self.active_db_tab_mut()
            && let Some(table_name) = &tab.selected_table
            && let Some(table) = tab.db.find_table(table_name)
        {
            let max_page = table.row_count().saturating_sub(1) / PAGE_SIZE;
            if tab.page < max_page {
                tab.page = tab.page.saturating_add(1);
            }
        }
    }

    /// Navigate to previous page.
    pub fn prev_page(&mut self) {
        if let Some(tab) = self.active_db_tab_mut() {
            tab.page = tab.page.saturating_sub(1);
        }
    }

    /// Add a filter from the filter builder.
    pub fn add_filter(&mut self) {
        let op = FilterOp::all()
            .get(self.filter_op_idx)
            .cloned()
            .unwrap_or(FilterOp::Equal);

        let filter = ActiveFilter {
            column_idx: self.filter_column_idx,
            op,
            value_str: self.filter_value.clone(),
        };

        if let Some(tab) = self.active_db_tab_mut() {
            tab.filters.push(filter);
            tab.page = 0;
        }
        self.filter_value.clear();
    }

    /// Remove a filter.
    pub fn remove_filter(&mut self, idx: usize) {
        if let Some(tab) = self.active_db_tab_mut()
            && idx < tab.filters.len()
        {
            tab.filters.remove(idx);
            tab.page = 0;
        }
    }

    /// Delete a row from the selected table.
    pub fn delete_row(&mut self, row_idx: usize) {
        if let Some(tab) = self.active_db_tab_mut()
            && let Some(table_name) = tab.selected_table.clone()
            && let Some(table) = tab.db.find_table_mut(&table_name)
            && row_idx < table.rows.len()
        {
            table.rows.remove(row_idx);
        }
    }

    /// Export current table data in the specified format.
    pub fn export_current_table(&self, format: ExportFormat) -> Option<String> {
        let tab = self.active_db_tab()?;
        let table_name = tab.selected_table.as_ref()?;
        let table = tab.db.find_table(table_name)?;

        Some(match format {
            ExportFormat::Csv => export_csv(table),
            ExportFormat::Json => export_json(table),
            ExportFormat::SqlInserts => export_sql_inserts(table),
        })
    }

    /// Import CSV data into the active database.
    pub fn import_csv_data(&mut self, name: &str, csv_data: &str) -> Result<(), String> {
        let table = import_csv(name, csv_data)?;
        if let Some(tab) = self.active_db_tab_mut() {
            tab.db.create_table(table)?;
            tab.refresh_tree();
        }
        Ok(())
    }

    // ========================================================================
    // Drawing
    // ========================================================================

    /// Build the whole picture for a window of `w` by `h`, and with it the hit
    /// boxes that answer for what was drawn.
    ///
    /// One pass produces both, so a control cannot be drawn in one place and
    /// answer in another, and a control the window was too small to draw is a
    /// control that cannot be pressed.
    #[must_use]
    pub fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::solve(w, h);
        let mut f = Frame::new(l.window.w, l.window.h);
        f.push(fill(l.window, BASE, 0.0));

        self.draw_toolbar(&mut f, l.toolbar);
        self.draw_db_tabs(&mut f, l.tabs);
        self.draw_sidebar(&mut f, l.sidebar);
        self.draw_data_grid(&mut f, &l);
        self.draw_bottom_panels(&mut f, &l);
        self.draw_status_bar(&mut f, l.status);
        f
    }

    /// The picture as a flat command list, for a caller that wants the ink
    /// without the hit boxes.
    #[must_use]
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        self.frame(width, height).commands().to_vec()
    }

    /// The toolbar: the title, then the buttons, left to right.
    fn draw_toolbar(&self, f: &mut Frame<Target>, area: Rect) {
        if area.is_empty() {
            return;
        }
        f.push(fill(area, MANTLE, 0.0));

        // The cursor starts after the title and stops at the right-hand edge.
        // The old pass started it at a hard-coded `x = 130` and never asked how
        // wide the window was, so at 400 points across Import was painted past
        // the edge -- and a button that is off the edge and still answers a
        // press is worse than one that is not drawn at all.
        let mut bx = area.x + 12.0;
        let title_w = (area.right() - bx).clamp(0.0, 100.0);
        put_text(
            f,
            Rect::new(bx, area.y, title_w, area.h),
            "DB Viewer",
            14.0,
            BLUE,
            FontWeightHint::Bold,
        );
        bx += title_w + 18.0;

        let btn_h = (area.h - 12.0).max(0.0);
        for (label, target, color) in TOOLBAR_BUTTONS {
            let bw = text::padded_width(label, 8.0, 11.0, FontWeightHint::Regular);
            let btn = Rect::new(bx, area.y + 6.0, bw, btn_h);
            if btn.is_empty() || btn.right() > area.right() {
                break;
            }
            f.push(fill(btn, SURFACE0, CORNER_RADIUS));
            put_text(
                f,
                inset_x(btn, 8.0),
                label,
                11.0,
                *color,
                FontWeightHint::Regular,
            );
            f.hit(*target, btn);
            bx = btn.right() + 8.0;
        }
    }

    /// The strip of database-connection tabs, each with its own close box.
    fn draw_db_tabs(&self, f: &mut Frame<Target>, area: Rect) {
        if area.is_empty() {
            return;
        }
        f.push(fill(area, CRUST, 0.0));

        let top_corners = CornerRadii {
            top_left: CORNER_RADIUS,
            top_right: CORNER_RADIUS,
            bottom_left: 0.0,
            bottom_right: 0.0,
        };

        let mut tx = area.x + 4.0;
        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab;
            let tw = text::padded_width_any_weight(&tab.db.name, 16.0, 12.0);
            let cell = Rect::new(tx, area.y + 4.0, tw, (area.h - 4.0).max(0.0));
            // A tab that does not fit is not drawn. The strip does not scroll,
            // so the alternative is a row of tabs running out of the window,
            // each of them clickable where nobody can see it.
            if cell.is_empty() || cell.right() > area.right() {
                break;
            }

            f.push(RenderCommand::FillRect {
                x: cell.x,
                y: cell.y,
                width: cell.w,
                height: cell.h,
                color: if is_active { BASE } else { CRUST },
                corner_radii: top_corners,
            });

            let close = Rect::new((cell.right() - 16.0).max(cell.x), cell.y, 16.0, cell.h);
            let label = Rect::new(
                cell.x + 8.0,
                cell.y,
                (close.x - cell.x - 8.0).max(0.0),
                cell.h,
            );
            put_text(
                f,
                label,
                &tab.db.name,
                11.0,
                if is_active { TEXT } else { SUBTEXT0 },
                if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            );
            put_text(f, close, "x", 10.0, OVERLAY0, FontWeightHint::Regular);

            // The tab first and its close box second: the topmost hit box wins,
            // so recorded the other way round the tab would swallow the `x` and
            // pressing it would merely select the tab it was trying to close.
            f.hit(Target::SelectTab(i), cell);
            f.hit(Target::CloseTab(i), close);

            tx = cell.right() + 2.0;
        }

        let plus = Rect::new(tx, area.y + 6.0, 24.0, (area.h - 12.0).max(0.0));
        if !plus.is_empty() && plus.right() <= area.right() {
            f.push(fill(plus, SURFACE0, CORNER_RADIUS));
            put_text(f, plus, "+", 12.0, SUBTEXT0, FontWeightHint::Bold);
            f.hit(Target::AddTab, plus);
        }
    }

    /// The object tree, with the filter builder reserved out of the bottom.
    ///
    /// Every row asks whether it is still inside the sidebar before it is
    /// drawn. The old pass asked once, at the top of the tree loop
    /// (`if ny > y + height { break; }`), which let a row straddling the bottom
    /// edge be drawn whole, and asked not at all for the filter builder below
    /// it -- so in a short window the filters in force were painted over the
    /// bottom panel and the status bar.
    ///
    /// The builder is carved off the bottom *before* the tree is laid out
    /// rather than drawn wherever the tree's cursor happened to stop. Written
    /// the other way round, a database of a dozen tables pushed the whole
    /// builder -- the only way to remove a filter -- off the end of the window,
    /// and the filter it could not remove was still hiding rows.
    fn draw_sidebar(&self, f: &mut Frame<Target>, area: Rect) {
        if area.is_empty() {
            return;
        }
        f.push(fill(area, MANTLE, 0.0));
        f.push(RenderCommand::Line {
            x1: area.right(),
            y1: area.y,
            x2: area.right(),
            y2: area.bottom(),
            color: SURFACE0,
            width: 1.0,
        });

        let Some(tab) = self.active_db_tab() else {
            return;
        };

        // What the builder would like, and what it is allowed: never more than
        // three fifths of the sidebar, so the tree is never squeezed to nothing.
        let builder_h = if self.show_filter_builder {
            let wanted = 20.0 + 4.0 * TREE_ROW_HEIGHT + tab.filters.len() as f32 * 20.0;
            wanted.min(area.h * 0.6)
        } else {
            0.0
        };
        let builder = Rect::new(
            area.x,
            (area.bottom() - builder_h).max(area.y),
            area.w,
            builder_h,
        );
        let tree_area = Rect::new(area.x, area.y, area.w, (builder.y - area.y).max(0.0));

        self.draw_object_tree(f, tree_area, tab);
        if builder_h > 0.0 {
            self.draw_filter_builder(f, builder, tab);
        }
    }

    /// The database's name and its tables, indexes, views and triggers.
    fn draw_object_tree(&self, f: &mut Frame<Target>, area: Rect, tab: &DbTab) {
        if area.is_empty() {
            return;
        }
        let mut y = area.y + 8.0;

        let name_row = Rect::new(area.x + 10.0, y, (area.w - 20.0).max(0.0), 18.0);
        if name_row.bottom() > area.bottom() {
            return;
        }
        put_text(f, name_row, &tab.db.name, 12.0, BLUE, FontWeightHint::Bold);
        y += 22.0;

        if y >= area.bottom() {
            return;
        }
        f.push(hline(inset_x(area, 8.0), y, SURFACE0));
        y += 8.0;

        for (i, node) in tab.tree_nodes.iter().enumerate() {
            let row = Rect::new(
                area.x + 4.0,
                y,
                (area.w - 8.0).max(0.0),
                TREE_ROW_HEIGHT - 2.0,
            );
            if row.is_empty() || row.bottom() > area.bottom() {
                break;
            }

            let indent = node.depth as f32 * 16.0;
            let is_selected = match &node.kind {
                TreeNodeKind::Table(name) => tab.selected_table.as_deref() == Some(name.as_str()),
                _ => false,
            };
            let (icon, label, color) = match &node.kind {
                TreeNodeKind::TablesHeader => ("T", "Tables".to_owned(), BLUE),
                TreeNodeKind::Table(name) => (
                    "  ",
                    name.clone(),
                    if is_selected { TEXT } else { SUBTEXT1 },
                ),
                TreeNodeKind::IndexesHeader => ("I", "Indexes".to_owned(), PEACH),
                TreeNodeKind::Index(name) => ("  ", name.clone(), SUBTEXT0),
                TreeNodeKind::ViewsHeader => ("V", "Views".to_owned(), GREEN),
                TreeNodeKind::View(name) => ("  ", name.clone(), SUBTEXT0),
                TreeNodeKind::TriggersHeader => ("!", "Triggers".to_owned(), RED),
                TreeNodeKind::Trigger(name) => ("  ", name.clone(), SUBTEXT0),
            };
            let is_header = node.depth == 0;

            if is_selected {
                f.push(fill(row, SURFACE0, 3.0));
            }

            let weight = if is_header {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };
            let size = if is_header { 10.0 } else { 11.0 };
            let icon_box = Rect::new(row.x + 6.0 + indent, row.y, 16.0, row.h);
            put_text(f, icon_box, icon, size, color, weight);
            let label_box = Rect::new(
                icon_box.right() + 6.0,
                row.y,
                (row.right() - icon_box.right() - 12.0).max(0.0),
                row.h,
            );
            put_text(f, label_box, &label, size, color, weight);

            // A heading names a category, not an object; there is nothing for
            // pressing one to mean, so it is not a control. The three kinds
            // that are not tables answer by saying what they are -- before this
            // the sidebar was the only place an index, view or trigger was
            // mentioned at all, and it could not be asked about one.
            if !is_header {
                f.hit(Target::TreeNode(i), row);
            }

            y += TREE_ROW_HEIGHT;
        }
    }

    /// The filter builder: a column, a comparison, a value, and the filters in
    /// force with a way to remove each of them.
    fn draw_filter_builder(&self, f: &mut Frame<Target>, area: Rect, tab: &DbTab) {
        if area.is_empty() {
            return;
        }
        f.push(hline(inset_x(area, 8.0), area.y, SURFACE0));

        let cols: Vec<String> = tab
            .selected_table
            .as_deref()
            .and_then(|n| tab.db.find_table(n))
            .map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        let col_name = cols
            .get(self.filter_column_idx)
            .map_or("(no column)", String::as_str);
        let op = FilterOp::all()
            .get(self.filter_op_idx)
            .map_or("=", FilterOp::label);

        let mut y = area.y + 4.0;
        let heading = Rect::new(area.x + 10.0, y, (area.w - 20.0).max(0.0), 14.0);
        if heading.bottom() > area.bottom() {
            return;
        }
        put_text(
            f,
            heading,
            "FILTER BUILDER",
            10.0,
            YELLOW,
            FontWeightHint::Bold,
        );
        y += 18.0;

        // Column, comparison, value and Add, each a row of its own. Three of
        // the four step a field that the program already had and that nothing
        // could reach: `filter_column_idx`, `filter_op_idx` and `filter_value`
        // were set by tests and by nothing else.
        let rows: [(String, Target, Color); 4] = [
            (
                format!("Column: {col_name}"),
                Target::FilterColumn,
                LAVENDER,
            ),
            (format!("Where: {op}"), Target::FilterOp, MAUVE),
            (
                if self.filter_value.is_empty() {
                    "Value: (type here)".to_owned()
                } else {
                    format!("Value: {}", self.filter_value)
                },
                Target::FilterValue,
                if self.focus == Focus::FilterValue {
                    TEXT
                } else {
                    SUBTEXT0
                },
            ),
            ("+ Add filter".to_owned(), Target::AddFilter, GREEN),
        ];
        for (label, target, color) in rows {
            let row = Rect::new(
                area.x + 8.0,
                y,
                (area.w - 16.0).max(0.0),
                TREE_ROW_HEIGHT - 4.0,
            );
            if row.is_empty() || row.bottom() > area.bottom() {
                return;
            }
            f.push(fill(row, SURFACE0, 3.0));
            if target == Target::FilterValue && self.focus == Focus::FilterValue {
                f.push(stroke(row, BLUE, 3.0));
            }
            put_text(
                f,
                inset_x(row, 6.0),
                &label,
                10.0,
                color,
                FontWeightHint::Regular,
            );
            f.hit(target, row);
            y += TREE_ROW_HEIGHT;
        }

        for (fi, filter) in tab.filters.iter().enumerate() {
            let row = Rect::new(area.x + 8.0, y, (area.w - 16.0).max(0.0), 18.0);
            if row.is_empty() || row.bottom() > area.bottom() {
                return;
            }
            let name = cols.get(filter.column_idx).map_or("?", String::as_str);
            f.push(fill(row, SURFACE0, 3.0));
            let remove = Rect::new((row.right() - 16.0).max(row.x), row.y, 16.0, row.h);
            let text_box = Rect::new(row.x + 4.0, row.y, (remove.x - row.x - 4.0).max(0.0), row.h);
            put_text(
                f,
                text_box,
                &format!("{name} {} {}", filter.op.label(), filter.value_str),
                10.0,
                TEAL,
                FontWeightHint::Regular,
            );
            put_text(f, remove, "x", 10.0, RED, FontWeightHint::Regular);
            f.hit(Target::RemoveFilter(fi), remove);
            y += 20.0;
        }
    }

    /// The data grid: headers that sort, the rows of the current page each
    /// with a delete box, and the pagination bar under them.
    fn draw_data_grid(&self, f: &mut Frame<Target>, l: &Layout) {
        let area = l.grid;
        if area.is_empty() {
            return;
        }
        f.push(fill(area, BASE, 0.0));

        let Some(tab) = self.active_db_tab() else {
            return;
        };
        let Some((col_names, all_rows)) = tab.current_table_data() else {
            put_text(
                f,
                inset(area, 20.0),
                "No table selected",
                13.0,
                OVERLAY0,
                FontWeightHint::Regular,
            );
            return;
        };

        let total_rows = all_rows.len();
        let start = tab.page.saturating_mul(PAGE_SIZE);
        let end = start.saturating_add(PAGE_SIZE).min(total_rows);
        let page_rows = all_rows.get(start..end).unwrap_or(&[]);

        // The delete box is taken out of the right-hand end before the columns
        // are measured, so a cell is never drawn underneath it and the box is
        // never drawn over a cell's last few characters.
        let delete_w = 20.0_f32.min(area.w);
        let body = Rect::new(area.x, area.y, (area.w - delete_w).max(0.0), area.h);
        let col_count = col_names.len();
        let col_width = if col_count > 0 {
            (body.w / col_count as f32).max(MIN_COL_WIDTH)
        } else {
            DEFAULT_COL_WIDTH
        };
        // How many columns fit whole. The rest are not drawn: the grid does not
        // scroll sideways, so a column past the right-hand edge is one nobody
        // can read and nobody can sort by, and the count below says so rather
        // than letting the table look complete.
        let shown = col_names
            .iter()
            .enumerate()
            .take_while(|(ci, _)| body.x + (*ci as f32 + 1.0) * col_width <= body.right() + 0.01)
            .count();

        let header = l.grid_header();
        let rows_area = l.grid_rows();

        // --- header row ---
        if !header.is_empty() {
            f.push(fill(
                Rect::new(header.x, header.y, body.w, header.h),
                SURFACE0,
                0.0,
            ));
            for (ci, col_name) in col_names.iter().enumerate().take(shown) {
                let cell = Rect::new(
                    body.x + ci as f32 * col_width,
                    header.y,
                    col_width,
                    header.h,
                );
                let arrow = tab.sort_state.as_ref().and_then(|s| {
                    (s.column_idx == ci).then_some(match s.direction {
                        SortDir::Ascending => " ^",
                        SortDir::Descending => " v",
                    })
                });
                put_text(
                    f,
                    inset_x(cell, CELL_PADDING),
                    &format!("{col_name}{}", arrow.unwrap_or("")),
                    11.0,
                    LAVENDER,
                    FontWeightHint::Bold,
                );
                // The header is the sort control. Before this, `toggle_sort`
                // had no caller outside the tests: the arrow was drawn, and
                // there was no way to put it there.
                f.hit(Target::SortColumn(ci), cell);
                if ci > 0 {
                    f.push(RenderCommand::Line {
                        x1: cell.x,
                        y1: header.y,
                        x2: cell.x,
                        y2: rows_area.bottom(),
                        color: SURFACE1,
                        width: 1.0,
                    });
                }
            }
            f.push(hline(
                Rect::new(header.x, header.y, body.w, header.h),
                header.bottom(),
                SURFACE1,
            ));
        }

        // --- data rows ---
        //
        // Clipped *and* bounded. The clip alone is not enough: it stops the
        // renderer showing a row past the bottom edge and it makes `Frame::hit`
        // drop that row's boxes, but a fill is pushed exactly as asked, so the
        // old pass -- which tested `ry > y + height` at the *top* of a row, and
        // so drew whole any row that merely started inside -- painted a 26-point
        // band of row colour over the pagination bar below it.
        f.clip(rows_area);
        let mut ry = rows_area.y;
        for (ri, (source_idx, row)) in page_rows.iter().enumerate() {
            let line = Rect::new(rows_area.x, ry, area.w, ROW_HEIGHT);
            if line.bottom() > rows_area.bottom() {
                break;
            }
            f.push(fill(
                Rect::new(line.x, line.y, body.w, line.h),
                if ri % 2 == 0 { BASE } else { SURFACE0 },
                0.0,
            ));

            for (ci, cell) in row.iter().enumerate().take(shown) {
                let cell_box = Rect::new(body.x + ci as f32 * col_width, ry, col_width, ROW_HEIGHT);
                put_text(
                    f,
                    inset_x(cell_box, CELL_PADDING),
                    &cell.display(),
                    11.0,
                    cell_color(cell),
                    FontWeightHint::Regular,
                );
            }

            let del = Rect::new(body.right(), ry, delete_w, ROW_HEIGHT);
            put_text(f, del, "x", 10.0, RED, FontWeightHint::Regular);
            // Named by where the row sits in the *table*, not by where it sits
            // on the screen. With a sort in force the two differ, and deleting
            // by screen position removes a row the user was not pointing at.
            f.hit(Target::DeleteRow(*source_idx), del);

            f.push(hline(line, line.bottom(), SURFACE0));
            ry += ROW_HEIGHT;
        }
        f.unclip();

        // --- pagination bar ---
        let bar = l.page_bar();
        if bar.is_empty() {
            return;
        }
        f.push(fill(bar, MANTLE, 0.0));

        let total_pages = if total_rows == 0 {
            1
        } else {
            // Ceiling division — guaranteed >= 1 here since total_rows >= 1.
            // Use div_ceil to avoid the `(x-1)/n + 1` underflow/overflow trap.
            total_rows.div_ceil(PAGE_SIZE)
        };
        let hidden = col_count.saturating_sub(shown);
        let mut caption = format!(
            "Page {} of {total_pages} ({total_rows} rows)",
            tab.page.saturating_add(1),
        );
        if hidden > 0 {
            caption.push_str(&format!(" — {shown} of {col_count} columns shown"));
        }

        // The buttons are placed first and the caption is given what is left of
        // the bar, so the two cannot be drawn on top of each other in a narrow
        // window. The old pass gave the caption `width - 200` whatever the bar
        // measured, which in a 180-point grid is a negative width.
        let mut bx = bar.right();
        for (label, target) in [("Next >", Target::NextPage), ("< Prev", Target::PrevPage)] {
            let btn = Rect::new(bx - 60.0, bar.y + 2.0, 54.0, (bar.h - 4.0).max(0.0));
            if btn.is_empty() || btn.x < bar.x {
                break;
            }
            f.push(fill(btn, SURFACE0, 3.0));
            put_text(
                f,
                inset_x(btn, 6.0),
                label,
                10.0,
                SUBTEXT1,
                FontWeightHint::Regular,
            );
            f.hit(target, btn);
            bx = btn.x;
        }
        let caption_box = Rect::new(bar.x + 12.0, bar.y, (bx - bar.x - 18.0).max(0.0), bar.h);
        put_text(
            f,
            caption_box,
            &caption,
            10.0,
            SUBTEXT0,
            FontWeightHint::Regular,
        );
    }

    /// The four bottom-panel tabs, and whichever panel they name.
    fn draw_bottom_panels(&self, f: &mut Frame<Target>, l: &Layout) {
        let area = l.panel;
        if area.is_empty() {
            return;
        }
        f.push(fill(area, MANTLE, 0.0));
        f.push(hline(area, area.y, SURFACE1));

        let tabs = l.panel_tabs();
        let top_corners = CornerRadii {
            top_left: 3.0,
            top_right: 3.0,
            bottom_left: 0.0,
            bottom_right: 0.0,
        };
        let mut tx = tabs.x + 4.0;
        for panel in BottomPanel::all() {
            let label = panel.label();
            let tw = text::padded_width_any_weight(label, 8.0, 11.0);
            let cell = Rect::new(tx, tabs.y + 2.0, tw, (tabs.h - 4.0).max(0.0));
            if cell.is_empty() || cell.right() > tabs.right() {
                break;
            }
            let is_active = *panel == self.bottom_panel;
            f.push(RenderCommand::FillRect {
                x: cell.x,
                y: cell.y,
                width: cell.w,
                height: cell.h,
                color: if is_active { SURFACE0 } else { MANTLE },
                corner_radii: top_corners,
            });
            put_text(
                f,
                inset_x(cell, 8.0),
                label,
                10.0,
                if is_active { TEXT } else { SUBTEXT0 },
                if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            );
            // Four tabs that named four panels and could not switch between
            // them: `bottom_panel` was set once in `new`, and again by
            // `execute_query`, and by nothing a user could press.
            f.hit(Target::ShowPanel(*panel), cell);
            tx = cell.right() + 2.0;
        }

        let body = l.panel_body();
        if body.is_empty() {
            return;
        }
        f.clip(body);
        match self.bottom_panel {
            BottomPanel::SqlEditor => self.draw_sql_editor(f, body),
            BottomPanel::Results => self.draw_results(f, body),
            BottomPanel::Schema => self.draw_schema(f, body),
            BottomPanel::Diagram => self.draw_diagram(f, body),
        }
        f.unclip();
    }

    /// The SQL editor and the query history under it.
    fn draw_sql_editor(&self, f: &mut Frame<Target>, area: Rect) {
        // The box takes what it needs off the top of the pane and no more, and
        // the history starts where the box ends. Both used to be placed at
        // fixed offsets from the top of a pane whose height nobody asked for.
        //
        // What it needs is one line. The editor draws a *single* run of tokens
        // -- there is no wrapping, no second line, and no way to put a caret on
        // one -- so the 80 points it used to ask for were 64 points of empty
        // box. In the 114-point pane the layout actually gives it, those 64
        // points were enough to push every history row past the bottom edge:
        // `HISTORY (n queries)` was drawn as a title over an empty strip in
        // every window there has ever been, and no query could be recalled or
        // starred because neither control was ever drawn at all.
        let line_h = 16.0_f32;
        let box_h = (line_h + 8.0).min((area.h - 24.0).max(0.0));
        let editor = Rect::new(area.x + 8.0, area.y + 4.0, (area.w - 16.0).max(0.0), box_h);
        if editor.is_empty() {
            return;
        }
        f.push(fill(editor, CRUST, CORNER_RADIUS));
        f.push(stroke(
            editor,
            if self.focus == Focus::Editor {
                BLUE
            } else {
                SURFACE1
            },
            CORNER_RADIUS,
        ));
        f.hit(Target::SqlEditor, editor);

        let line = Rect::new(
            editor.x + 8.0,
            editor.y + 4.0,
            (editor.w - 16.0).max(0.0),
            line_h,
        );
        if self.sql_input.is_empty() {
            put_text(
                f,
                line,
                "Enter SQL query...",
                12.0,
                OVERLAY0,
                FontWeightHint::Regular,
            );
        } else {
            let mut tx = line.x;
            for token in tokenize_sql(&self.sql_input) {
                let (s, color, weight) = token_ink(&token);
                // Measured in the token's *own* weight: keywords are drawn
                // bold, so a fixed cell laid the next token on top of the tail
                // of every SELECT and WHERE. And a quoted string literal is
                // drawn with its quotes, which the byte count did include but
                // only by accident of them being one byte each.
                let w = text::measure(&s, 12.0, weight);
                if tx + w > line.right() {
                    break;
                }
                put_text(
                    f,
                    Rect::new(tx, line.y, w + 4.0, line.h),
                    &s,
                    12.0,
                    color,
                    weight,
                );
                tx += w;
            }
        }

        let mut hy = editor.bottom() + 8.0;
        let head = Rect::new(area.x + 12.0, hy, (area.w - 24.0).max(0.0), 12.0);
        if head.bottom() > area.bottom() {
            return;
        }
        hy += 16.0;

        // Placed before anything is painted, so the heading can say how many
        // of the queries are on the screen. The list is cut twice -- to the
        // five newest, and again by whatever is left of the pane -- and a
        // heading that only counted the queries let a pane showing three of
        // twenty look like a pane showing all twenty.
        //
        // Newest first, and each row carries its index in the history rather
        // than its position on the screen: the list is reversed and cut, so the
        // two are different numbers and starring by the second would star the
        // wrong query.
        let mut placed: Vec<(usize, Rect)> = Vec::new();
        for (i, _) in self.history.iter().enumerate().rev().take(5) {
            let row = Rect::new(
                area.x + 8.0,
                hy,
                (area.w - 16.0).max(0.0),
                HISTORY_ROW_HEIGHT - 2.0,
            );
            if row.is_empty() || row.bottom() > area.bottom() {
                break;
            }
            placed.push((i, row));
            hy += HISTORY_ROW_HEIGHT;
        }

        let total = self.history.len();
        put_text(
            f,
            head,
            &if placed.len() == total {
                format!("HISTORY ({total} queries)")
            } else {
                format!("HISTORY ({} of {total} shown)", placed.len())
            },
            10.0,
            OVERLAY0,
            FontWeightHint::Bold,
        );

        for (i, row) in placed {
            let Some(entry) = self.history.get(i) else {
                continue;
            };
            f.push(fill(row, SURFACE0, 2.0));
            let dot = Rect::new(row.x + 4.0, row.y + (row.h - 6.0) / 2.0, 6.0, 6.0);
            f.push(fill(dot, if entry.success { GREEN } else { RED }, 3.0));
            let star = Rect::new((row.right() - 16.0).max(row.x), row.y, 16.0, row.h);
            put_text(
                f,
                Rect::new(
                    dot.right() + 6.0,
                    row.y,
                    (star.x - dot.right() - 8.0).max(0.0),
                    row.h,
                ),
                &entry.sql,
                10.0,
                SUBTEXT0,
                FontWeightHint::Regular,
            );
            put_text(
                f,
                star,
                if entry.favorite { "*" } else { "-" },
                10.0,
                if entry.favorite { YELLOW } else { OVERLAY0 },
                FontWeightHint::Bold,
            );
            // The row first, the star second: the last box recorded is the one
            // `hit_test` answers with, so recorded the other way round the row
            // would swallow its own star.
            f.hit(Target::HistoryEntry(i), row);
            f.hit(Target::FavoriteEntry(i), star);
        }
    }

    /// The last query's message, and the rows it returned under it.
    fn draw_results(&self, f: &mut Frame<Target>, area: Rect) {
        let Some(result) = self.query_result.as_ref() else {
            put_text(
                f,
                Rect::new(area.x + 16.0, area.y + 8.0, (area.w - 32.0).max(0.0), 16.0),
                "No query results. Execute a query first.",
                12.0,
                OVERLAY0,
                FontWeightHint::Regular,
            );
            return;
        };

        // Message. `RenderCommand::Text` clips at `max_width` rather than
        // wrapping, so a message wider than the pane used to be cut mid-word
        // with nothing to mark the cut -- and the messages that run long are
        // exactly the ones worth reading, the SQL errors saying what the engine
        // rejected and where.
        let msg_color = if result.is_error { RED } else { GREEN };
        let msg_width = (area.w - 24.0).max(0.0);
        let mut message = text::wrap(
            &result.message,
            msg_width,
            RESULT_MSG_FONT_SIZE,
            FontWeightHint::Bold,
        );
        // The message is bounded, and the overflow is marked rather than
        // dropped in silence.
        let fits_in_pane =
            (((area.h - 4.0 - RESULT_MSG_GAP) / RESULT_MSG_LINE_HEIGHT) as usize).max(1);
        let max_lines = if result.columns.is_empty() {
            fits_in_pane
        } else {
            fits_in_pane.min(RESULT_MSG_MAX_LINES_WITH_TABLE)
        };
        if message.len() > max_lines {
            message.truncate(max_lines);
            if let Some(last) = message.last_mut() {
                *last = text::elide(
                    &format!("{last}…"),
                    msg_width,
                    "…",
                    RESULT_MSG_FONT_SIZE,
                    FontWeightHint::Bold,
                );
            }
        }
        let mut my = area.y + 4.0;
        for line in &message {
            let row = Rect::new(area.x + 12.0, my, msg_width, RESULT_MSG_LINE_HEIGHT);
            if row.bottom() > area.bottom() {
                break;
            }
            put_text(
                f,
                row,
                line,
                RESULT_MSG_FONT_SIZE,
                msg_color,
                FontWeightHint::Bold,
            );
            my += RESULT_MSG_LINE_HEIGHT;
        }

        if result.columns.is_empty() {
            return;
        }

        // The table follows the message rather than sitting at a fixed offset
        // from the top of the pane, so a message that grew cannot be drawn over
        // its own headers.
        let table_top =
            area.y + 4.0 + message.len() as f32 * RESULT_MSG_LINE_HEIGHT + RESULT_MSG_GAP;
        let table = Rect::new(
            area.x,
            table_top,
            area.w,
            (area.bottom() - table_top).max(0.0),
        );
        if table.is_empty() {
            return;
        }

        let col_count = result.columns.len();
        let col_w = (table.w / col_count as f32).max(MIN_COL_WIDTH);
        // As in the data grid: a column past the right-hand edge is one nobody
        // can read, and the clip would hide the fact that it was ever asked for.
        let shown = (0..col_count)
            .take_while(|ci| table.x + (*ci as f32 + 1.0) * col_w <= table.right() + 0.01)
            .count();

        let header = Rect::new(table.x, table.y, table.w, 20.0_f32.min(table.h));
        if !header.is_empty() {
            f.push(fill(header, SURFACE0, 0.0));
            for (ci, col_name) in result.columns.iter().enumerate().take(shown) {
                let cell = Rect::new(table.x + ci as f32 * col_w, header.y, col_w, header.h);
                put_text(
                    f,
                    inset_x(cell, 6.0),
                    col_name,
                    10.0,
                    LAVENDER,
                    FontWeightHint::Bold,
                );
            }
        }

        let mut ry = header.bottom() + 2.0;
        for row in result.rows.iter().take(RESULT_ROWS_SHOWN) {
            let line = Rect::new(table.x, ry, table.w, 16.0);
            // Bottom edge, not top: a row that merely *starts* inside the pane
            // is a row whose second half is drawn over whatever is below it.
            if line.bottom() > table.bottom() {
                break;
            }
            for (ci, cell) in row.iter().enumerate().take(shown) {
                let cell_box = Rect::new(table.x + ci as f32 * col_w, ry, col_w, line.h);
                put_text(
                    f,
                    inset_x(cell_box, 6.0),
                    &cell.display(),
                    10.0,
                    cell_color(cell),
                    FontWeightHint::Regular,
                );
            }
            ry += 16.0;
        }
    }

    /// The selected table's columns, their types and constraints, and the
    /// foreign keys that leave it.
    fn draw_schema(&self, f: &mut Frame<Target>, area: Rect) {
        let Some(tab) = self.active_db_tab() else {
            return;
        };
        let Some(table_name) = tab.selected_table.as_ref() else {
            put_text(
                f,
                Rect::new(area.x + 16.0, area.y + 8.0, (area.w - 32.0).max(0.0), 16.0),
                "Select a table to view its schema.",
                12.0,
                OVERLAY0,
                FontWeightHint::Regular,
            );
            return;
        };
        let Some(table) = tab.db.find_table(table_name) else {
            return;
        };

        let inner = (area.w - 24.0).max(0.0);
        let title = Rect::new(area.x + 12.0, area.y + 4.0, inner, 16.0);
        if title.bottom() > area.bottom() {
            return;
        }
        put_text(
            f,
            title,
            &format!("SCHEMA: {table_name}"),
            12.0,
            BLUE,
            FontWeightHint::Bold,
        );

        // Three columns that used to be 180, 100 and 200 points wide whatever
        // the pane measured -- 480 points of table drawn into a pane that, with
        // the sidebar taking its share of a small window, is routinely narrower
        // than that. The constraints column was then drawn entirely outside the
        // pane, and the clip swallowed it: the schema looked as though the table
        // had no constraints at all. Scaled together, the three keep their
        // familiar proportions and all three stay inside.
        let scale = (inner / SCHEMA_COL_WIDTHS.iter().sum::<f32>()).min(1.0);
        let col_w = SCHEMA_COL_WIDTHS.map(|w| w * scale);

        let mut cy = area.y + 24.0;
        let head = Rect::new(area.x + 8.0, cy, (area.w - 16.0).max(0.0), 18.0);
        if head.bottom() > area.bottom() {
            return;
        }
        f.push(fill(head, SURFACE0, 2.0));
        let mut hx = area.x + 12.0;
        for (hi, header) in ["Column", "Type", "Constraints"].iter().enumerate() {
            let w = col_w.get(hi).copied().unwrap_or(0.0);
            put_text(
                f,
                Rect::new(hx, cy + 1.0, w, 16.0),
                header,
                10.0,
                LAVENDER,
                FontWeightHint::Bold,
            );
            hx += w;
        }
        cy += 22.0;

        for col in &table.columns {
            let row = Rect::new(area.x + 12.0, cy, inner, SCHEMA_ROW_HEIGHT);
            if row.bottom() > area.bottom() {
                return;
            }
            let cells: [(String, Color); 3] = [
                (col.name.clone(), TEXT),
                (col.data_type.label().to_owned(), col.data_type.color()),
                (col.constraints.describe(), YELLOW),
            ];
            let mut rx = row.x;
            for (ci, (label, color)) in cells.into_iter().enumerate() {
                let w = col_w.get(ci).copied().unwrap_or(0.0);
                put_text(
                    f,
                    Rect::new(rx, row.y, w, row.h),
                    &label,
                    10.0,
                    color,
                    FontWeightHint::Regular,
                );
                rx += w;
            }
            cy += SCHEMA_ROW_HEIGHT;
        }

        let fks: Vec<&ForeignKey> = tab
            .db
            .foreign_keys
            .iter()
            .filter(|fk| fk.from_table.to_uppercase() == table_name.to_uppercase())
            .collect();
        if fks.is_empty() {
            return;
        }

        cy += 8.0;
        let heading = Rect::new(area.x + 12.0, cy, inner, 14.0);
        // The foreign-key list used to ask nothing at all about where it was
        // being drawn: a table with a dozen columns put the whole of it below
        // the pane, and the clip made that look like a table with no foreign
        // keys rather than a pane too short to show them.
        if heading.bottom() > area.bottom() {
            return;
        }
        put_text(
            f,
            heading,
            "FOREIGN KEYS",
            10.0,
            PEACH,
            FontWeightHint::Bold,
        );
        cy += 16.0;

        for fk in fks {
            let row = Rect::new(area.x + 16.0, cy, (area.w - 32.0).max(0.0), 14.0);
            if row.bottom() > area.bottom() {
                return;
            }
            put_text(
                f,
                row,
                &format!(
                    "{}.{} -> {}.{}",
                    fk.from_table, fk.from_column, fk.to_table, fk.to_column
                ),
                10.0,
                TEAL,
                FontWeightHint::Regular,
            );
            cy += 16.0;
        }
    }

    /// The tables as boxes, wrapped to the width of the pane, with the foreign
    /// keys drawn between the boxes that were placed.
    ///
    /// The boxes used to be laid in one unbounded row at `start_x + ti * 200`,
    /// so the third table in a 400-point pane was drawn entirely outside it and
    /// the clip made the database look as though it had two tables. They wrap
    /// now, and when the pane runs out of height the heading says how many of
    /// the tables are being shown rather than letting the diagram look complete.
    fn draw_diagram(&self, f: &mut Frame<Target>, area: Rect) {
        let Some(tab) = self.active_db_tab() else {
            return;
        };

        let total = tab.db.tables.len();
        if total == 0 {
            put_text(
                f,
                Rect::new(area.x + 12.0, area.y + 4.0, (area.w - 24.0).max(0.0), 16.0),
                "SCHEMA DIAGRAM",
                12.0,
                BLUE,
                FontWeightHint::Bold,
            );
            put_text(
                f,
                Rect::new(area.x + 16.0, area.y + 28.0, (area.w - 32.0).max(0.0), 14.0),
                "No tables in database.",
                11.0,
                OVERLAY0,
                FontWeightHint::Regular,
            );
            return;
        }

        let top = area.y + 28.0;
        let content = Rect::new(
            area.x + 20.0,
            top,
            (area.w - 40.0).max(0.0),
            (area.bottom() - top).max(0.0),
        );

        // --- placement, before anything is painted ---
        let mut placed: Vec<(Rect, &Table)> = Vec::new();
        if !content.is_empty() {
            let box_w = DIAGRAM_BOX_WIDTH.min(content.w);
            let per_row = (((content.w + DIAGRAM_BOX_SPACING) / (box_w + DIAGRAM_BOX_SPACING))
                as usize)
                .max(1);
            let mut row_y = content.y;
            for chunk in tab.db.tables.chunks(per_row) {
                let room = (content.bottom() - row_y).max(0.0);
                if room < DIAGRAM_HEADER_HEIGHT {
                    break;
                }
                let mut row_h: f32 = 0.0;
                for (i, table) in chunk.iter().enumerate() {
                    let natural =
                        DIAGRAM_HEADER_HEIGHT + 2.0 + table.columns.len() as f32 * DIAGRAM_COL_STEP;
                    let h = natural.min(room);
                    let bx = content.x + i as f32 * (box_w + DIAGRAM_BOX_SPACING);
                    let r = Rect::new(bx, row_y, box_w, h);
                    if r.right() > content.right() + 0.01 {
                        break;
                    }
                    placed.push((r, table));
                    row_h = row_h.max(h);
                }
                row_y += row_h + DIAGRAM_ROW_GAP;
            }
        }

        let mut heading = "SCHEMA DIAGRAM".to_owned();
        if placed.len() < total {
            heading.push_str(&format!(" — {} of {total} tables shown", placed.len()));
        }
        put_text(
            f,
            Rect::new(area.x + 12.0, area.y + 4.0, (area.w - 24.0).max(0.0), 16.0),
            &heading,
            12.0,
            BLUE,
            FontWeightHint::Bold,
        );

        // --- the boxes ---
        for (r, table) in &placed {
            f.push(fill(*r, SURFACE0, CORNER_RADIUS));
            f.push(stroke(*r, BLUE, CORNER_RADIUS));
            let head = Rect::new(r.x, r.y, r.w, DIAGRAM_HEADER_HEIGHT.min(r.h));
            f.push(RenderCommand::FillRect {
                x: head.x,
                y: head.y,
                width: head.w,
                height: head.h,
                color: BLUE,
                corner_radii: CornerRadii {
                    top_left: CORNER_RADIUS,
                    top_right: CORNER_RADIUS,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });
            put_text(
                f,
                inset_x(head, 6.0),
                &table.name,
                10.0,
                CRUST,
                FontWeightHint::Bold,
            );

            let mut cy = head.bottom() + 4.0;
            for col in &table.columns {
                let line = Rect::new(r.x + 6.0, cy, (r.w - 12.0).max(0.0), DIAGRAM_COL_STEP);
                if line.bottom() > r.bottom() {
                    break;
                }
                let pk = if col.constraints.primary_key {
                    "PK "
                } else {
                    ""
                };
                put_text(
                    f,
                    line,
                    &format!("{pk}{}: {}", col.name, col.data_type.label()),
                    9.0,
                    SUBTEXT1,
                    FontWeightHint::Regular,
                );
                cy += DIAGRAM_COL_STEP;
            }
        }

        // --- the foreign keys between them ---
        for fk in &tab.db.foreign_keys {
            let find = |name: &str| {
                placed
                    .iter()
                    .find(|(_, t)| t.name.to_uppercase() == name.to_uppercase())
                    .map(|(r, _)| *r)
            };
            let (Some(from), Some(to)) = (find(&fk.from_table), find(&fk.to_table)) else {
                continue;
            };
            let from_y = from.y + 10.0;
            let to_y = to.y + 10.0;
            f.push(RenderCommand::Line {
                x1: from.right(),
                y1: from_y,
                x2: to.x,
                y2: to_y,
                color: PEACH,
                width: 1.5,
            });
            let mid_x = f32::midpoint(from.right(), to.x);
            let mid_y = f32::midpoint(from_y, to_y) - 8.0;
            let label = Rect::new(
                mid_x,
                mid_y,
                (content.right() - mid_x).clamp(0.0, 120.0),
                10.0,
            );
            put_text(
                f,
                label,
                &format!("{} -> {}", fk.from_column, fk.to_column),
                8.0,
                PEACH,
                FontWeightHint::Regular,
            );
        }
    }

    /// The status line: which database, which table, what just happened, and
    /// how many queries have been run.
    ///
    /// The four readings used to be placed at `x + 10`, `x + 200` and
    /// `x + width - 150` whatever the window measured, so in a narrow window the
    /// table reading was drawn over the database name and the query count was
    /// drawn over both. They are laid left to right from measured widths now,
    /// with the count reserved out of the right-hand end first, and each one
    /// stops as soon as the next would not fit.
    fn draw_status_bar(&self, f: &mut Frame<Target>, area: Rect) {
        if area.is_empty() {
            return;
        }
        f.push(fill(area, CRUST, 0.0));

        let tab = self.active_db_tab();

        let count = format!("Queries: {}", self.history.len());
        let count_w = text::measure(&count, 10.0, FontWeightHint::Regular);
        let mut right = area.right() - 10.0;
        if right - count_w >= area.x + 10.0 {
            put_text(
                f,
                Rect::new(right - count_w, area.y, count_w, area.h),
                &count,
                10.0,
                SUBTEXT0,
                FontWeightHint::Regular,
            );
            right -= count_w + 12.0;
        }

        let mut readings: Vec<(String, Color)> = Vec::new();
        readings.push((
            format!("DB: {}", tab.map_or("No database", |t| t.db.name.as_str())),
            BLUE,
        ));
        if let Some(t) = tab
            && let Some(table_name) = t.selected_table.as_ref()
            && let Some(table) = t.db.find_table(table_name)
        {
            readings.push((
                format!(
                    "Table: {table_name} ({} cols, {} rows)",
                    table.col_count(),
                    table.row_count()
                ),
                SUBTEXT0,
            ));
        }
        // What the last thing the user pressed did. Before this the program had
        // no way to say so: every control was a painted rectangle, and the
        // status line reported only what was already visible elsewhere.
        readings.push((self.status.clone(), SUBTEXT1));

        let mut sx = area.x + 10.0;
        for (label, color) in readings {
            let w = text::measure(&label, 10.0, FontWeightHint::Regular);
            if sx + w > right {
                break;
            }
            put_text(
                f,
                Rect::new(sx, area.y, w, area.h),
                &label,
                10.0,
                color,
                FontWeightHint::Regular,
            );
            sx += w + 14.0;
        }
    }
}

// ============================================================================
// What a press and a keystroke do
// ============================================================================

impl DbViewerApp {
    /// Route an event to whatever the drawing pass put under it.
    fn handle_event(&mut self, event: &Event, size: (f32, f32)) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me, size),
            _ => {}
        }
    }

    /// Route a press to the control the last frame drew at that point.
    ///
    /// The hit boxes come from a frame drawn at the same size, so a control the
    /// window was too small to draw is a control that cannot be pressed -- and
    /// the toolbar button that ran off the right-hand edge of a narrow window
    /// cannot be pressed from off the edge.
    fn handle_mouse(&mut self, event: &MouseEvent, size: (f32, f32)) {
        let MouseEventKind::Press(MouseButton::Left) = event.kind else {
            return;
        };
        let frame = self.frame(size.0, size.1);
        let Some(target) = frame.hit_test(event.x, event.y) else {
            return;
        };
        self.activate(target);
    }

    /// The columns of the table the sidebar has selected, if it has one.
    fn selected_columns(&self) -> Vec<String> {
        self.active_db_tab()
            .and_then(|tab| {
                let name = tab.selected_table.as_deref()?;
                let table = tab.db.find_table(name)?;
                Some(table.columns.iter().map(|c| c.name.clone()).collect())
            })
            .unwrap_or_default()
    }

    /// Do what pressing `target` means, and say on the status line what it did.
    fn activate(&mut self, target: Target) {
        match target {
            Target::Execute => {
                self.execute_query();
                self.status = self
                    .query_result
                    .as_ref()
                    .map_or_else(|| String::from("Ready"), |r| r.message.clone());
            }
            Target::NewTab | Target::AddTab => {
                let name = format!("database{}", self.tabs.len().saturating_add(1));
                self.add_tab(&name);
                self.status = format!("Opened {name}");
            }
            Target::Export(format) => match self.export_current_table(format) {
                Some(text) => {
                    // The export is put in the editor rather than thrown away.
                    // `export_current_table` returned a `String` that no caller
                    // outside the tests ever received: the three formats were
                    // written, and the program had nowhere to put the result.
                    // The editor is the one text surface the window has, and it
                    // makes export-then-import a round trip a user can perform.
                    self.status = format!(
                        "Exported {} as {} into the editor ({} bytes)",
                        self.active_db_tab()
                            .and_then(|t| t.selected_table.clone())
                            .unwrap_or_default(),
                        format.label(),
                        text.len()
                    );
                    self.sql_input = text;
                    self.bottom_panel = BottomPanel::SqlEditor;
                    self.focus = Focus::Editor;
                }
                None => self.status = String::from("Nothing to export: no table selected"),
            },
            Target::Import => {
                let name = format!("imported{}", self.tabs.len().saturating_add(1));
                let csv = self.sql_input.clone();
                self.status = match self.import_csv_data(&name, &csv) {
                    Ok(()) => {
                        self.select_table(&name);
                        format!("Imported {name}")
                    }
                    Err(e) => format!("Import failed: {e}"),
                };
            }
            Target::SelectTab(i) => {
                if i < self.tabs.len() {
                    self.active_tab = i;
                    self.status = self
                        .active_db_tab()
                        .map_or_else(String::new, |t| format!("Switched to {}", t.db.name));
                }
            }
            Target::CloseTab(i) => {
                if self.tabs.len() <= 1 {
                    self.status = String::from("The last database tab stays open");
                } else {
                    let name = self
                        .tabs
                        .get(i)
                        .map(|t| t.db.name.clone())
                        .unwrap_or_default();
                    self.close_tab(i);
                    self.status = format!("Closed {name}");
                }
            }
            Target::TreeNode(i) => {
                let node = self
                    .active_db_tab()
                    .and_then(|t| t.tree_nodes.get(i))
                    .map(|n| n.kind.clone());
                match node {
                    Some(TreeNodeKind::Table(name)) => {
                        self.select_table(&name);
                        self.status = format!("Table {name}");
                    }
                    Some(TreeNodeKind::Index(name)) => self.status = format!("Index {name}"),
                    Some(TreeNodeKind::View(name)) => self.status = format!("View {name}"),
                    Some(TreeNodeKind::Trigger(name)) => self.status = format!("Trigger {name}"),
                    _ => {}
                }
            }
            Target::ToggleFilterBuilder => {
                self.show_filter_builder = !self.show_filter_builder;
                self.status = if self.show_filter_builder {
                    String::from("Filter builder shown")
                } else {
                    self.focus = Focus::None;
                    String::from("Filter builder hidden")
                };
            }
            Target::FilterColumn => {
                let cols = self.selected_columns();
                if cols.is_empty() {
                    self.status = String::from("No table selected");
                } else {
                    self.filter_column_idx = self
                        .filter_column_idx
                        .saturating_add(1)
                        .checked_rem(cols.len())
                        .unwrap_or(0);
                    self.status = cols
                        .get(self.filter_column_idx)
                        .map_or_else(String::new, |c| format!("Filter column: {c}"));
                }
            }
            Target::FilterOp => {
                let ops = FilterOp::all();
                self.filter_op_idx = self
                    .filter_op_idx
                    .saturating_add(1)
                    .checked_rem(ops.len())
                    .unwrap_or(0);
                self.status = ops
                    .get(self.filter_op_idx)
                    .map_or_else(String::new, |o| format!("Filter test: {}", o.label()));
            }
            Target::FilterValue => {
                self.focus = Focus::FilterValue;
                self.status = String::from("Type the value to filter by");
            }
            Target::AddFilter => {
                let cols = self.selected_columns();
                let name = cols
                    .get(self.filter_column_idx)
                    .cloned()
                    .unwrap_or_else(|| String::from("(no column)"));
                self.add_filter();
                self.status = format!("Filtering on {name}");
            }
            Target::RemoveFilter(i) => {
                self.remove_filter(i);
                self.status = String::from("Filter removed");
            }
            Target::SortColumn(i) => {
                self.toggle_sort(i);
                let dir = self
                    .active_db_tab()
                    .and_then(|t| t.sort_state.as_ref())
                    .map_or("", |s| match s.direction {
                        SortDir::Ascending => "ascending",
                        SortDir::Descending => "descending",
                    });
                let name = self.selected_columns().get(i).cloned().unwrap_or_default();
                self.status = format!("Sorted by {name}, {dir}");
            }
            Target::DeleteRow(i) => {
                self.delete_row(i);
                self.status = format!("Deleted row {}", i.saturating_add(1));
            }
            Target::PrevPage => {
                self.prev_page();
                self.status = self.active_db_tab().map_or_else(String::new, |t| {
                    format!("Page {}", t.page.saturating_add(1))
                });
            }
            Target::NextPage => {
                self.next_page();
                self.status = self.active_db_tab().map_or_else(String::new, |t| {
                    format!("Page {}", t.page.saturating_add(1))
                });
            }
            Target::ShowPanel(panel) => {
                self.bottom_panel = panel;
                self.focus = if panel == BottomPanel::SqlEditor {
                    Focus::Editor
                } else {
                    Focus::None
                };
                self.status = panel.label().to_owned();
            }
            Target::SqlEditor => {
                self.focus = Focus::Editor;
                self.status = String::from("Type a query, Enter to run it");
            }
            Target::HistoryEntry(i) => {
                if let Some(entry) = self.history.get(i) {
                    self.sql_input = entry.sql.clone();
                    self.focus = Focus::Editor;
                    self.bottom_panel = BottomPanel::SqlEditor;
                    self.status = String::from("Query recalled");
                }
            }
            Target::FavoriteEntry(i) => {
                self.toggle_favorite(i);
                self.status = if self.history.get(i).is_some_and(|e| e.favorite) {
                    String::from("Starred")
                } else {
                    String::from("Unstarred")
                };
            }
        }
    }

    /// The text box that has the keyboard, if any.
    fn focused_text(&mut self) -> Option<&mut String> {
        match self.focus {
            Focus::Editor => Some(&mut self.sql_input),
            Focus::FilterValue => Some(&mut self.filter_value),
            Focus::None => None,
        }
    }

    /// Route a keystroke to whatever has the keyboard.
    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }
        match event.key {
            Key::Escape => {
                self.focus = Focus::None;
                self.status = String::from("Keyboard released");
                return;
            }
            Key::Tab => {
                self.focus = match self.focus {
                    Focus::Editor if self.show_filter_builder => Focus::FilterValue,
                    _ => Focus::Editor,
                };
                return;
            }
            Key::Backspace => {
                if let Some(text) = self.focused_text() {
                    text.pop();
                    return;
                }
            }
            Key::Enter => match self.focus {
                Focus::Editor => {
                    self.activate(Target::Execute);
                    return;
                }
                Focus::FilterValue => {
                    self.activate(Target::AddFilter);
                    return;
                }
                Focus::None => {}
            },
            _ => {}
        }

        // Printable text. `KeyEvent::text` is what the platform's keyboard
        // layout produced, shift and dead keys included; deriving a character
        // from the key code instead is what makes a `*` impossible to type on
        // any layout but the one the table was written for -- and `SELECT *` is
        // the first query anybody types.
        let typed = event.text.clone();
        if !typed.is_empty()
            && !typed.chars().any(char::is_control)
            && let Some(text) = self.focused_text()
        {
            text.push_str(&typed);
            return;
        }

        // Nothing has the keyboard, so letters are shortcuts.
        match event.key {
            Key::N => self.activate(Target::NewTab),
            Key::F => self.activate(Target::ToggleFilterBuilder),
            Key::E => self.activate(Target::SqlEditor),
            Key::R => self.activate(Target::ShowPanel(BottomPanel::Results)),
            Key::S => self.activate(Target::ShowPanel(BottomPanel::Schema)),
            Key::D => self.activate(Target::ShowPanel(BottomPanel::Diagram)),
            Key::PageDown => self.activate(Target::NextPage),
            Key::PageUp => self.activate(Target::PrevPage),
            _ => {}
        }
    }
}

// ============================================================================
// The window
// ============================================================================

impl App for DbViewerApp {
    fn title(&self) -> String {
        String::from("DB Viewer")
    }

    fn app_id(&self) -> String {
        String::from("dbviewer")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<Duration> {
        // A database browser changes when someone asks it something, and at no
        // other time. There is no clock on the screen to keep.
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            Event::CloseRequested => Response::Exit,
            Event::Resize { width, height } => {
                // Remembered here as well as in `render`, because a press can
                // arrive after a resize and before the next frame, and it has to
                // be answered against the window's real size.
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                Response::Redraw
            }
            _ => {
                self.handle_event(event, (self.window_width, self.window_height));
                Response::Redraw
            }
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.window_width = width;
        self.window_height = height;
        self.frame(width, height).into_tree()
    }
}

impl Probe for DbViewerApp {
    type Target = Target;
    type Outcome = ();
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) {
        self.window_width = size.0;
        self.window_height = size.1;
        self.handle_event(
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
            size,
        );
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) {
        self.window_width = size.0;
        self.window_height = size.1;
        self.handle_event(&Event::Key(key.clone()), size);
    }
}

// ============================================================================
// Export format enum
// ============================================================================

/// Supported export formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExportFormat {
    Csv,
    Json,
    SqlInserts,
}

impl ExportFormat {
    /// What the status line calls this format.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Json => "JSON",
            Self::SqlInserts => "SQL",
        }
    }
}

// ============================================================================
// Helper functions
// ============================================================================

// `truncate_str` was removed here. It cut the SQL-history line at 80
// characters and a result cell at 30 before handing either to a `Text`
// command that already carried `max_width` and `TextOverflow::Ellipsis` —
// so the renderer, which measures with the real font, was being handed a
// string that a fixed character count had already cut. Two answers to one
// question, and the guess won because it cut first: a narrow column still
// overflowed (30 characters is wider than a 90 px column) while a wide one
// was cut with room to spare. The counts were also compared against `len()`,
// which is bytes, so a cell of accented or CJK text lost half its content.
// Pass the string whole; the renderer knows the width and this does not.

// ============================================================================
// Main
// ============================================================================

fn main() -> ExitCode {
    // The previous `main` built a sample database, drew one frame into a `Vec`,
    // asserted the `Vec` was not empty and dropped it. It exercised the drawing
    // code and showed nobody the result -- and every control the drawing code
    // painted answered nothing, because nothing was reading presses.
    let mut app = DbViewerApp::new();
    app::launch("dbviewer", &mut app)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use guitk::probe;

    use super::*;

    // --- LIKE pattern matching ---

    #[test]
    fn like_underscore_matches_one_character_not_one_byte() {
        let mut checked = 0;
        for (ch, width) in [("é", 2), ("日", 3), ("😀", 4)] {
            assert!(
                simple_like_match(ch, "_"),
                "`_` should match the single character {ch:?}"
            );
            let many = "_".repeat(width);
            assert!(
                !simple_like_match(ch, &many),
                "{width} underscores must not match the {width} bytes of {ch:?}"
            );
            checked += 1;
        }
        assert!(checked >= 3, "only {checked} checked");

        assert!(simple_like_match("日本", "__"));
        assert!(simple_like_match("日本語.txt", "___.txt"));
        assert!(!simple_like_match("日本", "_"));
    }

    #[test]
    fn like_percent_combined_with_underscore_counts_characters() {
        // Literals and `%` on their own survive byte matching -- a
        // well-formed needle can only ever match starting on a character
        // boundary, by UTF-8 self-synchronization -- so they are not what
        // needs pinning down here. The discriminating cases are those where
        // `%` absorbs the slack and `_` must still count characters: under
        // the byte matcher a single kanji had three `_`s worth of room.
        assert!(!simple_like_match("日", "%_%_%"));
        assert!(!simple_like_match("日本", "%_%_%_%"));
        assert!(simple_like_match("日本", "%_%_%"));

        // Sanity: non-ASCII literals and `%` still behave.
        assert!(simple_like_match("日本語", "日%"));
        assert!(simple_like_match("日本語", "%語"));
        assert!(simple_like_match("日本語", "%本%"));
        assert!(!simple_like_match("日本語", "%犬%"));
    }

    #[test]
    fn like_on_ascii_is_unchanged() {
        let mut checked = 0;
        for (text, pat, want) in [
            ("hello", "hello", true),
            ("hello", "h%", true),
            ("hello", "%o", true),
            ("hello", "h_llo", true),
            ("hello", "h__lo", true),   // _ = e, _ = l
            ("hello", "h___lo", false), // one more character than there is
            ("hello", "%ell%", true),
            ("hello", "%xyz%", false),
            ("HELLO", "hello", true), // matching is case-insensitive
            ("", "", true),
            ("", "%", true),
            ("a", "", false),
        ] {
            assert_eq!(simple_like_match(text, pat), want, "{text:?} LIKE {pat:?}");
            checked += 1;
        }
        assert!(checked >= 12, "only {checked} checked");
    }

    // --- text measurement ---

    #[test]
    fn toolbar_buttons_fit_their_labels() {
        for label in ["Execute", "New Tab", "Export", "Import"] {
            let w = text::padded_width(label, 8.0, 11.0, FontWeightHint::Regular);
            let drawn = text::measure(label, 11.0, FontWeightHint::Regular);
            assert!(drawn + 16.0 <= w + 0.01, "{label:?} overflows its button");
        }
    }

    #[test]
    fn a_database_tab_fits_its_name_at_either_weight() {
        // Database names are filenames, so any byte but `/` and NUL.
        for name in ["main.db", "inventário.sqlite", "顧客.db"] {
            let w = text::padded_width_any_weight(name, 16.0, 12.0);
            for weight in [FontWeightHint::Bold, FontWeightHint::Regular] {
                assert!(
                    text::measure(name, 12.0, weight) + 32.0 <= w + 0.01,
                    "{name:?} overflows its tab at {weight:?}"
                );
            }
        }
    }

    #[test]
    fn highlighted_sql_tokens_do_not_overlap() {
        // Keywords are drawn bold. A fixed 7.2 px cell laid the next token on
        // top of the tail of every SELECT and WHERE.
        let tokens: [(&str, FontWeightHint); 6] = [
            ("SELECT", FontWeightHint::Bold),
            (" ", FontWeightHint::Regular),
            ("*", FontWeightHint::Bold),
            (" ", FontWeightHint::Regular),
            ("FROM", FontWeightHint::Bold),
            (" clientes", FontWeightHint::Regular),
        ];
        let mut x = 0.0_f32;
        let mut spans = Vec::new();
        for (t, weight) in tokens {
            let w = text::measure(t, 12.0, weight);
            spans.push((x, x + w));
            x += w;
        }
        for pair in spans.windows(2) {
            let (_, end) = pair[0];
            let (next_start, _) = pair[1];
            assert!(
                next_start >= end - 0.01,
                "a token starts at {next_start} but the one before it ends at {end}"
            );
        }
    }

    // --- Data type tests ---

    #[test]
    fn test_data_type_from_str_loose_integer() {
        assert_eq!(DataType::from_str_loose("INTEGER"), DataType::Integer);
        assert_eq!(DataType::from_str_loose("INT"), DataType::Integer);
        assert_eq!(DataType::from_str_loose("BIGINT"), DataType::Integer);
    }

    #[test]
    fn test_data_type_from_str_loose_real() {
        assert_eq!(DataType::from_str_loose("REAL"), DataType::Real);
        assert_eq!(DataType::from_str_loose("FLOAT"), DataType::Real);
        assert_eq!(DataType::from_str_loose("DOUBLE"), DataType::Real);
    }

    #[test]
    fn test_data_type_from_str_loose_text() {
        assert_eq!(DataType::from_str_loose("TEXT"), DataType::Text);
        assert_eq!(DataType::from_str_loose("VARCHAR"), DataType::Text);
        assert_eq!(DataType::from_str_loose("unknown_type"), DataType::Text);
    }

    #[test]
    fn test_data_type_from_str_loose_blob() {
        assert_eq!(DataType::from_str_loose("BLOB"), DataType::Blob);
    }

    #[test]
    fn test_data_type_label() {
        assert_eq!(DataType::Integer.label(), "INTEGER");
        assert_eq!(DataType::Real.label(), "REAL");
        assert_eq!(DataType::Text.label(), "TEXT");
        assert_eq!(DataType::Blob.label(), "BLOB");
        assert_eq!(DataType::Null.label(), "NULL");
    }

    // --- Cell value tests ---

    #[test]
    fn test_cell_value_display_integer() {
        assert_eq!(CellValue::Integer(42).display(), "42");
    }

    #[test]
    fn test_cell_value_display_real() {
        assert_eq!(CellValue::Real(3.25).display(), "3.250000");
    }

    #[test]
    fn test_cell_value_display_text() {
        assert_eq!(CellValue::Text("hello".to_owned()).display(), "hello");
    }

    #[test]
    fn test_cell_value_display_blob() {
        assert_eq!(CellValue::Blob(vec![1, 2, 3]).display(), "<BLOB 3 bytes>");
    }

    #[test]
    fn test_cell_value_display_null() {
        assert_eq!(CellValue::Null.display(), "NULL");
    }

    #[test]
    fn test_cell_value_parse_as_integer() {
        assert_eq!(
            CellValue::parse_as("42", &DataType::Integer),
            CellValue::Integer(42)
        );
        assert_eq!(
            CellValue::parse_as("null", &DataType::Integer),
            CellValue::Null
        );
    }

    #[test]
    fn test_cell_value_parse_as_real() {
        assert_eq!(
            CellValue::parse_as("3.25", &DataType::Real),
            CellValue::Real(3.25)
        );
    }

    #[test]
    fn test_cell_value_parse_as_text() {
        assert_eq!(
            CellValue::parse_as("hello", &DataType::Text),
            CellValue::Text("hello".to_owned())
        );
    }

    // --- Sort key tests ---

    #[test]
    fn test_sort_key_null_ordering() {
        assert!(CellValue::Null.as_sort_key() < CellValue::Integer(0).as_sort_key());
    }

    #[test]
    fn test_sort_key_integer_ordering() {
        assert!(CellValue::Integer(1).as_sort_key() < CellValue::Integer(2).as_sort_key());
    }

    #[test]
    fn test_sort_key_text_ordering() {
        assert!(
            CellValue::Text("a".to_owned()).as_sort_key()
                < CellValue::Text("b".to_owned()).as_sort_key()
        );
    }

    // --- Table tests ---

    #[test]
    fn test_table_insert_row() {
        let mut table = Table::new(
            "test",
            vec![
                ColumnDef::new("id", DataType::Integer),
                ColumnDef::new("name", DataType::Text),
            ],
        );
        let result = table.insert_row(vec![
            CellValue::Integer(1),
            CellValue::Text("Alice".to_owned()),
        ]);
        assert!(result.is_ok());
        assert_eq!(table.row_count(), 1);
    }

    #[test]
    fn test_table_insert_row_column_mismatch() {
        let mut table = Table::new("test", vec![ColumnDef::new("id", DataType::Integer)]);
        let result = table.insert_row(vec![
            CellValue::Integer(1),
            CellValue::Text("extra".to_owned()),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_table_not_null_constraint() {
        let mut table = Table::new(
            "test",
            vec![ColumnDef::new("name", DataType::Text).with_not_null()],
        );
        let result = table.insert_row(vec![CellValue::Null]);
        assert!(result.is_err());
    }

    #[test]
    fn test_table_unique_constraint() {
        let mut table = Table::new(
            "test",
            vec![ColumnDef::new("email", DataType::Text).with_unique()],
        );
        assert!(
            table
                .insert_row(vec![CellValue::Text("a@b.com".to_owned())])
                .is_ok()
        );
        assert!(
            table
                .insert_row(vec![CellValue::Text("a@b.com".to_owned())])
                .is_err()
        );
    }

    #[test]
    fn test_table_auto_increment() {
        let mut table = Table::new(
            "test",
            vec![
                ColumnDef::new("id", DataType::Integer)
                    .with_primary_key()
                    .with_auto_increment(),
                ColumnDef::new("name", DataType::Text),
            ],
        );
        let _ = table.insert_row(vec![CellValue::Null, CellValue::Text("A".to_owned())]);
        let _ = table.insert_row(vec![CellValue::Null, CellValue::Text("B".to_owned())]);
        assert_eq!(
            table.rows.first().and_then(|r| r.first()),
            Some(&CellValue::Integer(1))
        );
        assert_eq!(
            table.rows.get(1).and_then(|r| r.first()),
            Some(&CellValue::Integer(2))
        );
    }

    #[test]
    fn test_table_column_index() {
        let table = Table::new(
            "test",
            vec![
                ColumnDef::new("id", DataType::Integer),
                ColumnDef::new("name", DataType::Text),
            ],
        );
        assert_eq!(table.column_index("id"), Some(0));
        assert_eq!(table.column_index("NAME"), Some(1)); // case insensitive
        assert_eq!(table.column_index("missing"), None);
    }

    #[test]
    fn test_table_delete_where() {
        let mut table = Table::new("test", vec![ColumnDef::new("v", DataType::Integer)]);
        let _ = table.insert_row(vec![CellValue::Integer(1)]);
        let _ = table.insert_row(vec![CellValue::Integer(2)]);
        let _ = table.insert_row(vec![CellValue::Integer(3)]);
        let deleted = table.delete_where(0, &FilterOp::Equal, &CellValue::Integer(2));
        assert_eq!(deleted, 1);
        assert_eq!(table.row_count(), 2);
    }

    #[test]
    fn test_table_update_where() {
        let mut table = Table::new("test", vec![ColumnDef::new("v", DataType::Integer)]);
        let _ = table.insert_row(vec![CellValue::Integer(1)]);
        let _ = table.insert_row(vec![CellValue::Integer(2)]);
        let updated = table.update_where(
            0,
            &CellValue::Integer(99),
            0,
            &FilterOp::Equal,
            &CellValue::Integer(1),
        );
        assert_eq!(updated, 1);
        assert_eq!(
            table.rows.first().and_then(|r| r.first()),
            Some(&CellValue::Integer(99))
        );
    }

    // --- Database tests ---

    #[test]
    fn test_database_create_table() {
        let mut db = Database::new("test.db");
        let table = Table::new("t1", vec![ColumnDef::new("id", DataType::Integer)]);
        assert!(db.create_table(table).is_ok());
        assert_eq!(db.table_names().len(), 1);
    }

    #[test]
    fn test_database_create_duplicate_table() {
        let mut db = Database::new("test.db");
        let _ = db.create_table(Table::new("t1", vec![]));
        assert!(db.create_table(Table::new("t1", vec![])).is_err());
    }

    #[test]
    fn test_database_drop_table() {
        let mut db = Database::new("test.db");
        let _ = db.create_table(Table::new("t1", vec![]));
        assert!(db.drop_table("t1").is_ok());
        assert!(db.table_names().is_empty());
    }

    #[test]
    fn test_database_drop_nonexistent() {
        let mut db = Database::new("test.db");
        assert!(db.drop_table("missing").is_err());
    }

    #[test]
    fn test_database_sample() {
        let db = Database::sample();
        assert_eq!(db.tables.len(), 3);
        assert!(db.find_table("users").is_some());
        assert!(db.find_table("products").is_some());
        assert!(db.find_table("orders").is_some());
        assert!(!db.foreign_keys.is_empty());
    }

    // --- Filter tests ---

    #[test]
    fn test_filter_equal() {
        assert!(matches_filter(
            &CellValue::Integer(5),
            &FilterOp::Equal,
            &CellValue::Integer(5)
        ));
        assert!(!matches_filter(
            &CellValue::Integer(5),
            &FilterOp::Equal,
            &CellValue::Integer(6)
        ));
    }

    #[test]
    fn test_filter_not_equal() {
        assert!(matches_filter(
            &CellValue::Integer(5),
            &FilterOp::NotEqual,
            &CellValue::Integer(6)
        ));
    }

    #[test]
    fn test_filter_less_than() {
        assert!(matches_filter(
            &CellValue::Integer(3),
            &FilterOp::LessThan,
            &CellValue::Integer(5)
        ));
        assert!(!matches_filter(
            &CellValue::Integer(5),
            &FilterOp::LessThan,
            &CellValue::Integer(3)
        ));
    }

    #[test]
    fn test_filter_greater_than() {
        assert!(matches_filter(
            &CellValue::Integer(5),
            &FilterOp::GreaterThan,
            &CellValue::Integer(3)
        ));
    }

    #[test]
    fn test_filter_is_null() {
        assert!(matches_filter(
            &CellValue::Null,
            &FilterOp::IsNull,
            &CellValue::Null
        ));
        assert!(!matches_filter(
            &CellValue::Integer(1),
            &FilterOp::IsNull,
            &CellValue::Null
        ));
    }

    #[test]
    fn test_filter_is_not_null() {
        assert!(matches_filter(
            &CellValue::Integer(1),
            &FilterOp::IsNotNull,
            &CellValue::Null
        ));
        assert!(!matches_filter(
            &CellValue::Null,
            &FilterOp::IsNotNull,
            &CellValue::Null
        ));
    }

    #[test]
    fn test_filter_like() {
        let cell = CellValue::Text("Hello World".to_owned());
        assert!(matches_filter(
            &cell,
            &FilterOp::Like,
            &CellValue::Text("%world".to_owned())
        ));
        assert!(matches_filter(
            &cell,
            &FilterOp::Like,
            &CellValue::Text("hello%".to_owned())
        ));
        assert!(matches_filter(
            &cell,
            &FilterOp::Like,
            &CellValue::Text("%lo w%".to_owned())
        ));
        assert!(!matches_filter(
            &cell,
            &FilterOp::Like,
            &CellValue::Text("xyz%".to_owned())
        ));
    }

    #[test]
    fn test_like_underscore_wildcard() {
        assert!(simple_like_match("abc", "a_c"));
        assert!(!simple_like_match("ac", "a_c"));
    }

    // --- SQL Tokenizer tests ---

    #[test]
    fn test_tokenize_select() {
        let tokens = tokenize_sql("SELECT * FROM users");
        let non_ws: Vec<_> = tokens
            .into_iter()
            .filter(|t| *t != SqlToken::Whitespace)
            .collect();
        assert_eq!(non_ws.len(), 4);
        assert_eq!(non_ws[0], SqlToken::Keyword("SELECT".to_owned()));
        assert_eq!(non_ws[1], SqlToken::Star);
        assert_eq!(non_ws[2], SqlToken::Keyword("FROM".to_owned()));
        assert_eq!(non_ws[3], SqlToken::Identifier("users".to_owned()));
    }

    #[test]
    fn test_tokenize_string_literal() {
        let tokens = tokenize_sql("'hello world'");
        let non_ws: Vec<_> = tokens
            .into_iter()
            .filter(|t| *t != SqlToken::Whitespace)
            .collect();
        assert_eq!(non_ws.len(), 1);
        assert_eq!(non_ws[0], SqlToken::StringLiteral("hello world".to_owned()));
    }

    #[test]
    fn test_tokenize_number() {
        let tokens = tokenize_sql("42 3.14");
        let non_ws: Vec<_> = tokens
            .into_iter()
            .filter(|t| *t != SqlToken::Whitespace)
            .collect();
        assert_eq!(non_ws.len(), 2);
        assert_eq!(non_ws[0], SqlToken::NumberLiteral("42".to_owned()));
        assert_eq!(non_ws[1], SqlToken::NumberLiteral("3.14".to_owned()));
    }

    #[test]
    fn test_tokenize_operators() {
        let tokens = tokenize_sql("= != < > <= >=");
        let non_ws: Vec<_> = tokens
            .into_iter()
            .filter(|t| *t != SqlToken::Whitespace)
            .collect();
        assert_eq!(non_ws.len(), 6);
    }

    // --- SQL Parser tests ---

    #[test]
    fn test_parse_select_all() {
        let stmt = parse_sql("SELECT * FROM users").unwrap();
        match stmt {
            SqlStatement::Select { columns, table, .. } => {
                assert_eq!(table, "users");
                assert!(matches!(columns[0], SelectColumn::AllColumns));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_columns() {
        let stmt = parse_sql("SELECT name, age FROM users").unwrap();
        match stmt {
            SqlStatement::Select { columns, .. } => {
                assert_eq!(columns.len(), 2);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_where() {
        let stmt = parse_sql("SELECT * FROM users WHERE age > 30").unwrap();
        match stmt {
            SqlStatement::Select { where_clause, .. } => {
                let wc = where_clause.unwrap();
                assert_eq!(wc.column, "age");
                assert_eq!(wc.op, FilterOp::GreaterThan);
                assert_eq!(wc.value, "30");
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_order_by() {
        let stmt = parse_sql("SELECT * FROM users ORDER BY name DESC").unwrap();
        match stmt {
            SqlStatement::Select { order_by, .. } => {
                let (col, dir) = order_by.unwrap();
                assert_eq!(col, "name");
                assert_eq!(dir, SortDir::Descending);
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_limit() {
        let stmt = parse_sql("SELECT * FROM users LIMIT 10").unwrap();
        match stmt {
            SqlStatement::Select { limit, .. } => {
                assert_eq!(limit, Some(10));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_select_aggregate() {
        let stmt = parse_sql("SELECT COUNT(*) FROM users").unwrap();
        match stmt {
            SqlStatement::Select { columns, .. } => {
                assert!(matches!(
                    columns[0],
                    SelectColumn::Aggregate {
                        func: AggFunc::Count,
                        ..
                    }
                ));
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_insert() {
        let stmt = parse_sql("INSERT INTO users (name) VALUES ('Alice')").unwrap();
        match stmt {
            SqlStatement::Insert {
                table,
                columns,
                values,
            } => {
                assert_eq!(table, "users");
                assert_eq!(columns, vec!["name"]);
                assert_eq!(values.len(), 1);
                assert_eq!(values[0][0], "Alice");
            }
            _ => panic!("Expected INSERT"),
        }
    }

    #[test]
    fn test_parse_update() {
        let stmt = parse_sql("UPDATE users SET name = 'Bob' WHERE id = 1").unwrap();
        match stmt {
            SqlStatement::Update {
                table,
                set_clauses,
                where_clause,
            } => {
                assert_eq!(table, "users");
                assert_eq!(set_clauses.len(), 1);
                assert!(where_clause.is_some());
            }
            _ => panic!("Expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let stmt = parse_sql("DELETE FROM users WHERE id = 1").unwrap();
        match stmt {
            SqlStatement::Delete {
                table,
                where_clause,
            } => {
                assert_eq!(table, "users");
                assert!(where_clause.is_some());
            }
            _ => panic!("Expected DELETE"),
        }
    }

    #[test]
    fn test_parse_create_table() {
        let stmt =
            parse_sql("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT NOT NULL)").unwrap();
        match stmt {
            SqlStatement::CreateTable {
                name,
                columns,
                if_not_exists,
            } => {
                assert_eq!(name, "test");
                assert_eq!(columns.len(), 2);
                assert!(columns[0].primary_key);
                assert!(columns[1].not_null);
                assert!(!if_not_exists);
            }
            _ => panic!("Expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_drop_table() {
        let stmt = parse_sql("DROP TABLE IF EXISTS test").unwrap();
        match stmt {
            SqlStatement::DropTable { name, if_exists } => {
                assert_eq!(name, "test");
                assert!(if_exists);
            }
            _ => panic!("Expected DROP TABLE"),
        }
    }

    #[test]
    fn test_parse_empty_query() {
        assert!(parse_sql("").is_err());
    }

    // --- SQL Execution tests ---

    #[test]
    fn test_execute_select_all() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT * FROM users").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert_eq!(result.columns.len(), 5);
        assert_eq!(result.rows.len(), 10);
    }

    #[test]
    fn test_execute_select_with_where() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT * FROM users WHERE age > 30").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert!(result.rows.len() < 10);
        // All returned rows should have age > 30
        for row in &result.rows {
            if let Some(CellValue::Integer(age)) = row.get(3) {
                assert!(*age > 30);
            }
        }
    }

    #[test]
    fn test_execute_select_order_by() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT * FROM users ORDER BY age ASC").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        let ages: Vec<i64> = result
            .rows
            .iter()
            .filter_map(|r| {
                if let Some(CellValue::Integer(v)) = r.get(3) {
                    Some(*v)
                } else {
                    None
                }
            })
            .collect();
        for w in ages.windows(2) {
            assert!(w[0] <= w[1], "Should be sorted ascending");
        }
    }

    #[test]
    fn test_execute_select_limit() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT * FROM users LIMIT 3").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert_eq!(result.rows.len(), 3);
    }

    #[test]
    fn test_execute_count() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT COUNT(*) FROM users").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], CellValue::Integer(10));
    }

    #[test]
    fn test_execute_sum() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT SUM(age) FROM users").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        if let CellValue::Real(sum) = &result.rows[0][0] {
            assert!(*sum > 0.0);
        }
    }

    #[test]
    fn test_execute_avg() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT AVG(score) FROM users").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
    }

    #[test]
    fn test_execute_min_max() {
        let mut db = Database::sample();
        let min_stmt = parse_sql("SELECT MIN(age) FROM users").unwrap();
        let min_result = execute_sql(&mut db, &min_stmt);
        let max_stmt = parse_sql("SELECT MAX(age) FROM users").unwrap();
        let max_result = execute_sql(&mut db, &max_stmt);
        assert!(!min_result.is_error);
        assert!(!max_result.is_error);
    }

    #[test]
    fn test_execute_insert() {
        let mut db = Database::sample();
        let stmt =
            parse_sql("INSERT INTO users (name, email, age) VALUES ('Zoe', 'zoe@example.com', 26)")
                .unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert_eq!(result.affected_rows, 1);
        assert_eq!(db.find_table("users").unwrap().row_count(), 11);
    }

    #[test]
    fn test_execute_update() {
        let mut db = Database::sample();
        let stmt = parse_sql("UPDATE users SET age = 99 WHERE name = 'Alice'").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert_eq!(result.affected_rows, 1);
    }

    #[test]
    fn test_execute_delete() {
        let mut db = Database::sample();
        let stmt = parse_sql("DELETE FROM users WHERE name = 'Alice'").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert_eq!(result.affected_rows, 1);
        assert_eq!(db.find_table("users").unwrap().row_count(), 9);
    }

    #[test]
    fn test_execute_create_table() {
        let mut db = Database::new("test.db");
        let stmt = parse_sql("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert!(db.find_table("test").is_some());
    }

    #[test]
    fn test_execute_drop_table() {
        let mut db = Database::sample();
        let stmt = parse_sql("DROP TABLE users").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert!(db.find_table("users").is_none());
    }

    #[test]
    fn test_execute_select_nonexistent_table() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT * FROM nonexistent").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(result.is_error);
    }

    #[test]
    fn test_execute_group_by() {
        let mut db = Database::sample();
        let stmt = parse_sql("SELECT category, COUNT(*) FROM products GROUP BY category").unwrap();
        let result = execute_sql(&mut db, &stmt);
        assert!(!result.is_error);
        assert!(result.rows.len() >= 2); // At least Electronics and Furniture
    }

    // --- Export tests ---

    #[test]
    fn test_export_csv() {
        let mut table = Table::new(
            "test",
            vec![
                ColumnDef::new("id", DataType::Integer),
                ColumnDef::new("name", DataType::Text),
            ],
        );
        let _ = table.insert_row(vec![
            CellValue::Integer(1),
            CellValue::Text("Alice".to_owned()),
        ]);
        let csv = export_csv(&table);
        assert!(csv.contains("id,name"));
        assert!(csv.contains("1,\"Alice\""));
    }

    #[test]
    fn test_export_json() {
        let mut table = Table::new(
            "test",
            vec![
                ColumnDef::new("id", DataType::Integer),
                ColumnDef::new("name", DataType::Text),
            ],
        );
        let _ = table.insert_row(vec![
            CellValue::Integer(1),
            CellValue::Text("Alice".to_owned()),
        ]);
        let json = export_json(&table);
        assert!(json.contains("\"id\": 1"));
        assert!(json.contains("\"name\": \"Alice\""));
    }

    #[test]
    fn test_export_sql_inserts() {
        let mut table = Table::new(
            "test",
            vec![
                ColumnDef::new("id", DataType::Integer),
                ColumnDef::new("name", DataType::Text),
            ],
        );
        let _ = table.insert_row(vec![
            CellValue::Integer(1),
            CellValue::Text("Alice".to_owned()),
        ]);
        let sql = export_sql_inserts(&table);
        // Identifiers are quoted, so a name that collides with a keyword or
        // contains a space still names the right object.
        assert!(sql.contains("INSERT INTO \"test\" (\"id\", \"name\")"));
        assert!(sql.contains("1, 'Alice'"));
    }

    /// A one-row table whose *column name* is chosen by the caller. Column
    /// names are not privileged data: `import_csv` takes them straight from
    /// the header line of a file the user opened.
    fn table_with_column_named(col: &str) -> Table {
        let mut table = Table::new(
            "test",
            vec![
                ColumnDef::new("id", DataType::Integer),
                ColumnDef::new(col, DataType::Text),
            ],
        );
        let _ = table.insert_row(vec![
            CellValue::Integer(1),
            CellValue::Text("Alice".to_owned()),
        ]);
        table
    }

    #[test]
    fn a_hostile_column_name_cannot_forge_a_csv_column() {
        let csv = export_csv(&table_with_column_named("name,forged"));
        let header = csv.lines().next().expect("header");
        // Walk the header the way a reader does, so an escaped comma inside
        // a quoted name is not mistaken for a real separator.
        let mut fields = 1;
        let mut in_quotes = false;
        for c in header.chars() {
            match c {
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => fields += 1,
                _ => {}
            }
        }
        assert_eq!(fields, 2, "column name forged a third column: {header}");
    }

    /// Count JSON string tokens, honouring backslash escapes.
    ///
    /// A bare `json.contains("\"admin\":")` cannot be used here: correctly
    /// escaped output *does* contain that substring, preceded by a backslash
    /// that makes it inert. The question is how many strings a parser sees.
    fn json_string_token_count(text: &str) -> usize {
        let mut count: usize = 0;
        let mut in_string = false;
        let mut escaped = false;
        for c in text.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_string = false;
                }
            } else if c == '"' {
                in_string = true;
                count = count.saturating_add(1);
            }
        }
        count
    }

    #[test]
    fn a_hostile_column_name_cannot_forge_a_json_key() {
        let json = export_json(&table_with_column_named("name\", \"admin"));
        // Three strings: the two column names and the one Text value. Left
        // unescaped the payload would split into four.
        assert_eq!(
            json_string_token_count(&json),
            3,
            "column name forged a key: {json}"
        );
    }

    #[test]
    fn a_value_ending_in_a_backslash_does_not_truncate_the_json() {
        let mut table = Table::new("test", vec![ColumnDef::new("path", DataType::Text)]);
        let _ = table.insert_row(vec![CellValue::Text("C:\\".to_owned())]);
        let json = export_json(&table);
        // The backslash must be escaped, so the string is still terminated by
        // a real closing quote and the object still closes.
        assert!(
            json.contains("\"path\": \"C:\\\\\""),
            "backslash not escaped: {json}"
        );
        assert!(json.trim_end().ends_with(']'), "document truncated: {json}");
    }

    /// Count statement terminators the way a SQL lexer does: semicolons that
    /// are outside both a `'...'` literal and a `"..."` identifier.
    ///
    /// Counting every `;` would flag correctly-quoted output, because the
    /// hostile payload legitimately *contains* one -- inertly, inside an
    /// identifier.
    fn sql_statement_count(text: &str) -> usize {
        let mut count: usize = 0;
        let mut in_ident = false;
        let mut in_literal = false;
        for c in text.chars() {
            match c {
                '"' if !in_literal => in_ident = !in_ident,
                '\'' if !in_ident => in_literal = !in_literal,
                ';' if !in_ident && !in_literal => count = count.saturating_add(1),
                _ => {}
            }
        }
        count
    }

    #[test]
    fn a_hostile_identifier_cannot_forge_a_sql_statement() {
        let sql = export_sql_inserts(&table_with_column_named(
            "name) VALUES (1, 'x'); DROP TABLE t--",
        ));
        assert_eq!(
            sql_statement_count(&sql),
            1,
            "identifier forged a statement: {sql}"
        );
        // The payload survives verbatim as an identifier, with its `"` (none
        // here) doubled -- it is data, not syntax.
        assert!(
            sql.contains("\"name) VALUES (1, 'x'); DROP TABLE t--\""),
            "identifier not quoted: {sql}"
        );
    }

    #[test]
    fn a_csv_export_can_be_imported_back() {
        // Every field here is one the old line-oriented importer mangled: a
        // comma and a newline in a column name, and the same inside a value.
        let mut table = Table::new(
            "t",
            vec![
                ColumnDef::new("first,second", DataType::Text),
                ColumnDef::new("with \"quotes\"", DataType::Text),
                ColumnDef::new("two\nlines", DataType::Text),
            ],
        );
        let _ = table.insert_row(vec![
            CellValue::Text("a,b".to_owned()),
            CellValue::Text("say \"hi\"".to_owned()),
            CellValue::Text("line1\nline2".to_owned()),
        ]);

        let csv = export_csv(&table);
        let back = import_csv("t", &csv).expect("re-import");

        let names: Vec<&str> = back.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["first,second", "with \"quotes\"", "two\nlines"]);
        assert_eq!(back.row_count(), 1, "record count changed: {csv}");
        let row: Vec<String> = back.rows[0].iter().map(CellValue::display).collect();
        assert_eq!(row, ["a,b", "say \"hi\"", "line1\nline2"]);
    }

    #[test]
    fn a_quoted_csv_field_keeps_its_spaces() {
        // Quoting is how a writer says the whitespace is data; only unquoted
        // fields get the lenient trim.
        let table = import_csv("t", "a,b\n\"  padded  \",  bare  ").expect("import");
        let row: Vec<String> = table.rows[0].iter().map(CellValue::display).collect();
        assert_eq!(row, ["  padded  ", "bare"]);
    }

    #[test]
    fn a_quote_in_an_identifier_is_doubled() {
        let sql = export_sql_inserts(&table_with_column_named("na\"me"));
        assert!(sql.contains("\"na\"\"me\""), "quote not doubled: {sql}");
        assert_eq!(sql_statement_count(&sql), 1, "unbalanced quoting: {sql}");
    }

    // --- Import tests ---

    #[test]
    fn test_import_csv_basic() {
        let csv = "name,age\nAlice,30\nBob,25";
        let table = import_csv("imported", csv).unwrap();
        assert_eq!(table.name, "imported");
        assert_eq!(table.col_count(), 2);
        assert_eq!(table.row_count(), 2);
    }

    #[test]
    fn test_import_csv_type_inference() {
        let csv = "id,score\n1,95.5\n2,82.3";
        let table = import_csv("data", csv).unwrap();
        // id column should be inferred as Integer
        assert_eq!(table.columns[0].data_type, DataType::Integer);
        // score column should be inferred as Real
        assert_eq!(table.columns[1].data_type, DataType::Real);
    }

    #[test]
    fn test_import_csv_quoted() {
        let csv = "name,bio\n\"Alice\",\"She said \"\"hello\"\"\"";
        let table = import_csv("quoted", csv).unwrap();
        assert_eq!(table.row_count(), 1);
    }

    #[test]
    fn test_import_csv_empty() {
        assert!(import_csv("empty", "").is_err());
    }

    // --- Constraint describe tests ---

    #[test]
    fn test_constraint_describe_pk() {
        let c = ColumnConstraints {
            primary_key: true,
            ..Default::default()
        };
        assert!(c.describe().contains("PK"));
    }

    #[test]
    fn test_constraint_describe_not_null() {
        let c = ColumnConstraints {
            not_null: true,
            ..Default::default()
        };
        assert!(c.describe().contains("NN"));
    }

    #[test]
    fn test_constraint_describe_unique() {
        let c = ColumnConstraints {
            unique: true,
            ..Default::default()
        };
        assert!(c.describe().contains("UQ"));
    }

    // --- App tests ---

    #[test]
    fn test_app_new() {
        let app = DbViewerApp::new();
        assert_eq!(app.tabs.len(), 1);
        assert!(app.active_db_tab().is_some());
    }

    #[test]
    fn test_app_execute_query() {
        let mut app = DbViewerApp::new();
        app.sql_input = "SELECT * FROM users".to_owned();
        app.execute_query();
        assert!(app.query_result.is_some());
        let result = app.query_result.as_ref().unwrap();
        assert!(!result.is_error);
        assert_eq!(result.rows.len(), 10);
    }

    const LONG_ERROR: &str = "near \"FORM\": syntax error at column 22 — the \
        FROM clause of a SELECT must name a table that exists in the attached \
        database, and no table named \"userz\" was found; did you mean \"users\"?";

    /// An app whose results pane is showing `result`.
    fn app_showing(result: QueryResult) -> DbViewerApp {
        let mut app = DbViewerApp::new();
        app.query_result = Some(result);
        app.bottom_panel = BottomPanel::Results;
        app
    }

    /// Height of the pane the results are rendered into by the helpers below.
    const TEST_PANE_HEIGHT: f32 = 400.0;

    /// Width of that pane, for the tests that do not care how wide it is.
    const TEST_PANE_WIDTH: f32 = 1200.0;

    /// The `(y, text)` of every result-message line, and the `y` of the results
    /// table's header row.
    ///
    /// Renders the results pane on its own rather than the whole app, so that
    /// text elsewhere in the window — the status bar and the SQL editor also
    /// draw 11pt in red and green — cannot be mistaken for the message.
    fn results_pane_layout(app: &DbViewerApp) -> (Vec<(f32, String)>, Option<f32>) {
        results_pane_layout_at(app, TEST_PANE_WIDTH)
    }

    /// The same, at a caller-chosen width.
    ///
    /// Wrapping depends on how wide the host's font draws the message, so a
    /// test about wrapping has to pick its width from that rather than from a
    /// constant — see `a_long_query_error_is_wrapped_not_cut_mid_word`.
    fn results_pane_layout_at(app: &DbViewerApp, width: f32) -> (Vec<(f32, String)>, Option<f32>) {
        let mut f = Frame::new(width, TEST_PANE_HEIGHT);
        app.draw_results(&mut f, Rect::new(0.0, 0.0, width, TEST_PANE_HEIGHT));
        let cmds = f.commands();
        let lines = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y,
                    text,
                    font_size,
                    color,
                    ..
                } if (font_size - RESULT_MSG_FONT_SIZE).abs() < 0.01
                    && (*color == RED || *color == GREEN) =>
                {
                    Some((*y, text.clone()))
                }
                _ => None,
            })
            .collect();
        // The header row is the only 20px-tall fill in the pane.
        let header_y = cmds.iter().find_map(|c| match c {
            RenderCommand::FillRect { y, height, .. } if (height - 20.0).abs() < 0.01 => Some(*y),
            _ => None,
        });
        (lines, header_y)
    }

    #[test]
    fn a_long_query_error_is_wrapped_not_cut_mid_word() {
        // `RenderCommand::Text` clips at `max_width`, so the error the engine
        // reported used to reach the user as its first line and no more.
        //
        // The pane is sized from what the message *measures*, not from a
        // constant, because the constant went stale and took the test with
        // it. This was written against a 1200px pane, which leaves the
        // message 1176px, back when the toolkit measured with the built-in
        // 8x16 bitmap font and these 184 characters came to ~1470px. Once
        // `SystemFont` began resolving a real proportional host face the same
        // string measured 988px, fit on one line, and the test failed —
        // reporting a wrapping bug where there was none, purely because the
        // font got narrower. Asking for half the message's own width forces
        // an overflow on any face, however wide it draws.
        let msg_width = text::measure(LONG_ERROR, RESULT_MSG_FONT_SIZE, FontWeightHint::Bold) / 2.0;
        let app = app_showing(QueryResult::error(LONG_ERROR));
        // `render_results` gives the message the pane's width less 24.
        let lines = results_pane_layout_at(&app, msg_width + 24.0).0;
        assert!(
            lines.len() > 1,
            "the error was drawn as {} command(s) in {msg_width}px",
            lines.len()
        );
        let drawn: String = lines
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for word in LONG_ERROR.split_whitespace() {
            assert!(drawn.contains(word), "the error lost the word {word:?}");
        }
    }

    #[test]
    fn a_wordy_message_does_not_crowd_out_the_results() {
        // The message is bounded by what is left of the pane after the table
        // it introduces, and the cut is marked rather than silent.
        let mut result = QueryResult::with_data(
            vec!["id".to_owned(), "name".to_owned()],
            vec![vec![CellValue::Integer(1), CellValue::Text("a".to_owned())]],
        );
        result.message = "word ".repeat(2000);
        let app = app_showing(result);

        let (lines, header_y) = results_pane_layout(&app);
        let last = lines.last().map(|(_, t)| t.clone()).unwrap_or_default();
        assert!(
            last.ends_with('…'),
            "the message was cut without a mark: {last:?}"
        );
        assert!(
            lines.len() <= RESULT_MSG_MAX_LINES_WITH_TABLE,
            "the message took {} lines over a results table",
            lines.len()
        );

        // The column headers are still below the message, not under it.
        let header_y = header_y.expect("the results pane drew no column headers");
        let message_bottom = lines
            .iter()
            .map(|(y, _)| y + RESULT_MSG_LINE_HEIGHT)
            .fold(f32::MIN, f32::max);
        assert!(
            header_y + 0.01 >= message_bottom,
            "the headers at {header_y} sit inside the message, which ends at \
             {message_bottom}"
        );
    }

    #[test]
    fn a_one_line_message_leaves_the_table_where_it_was() {
        // The table follows the message now, so check the common case did not
        // shift: a short message must still put the headers at y + 22.
        let app = app_showing(QueryResult::with_data(
            vec!["id".to_owned()],
            vec![vec![CellValue::Integer(1)]],
        ));
        let (lines, header_y) = results_pane_layout(&app);
        assert_eq!(lines.len(), 1, "the short message did not fit on one line");
        let header_y = header_y.expect("the results pane drew no column headers");
        // The pane is rendered at y = 0, and the header row has always been
        // 22px down from the top of it.
        assert!(
            (header_y - 22.0).abs() < 0.01,
            "the header row moved: {header_y} vs the expected 22"
        );
    }

    #[test]
    fn test_app_execute_bad_query() {
        let mut app = DbViewerApp::new();
        app.sql_input = "INVALID SQL".to_owned();
        app.execute_query();
        assert!(app.query_result.as_ref().unwrap().is_error);
    }

    #[test]
    fn test_app_execute_empty_query() {
        let mut app = DbViewerApp::new();
        app.sql_input.clear();
        app.execute_query();
        assert!(app.query_result.as_ref().unwrap().is_error);
    }

    #[test]
    fn test_app_add_tab() {
        let mut app = DbViewerApp::new();
        app.add_tab("new.db");
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_tab, 1);
    }

    #[test]
    fn test_app_close_tab() {
        let mut app = DbViewerApp::new();
        app.add_tab("second.db");
        app.close_tab(0);
        assert_eq!(app.tabs.len(), 1);
    }

    #[test]
    fn test_app_close_last_tab() {
        let mut app = DbViewerApp::new();
        app.close_tab(0);
        assert_eq!(app.tabs.len(), 1); // Should not close last tab
    }

    #[test]
    fn test_app_select_table() {
        let mut app = DbViewerApp::new();
        app.select_table("products");
        assert_eq!(
            app.active_db_tab().unwrap().selected_table.as_deref(),
            Some("products")
        );
    }

    #[test]
    fn test_app_toggle_sort() {
        let mut app = DbViewerApp::new();
        app.toggle_sort(0);
        let sort = app.active_db_tab().unwrap().sort_state.as_ref().unwrap();
        assert_eq!(sort.column_idx, 0);
        assert_eq!(sort.direction, SortDir::Ascending);

        app.toggle_sort(0);
        let sort = app.active_db_tab().unwrap().sort_state.as_ref().unwrap();
        assert_eq!(sort.direction, SortDir::Descending);
    }

    #[test]
    fn test_app_pagination() {
        let mut app = DbViewerApp::new();
        assert_eq!(app.active_db_tab().unwrap().page, 0);
        app.next_page(); // Only 10 rows with PAGE_SIZE=50, no change
        app.prev_page();
        assert_eq!(app.active_db_tab().unwrap().page, 0);
    }

    #[test]
    fn test_app_add_filter() {
        let mut app = DbViewerApp::new();
        app.filter_column_idx = 3; // age
        app.filter_op_idx = 0; // Equal
        app.filter_value = "30".to_owned();
        app.add_filter();
        assert_eq!(app.active_db_tab().unwrap().filters.len(), 1);
    }

    #[test]
    fn test_app_remove_filter() {
        let mut app = DbViewerApp::new();
        app.filter_value = "test".to_owned();
        app.add_filter();
        app.remove_filter(0);
        assert!(app.active_db_tab().unwrap().filters.is_empty());
    }

    #[test]
    fn test_app_delete_row() {
        let mut app = DbViewerApp::new();
        let initial_count = app
            .active_db_tab()
            .unwrap()
            .db
            .find_table("users")
            .unwrap()
            .row_count();
        app.delete_row(0);
        let new_count = app
            .active_db_tab()
            .unwrap()
            .db
            .find_table("users")
            .unwrap()
            .row_count();
        assert_eq!(new_count, initial_count - 1);
    }

    #[test]
    fn test_app_export_csv() {
        let app = DbViewerApp::new();
        let csv = app.export_current_table(ExportFormat::Csv);
        assert!(csv.is_some());
        assert!(csv.unwrap().contains("id,name,email,age,score"));
    }

    #[test]
    fn test_app_export_json() {
        let app = DbViewerApp::new();
        let json = app.export_current_table(ExportFormat::Json);
        assert!(json.is_some());
        assert!(json.unwrap().contains("\"name\""));
    }

    #[test]
    fn test_app_export_sql() {
        let app = DbViewerApp::new();
        let sql = app.export_current_table(ExportFormat::SqlInserts);
        assert!(sql.is_some());
        assert!(sql.unwrap().contains("INSERT INTO \"users\""));
    }

    #[test]
    fn test_app_import_csv() {
        let mut app = DbViewerApp::new();
        let csv = "city,pop\nNY,8000000\nLA,4000000";
        assert!(app.import_csv_data("cities", csv).is_ok());
        assert!(
            app.active_db_tab()
                .unwrap()
                .db
                .find_table("cities")
                .is_some()
        );
    }

    #[test]
    fn test_app_toggle_favorite() {
        let mut app = DbViewerApp::new();
        app.sql_input = "SELECT 1".to_owned();
        app.execute_query();
        assert!(!app.history[0].favorite);
        app.toggle_favorite(0);
        assert!(app.history[0].favorite);
    }

    #[test]
    fn test_app_history() {
        let mut app = DbViewerApp::new();
        app.sql_input = "SELECT * FROM users".to_owned();
        app.execute_query();
        app.sql_input = "SELECT * FROM products".to_owned();
        app.execute_query();
        assert_eq!(app.history.len(), 2);
    }

    #[test]
    fn test_app_render() {
        let app = DbViewerApp::new();
        let cmds = app.render(1200.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_results_panel() {
        let mut app = DbViewerApp::new();
        app.sql_input = "SELECT * FROM users".to_owned();
        app.execute_query();
        let cmds = app.render(1200.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_current_table_data_with_filter() {
        let mut app = DbViewerApp::new();
        app.filter_column_idx = 3; // age
        app.filter_op_idx = 5; // GreaterOrEqual (FilterOp::all() index)
        app.filter_value = "35".to_owned();
        app.add_filter();
        let (_, rows) = app.active_db_tab().unwrap().current_table_data().unwrap();
        for (_, row) in &rows {
            if let Some(CellValue::Integer(age)) = row.get(3) {
                assert!(*age >= 35);
            }
        }
    }

    #[test]
    fn test_current_table_data_with_sort() {
        let mut app = DbViewerApp::new();
        app.toggle_sort(3); // Sort by age ascending
        let (_, rows) = app.active_db_tab().unwrap().current_table_data().unwrap();
        let ages: Vec<i64> = rows
            .iter()
            .filter_map(|(_, r)| {
                if let Some(CellValue::Integer(v)) = r.get(3) {
                    Some(*v)
                } else {
                    None
                }
            })
            .collect();
        for w in ages.windows(2) {
            assert!(w[0] <= w[1]);
        }
    }

    // --- Tree node tests ---

    #[test]
    fn test_build_tree_nodes() {
        let db = Database::sample();
        let nodes = build_tree_nodes(&db);
        assert!(!nodes.is_empty());
        // Should have headers for tables, indexes, views, triggers
        assert!(nodes.iter().any(|n| n.kind == TreeNodeKind::TablesHeader));
        assert!(nodes.iter().any(|n| n.kind == TreeNodeKind::IndexesHeader));
        assert!(nodes.iter().any(|n| n.kind == TreeNodeKind::ViewsHeader));
        assert!(nodes.iter().any(|n| n.kind == TreeNodeKind::TriggersHeader));
    }

    /// A cell wider than its column is cut by the renderer, which measures it,
    /// rather than by a character count here, which cannot. The count was 30
    /// for every column, so a narrow column overflowed and a wide one was cut
    /// with room to spare — and 30 was compared against `len()`, which is
    /// bytes, so a column of accented text lost a third of its characters.
    #[test]
    fn a_cell_is_bounded_by_its_column_not_by_a_character_count() {
        let mut app = DbViewerApp::new();
        app.query_result = Some(QueryResult {
            columns: vec!["note".to_string()],
            rows: vec![vec![CellValue::Text(
                "Ünterstützung für Änderungen an Verträgen".to_string(),
            )]],
            message: String::new(),
            affected_rows: 0,
            is_error: false,
        });
        let mut f = Frame::new(600.0, 400.0);
        app.draw_results(&mut f, Rect::new(0.0, 0.0, 600.0, 400.0));
        let cell = f
            .commands()
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::Text {
                    text, max_width, ..
                } if text.starts_with('\u{dc}') => Some((text.clone(), *max_width)),
                _ => None,
            })
            .expect("the cell is drawn");
        assert_eq!(
            cell.0, "Ünterstützung für Änderungen an Verträgen",
            "the cell text reaches the renderer whole: {cell:?}"
        );
        assert!(
            cell.1.is_some(),
            "and the renderer is told the column width: {cell:?}"
        );
    }

    // --- Filter op tests ---

    #[test]
    fn test_filter_op_labels() {
        assert_eq!(FilterOp::Equal.label(), "=");
        assert_eq!(FilterOp::NotEqual.label(), "!=");
        assert_eq!(FilterOp::Like.label(), "LIKE");
        assert_eq!(FilterOp::IsNull.label(), "IS NULL");
    }

    #[test]
    fn test_filter_op_all() {
        assert_eq!(FilterOp::all().len(), 9);
    }

    // --- Aggregate function tests ---

    #[test]
    fn test_agg_func_from_keyword() {
        assert_eq!(AggFunc::from_keyword("COUNT"), Some(AggFunc::Count));
        assert_eq!(AggFunc::from_keyword("sum"), Some(AggFunc::Sum));
        assert_eq!(AggFunc::from_keyword("avg"), Some(AggFunc::Avg));
        assert_eq!(AggFunc::from_keyword("MIN"), Some(AggFunc::Min));
        assert_eq!(AggFunc::from_keyword("MAX"), Some(AggFunc::Max));
        assert_eq!(AggFunc::from_keyword("INVALID"), None);
    }

    #[test]
    fn test_agg_func_labels() {
        assert_eq!(AggFunc::Count.label(), "COUNT");
        assert_eq!(AggFunc::Sum.label(), "SUM");
    }

    // --- Column def builder tests ---

    #[test]
    fn test_column_def_builder() {
        let col = ColumnDef::new("id", DataType::Integer)
            .with_primary_key()
            .with_auto_increment();
        assert!(col.constraints.primary_key);
        assert!(col.constraints.auto_increment);
        assert!(col.constraints.not_null); // PK implies NN
    }

    #[test]
    fn test_column_def_default() {
        let col = ColumnDef::new("score", DataType::Real).with_default("0.0");
        assert_eq!(col.constraints.default_value.as_deref(), Some("0.0"));
    }

    // --- Bottom panel tests ---

    #[test]
    fn test_bottom_panel_labels() {
        assert_eq!(BottomPanel::SqlEditor.label(), "SQL Editor");
        assert_eq!(BottomPanel::Results.label(), "Results");
        assert_eq!(BottomPanel::Schema.label(), "Schema");
        assert_eq!(BottomPanel::Diagram.label(), "Diagram");
    }

    #[test]
    fn test_bottom_panel_all() {
        assert_eq!(BottomPanel::all().len(), 4);
    }

    // -----------------------------------------------------------------------
    // The window: geometry
    //
    // Everything below this line exists because the database browser had no
    // window. `main` built a sample database, drew one frame into a `Vec`,
    // asserted the `Vec` was not empty and dropped it, so the picture was
    // never wrong about anything -- nobody looked at it and nothing could be
    // pressed. These tests ask the picture the questions a user would ask it
    // with a mouse.
    // -----------------------------------------------------------------------

    /// The window sizes every geometry sweep is run at.
    ///
    /// Not a grid for its own sake: each entry breaks a different assumption --
    /// too narrow for the 220-point sidebar, too short for the 140-point bottom
    /// panel, too narrow for six toolbar buttons, wider than it is tall and the
    /// other way round, and three that are not really windows at all.
    const GRID: [(f32, f32); 12] = [
        (1200.0, 800.0),
        (1920.0, 1080.0),
        (1024.0, 768.0),
        (640.0, 480.0),
        (420.0, 900.0),
        (1600.0, 200.0),
        (380.0, 320.0),
        (300.0, 240.0),
        (200.0, 160.0),
        (120.0, 90.0),
        (40.0, 30.0),
        (2.0, 2.0),
    ];

    /// The browser as the window opens it, with a table chosen -- which is the
    /// state nearly every control in the program needs to have anything to act
    /// on. `DbViewerApp::new` selects one already; this says so out loud so a
    /// change to `new` does not quietly empty half the sweeps below.
    fn wired() -> DbViewerApp {
        let mut app = DbViewerApp::new();
        app.select_table("users");
        assert!(
            app.active_db_tab()
                .and_then(|t| t.selected_table.clone())
                .is_some(),
            "the sample database has no `users` table to select"
        );
        app
    }

    /// The states every geometry sweep is run over.
    ///
    /// A window is only as right as its worst state. Sweeping the default state
    /// proves the default state: it is the pane showing a paragraph of error,
    /// the grid sorted so screen order and table order disagree, the sidebar
    /// carrying a filter builder as tall as the tree, and the tab strip holding
    /// more tabs than fit that have somewhere to go wrong.
    fn states() -> Vec<(&'static str, DbViewerApp)> {
        let mut out: Vec<(&'static str, DbViewerApp)> = Vec::new();

        out.push(("as opened", DbViewerApp::new()));
        out.push(("a table selected", wired()));

        let mut empty = DbViewerApp::new();
        empty.add_tab("empty");
        out.push(("an empty database", empty));

        let mut sorted = wired();
        sorted.toggle_sort(3);
        sorted.toggle_sort(3);
        out.push(("sorted descending", sorted));

        let mut filtered = wired();
        filtered.filter_column_idx = 1;
        filtered.filter_op_idx = 0;
        filtered.filter_value = String::from("nobody at all");
        filtered.add_filter();
        filtered.show_filter_builder = true;
        out.push(("filtered down to nothing", filtered));

        let mut many_filters = wired();
        many_filters.show_filter_builder = true;
        for _ in 0..8 {
            many_filters.filter_value = String::from("x");
            many_filters.add_filter();
        }
        out.push(("the filter builder, eight filters deep", many_filters));

        let mut results = wired();
        results.sql_input = String::from("SELECT * FROM users");
        results.execute_query();
        out.push(("a result showing", results));

        let mut wordy = wired();
        wordy.query_result = Some(QueryResult::error(&"word ".repeat(400)));
        wordy.bottom_panel = BottomPanel::Results;
        out.push(("a result whose message will not stop", wordy));

        let mut schema = wired();
        schema.bottom_panel = BottomPanel::Schema;
        out.push(("the schema panel", schema));

        let mut diagram = wired();
        diagram.bottom_panel = BottomPanel::Diagram;
        out.push(("the diagram panel", diagram));

        let mut tabs = wired();
        for i in 0..8 {
            tabs.add_tab(&format!("a database with a very long name {i}"));
        }
        out.push(("more tabs than fit", tabs));

        let mut history = wired();
        for i in 0..20 {
            history.sql_input = format!("SELECT {i} FROM users WHERE name LIKE '%{i}%'");
            history.execute_query();
        }
        history.bottom_panel = BottomPanel::SqlEditor;
        history.toggle_favorite(0);
        out.push(("a long history", history));

        let mut paged = wired();
        for i in 0..300 {
            if let Some(tab) = paged.active_db_tab_mut()
                && let Some(table) = tab.db.find_table_mut("users")
            {
                let row: Vec<CellValue> = table
                    .columns
                    .iter()
                    .map(|c| CellValue::parse_as(&format!("{i}"), &c.data_type))
                    .collect();
                table.rows.push(row);
            }
        }
        paged.next_page();
        out.push(("three hundred rows, on the second page", paged));

        out
    }

    /// Every run of text the frame drew, as `(text, x, y, size, max_width)`.
    fn text_runs(frame: &Frame<Target>) -> Vec<(String, f32, f32, f32, Option<f32>)> {
        frame
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    max_width,
                    ..
                } => Some((text.clone(), *x, *y, *font_size, *max_width)),
                _ => None,
            })
            .collect()
    }

    /// Every clip the frame pushed, as a rectangle.
    fn clips(frame: &Frame<Target>) -> Vec<Rect> {
        frame
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            })
            .collect()
    }

    /// The clip that was in force at each command: the intersection of the
    /// whole stack, or the window if nothing was clipped.
    ///
    /// The intersection matters and an "innermost clip wins" reading would be
    /// wrong: clips nest, and a pass that pushed a second, wider clip inside a
    /// narrow one would otherwise be measured against the wider of the two.
    /// An unclipped command is measured against the window itself, because
    /// something drawn outside a window nobody clipped is just as lost as
    /// something that escaped a clip.
    fn walk_clips<T>(
        frame: &Frame<Target>,
        size: (f32, f32),
        mut pick: impl FnMut(&RenderCommand, Rect) -> Option<T>,
    ) -> Vec<T> {
        let window = Rect::new(0.0, 0.0, size.0, size.1);
        let mut stack: Vec<Rect> = Vec::new();
        let mut out = Vec::new();
        for c in frame.commands() {
            match c {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => {
                    let next = Rect::new(*x, *y, *width, *height);
                    let merged = stack
                        .last()
                        .map_or(next, |outer| outer.intersect(next).unwrap_or(Rect::EMPTY));
                    stack.push(merged);
                }
                RenderCommand::PopClip => {
                    stack.pop();
                }
                other => {
                    if let Some(v) = pick(other, stack.last().copied().unwrap_or(window)) {
                        out.push(v);
                    }
                }
            }
        }
        out
    }

    /// Every run of text, paired with the clip that was in force when it was
    /// drawn.
    fn text_runs_clipped(
        frame: &Frame<Target>,
        size: (f32, f32),
    ) -> Vec<(String, f32, f32, f32, Option<f32>, Rect)> {
        walk_clips(frame, size, |c, clip| match c {
            RenderCommand::Text {
                x,
                y,
                text,
                font_size,
                max_width,
                ..
            } => Some((text.clone(), *x, *y, *font_size, *max_width, clip)),
            _ => None,
        })
    }

    /// Every filled box, paired with the clip that was in force when it was
    /// drawn -- the same rule `text_runs_clipped` applies to text.
    fn fills_clipped(frame: &Frame<Target>, size: (f32, f32)) -> Vec<(Rect, Rect)> {
        walk_clips(frame, size, |c, clip| match c {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } => Some((Rect::new(*x, *y, *width, *height), clip)),
            _ => None,
        })
    }

    /// Where the picture drew a given run of text, as a point inside it.
    ///
    /// This is how a control is found when the point of the test is *which*
    /// control the press reaches. Asking the frame for the rectangle of a
    /// `Target` asks the drawing pass where it thinks its own controls are, and
    /// a pass that recorded every row one row off would answer with the same
    /// offset it made the mistake with -- finding and clicking would cancel out,
    /// and the test would pass over a grid where every row deleted its
    /// neighbour. The words are the only thing a user can read.
    fn text_point(app: &DbViewerApp, size: (f32, f32), wanted: &str) -> Option<(f32, f32)> {
        let frame = app.frame(size.0, size.1);
        let found: Vec<(f32, f32)> = text_runs(&frame)
            .into_iter()
            .filter(|(text, ..)| text == wanted)
            .map(|(_, x, y, font_size, max_width)| {
                (
                    x + max_width.unwrap_or(font_size) * 0.5,
                    y + font_size * 0.5,
                )
            })
            .collect();
        // Two runs reading the same words make "press the thing that says X"
        // ambiguous, and the caller would silently get whichever came first.
        assert!(
            found.len() <= 1,
            "{} runs of text read {wanted:?}",
            found.len()
        );
        found.first().copied()
    }

    /// `text_point`, insisting the words are there.
    fn point(app: &DbViewerApp, size: (f32, f32), wanted: &str) -> (f32, f32) {
        text_point(app, size, wanted)
            .unwrap_or_else(|| panic!("nothing in the picture reads {wanted:?}"))
    }

    /// Whether the picture says something.
    fn shows(app: &DbViewerApp, size: (f32, f32), wanted: &str) -> bool {
        text_runs(&app.frame(size.0, size.1))
            .iter()
            .any(|(text, ..)| text == wanted)
    }

    /// A left press at a point, answered against a window of `size`.
    ///
    /// Named `click` rather than `press` because `guitk::probe::press` builds a
    /// *key* press, and both are used constantly here.
    fn click(app: &mut DbViewerApp, at: (f32, f32), size: (f32, f32)) {
        app.click_at(at.0, at.1, MouseButton::Left, size);
    }

    /// Press the control whose words the picture shows.
    ///
    /// The two steps are one function because splitting them borrows the app
    /// immutably to find the point and mutably to press it.
    fn click_text(app: &mut DbViewerApp, size: (f32, f32), wanted: &str) {
        let at = point(app, size, wanted);
        click(app, at, size);
    }

    /// The default window size, which most wiring tests use.
    const FULL: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    /// Press the middle of the box the drawing pass recorded for a target.
    ///
    /// Used only where the control has no words of its own to aim at -- a
    /// close box, a delete box, a star -- and never where the words exist,
    /// because then the picture, not the pass, is the thing under test.
    fn click_target(app: &mut DbViewerApp, size: (f32, f32), target: Target) {
        let rect = app
            .frame(size.0, size.1)
            .rect_of(|t| *t == target)
            .unwrap_or_else(|| panic!("{target:?} has no box in the picture"));
        click(app, rect.centre(), size);
    }

    #[test]
    fn the_window_is_painted_edge_to_edge_at_every_size() {
        for (w, h) in GRID {
            let frame = wired().frame(w, h);
            let first = frame.commands().first().expect("a frame draws something");
            match first {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => {
                    assert!(
                        x.abs() < 0.01 && y.abs() < 0.01,
                        "{w}x{h}: the background starts at ({x}, {y}), not the corner"
                    );
                    assert!(
                        (*width - w).abs() < 0.01 && (*height - h).abs() < 0.01,
                        "{w}x{h}: the background is {width}x{height} -- the compositor \
                         would show whatever was in the window before us in the rest"
                    );
                }
                other => panic!("{w}x{h}: the frame opens with {other:?}, not a background"),
            }
        }
    }

    #[test]
    fn the_frame_is_balanced_at_every_size_and_state() {
        for (name, app) in states() {
            for (w, h) in GRID {
                assert!(
                    app.frame(w, h).is_balanced(),
                    "{name} at {w}x{h}: a clip was pushed and not popped, so every \
                     later hit box is measured against the wrong rectangle"
                );
            }
        }
    }

    #[test]
    fn every_control_lies_inside_the_window() {
        for (name, app) in states() {
            for (w, h) in GRID {
                let frame = app.frame(w, h);
                for (target, rect) in frame.hits() {
                    assert!(
                        rect.x >= -0.01
                            && rect.y >= -0.01
                            && rect.right() <= w + 0.01
                            && rect.bottom() <= h + 0.01,
                        "{name} at {w}x{h}: {target:?} answers presses at {rect:?}, \
                         which is partly outside the window"
                    );
                    assert!(
                        !rect.is_empty(),
                        "{name} at {w}x{h}: {target:?} has an empty hit box"
                    );
                }
            }
        }
    }

    #[test]
    fn every_clip_lies_inside_the_window() {
        // Bounding a run to its clip is only worth anything if the clip is
        // itself inside the window: a clip that reached past the bottom edge
        // would let every run in it do the same.
        for (name, app) in states() {
            for (w, h) in GRID {
                for clip in clips(&app.frame(w, h)) {
                    assert!(
                        clip.x >= -0.01
                            && clip.y >= -0.01
                            && clip.right() <= w + 0.01
                            && clip.bottom() <= h + 0.01,
                        "{name} at {w}x{h}: a clip of {clip:?} reaches outside the window"
                    );
                }
            }
        }
    }

    #[test]
    fn every_run_of_text_is_bounded_and_inside_the_window() {
        // The old drawing pass bounded most of its runs to constants -- a
        // schema column to 180, a status reading to 200, a pagination caption
        // to `width - 200` -- which is not a bound related to the box the run
        // is in, and in a narrow window is not a bound at all.
        //
        // Sideways a run must fit its clip outright: nothing here scrolls
        // sideways, so a run hanging over the edge is a run cut in half.
        // Vertically it need only be *partly* inside, because a grid row at the
        // bottom edge is meant to be half drawn. What is forbidden is a run
        // wholly outside -- ink nobody can ever see.
        for (name, app) in states() {
            for (w, h) in GRID {
                let frame = app.frame(w, h);
                for (text, x, y, size, max_width, clip) in text_runs_clipped(&frame, (w, h)) {
                    let Some(bound) = max_width else {
                        panic!(
                            "{name} at {w}x{h}: {text:?} is drawn with no max_width, so it \
                             runs as far as the string is long and over whatever is beside it"
                        );
                    };
                    assert!(
                        bound.is_finite() && bound > 0.0,
                        "{name} at {w}x{h}: {text:?} is bounded to {bound}"
                    );
                    assert!(
                        x >= clip.x - 0.01 && x + bound <= clip.right() + 0.01,
                        "{name} at {w}x{h}: {text:?} spans {x}..{} across {clip:?}",
                        x + bound
                    );
                    assert!(
                        y + size > clip.y - 0.01 && y < clip.bottom() + 0.01,
                        "{name} at {w}x{h}: {text:?} spans {y}..{} down {clip:?}, \
                         which it misses entirely -- it is drawn where nothing can see it",
                        y + size
                    );
                }
            }
        }
    }

    #[test]
    fn nothing_is_drawn_over_the_status_line() {
        // The status line is drawn last, so it cannot be painted over -- but a
        // control reaching under it would still take the press, and the user
        // would be clicking a button they cannot see.
        for (name, app) in states() {
            for (w, h) in GRID {
                let l = Layout::solve(w, h);
                if l.status.is_empty() {
                    continue;
                }
                let frame = app.frame(w, h);
                for (target, rect) in frame.hits() {
                    // The *bottom* edge, not the top. Asking only where a
                    // control starts lets one through that starts above the
                    // line and ends below it, which is the whole shape of the
                    // bug: the visible half is what the user aims at and the
                    // hidden half is what takes the press.
                    assert!(
                        rect.bottom() < l.status.y + 0.01,
                        "{name} at {w}x{h}: {target:?} at {rect:?} reaches under the \
                         status line, which is drawn after it"
                    );
                }
                // Paint too, and not only controls. The grid's row cursor used
                // to run down past the pagination bar and paint a 26-point band
                // of row colour over it; the same cursor unchecked reaches the
                // status strip.
                for (rect, clip) in fills_clipped(&frame, (w, h)) {
                    let Some(seen) = clip.intersect(rect) else {
                        continue;
                    };
                    // The window background is the one fill allowed to reach
                    // the bottom edge: it is what the strip is drawn *on*. So is
                    // anything drawn inside the strip itself.
                    if seen.contains(l.window.w / 2.0, 0.5)
                        || l.status.intersect(seen) == Some(seen)
                    {
                        continue;
                    }
                    assert!(
                        seen.bottom() < l.status.y + 0.01,
                        "{name} at {w}x{h}: a filled box at {seen:?} is painted under the \
                         status line at {:?}",
                        l.status
                    );
                }
                // And the clips, which are where the rule is actually kept. A
                // box drawn under the strip is a symptom; a *clip* reaching
                // under it is the permission.
                for clip in clips(&frame) {
                    assert!(
                        clip.bottom() <= l.status.y + 0.01,
                        "{name} at {w}x{h}: a clip of {clip:?} reaches under the status \
                         line at {:?}, so whatever is drawn in it may too",
                        l.status
                    );
                }
            }
        }
    }

    #[test]
    fn nothing_is_painted_entirely_outside_the_clip_in_force() {
        // A clip makes what is outside it invisible; it does not make it free,
        // and it does not make a picture that claims to have painted a grid row
        // three hundred points below the grid into an honest picture. Every
        // cursor in this program -- the tree's, the filter list's, the grid's,
        // the history's, the schema's -- runs down over its items whether or not
        // the pane has room left, so this is the rule that stops them.
        //
        // *Entirely* outside, not partly: a row at the bottom edge is rightly
        // half drawn and half cut.
        //
        // With one exemption, which is the rule rather than a hole in it: the
        // unit of "do not draw this" is the *item*, and an item is drawn whole
        // or not at all. A row whose last sliver is visible draws its delete box
        // in full below the cut, and that is correct. So a fill wholly outside
        // the clip is excused exactly when some other fill drawn *under the same
        // clip* encloses it and is itself partly visible.
        for (name, app) in states() {
            for (w, h) in GRID {
                let frame = app.frame(w, h);
                let fills = fills_clipped(&frame, (w, h));
                for (i, (rect, clip)) in fills.iter().enumerate() {
                    if rect.is_empty() || clip.intersect(*rect).is_some() {
                        continue;
                    }
                    let carried = fills.iter().enumerate().any(|(j, (outer, outer_clip))| {
                        j != i
                            && outer_clip == clip
                            && outer_clip.intersect(*outer).is_some()
                            && outer.intersect(*rect) == Some(*rect)
                    });
                    assert!(
                        carried,
                        "{name} at {w}x{h}: a filled box at {rect:?} is painted \
                         entirely outside the clip {clip:?} that was in force, and \
                         no item drawn under that clip carries it"
                    );
                }
            }
        }
    }

    #[test]
    fn a_run_of_text_is_never_inked_outside_the_box_it_was_given() {
        // Every run in the program is centred in a box by `ink_box`, and a run
        // taller than its box centres to *outside* it at both ends -- which is
        // how a 12-point title comes to be drawn above and below an 8-point
        // panel tab in a window too short to have one.
        for (bw, bh) in [
            (100.0, 20.0),
            (100.0, 4.0),
            (100.0, 0.0),
            (0.0, 20.0),
            (3.0, 3.0),
        ] {
            for size in [1.0, 8.0, 10.0, 12.0, 20.0, 36.0] {
                let r = Rect::new(10.0, 20.0, bw, bh);
                let ink = ink_box(r, size);
                assert!(
                    ink.y >= r.y - 0.01 && ink.bottom() <= r.bottom() + 0.01,
                    "a {size}-point run in a {bw}x{bh} box inks {}..{}, \
                     outside the box's {}..{}",
                    ink.y,
                    ink.bottom(),
                    r.y,
                    r.bottom()
                );
            }
        }
    }

    #[test]
    fn the_layout_never_gives_a_region_a_negative_size() {
        // `content_height`, `grid_height` and `main_width` were all computed by
        // subtraction from constants -- `height - TOOLBAR_HEIGHT - TAB_HEIGHT -
        // STATUS_BAR_HEIGHT`, `width - SIDEBAR_WIDTH` -- with nothing between
        // the arithmetic and a window smaller than the constants. A 200-point
        // window handed the data grid a width of -20.
        //
        // Every width, not `GRID`'s twelve: the rule is a pure function of the
        // two numbers, and the sizes where a fault can hide are the *knees* --
        // where a region is given up -- not a dozen scattered points.
        for step in 0..=300_u32 {
            let v = f32::from(u16::try_from(step).unwrap_or(u16::MAX)) * 7.0;
            for (w, h) in [(v, 600.0), (900.0, v), (v, v)] {
                let l = Layout::solve(w, h);
                for (what, r) in [
                    ("toolbar", l.toolbar),
                    ("tabs", l.tabs),
                    ("sidebar", l.sidebar),
                    ("grid", l.grid),
                    ("panel", l.panel),
                    ("status", l.status),
                    ("grid header", l.grid_header()),
                    ("grid rows", l.grid_rows()),
                    ("page bar", l.page_bar()),
                    ("panel tabs", l.panel_tabs()),
                    ("panel body", l.panel_body()),
                ] {
                    assert!(
                        r.w >= 0.0 && r.h >= 0.0,
                        "{w}x{h}: the {what} is {}x{}",
                        r.w,
                        r.h
                    );
                    assert!(
                        r.x >= -0.01
                            && r.y >= -0.01
                            && r.right() <= w + 0.01
                            && r.bottom() <= h + 0.01,
                        "{w}x{h}: the {what} at {r:?} is outside the window"
                    );
                }
            }
        }
    }

    #[test]
    fn the_regions_of_the_layout_never_overlap() {
        // The toolbar, the tab strip, the sidebar, the grid, the panel and the
        // status strip are six places, not six offsets that happen not to
        // collide at 1200x800. Any pair that meets is a pair where one is drawn
        // over the other and the later one takes the presses.
        for step in 0..=200_u32 {
            let v = f32::from(u16::try_from(step).unwrap_or(u16::MAX)) * 9.0;
            for (w, h) in [(v, 700.0), (1000.0, v), (v, v)] {
                let l = Layout::solve(w, h);
                let named = [
                    ("toolbar", l.toolbar),
                    ("tabs", l.tabs),
                    ("sidebar", l.sidebar),
                    ("grid", l.grid),
                    ("panel", l.panel),
                    ("status", l.status),
                ];
                for (i, (an, a)) in named.iter().enumerate() {
                    for (bn, b) in named.iter().skip(i + 1) {
                        let Some(shared) = a.intersect(*b) else {
                            continue;
                        };
                        assert!(
                            shared.is_empty(),
                            "{w}x{h}: the {an} at {a:?} and the {bn} at {b:?} share {shared:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_window_of_no_size_and_a_window_of_nonsense_size_draw_without_panicking() {
        // `Layout::solve` is handed whatever the compositor says the window
        // measures. A zero comes from a window being minimised; the infinities
        // and the NaN come from nowhere in particular, which is exactly why the
        // arithmetic must not depend on them not arriving.
        for (w, h) in [
            (0.0, 0.0),
            (0.0, 800.0),
            (1200.0, 0.0),
            (-50.0, -50.0),
            (f32::INFINITY, 800.0),
            (1200.0, f32::NEG_INFINITY),
            (f32::NAN, f32::NAN),
        ] {
            for (name, app) in states() {
                let frame = app.frame(w, h);
                assert!(
                    frame.is_balanced(),
                    "{name} at {w}x{h}: the frame is unbalanced"
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // What each control does
    //
    // Every test below finds its control by the words the picture shows and
    // presses those words, so a drawing pass that recorded its boxes one row
    // off cannot pass by making the same mistake twice.
    // ------------------------------------------------------------------

    /// The users table with three hundred more rows in it, so the grid has
    /// pages to turn.
    fn many_rows() -> DbViewerApp {
        let mut app = wired();
        for i in 0..300 {
            if let Some(tab) = app.active_db_tab_mut()
                && let Some(table) = tab.db.find_table_mut("users")
            {
                let row: Vec<CellValue> = table
                    .columns
                    .iter()
                    .map(|c| CellValue::parse_as(&format!("{i}"), &c.data_type))
                    .collect();
                table.rows.push(row);
            }
        }
        app
    }

    /// The `name` column of every row the grid would show, in screen order.
    fn shown_names(app: &DbViewerApp) -> Vec<String> {
        app.active_db_tab()
            .and_then(DbTab::current_table_data)
            .map(|(_, rows)| {
                rows.iter()
                    .filter_map(|(_, cells)| cells.get(1).map(CellValue::display))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn execute_runs_the_query_the_editor_holds() {
        let mut app = wired();
        assert!(app.query_result.is_none(), "nothing has been run yet");
        click_text(&mut app, FULL, "Execute");
        let result = app.query_result.as_ref().expect("Execute ran nothing");
        assert!(
            !result.is_error,
            "the sample query failed: {}",
            result.message
        );
        assert_eq!(app.history.len(), 1, "the query was not remembered");
        assert_eq!(
            app.bottom_panel,
            BottomPanel::Results,
            "running a query did not show its result"
        );
    }

    #[test]
    fn a_query_that_does_not_parse_is_reported_and_still_remembered() {
        let mut app = wired();
        app.sql_input = String::from("this is not a query");
        click_text(&mut app, FULL, "Execute");
        assert!(
            app.query_result.as_ref().is_some_and(|r| r.is_error),
            "a query that cannot be parsed was reported as a success"
        );
        assert_eq!(
            app.history.first().map(|e| e.success),
            Some(false),
            "the history remembers a failed query as having worked"
        );
    }

    #[test]
    fn the_plus_opens_a_database_and_the_strip_selects_between_them() {
        let mut app = wired();
        click_text(&mut app, FULL, "+");
        assert_eq!(app.tabs.len(), 2, "the + opened nothing");
        assert_eq!(app.active_tab, 1, "the new database was not switched to");

        // The sidebar names whichever database is active, so the *inactive*
        // one's name appears exactly once -- in the strip -- and is the one
        // safe to aim at.
        click_text(&mut app, FULL, "sample.db");
        assert_eq!(app.active_tab, 0, "the strip did not switch back");
    }

    #[test]
    fn the_last_database_tab_stays_open() {
        let mut app = wired();
        assert_eq!(app.tabs.len(), 1);
        click_target(&mut app, FULL, Target::CloseTab(0));
        assert_eq!(
            app.tabs.len(),
            1,
            "closing the only database left the window with nothing to show"
        );
        assert!(
            app.status.contains("last"),
            "the refusal was silent: status reads {:?}",
            app.status
        );
    }

    #[test]
    fn closing_a_tab_closes_the_one_pointed_at_not_the_active_one() {
        let mut app = wired();
        app.add_tab("second");
        app.add_tab("third");
        assert_eq!(app.active_tab, 2, "the newest database is the active one");

        click_target(&mut app, FULL, Target::CloseTab(1));

        let names: Vec<String> = app.tabs.iter().map(|t| t.db.name.clone()).collect();
        assert_eq!(
            names,
            vec![String::from("sample.db"), String::from("third")],
            "closing the second database closed something else"
        );
    }

    #[test]
    fn a_tree_row_selects_the_table_it_names() {
        let mut app = wired();
        // Not `users`: the editor opens holding `SELECT * FROM users`, so those
        // words are on the screen twice and pressing "the one that says users"
        // would be a coin toss.
        click_text(&mut app, FULL, "products");
        assert_eq!(
            app.active_db_tab()
                .and_then(|t| t.selected_table.as_deref()),
            Some("products"),
            "pressing a table in the tree selected something else"
        );
        assert!(
            shown_names(&app).contains(&String::from("Laptop")),
            "the grid is still showing the previous table"
        );
    }

    #[test]
    fn an_index_a_view_and_a_trigger_say_what_they_are() {
        for (label, expected) in [
            ("idx_users_email", "Index idx_users_email"),
            ("user_orders_view", "View user_orders_view"),
            ("update_stock", "Trigger update_stock"),
        ] {
            let mut app = wired();
            click_text(&mut app, FULL, label);
            assert_eq!(
                app.status, expected,
                "pressing {label} in the tree said nothing useful"
            );
            assert_eq!(
                app.active_db_tab()
                    .and_then(|t| t.selected_table.as_deref()),
                Some("users"),
                "pressing {label} changed which table the grid shows"
            );
        }
    }

    #[test]
    fn a_heading_names_a_category_and_is_not_a_control() {
        let mut app = wired();
        let before = app.status.clone();
        click_text(&mut app, FULL, "Tables");
        assert_eq!(
            app.status, before,
            "pressing the `Tables` heading did something"
        );
    }

    #[test]
    fn a_column_header_sorts_then_reverses_then_the_arrow_says_which() {
        let mut app = wired();
        assert!(
            app.active_db_tab()
                .and_then(|t| t.sort_state.as_ref())
                .is_none(),
            "the grid opens unsorted"
        );

        click_text(&mut app, FULL, "email");
        assert_eq!(
            app.active_db_tab()
                .and_then(|t| t.sort_state.as_ref())
                .map(|s| (s.column_idx, s.direction)),
            Some((2, SortDir::Ascending)),
            "pressing the `email` header did not sort by email"
        );
        assert!(shows(&app, FULL, "email ^"), "the header shows no arrow");

        click_text(&mut app, FULL, "email ^");
        assert_eq!(
            app.active_db_tab()
                .and_then(|t| t.sort_state.as_ref())
                .map(|s| s.direction),
            Some(SortDir::Descending),
            "pressing a sorted header did not reverse it"
        );
        assert!(
            shows(&app, FULL, "email v"),
            "the arrow still points the old way"
        );
    }

    #[test]
    fn deleting_a_row_with_a_sort_in_force_removes_the_row_pointed_at() {
        let mut app = wired();
        // Descending, so screen order and table order disagree: ascending by
        // name happens to be the order the rows were inserted in, and a grid
        // that deleted by screen position would pass over it.
        click_text(&mut app, FULL, "name");
        click_text(&mut app, FULL, "name ^");
        assert_eq!(
            shown_names(&app).first().map(String::as_str),
            Some("Jack"),
            "the sort did not put Jack at the top"
        );

        // Aim at the delete box of the row the words `Jack` are drawn in: its
        // height is the row's, and the box is the last 20 points of the grid.
        let (_, jack_y) = point(&app, FULL, "Jack");
        let grid = Layout::solve(FULL.0, FULL.1).grid;
        click(&mut app, (grid.right() - 10.0, jack_y), FULL);

        let after = shown_names(&app);
        assert!(
            !after.contains(&String::from("Jack")),
            "the row the user pointed at survived: {after:?}"
        );
        assert_eq!(after.len(), 9, "the wrong number of rows was removed");
        assert!(
            after.contains(&String::from("Ivy")) && after.contains(&String::from("Alice")),
            "a row nobody pointed at was deleted: {after:?}"
        );
    }

    #[test]
    fn the_pagination_bar_turns_pages_and_stops_at_both_ends() {
        let mut app = many_rows();
        assert_eq!(app.active_db_tab().map(|t| t.page), Some(0));

        click_text(&mut app, FULL, "< Prev");
        assert_eq!(
            app.active_db_tab().map(|t| t.page),
            Some(0),
            "the first page has a page before it"
        );

        click_text(&mut app, FULL, "Next >");
        assert_eq!(
            app.active_db_tab().map(|t| t.page),
            Some(1),
            "`Next` turned no page"
        );
        assert!(
            shows(&app, FULL, "Page 2 of 7 (310 rows)"),
            "the caption does not say which page is showing"
        );

        for _ in 0..20 {
            click_text(&mut app, FULL, "Next >");
        }
        assert_eq!(
            app.active_db_tab().map(|t| t.page),
            Some(6),
            "the last page has a page after it"
        );

        click_text(&mut app, FULL, "< Prev");
        assert_eq!(
            app.active_db_tab().map(|t| t.page),
            Some(5),
            "`Prev` went nowhere"
        );
    }

    #[test]
    fn the_four_panel_tabs_each_show_their_own_panel() {
        let mut app = wired();
        for panel in BottomPanel::all() {
            click_text(&mut app, FULL, panel.label());
            assert_eq!(
                app.bottom_panel, *panel,
                "pressing the {:?} tab showed something else",
                panel
            );
        }
    }

    #[test]
    fn the_filters_button_shows_and_hides_the_builder() {
        let mut app = wired();
        assert!(
            !shows(&app, FULL, "FILTER BUILDER"),
            "the builder opens hidden"
        );

        click_text(&mut app, FULL, "Filters");
        assert!(app.show_filter_builder, "the toolbar button set nothing");
        assert!(
            shows(&app, FULL, "FILTER BUILDER"),
            "the flag changed and the sidebar did not"
        );

        click_text(&mut app, FULL, "Filters");
        assert!(
            !shows(&app, FULL, "FILTER BUILDER"),
            "the button only works once"
        );
    }

    #[test]
    fn the_builder_steps_its_column_and_its_comparison() {
        let mut app = wired();
        app.show_filter_builder = true;

        // `id` is the first column of `users`, `name` the second.
        click_text(&mut app, FULL, "Column: id");
        assert_eq!(app.filter_column_idx, 1, "the column did not step");
        assert!(
            shows(&app, FULL, "Column: name"),
            "the builder still names the old column"
        );

        let first = FilterOp::all()
            .first()
            .expect("there is a first comparison");
        let second = FilterOp::all()
            .get(1)
            .expect("there is a second comparison");
        click_text(&mut app, FULL, &format!("Where: {}", first.label()));
        assert_eq!(app.filter_op_idx, 1, "the comparison did not step");
        assert!(
            shows(&app, FULL, &format!("Where: {}", second.label())),
            "the builder still names the old comparison"
        );
    }

    #[test]
    fn the_builder_takes_a_value_adds_a_filter_and_the_x_takes_it_away() {
        let mut app = wired();
        app.show_filter_builder = true;
        click_text(&mut app, FULL, "Column: id");

        click_text(&mut app, FULL, "Value: (type here)");
        assert_eq!(
            app.focus,
            Focus::FilterValue,
            "the value box took no keyboard"
        );
        probe::type_str(&mut app, "Alice");
        assert_eq!(app.filter_value, "Alice", "the typing went somewhere else");
        assert!(
            shows(&app, FULL, "Value: Alice"),
            "the builder does not show what was typed"
        );

        click_text(&mut app, FULL, "+ Add filter");
        assert_eq!(
            app.active_db_tab().map(|t| t.filters.len()),
            Some(1),
            "`Add filter` added nothing"
        );
        assert_eq!(
            shown_names(&app),
            vec![String::from("Alice")],
            "the filter was added and the grid ignored it"
        );

        click_target(&mut app, FULL, Target::RemoveFilter(0));
        assert_eq!(
            app.active_db_tab().map(|t| t.filters.len()),
            Some(0),
            "the filter could not be removed"
        );
        assert_eq!(shown_names(&app).len(), 10, "the rows did not come back");
    }

    #[test]
    fn export_puts_the_table_in_the_editor_and_import_reads_it_back() {
        let mut app = wired();
        click_text(&mut app, FULL, "Export CSV");
        assert!(
            app.sql_input.starts_with("id,name,email"),
            "the export did not reach the editor: {:?}",
            app.sql_input
        );
        assert_eq!(
            app.bottom_panel,
            BottomPanel::SqlEditor,
            "the export was put somewhere nobody was looking"
        );
        assert_eq!(app.focus, Focus::Editor);

        click_text(&mut app, FULL, "Import");
        assert_eq!(
            app.active_db_tab()
                .and_then(|t| t.selected_table.as_deref()),
            Some("imported2"),
            "the import did not select what it read"
        );
        assert!(
            shown_names(&app).contains(&String::from("Alice")),
            "the round trip lost the rows: {:?}",
            shown_names(&app)
        );
        assert_eq!(
            shown_names(&app).len(),
            10,
            "the round trip changed how many rows there are"
        );
    }

    #[test]
    fn the_editor_takes_typing_and_enter_runs_what_was_typed() {
        let mut app = wired();
        click_target(&mut app, FULL, Target::SqlEditor);
        assert_eq!(app.focus, Focus::Editor, "the editor took no keyboard");

        for _ in 0..app.sql_input.chars().count() {
            probe::key(&mut app, &probe::press(Key::Backspace));
        }
        assert!(app.sql_input.is_empty(), "backspace did not empty the box");

        probe::type_str(&mut app, "SELECT * FROM products");
        assert_eq!(app.sql_input, "SELECT * FROM products");

        probe::key(&mut app, &probe::press(Key::Enter));
        let result = app.query_result.as_ref().expect("Enter ran nothing");
        assert!(
            !result.is_error,
            "the typed query failed: {}",
            result.message
        );
        assert_eq!(app.history.len(), 1, "Enter did not remember the query");
    }

    #[test]
    fn escape_releases_the_keyboard_and_typing_then_goes_nowhere() {
        let mut app = wired();
        click_target(&mut app, FULL, Target::SqlEditor);
        let before = app.sql_input.clone();

        probe::key(&mut app, &probe::press(Key::Escape));
        assert_eq!(app.focus, Focus::None, "escape released nothing");
        probe::type_str(&mut app, "zzz");
        assert_eq!(
            app.sql_input, before,
            "typing reached the editor after the keyboard was released"
        );
    }

    #[test]
    fn the_history_puts_a_query_back_and_the_star_is_its_own_control() {
        let mut app = wired();
        app.sql_input = String::from("SELECT * FROM products");
        app.execute_query();
        app.sql_input = String::from("SELECT * FROM orders");
        app.execute_query();
        app.bottom_panel = BottomPanel::SqlEditor;
        assert_eq!(app.history.len(), 2);

        // Newest first on the screen, so the *second* history entry is the top
        // row: a pass that numbered rows by screen position would recall the
        // wrong query here and nowhere else.
        click_target(&mut app, FULL, Target::HistoryEntry(0));
        assert_eq!(
            app.sql_input, "SELECT * FROM products",
            "recall fetched the wrong query"
        );

        assert!(app.history.first().is_some_and(|e| !e.favorite));
        click_target(&mut app, FULL, Target::FavoriteEntry(0));
        assert!(
            app.history.first().is_some_and(|e| e.favorite),
            "the star did not star"
        );
        assert_eq!(
            app.sql_input, "SELECT * FROM products",
            "the star also recalled the query, so the row swallowed its own star"
        );
    }

    #[test]
    fn the_grid_says_how_many_columns_it_is_showing_when_it_cannot_show_them_all() {
        let mut some_window_hid_a_column = false;
        for (w, h) in GRID {
            let app = wired();
            let frame = app.frame(w, h);
            let shown = frame
                .hits()
                .iter()
                .filter(|(t, _)| matches!(t, Target::SortColumn(_)))
                .count();
            let Some((caption, ..)) = text_runs(&frame)
                .into_iter()
                .find(|(t, ..)| t.starts_with("Page 1 of "))
            else {
                continue;
            };
            if shown == 5 {
                assert!(
                    !caption.contains("columns shown"),
                    "at {w}x{h} every column is shown and the caption says otherwise: \
                     {caption:?}"
                );
            } else {
                some_window_hid_a_column = true;
                assert!(
                    caption.contains(&format!("{shown} of 5 columns shown")),
                    "at {w}x{h} the grid drew {shown} of 5 columns and the caption \
                     reads {caption:?}"
                );
            }
        }
        assert!(
            some_window_hid_a_column,
            "no window in the grid was narrow enough to hide a column, so the \
             caption's honesty was never tested"
        );
    }

    #[test]
    fn the_schema_pane_never_loses_its_third_column() {
        let mut checked = 0_usize;
        for (w, h) in GRID {
            let mut app = wired();
            app.bottom_panel = BottomPanel::Schema;
            if !shows(&app, (w, h), "Column") {
                continue;
            }
            checked += 1;
            assert!(
                shows(&app, (w, h), "Constraints"),
                "at {w}x{h} the schema pane heads a Column column and no \
                 Constraints column: the third one is outside the pane and the \
                 clip is hiding it"
            );
        }
        assert!(checked > 0, "the schema pane was never drawn at all");
    }

    #[test]
    fn the_diagram_wraps_its_boxes_and_says_how_many_it_is_showing() {
        let mut some_window_hid_a_table = false;
        for (w, h) in GRID {
            let mut app = wired();
            app.bottom_panel = BottomPanel::Diagram;
            let body = Layout::solve(w, h).panel_body();
            let frame = app.frame(w, h);
            let runs = text_runs(&frame);
            let Some((heading, ..)) = runs.iter().find(|(t, ..)| t.starts_with("SCHEMA DIAGRAM"))
            else {
                continue;
            };
            // Counted inside the pane: the sidebar names the same three tables,
            // and counting the whole window would report the tree's rows as
            // boxes the diagram drew.
            let drawn = runs
                .iter()
                .filter(|(t, x, y, ..)| {
                    body.contains(*x, *y) && ["users", "products", "orders"].contains(&t.as_str())
                })
                .count();
            if drawn == 3 {
                assert_eq!(
                    heading, "SCHEMA DIAGRAM",
                    "at {w}x{h} all three tables are drawn and the heading \
                     apologises for something"
                );
            } else {
                some_window_hid_a_table = true;
                assert!(
                    heading.contains(&format!("{drawn} of 3 tables shown")),
                    "at {w}x{h} the diagram drew {drawn} of 3 tables and the \
                     heading reads {heading:?}"
                );
            }
        }
        assert!(
            some_window_hid_a_table,
            "no window in the grid was small enough to drop a table box, so the \
             heading's honesty was never tested"
        );
    }

    /// A target's name, with any index it carries replaced by `_`.
    ///
    /// `SortColumn(0)` and `SortColumn(3)` are the same control drawn twice;
    /// `Export(Csv)` and `Export(Json)` are two different controls.
    fn control_name(target: Target) -> String {
        match target {
            Target::SelectTab(_)
            | Target::CloseTab(_)
            | Target::TreeNode(_)
            | Target::RemoveFilter(_)
            | Target::SortColumn(_)
            | Target::DeleteRow(_)
            | Target::HistoryEntry(_)
            | Target::FavoriteEntry(_) => format!("{}(_)", probe::variant_name(target)),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn every_action_the_program_knows_has_a_control_somewhere() {
        let mut expected: Vec<String> = [
            Target::Execute,
            Target::NewTab,
            Target::Import,
            Target::AddTab,
            Target::ToggleFilterBuilder,
            Target::FilterColumn,
            Target::FilterOp,
            Target::FilterValue,
            Target::AddFilter,
            Target::PrevPage,
            Target::NextPage,
            Target::SqlEditor,
            Target::SelectTab(0),
            Target::CloseTab(0),
            Target::TreeNode(0),
            Target::RemoveFilter(0),
            Target::SortColumn(0),
            Target::DeleteRow(0),
            Target::HistoryEntry(0),
            Target::FavoriteEntry(0),
        ]
        .into_iter()
        .map(control_name)
        .collect();
        for format in [
            ExportFormat::Csv,
            ExportFormat::Json,
            ExportFormat::SqlInserts,
        ] {
            expected.push(control_name(Target::Export(format)));
        }
        for panel in BottomPanel::all() {
            expected.push(control_name(Target::ShowPanel(*panel)));
        }

        let mut drawn: Vec<String> = Vec::new();
        for (_, app) in states() {
            for (w, h) in GRID {
                for (target, _) in app.frame(w, h).hits() {
                    let name = control_name(*target);
                    if !drawn.contains(&name) {
                        drawn.push(name);
                    }
                }
            }
        }

        let missing: Vec<&String> = expected.iter().filter(|e| !drawn.contains(e)).collect();
        assert!(
            missing.is_empty(),
            "the program can be asked to do these and nothing on the screen asks: \
             {missing:?}"
        );
    }

    #[test]
    fn the_history_heading_is_never_a_title_over_an_empty_strip() {
        let mut app = wired();
        for i in 0..20 {
            app.sql_input = format!("SELECT {i} FROM users");
            app.execute_query();
        }
        app.bottom_panel = BottomPanel::SqlEditor;

        let mut checked = 0_usize;
        for (w, h) in GRID {
            let frame = app.frame(w, h);
            let runs = text_runs(&frame);
            let Some((heading, ..)) = runs.iter().find(|(t, ..)| t.starts_with("HISTORY (")) else {
                continue;
            };
            checked += 1;
            let rows = frame
                .hits()
                .iter()
                .filter(|(t, _)| matches!(t, Target::HistoryEntry(_)))
                .count();
            assert!(
                rows > 0,
                "at {w}x{h} the pane heads a history and draws none of it: {heading:?}"
            );
            assert!(
                heading.contains(&format!("{rows} of 20 shown")),
                "at {w}x{h} the pane draws {rows} of 20 queries and the heading \
                 reads {heading:?}"
            );
        }
        assert!(checked > 0, "the history was never drawn at all");
    }

    #[test]
    fn the_history_heading_counts_queries_when_it_is_showing_all_of_them() {
        let mut app = wired();
        for i in 0..2 {
            app.sql_input = format!("SELECT {i} FROM users");
            app.execute_query();
        }
        app.bottom_panel = BottomPanel::SqlEditor;
        assert!(
            shows(&app, FULL, "HISTORY (2 queries)"),
            "the heading apologises for hiding something it is showing"
        );
    }

    // ------------------------------------------------------------------
    // The window itself
    // ------------------------------------------------------------------

    #[test]
    fn the_window_says_what_it_is() {
        let app = DbViewerApp::new();
        assert_eq!(app.title(), "DB Viewer");
        assert_eq!(app.app_id(), "dbviewer");
        assert_eq!(app.initial_size(), (1200, 800));
        assert!(
            app.tick_interval().is_none(),
            "a database browser that redraws on a timer is a database browser \
             burning a core to show a picture that did not change"
        );
    }

    #[test]
    fn the_close_button_closes_the_window_and_nothing_else_does() {
        let mut app = DbViewerApp::new();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
        assert_eq!(
            app.on_event(&Event::Resize {
                width: 800,
                height: 600
            }),
            Response::Redraw
        );
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Escape))),
            Response::Redraw
        );
    }

    #[test]
    fn a_resize_moves_where_the_controls_answer() {
        let app = DbViewerApp::new();
        let wide = app.frame(1200.0, 800.0);
        let narrow = app.frame(700.0, 800.0);

        let of = |frame: &Frame<Target>, target: Target| {
            frame
                .rect_of(move |t| *t == target)
                .unwrap_or_else(|| panic!("{target:?} is not drawn"))
        };

        assert_eq!(
            of(&wide, Target::Execute),
            of(&narrow, Target::Execute),
            "the toolbar is laid from the left, so its first button does not move"
        );
        assert!(
            of(&narrow, Target::NextPage).x < of(&wide, Target::NextPage).x,
            "the pagination bar ends at the window's right-hand edge, and did \
             not follow it in"
        );
    }

    #[test]
    fn a_press_is_answered_against_the_size_the_last_frame_was_drawn_at() {
        let mut app = DbViewerApp::new();
        // The window is told it is 700 wide by a resize, with no frame drawn
        // in between. A press has to be answered against 700 and not against
        // the 1200 the app was built believing in.
        assert_eq!(
            app.on_event(&Event::Resize {
                width: 700,
                height: 800
            }),
            Response::Redraw
        );

        let bar = Layout::solve(700.0, 800.0).page_bar();
        let next = app
            .frame(700.0, 800.0)
            .rect_of(|t| *t == Target::NextPage)
            .expect("the pagination bar is drawn at 700 points across");
        assert!(
            bar.contains(next.centre().0, next.centre().1),
            "the test is aiming outside the bar it means to press"
        );
        let (x, y) = next.centre();
        app.on_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert!(
            app.status.starts_with("Page "),
            "the press was answered against the old window size: status reads \
             {:?}",
            app.status
        );
    }

    #[test]
    fn render_remembers_the_size_it_was_asked_for() {
        let mut app = DbViewerApp::new();
        // Spelled out rather than `app.render(...)`: there is an inherent
        // `render` that draws a frame and remembers nothing, and an inherent
        // method wins over a trait one. Written the short way this test drove
        // the drawing pass and never touched the window's own entry point --
        // which is the thing it exists to check.
        let tree = App::render(&mut app, 640.0, 480.0);
        assert!(
            !tree.is_empty(),
            "the window was drawn and nothing came out"
        );
        assert!(
            (app.window_width - 640.0).abs() < f32::EPSILON
                && (app.window_height - 480.0).abs() < f32::EPSILON,
            "the frame was drawn at one size and the app remembers another"
        );
    }

    /// The drawing passes that take a plain box, by name.
    ///
    /// `draw_data_grid` and `draw_bottom_panels` are not here: they take a
    /// whole `Layout` rather than a box, and what they draw into is already
    /// covered by the sweeps over the window.
    /// Whether `inner` lies inside `outer`, to within a hundredth of a point.
    ///
    /// Not `outer.intersect(inner) == Some(inner)`, which is the idiom the
    /// sweeps over the window use. That form asks for exact equality, and the
    /// intersection recomputes the height as `bottom - y` from two numbers that
    /// are already inexact: a filter row placed at `y = 111.99999` with
    /// `h = 18.0` comes back 18.000004 tall and compares unequal to itself.
    /// The window's own edges are whole numbers, so the sweeps never meet it;
    /// a box laid out from a chain of fractions does.
    fn bounded_by(outer: Rect, inner: Rect) -> bool {
        inner.x >= outer.x - 0.01
            && inner.y >= outer.y - 0.01
            && inner.right() <= outer.right() + 0.01
            && inner.bottom() <= outer.bottom() + 0.01
    }

    /// One drawing pass: the app it belongs to, the frame it writes into, and
    /// the box it is told to stay inside.
    type Pass = fn(&DbViewerApp, &mut Frame<Target>, Rect);

    fn passes() -> Vec<(&'static str, Pass)> {
        vec![
            ("toolbar", DbViewerApp::draw_toolbar),
            ("database tabs", DbViewerApp::draw_db_tabs),
            ("sidebar", DbViewerApp::draw_sidebar),
            ("sql editor", DbViewerApp::draw_sql_editor),
            ("results", DbViewerApp::draw_results),
            ("schema", DbViewerApp::draw_schema),
            ("diagram", DbViewerApp::draw_diagram),
            ("status bar", DbViewerApp::draw_status_bar),
        ]
    }

    #[test]
    fn no_pass_paints_outside_the_box_it_was_given() {
        // Phrased over *fills*, and it has to be. A clip stops the renderer
        // showing a run of text past the edge and makes `Frame::hit` drop the
        // boxes out there, so a pass that overran would look correct from both
        // -- but a fill is pushed exactly as asked. The fill is the only
        // witness left. (known-issues.md, Lesson 107.)
        for (state, app) in states() {
            for (name, pass) in passes() {
                for (w, h) in [
                    (300.0, 200.0),
                    (220.0, 120.0),
                    (400.0, 90.0),
                    (200.0, 400.0),
                    (120.0, 40.0),
                    (60.0, 18.0),
                ] {
                    // The frame is bigger than the box on every side, so an
                    // overrun has somewhere to go and is not quietly clamped.
                    let area = Rect::new(10.0, 10.0, w, h);
                    let mut f = Frame::new(area.right() + 40.0, area.bottom() + 40.0);
                    pass(&app, &mut f, area);
                    for command in f.commands() {
                        let RenderCommand::FillRect {
                            x,
                            y,
                            width,
                            height,
                            ..
                        } = command
                        else {
                            continue;
                        };
                        let filled = Rect::new(*x, *y, *width, *height);
                        if filled.is_empty() {
                            continue;
                        }
                        assert!(
                            bounded_by(area, filled),
                            "{state}: the {name} pass, given {area:?}, filled \
                             {filled:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_sidebar_is_given_up_before_the_grid_falls_below_a_table() {
        for step in 0..=400_u32 {
            let w = f32::from(u16::try_from(step).unwrap_or(u16::MAX)) * 6.0;
            let l = Layout::solve(w, 800.0);
            if l.sidebar.w <= 0.0 {
                continue;
            }
            assert!(
                l.sidebar.w >= MIN_SIDEBAR_WIDTH,
                "at {w} across the sidebar is {} wide, which is narrower than a \
                 table name",
                l.sidebar.w
            );
            assert!(
                l.grid.w >= MIN_GRID_WIDTH,
                "at {w} across the sidebar took {} and left the grid {}",
                l.sidebar.w,
                l.grid.w
            );
        }
    }

    #[test]
    fn the_bottom_panel_and_the_tab_strip_are_given_up_before_the_grid_is() {
        for step in 0..=400_u32 {
            let h = f32::from(u16::try_from(step).unwrap_or(u16::MAX)) * 4.0;
            let l = Layout::solve(1200.0, h);
            if l.panel.h > 0.0 {
                assert!(
                    l.grid.h >= MIN_GRID_HEIGHT,
                    "at {h} tall the panel took {} and left the grid {}, which \
                     is less than a header, a row and a pagination bar",
                    l.panel.h,
                    l.grid.h
                );
            }
            if l.tabs.h > 0.0 {
                assert!(
                    l.grid.h >= MIN_GRID_HEIGHT,
                    "at {h} tall the tab strip took {} and left the grid {}",
                    l.tabs.h,
                    l.grid.h
                );
            }
        }
    }
}
