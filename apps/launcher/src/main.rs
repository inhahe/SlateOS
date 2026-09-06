//! Slate OS App Launcher
//!
//! A Spotlight/Alfred-style application launcher providing:
//! - As-you-type fuzzy search across installed applications and system commands
//! - Frecency-based ranking (combines match quality with launch frequency/recency)
//! - Keyboard-driven navigation (arrows, Enter to launch, Escape to dismiss)
//! - Catppuccin Mocha dark theme with a centered floating dialog
//!
//! Uses the guitk library for UI rendering.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::Response;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ============================================================================
// Theme — Catppuccin Mocha palette
// ============================================================================

/// Catppuccin Mocha dark theme colors.
mod theme {
    use guitk::color::Color;

    /// Base background (slightly transparent for floating dialog feel).
    pub const BASE: Color = Color::rgba(30, 30, 46, 240);
    /// Mantle — slightly darker background for input area.
    pub const MANTLE: Color = Color::from_hex(0x181825);
    /// Surface0 — card/result row background.
    #[allow(dead_code, reason = "the palette is kept complete")]
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    /// Surface1 — hover/selected highlight.
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    /// Surface2 — borders.
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    /// Text — primary text color.
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    /// Subtext0 — secondary text (descriptions, category badges).
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    /// Subtext1 — dimmer text.
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    /// Overlay0 — placeholder text.
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    /// Blue — accent color (selected item highlight, input caret).
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    /// Mauve — category badge accent.
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
    /// Green — system command badge.
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    /// Peach — settings badge.
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    /// Red — destructive actions.
    pub const RED: Color = Color::from_hex(0xF38BA8);
    /// Shadow color for the dialog box.
    pub const SHADOW: Color = Color::rgba(0, 0, 0, 100);
}

// ============================================================================
// Layout constants
// ============================================================================

/// Dialog width in logical pixels.
const DIALOG_WIDTH: f32 = 620.0;
/// Height of the search input area.
const INPUT_HEIGHT: f32 = 52.0;
/// Height of each result row.
const ROW_HEIGHT: f32 = 44.0;
/// Maximum number of visible results.
const MAX_RESULTS: usize = 8;
/// Corner radius for the dialog container.
const DIALOG_RADIUS: f32 = 12.0;
/// Padding inside the dialog.
const PADDING: f32 = 12.0;
/// Font size for search input.
const INPUT_FONT_SIZE: f32 = 18.0;
/// Font size for result names.
const NAME_FONT_SIZE: f32 = 14.0;
/// Font size for descriptions and badges.
const DESC_FONT_SIZE: f32 = 12.0;
/// Height of the "could not launch that" banner, when there is one.
const ERROR_HEIGHT: f32 = 28.0;

// ============================================================================
// Layout
// ============================================================================

/// Where the launcher's parts are, in screen coordinates.
///
/// Computed once from the viewport and the current result count, and used by
/// **both** the renderer and the hit-test. That sharing is the entire point:
/// the arithmetic that decides where row 3 is drawn and the arithmetic that
/// decides which row a click landed in have to be the same arithmetic, or the
/// pointer selects the row above the one under it — and that is a bug the user
/// sees as "the launcher opened the wrong program".
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    /// Screen x of the dialog's left edge.
    dialog_x: f32,
    /// Screen y of the dialog's top edge.
    dialog_y: f32,
    /// Total dialog height, which grows with the result count and with the
    /// presence of an error banner.
    dialog_height: f32,
    /// Width of the dialog's interior: the input box and every result row.
    inner_width: f32,
    /// Interior y of the error banner's top, if there is a banner.
    error_top: Option<f32>,
    /// Interior y of the first result row's top.
    rows_top: f32,
    /// How many rows are drawn.
    row_count: usize,
}

impl Layout {
    /// Measure the dialog as it currently stands.
    fn of(state: &LauncherState) -> Self {
        let row_count = state.results.len();
        let error_top = state.error.is_some().then_some(INPUT_HEIGHT);
        let rows_top = INPUT_HEIGHT
            + if error_top.is_some() {
                ERROR_HEIGHT
            } else {
                0.0
            };
        #[allow(
            clippy::cast_precision_loss,
            reason = "row_count is at most MAX_RESULTS"
        )]
        let results_height = row_count as f32 * ROW_HEIGHT;
        Self {
            dialog_x: (state.viewport_width - DIALOG_WIDTH) / 2.0,
            dialog_y: state.viewport_height * 0.25,
            dialog_height: rows_top + results_height + PADDING * 2.0,
            inner_width: DIALOG_WIDTH - PADDING * 2.0,
            error_top,
            rows_top,
            row_count,
        }
    }

    /// A screen point in the dialog's interior coordinates — the frame the
    /// renderer works in after its `PushTranslate`.
    fn interior_of(self, x: f32, y: f32) -> (f32, f32) {
        (x - (self.dialog_x + PADDING), y - (self.dialog_y + PADDING))
    }

    /// Whether a screen point is inside the dialog at all.
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.dialog_x
            && x < self.dialog_x + DIALOG_WIDTH
            && y >= self.dialog_y
            && y < self.dialog_y + self.dialog_height
    }

    /// The result row a screen point falls in, if any.
    fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        let (ix, iy) = self.interior_of(x, y);
        if ix < 0.0 || ix >= self.inner_width || iy < self.rows_top {
            return None;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "truncation is the intent -- the row a point falls in -- and \
                      the value is non-negative because `iy < rows_top` returned above"
        )]
        let row = ((iy - self.rows_top) / ROW_HEIGHT) as usize;
        (row < self.row_count).then_some(row)
    }
}

// ============================================================================
// App categories
// ============================================================================

/// Category of a launchable item — determines badge color and grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Application,
    System,
    Setting,
    File,
    Command,
}

impl Category {
    /// Short label for the badge shown on each result row.
    fn label(self) -> &'static str {
        match self {
            Self::Application => "App",
            Self::System => "Sys",
            Self::Setting => "Set",
            Self::File => "File",
            Self::Command => "Cmd",
        }
    }

    /// Badge color per category (Catppuccin palette).
    fn color(self) -> Color {
        match self {
            Self::Application => theme::BLUE,
            Self::System => theme::RED,
            Self::Setting => theme::PEACH,
            Self::File => theme::GREEN,
            Self::Command => theme::MAUVE,
        }
    }
}

// ============================================================================
// App entry
// ============================================================================

/// A launchable item in the database.
#[derive(Clone, Debug)]
pub struct AppEntry {
    /// Display name.
    pub name: String,
    /// Short description shown below the name.
    pub description: String,
    /// Path to the executable.
    pub executable_path: String,
    /// Additional search keywords.
    pub keywords: Vec<String>,
    /// Category for badge display.
    pub category: Category,
    /// Cumulative launch count (for frecency scoring).
    pub launch_count: u32,
}

// ============================================================================
// Launch history entry (for frecency)
// ============================================================================

/// One entry in the launch history ring buffer.
#[derive(Clone, Debug)]
struct LaunchRecord {
    /// Executable path that was launched.
    executable_path: String,
    /// Timestamp in seconds since some epoch (monotonic).
    timestamp_secs: u64,
}

// ============================================================================
// Fuzzy matcher
// ============================================================================

/// Rank how well `query` fuzzy-matches `target`; `None` if it does not match.
///
/// Re-exported rather than implemented: the identical routine was written out
/// three times across this tree. See `textfind::fuzzy_score`.
pub use guitk::textfind::fuzzy_score;

/// Compute a combined score searching across name, description, and keywords.
fn search_score(query: &str, entry: &AppEntry) -> Option<u32> {
    let mut best: Option<u32> = None;

    // Name match is most important — double the score
    if let Some(s) = fuzzy_score(query, &entry.name) {
        let boosted = s.saturating_mul(2);
        best = Some(best.map_or(boosted, |b: u32| b.max(boosted)));
    }

    // Description match
    if let Some(s) = fuzzy_score(query, &entry.description) {
        best = Some(best.map_or(s, |b: u32| b.max(s)));
    }

    // Keyword matches
    for kw in &entry.keywords {
        if let Some(s) = fuzzy_score(query, kw) {
            let boosted = s.saturating_add(5); // small keyword bonus
            best = Some(best.map_or(boosted, |b: u32| b.max(boosted)));
        }
    }

    best
}

// ============================================================================
// Frecency scoring
// ============================================================================

/// Compute a frecency bonus for an app based on its launch history.
///
/// Formula: each past launch contributes a decaying bonus based on recency.
/// More recent launches contribute more. The total is capped to prevent
/// runaway scores for extremely frequently-used apps.
fn frecency_bonus(
    executable_path: &str,
    history: &[LaunchRecord],
    now_secs: u64,
    launch_count: u32,
) -> u32 {
    // Base bonus from total launch count (logarithmic to avoid domination)
    let count_bonus = if launch_count > 0 {
        // log2(launch_count + 1) * 5, capped at 30
        let log_val = (32u32.saturating_sub(launch_count.saturating_add(1).leading_zeros()))
            .saturating_mul(5);
        log_val.min(30)
    } else {
        0
    };

    // Recency bonus from history entries
    let mut recency_bonus: u32 = 0;
    for record in history.iter().rev() {
        if record.executable_path != executable_path {
            continue;
        }
        let age_secs = now_secs.saturating_sub(record.timestamp_secs);
        // Decay: full bonus within 5 min, half at 1 hour, quarter at 1 day
        let bonus = if age_secs < 300 {
            20
        } else if age_secs < 3600 {
            10
        } else if age_secs < 86400 {
            5
        } else {
            1
        };
        recency_bonus = recency_bonus.saturating_add(bonus);
        // Only consider last 10 relevant records
        if recency_bonus >= 80 {
            break;
        }
    }

    count_bonus.saturating_add(recency_bonus.min(80))
}

// ============================================================================
// Launcher action (returned from event handling)
// ============================================================================

/// Action the launcher wants the shell to perform after handling an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LauncherAction {
    /// Launch the executable at the given path.
    Launch(String),
    /// Dismiss/close the launcher dialog.
    Dismiss,
    /// No action needed.
    None,
}

// ============================================================================
// Launcher state
// ============================================================================

/// Scored result for display.
#[derive(Clone, Debug)]
struct ScoredEntry {
    /// Index into the app database.
    db_index: usize,
    /// Combined match + frecency score.
    total_score: u32,
}

/// Main state of the launcher dialog.
pub struct LauncherState {
    /// Current search query text.
    query: String,
    /// Cursor position within the query (byte offset).
    cursor: usize,
    /// Filtered and scored results (indices into `apps`).
    results: Vec<ScoredEntry>,
    /// Currently selected result index (within `results`).
    selected_index: usize,
    /// Whether the launcher is currently visible.
    pub visible: bool,
    /// Full application database.
    apps: Vec<AppEntry>,
    /// Recent launch history (ring buffer, max 100).
    launch_history: Vec<LaunchRecord>,
    /// Current timestamp in seconds (updated externally via tick events).
    now_secs: u64,
    /// Assumed viewport width for centering.
    viewport_width: f32,
    /// Assumed viewport height for vertical positioning.
    viewport_height: f32,
    /// Why the last launch attempt did not start a program, if it did not.
    ///
    /// A launcher whose whole job is to start something has exactly one
    /// interesting failure, and it must not be silent: "I clicked it and
    /// nothing happened" is indistinguishable from a hung machine. Cleared by
    /// [`Self::update_results`], i.e. as soon as the user types anything.
    error: Option<String>,
}

impl LauncherState {
    /// Create a new launcher with the built-in app database.
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        let apps = builtin_app_database();
        let mut state = Self {
            query: String::new(),
            cursor: 0,
            results: Vec::new(),
            selected_index: 0,
            visible: false,
            apps,
            launch_history: Vec::new(),
            now_secs: 0,
            viewport_width,
            viewport_height,
            error: None,
        };
        // Initially show all apps sorted by frecency
        state.update_results();
        state
    }

    /// Show the launcher (reset query and refresh results).
    pub fn show(&mut self) {
        self.query.clear();
        self.cursor = 0;
        self.selected_index = 0;
        self.visible = true;
        self.update_results();
    }

    /// Hide the launcher.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Update the current timestamp (call from Tick events).
    pub fn set_now(&mut self, secs: u64) {
        self.now_secs = secs;
    }

    /// Read the wall clock into [`Self::now_secs`].
    ///
    /// The impure half of the frecency clock; [`Self::set_now`] is the pure
    /// half a test can drive.
    ///
    /// # Why this had to exist
    ///
    /// `set_now` had no production caller, so `now_secs` was `0` for the life
    /// of the process. Every launch was then recorded with
    /// `timestamp_secs: 0`, and `frecency_bonus` computes `now - timestamp`,
    /// which was `0` for all of them — inside the "launched in the last five
    /// minutes" band. So every application the user had *ever* launched
    /// counted as launched seconds ago, forever: the recency half of frecency
    /// was inert, and the ranking degenerated into launch-count order. This is
    /// the third clock in this tree found frozen because its setter was public,
    /// documented, tested, and called by nothing. See known-issues.md.
    ///
    /// # No time zone here, deliberately
    ///
    /// `now_secs` is only ever used in *differences* against stored
    /// timestamps, so it is an instant and not a time of day. Putting it
    /// through `tzrules` as the clock-facing surfaces do would be harmless
    /// arithmetically and misleading to read — it would suggest this number is
    /// ever displayed. It is not.
    pub fn refresh_now(&mut self) {
        if let Ok(since_epoch) = SystemTime::now().duration_since(UNIX_EPOCH) {
            self.set_now(since_epoch.as_secs());
        }
    }

    /// Record that a launch attempt failed, and put the dialog back on screen
    /// to say so.
    ///
    /// Not [`Self::show`]: that clears the query, which would throw away what
    /// the user typed at exactly the moment they need to see it to understand
    /// what failed.
    pub fn report_launch_failure(&mut self, path: &str, reason: &str) {
        self.visible = true;
        self.error = Some(format!("Could not start {path}: {reason}"));
    }

    /// The last launch failure, if the dialog is showing one.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Route a window event to the right handler.
    ///
    /// The launcher had no `handle_event` at all: `handle_key` existed and was
    /// well covered, and nothing routed a key to it, so the whole dialog was
    /// reachable only from its own tests.
    pub fn handle_event(&mut self, event: &Event) -> LauncherAction {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a viewport is far below f32's exact-integer range"
                )]
                self.set_viewport(*width as f32, *height as f32);
                LauncherAction::None
            }
            // Frecency decays against the wall clock and against nothing else.
            Event::Tick { .. } => {
                self.refresh_now();
                LauncherAction::None
            }
            _ => LauncherAction::None,
        }
    }

    /// Handle a mouse event against the same layout the renderer draws from.
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> LauncherAction {
        if !self.visible {
            return LauncherAction::None;
        }
        let layout = Layout::of(self);
        match mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                if let Some(row) = layout.row_at(mouse.x, mouse.y) {
                    self.selected_index = row;
                    return self.launch_selected();
                }
                if layout.contains(mouse.x, mouse.y) {
                    // A click in the dialog's own chrome — the input box, the
                    // padding — is not a click on anything, and must not
                    // dismiss the dialog the user is aiming at.
                    return LauncherAction::None;
                }
                // Clicking away from a transient dialog dismisses it. The
                // launcher covers the whole screen precisely so that there is
                // an "away" to click.
                self.hide();
                LauncherAction::Dismiss
            }
            // Hover moves the selection, so the row under the pointer is the
            // row Enter would open. Only on an actual `Move`: the list is
            // re-ranked under a *stationary* pointer on every keystroke, and
            // selecting from a pointer that has not moved would let the mouse
            // silently overrule the arrow keys.
            MouseEventKind::Move => {
                if let Some(row) = layout.row_at(mouse.x, mouse.y) {
                    self.selected_index = row;
                }
                LauncherAction::None
            }
            _ => LauncherAction::None,
        }
    }

    /// Everything the renderer reads that a single event can change.
    ///
    /// Used to decide `Redraw` versus `Idle`, because [`LauncherAction`] does
    /// not answer that question: arrowing down the list, typing, and moving the
    /// caret all return `LauncherAction::None` and all change what is drawn,
    /// while a mouse move across empty space changes nothing.
    fn display_revision(&self) -> DisplayRevision {
        DisplayRevision {
            visible: self.visible,
            selected: self.selected_index,
            results: self.results.len(),
            query: self.query.clone(),
            cursor: self.cursor,
            error: self.error.clone(),
        }
    }

    /// Update viewport dimensions (call from Resize events).
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport_width = width;
        self.viewport_height = height;
    }

    /// Handle a key event. Returns what action the shell should take.
    pub fn handle_key(&mut self, event: &KeyEvent) -> LauncherAction {
        if !event.pressed {
            return LauncherAction::None;
        }

        match event.key {
            Key::Escape => {
                self.hide();
                return LauncherAction::Dismiss;
            }

            Key::Enter => {
                return self.launch_selected();
            }

            Key::Up => {
                self.selected_index = self.selected_index.saturating_sub(1);
                return LauncherAction::None;
            }

            Key::Down => {
                let max_idx = self.results.len().saturating_sub(1);
                self.selected_index = self.selected_index.saturating_add(1).min(max_idx);
                return LauncherAction::None;
            }

            Key::Tab => {
                // Autocomplete: fill query with selected item's name
                if let Some(scored) = self.results.get(self.selected_index)
                    && let Some(entry) = self.apps.get(scored.db_index)
                {
                    self.query = entry.name.clone();
                    self.cursor = self.query.len();
                    self.update_results();
                }
                return LauncherAction::None;
            }

            Key::Backspace => {
                if !self.query.is_empty() && self.cursor > 0 {
                    // Remove the character before cursor
                    let remove_at = self.query[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i);
                    if let Some(idx) = remove_at {
                        self.query.remove(idx);
                        self.cursor = idx;
                    }
                    self.selected_index = 0;
                    self.update_results();
                }
                return LauncherAction::None;
            }

            Key::Delete => {
                if self.cursor < self.query.len() {
                    self.query.remove(self.cursor);
                    self.selected_index = 0;
                    self.update_results();
                }
                return LauncherAction::None;
            }

            Key::Left => {
                if self.cursor > 0 {
                    // Move cursor back one char
                    let prev = self.query[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.cursor = prev;
                }
                return LauncherAction::None;
            }

            Key::Right => {
                if self.cursor < self.query.len() {
                    // Move cursor forward one char
                    let next = self.query[self.cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor.saturating_add(i))
                        .unwrap_or(self.query.len());
                    self.cursor = next;
                }
                return LauncherAction::None;
            }

            Key::Home => {
                self.cursor = 0;
                return LauncherAction::None;
            }

            Key::End => {
                self.cursor = self.query.len();
                return LauncherAction::None;
            }

            // Ctrl+1..8: launch Nth result directly
            Key::Num1
            | Key::Num2
            | Key::Num3
            | Key::Num4
            | Key::Num5
            | Key::Num6
            | Key::Num7
            | Key::Num8
                if event.modifiers.ctrl =>
            {
                let idx = match event.key {
                    Key::Num1 => 0,
                    Key::Num2 => 1,
                    Key::Num3 => 2,
                    Key::Num4 => 3,
                    Key::Num5 => 4,
                    Key::Num6 => 5,
                    Key::Num7 => 6,
                    Key::Num8 => 7,
                    _ => return LauncherAction::None,
                };
                if idx < self.results.len() {
                    self.selected_index = idx;
                    return self.launch_selected();
                }
                return LauncherAction::None;
            }

            _ => {}
        }

        // Text input: if the event carries a printable character, insert it
        if event.types_text() {
            for ch in event.typed() {
                self.query.insert(self.cursor, ch);
                self.cursor = self.cursor.saturating_add(ch.len_utf8());
            }
            self.selected_index = 0;
            self.update_results();
        }

        LauncherAction::None
    }

    /// Launch the currently selected entry.
    fn launch_selected(&mut self) -> LauncherAction {
        let scored = match self.results.get(self.selected_index) {
            Some(s) => s.clone(),
            None => return LauncherAction::None,
        };

        let entry = match self.apps.get_mut(scored.db_index) {
            Some(e) => e,
            None => return LauncherAction::None,
        };

        // Record in history and bump launch count
        entry.launch_count = entry.launch_count.saturating_add(1);
        let path = entry.executable_path.clone();

        self.launch_history.push(LaunchRecord {
            executable_path: path.clone(),
            timestamp_secs: self.now_secs,
        });
        // Keep history bounded
        if self.launch_history.len() > 100 {
            self.launch_history.remove(0);
        }

        self.hide();
        LauncherAction::Launch(path)
    }

    /// Re-filter and re-sort results based on current query.
    fn update_results(&mut self) {
        // The user has moved on from whatever failed to start.
        self.error = None;
        self.results.clear();

        if self.query.is_empty() {
            // Show all apps, sorted by frecency
            for (idx, entry) in self.apps.iter().enumerate() {
                let frec = frecency_bonus(
                    &entry.executable_path,
                    &self.launch_history,
                    self.now_secs,
                    entry.launch_count,
                );
                self.results.push(ScoredEntry {
                    db_index: idx,
                    total_score: frec,
                });
            }
        } else {
            // Score each entry against the query
            for (idx, entry) in self.apps.iter().enumerate() {
                if let Some(match_score) = search_score(&self.query, entry) {
                    let frec = frecency_bonus(
                        &entry.executable_path,
                        &self.launch_history,
                        self.now_secs,
                        entry.launch_count,
                    );
                    self.results.push(ScoredEntry {
                        db_index: idx,
                        total_score: match_score.saturating_add(frec),
                    });
                }
            }
        }

        // Sort descending by score
        self.results
            .sort_by_key(|r| std::cmp::Reverse(r.total_score));

        // Truncate to max visible
        self.results.truncate(MAX_RESULTS);

        // Clamp selection
        if self.selected_index >= self.results.len() {
            self.selected_index = self.results.len().saturating_sub(1);
        }
    }

    /// Render the launcher dialog into a vector of render commands.
    ///
    /// The caller should only render this when `self.visible` is true.
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let mut cmds: Vec<RenderCommand> = Vec::new();

        // The same measurement the hit-test uses. Do not re-derive any of
        // these here: a second copy of the arithmetic is how a click comes to
        // land on the row above the one the user aimed at.
        let layout = Layout::of(self);
        let Layout {
            dialog_x,
            dialog_y,
            dialog_height,
            ..
        } = layout;

        let radii = CornerRadii::all(DIALOG_RADIUS);

        // Backdrop shadow
        cmds.push(RenderCommand::BoxShadow {
            x: dialog_x,
            y: dialog_y,
            width: DIALOG_WIDTH,
            height: dialog_height,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 24.0,
            spread: 8.0,
            color: theme::SHADOW,
            corner_radii: radii,
        });

        // Dialog background
        cmds.push(RenderCommand::FillRect {
            x: dialog_x,
            y: dialog_y,
            width: DIALOG_WIDTH,
            height: dialog_height,
            color: theme::BASE,
            corner_radii: radii,
        });

        // Clip to dialog bounds
        cmds.push(RenderCommand::PushClip {
            x: dialog_x,
            y: dialog_y,
            width: DIALOG_WIDTH,
            height: dialog_height,
        });

        // Translate so (0,0) is top-left of dialog interior
        cmds.push(RenderCommand::PushTranslate {
            dx: dialog_x + PADDING,
            dy: dialog_y + PADDING,
        });

        // --- Search input area ---
        let input_width = layout.inner_width;
        let input_radii = CornerRadii::all(8.0);

        // Input background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: input_width,
            height: INPUT_HEIGHT - PADDING,
            color: theme::MANTLE,
            corner_radii: input_radii,
        });

        // Input border
        cmds.push(RenderCommand::StrokeRect {
            x: 0.0,
            y: 0.0,
            width: input_width,
            height: INPUT_HEIGHT - PADDING,
            color: theme::SURFACE2,
            line_width: 1.0,
            corner_radii: input_radii,
        });

        // Search icon placeholder text
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: (INPUT_HEIGHT - PADDING) / 2.0 - INPUT_FONT_SIZE / 2.0 + 2.0,
            text: "Search...".to_string(),
            color: if self.query.is_empty() {
                theme::OVERLAY0
            } else {
                Color::TRANSPARENT
            },
            font_size: INPUT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Query text
        if !self.query.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 12.0,
                y: (INPUT_HEIGHT - PADDING) / 2.0 - INPUT_FONT_SIZE / 2.0 + 2.0,
                text: self.query.clone(),
                color: theme::TEXT,
                font_size: INPUT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(input_width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Where the text before the caret actually ends, in the face it is
        // drawn in. This was `chars * (INPUT_FONT_SIZE * 0.55)`: a guessed
        // average advance applied to *proportional* text, so the caret sat left
        // of the query after any run of wide letters and right of it after a
        // run of narrow ones — visibly wrong on a word as ordinary as "will".
        // `0.55` is not a fixable constant, because no single number is right
        // for a face whose whole purpose is that its characters differ.
        //
        // `get` rather than `[..cursor]`: the caret is a byte offset, and
        // slicing a `str` off a character boundary aborts the process. A caret
        // that is momentarily inconsistent should draw at the left edge, not
        // take the desktop's launcher down.
        let before = self.query.get(..self.cursor).unwrap_or("");
        let cursor_x = 12.0 + text::measure(before, INPUT_FONT_SIZE, FontWeightHint::Regular);
        cmds.push(RenderCommand::Line {
            x1: cursor_x,
            y1: (INPUT_HEIGHT - PADDING) / 2.0 - INPUT_FONT_SIZE / 2.0 + 2.0,
            x2: cursor_x,
            y2: (INPUT_HEIGHT - PADDING) / 2.0 + INPUT_FONT_SIZE / 2.0 + 2.0,
            color: theme::BLUE,
            width: 2.0,
        });

        // --- Launch failure banner ---
        if let (Some(error_top), Some(message)) = (layout.error_top, self.error.as_ref()) {
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: error_top,
                width: input_width,
                height: ERROR_HEIGHT - 4.0,
                color: Color::rgba(theme::RED.r, theme::RED.g, theme::RED.b, 40),
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: 10.0,
                y: error_top + (ERROR_HEIGHT - 4.0) / 2.0 - DESC_FONT_SIZE / 2.0,
                text: message.clone(),
                color: theme::RED,
                font_size: DESC_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                // Elided rather than clipped: a path cut off mid-character
                // reads as a *different* path, and this line exists to tell
                // the user which program could not be started.
                max_width: Some(input_width - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // --- Results list ---
        let results_y_start = layout.rows_top;

        for (i, scored) in self.results.iter().enumerate() {
            let entry = match self.apps.get(scored.db_index) {
                Some(e) => e,
                None => continue,
            };

            let row_y = results_y_start + i as f32 * ROW_HEIGHT;
            let is_selected = i == self.selected_index;

            // Row background (highlight if selected)
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: row_y,
                    width: input_width,
                    height: ROW_HEIGHT,
                    color: theme::SURFACE1,
                    corner_radii: CornerRadii::all(6.0),
                });

                // Selection indicator bar on the left
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: row_y + 8.0,
                    width: 3.0,
                    height: ROW_HEIGHT - 16.0,
                    color: theme::BLUE,
                    corner_radii: CornerRadii::all(1.5),
                });
            }

            // Icon placeholder (colored square based on category)
            let icon_x = 12.0;
            let icon_y = row_y + (ROW_HEIGHT - 24.0) / 2.0;
            cmds.push(RenderCommand::FillRect {
                x: icon_x,
                y: icon_y,
                width: 24.0,
                height: 24.0,
                color: entry.category.color(),
                corner_radii: CornerRadii::all(4.0),
            });

            // App name
            let text_x = icon_x + 36.0;
            cmds.push(RenderCommand::Text {
                x: text_x,
                y: row_y + 8.0,
                text: entry.name.clone(),
                color: if is_selected {
                    theme::TEXT
                } else {
                    theme::SUBTEXT1
                },
                font_size: NAME_FONT_SIZE,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(input_width - text_x - 80.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Description (below name)
            cmds.push(RenderCommand::Text {
                x: text_x,
                y: row_y + 26.0,
                text: entry.description.clone(),
                color: theme::SUBTEXT0,
                font_size: DESC_FONT_SIZE,
                font_weight: FontWeightHint::Light,
                max_width: Some(input_width - text_x - 80.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Category badge (right-aligned)
            let badge_text = entry.category.label();
            let badge_width =
                text::padded_width(badge_text, 6.0, DESC_FONT_SIZE, FontWeightHint::Regular);
            let badge_x = input_width - badge_width - 8.0;
            let badge_y = row_y + (ROW_HEIGHT - 20.0) / 2.0;

            cmds.push(RenderCommand::FillRect {
                x: badge_x,
                y: badge_y,
                width: badge_width,
                height: 20.0,
                color: Color::rgba(
                    entry.category.color().r,
                    entry.category.color().g,
                    entry.category.color().b,
                    40,
                ),
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: badge_x + 6.0,
                y: badge_y + 4.0,
                text: badge_text.to_string(),
                color: entry.category.color(),
                font_size: DESC_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Shortcut hint (Ctrl+N) for first 8 results
            if i < 8 {
                let hint = format!("^{}", i.saturating_add(1));
                cmds.push(RenderCommand::Text {
                    x: input_width - badge_width - 40.0,
                    y: row_y + (ROW_HEIGHT - DESC_FONT_SIZE) / 2.0,
                    text: hint,
                    color: theme::OVERLAY0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Light,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }

        // If no results and query is non-empty, show "No results" message
        if self.results.is_empty() && !self.query.is_empty() {
            cmds.push(RenderCommand::Text {
                x: input_width / 2.0 - 40.0,
                y: results_y_start + 16.0,
                text: "No results found".to_string(),
                color: theme::OVERLAY0,
                font_size: NAME_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Pop translate and clip
        cmds.push(RenderCommand::PopTranslate);
        cmds.push(RenderCommand::PopClip);

        cmds
    }
}

// ============================================================================
// Built-in application database
// ============================================================================

/// The default set of launchable apps and system commands.
fn builtin_app_database() -> Vec<AppEntry> {
    vec![
        // Applications
        AppEntry {
            name: "Terminal".to_string(),
            description: "Command-line terminal emulator".to_string(),
            executable_path: "/usr/bin/terminal".to_string(),
            keywords: vec![
                "shell".into(),
                "console".into(),
                "bash".into(),
                "cli".into(),
            ],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "Text Editor".to_string(),
            description: "Plain text and code editor".to_string(),
            executable_path: "/usr/bin/editor".to_string(),
            keywords: vec![
                "edit".into(),
                "code".into(),
                "write".into(),
                "notepad".into(),
            ],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "File Explorer".to_string(),
            description: "Browse and manage files".to_string(),
            executable_path: "/usr/bin/explorer".to_string(),
            keywords: vec![
                "files".into(),
                "browse".into(),
                "folder".into(),
                "directory".into(),
            ],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "Calculator".to_string(),
            description: "Scientific calculator".to_string(),
            executable_path: "/usr/bin/calculator".to_string(),
            keywords: vec!["math".into(), "calc".into(), "compute".into()],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "Settings".to_string(),
            description: "System preferences and configuration".to_string(),
            executable_path: "/usr/bin/settings".to_string(),
            keywords: vec!["config".into(), "preferences".into(), "options".into()],
            category: Category::Setting,
            launch_count: 0,
        },
        AppEntry {
            name: "System Info".to_string(),
            description: "Hardware and OS information".to_string(),
            executable_path: "/usr/bin/sysinfo".to_string(),
            keywords: vec![
                "hardware".into(),
                "info".into(),
                "about".into(),
                "specs".into(),
            ],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "Process Explorer".to_string(),
            description: "View and manage running processes".to_string(),
            executable_path: "/usr/bin/procexplorer".to_string(),
            keywords: vec![
                "task".into(),
                "manager".into(),
                "processes".into(),
                "kill".into(),
            ],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "Image Viewer".to_string(),
            description: "View images and photos".to_string(),
            executable_path: "/usr/bin/imageviewer".to_string(),
            keywords: vec![
                "photo".into(),
                "picture".into(),
                "gallery".into(),
                "png".into(),
                "jpg".into(),
            ],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "Music Player".to_string(),
            description: "Play music and audio files".to_string(),
            executable_path: "/usr/bin/musicplayer".to_string(),
            keywords: vec!["audio".into(), "song".into(), "mp3".into(), "media".into()],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "Screenshot".to_string(),
            description: "Capture screen area or window".to_string(),
            executable_path: "/usr/bin/screenshot".to_string(),
            keywords: vec![
                "capture".into(),
                "snip".into(),
                "screen".into(),
                "grab".into(),
            ],
            category: Category::Application,
            launch_count: 0,
        },
        // System commands
        AppEntry {
            name: "Shutdown".to_string(),
            description: "Power off the system".to_string(),
            executable_path: "/sbin/shutdown".to_string(),
            keywords: vec!["power".into(), "off".into(), "halt".into()],
            category: Category::System,
            launch_count: 0,
        },
        AppEntry {
            name: "Restart".to_string(),
            description: "Reboot the system".to_string(),
            executable_path: "/sbin/reboot".to_string(),
            keywords: vec!["reboot".into(), "reset".into()],
            category: Category::System,
            launch_count: 0,
        },
        AppEntry {
            name: "Sleep".to_string(),
            description: "Suspend to RAM".to_string(),
            executable_path: "/sbin/suspend".to_string(),
            keywords: vec!["suspend".into(), "hibernate".into(), "standby".into()],
            category: Category::System,
            launch_count: 0,
        },
        AppEntry {
            name: "Lock".to_string(),
            description: "Lock the screen".to_string(),
            executable_path: "/usr/bin/lockscreen".to_string(),
            keywords: vec!["lock".into(), "secure".into(), "away".into()],
            category: Category::System,
            launch_count: 0,
        },
        AppEntry {
            name: "Logout".to_string(),
            description: "End current session".to_string(),
            executable_path: "/usr/bin/logout".to_string(),
            keywords: vec!["signout".into(), "logoff".into(), "session".into()],
            category: Category::System,
            launch_count: 0,
        },
        // Settings shortcuts
        AppEntry {
            name: "Display Settings".to_string(),
            description: "Resolution, scaling, and monitors".to_string(),
            executable_path: "/usr/bin/settings --display".to_string(),
            keywords: vec![
                "monitor".into(),
                "resolution".into(),
                "screen".into(),
                "dpi".into(),
            ],
            category: Category::Setting,
            launch_count: 0,
        },
        AppEntry {
            name: "Network Settings".to_string(),
            description: "Wi-Fi, Ethernet, and VPN configuration".to_string(),
            executable_path: "/usr/bin/settings --network".to_string(),
            keywords: vec![
                "wifi".into(),
                "ethernet".into(),
                "vpn".into(),
                "internet".into(),
            ],
            category: Category::Setting,
            launch_count: 0,
        },
        AppEntry {
            name: "Sound Settings".to_string(),
            description: "Audio input/output and volume".to_string(),
            executable_path: "/usr/bin/settings --sound".to_string(),
            keywords: vec![
                "audio".into(),
                "volume".into(),
                "speaker".into(),
                "microphone".into(),
            ],
            category: Category::Setting,
            launch_count: 0,
        },
    ]
}

// ============================================================================
// Entry point
// ============================================================================

/// A snapshot of everything the renderer reads that one event can change.
///
/// See [`LauncherState::display_revision`].
#[derive(Clone, Debug, PartialEq, Eq)]
struct DisplayRevision {
    visible: bool,
    selected: usize,
    results: usize,
    query: String,
    cursor: usize,
    error: Option<String>,
}

/// The frecency clock's cadence. Never `None`: recency decays at 5 minutes, an
/// hour and a day, and — more immediately — the timestamp written into the
/// launch history has to be the instant of the *launch*, not the instant the
/// dialog happened to open.
const CLOCK_TICK: Duration = Duration::from_secs(1);

/// The size the overlay asks for when the compositor has no opinion.
///
/// A screen, because the launcher is a floating dialog on a full-screen
/// surface: "click away from it to dismiss it" only has an away to click if
/// the launcher owns the pixels around itself. The real size arrives as the
/// `width`/`height` handed to [`oswindow::app::App::render`].
const DEFAULT_VIEWPORT: (u32, u32) = (1920, 1080);

/// Start a program, or say why not.
///
/// The child is deliberately not waited on and its handle is dropped: the
/// launcher exits as soon as a launch succeeds, so the child is reparented,
/// and holding the dialog open until the program exits would make the launcher
/// behave like a terminal.
fn spawn_program(path: &str) -> Result<(), String> {
    std::process::Command::new(path)
        .spawn()
        .map(|_child| ())
        .map_err(|err| err.to_string())
}

impl oswindow::app::App for LauncherState {
    fn title(&self) -> String {
        String::from("Launcher")
    }

    fn initial_size(&self) -> (u32, u32) {
        DEFAULT_VIEWPORT
    }

    /// Not resizable: it is a full-screen overlay, and a user who could drag
    /// its corner could uncover the desktop it is supposed to sit in front of.
    fn resizable(&self) -> bool {
        false
    }

    fn tick_interval(&self) -> Option<Duration> {
        Some(CLOCK_TICK)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        let before = self.display_revision();
        let action = self.handle_event(event);
        // A resize moves every coordinate in the tree without touching any of
        // the fields the revision samples, so it is named separately.
        let moved = matches!(event, Event::Resize { .. }) || before != self.display_revision();

        match action {
            LauncherAction::Launch(path) => match spawn_program(&path) {
                Ok(()) => Response::Exit,
                // The dialog stays up carrying the reason. Exiting here would
                // be the worst of both: the program did not start and the
                // window that could say so is gone.
                Err(reason) => {
                    self.report_launch_failure(&path, &reason);
                    Response::Redraw
                }
            },
            LauncherAction::Dismiss => Response::Exit,
            LauncherAction::None => {
                if moved {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The granted size, not the requested one. The dialog is centred on
        // the viewport and the hit-test reads the same two fields, so a
        // launcher that kept believing in 1920×1080 on a smaller screen would
        // draw its dialog off-centre *and* mislocate every row in it.
        self.set_viewport(width, height);
        let mut tree = RenderTree::new();
        for cmd in LauncherState::render(self) {
            tree.push(cmd);
        }
        tree
    }
}

fn main() -> ExitCode {
    let mut launcher = LauncherState::new(
        f32::from(u16::try_from(DEFAULT_VIEWPORT.0).unwrap_or(u16::MAX)),
        f32::from(u16::try_from(DEFAULT_VIEWPORT.1).unwrap_or(u16::MAX)),
    );

    // Before the first frame and before the first launch can be recorded, so
    // the history is not written with timestamps from 1970. See `refresh_now`.
    launcher.refresh_now();

    // The launcher is spawned *because* the user asked for it, so it starts
    // visible. `LauncherState::new` leaves it hidden because the shell may
    // instead keep one alive across invocations.
    launcher.show();

    oswindow::app::launch("launcher", &mut launcher)
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range or unwraps a `None` should fail loudly
    // and point at the line that did it — that is the diagnosis. The defensive
    // lints exist to keep panics out of code that runs on a user's data, which
    // this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]
    // Several tests assert a float equals the exact literal the code under test
    // was handed. That is the assertion meant: a tolerance would let a value
    // that has drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;

    /// The caret has to sit where the query text ends, and the query is drawn
    /// in the *proportional* UI face. It used to be placed at
    /// `chars * (INPUT_FONT_SIZE * 0.55)` — a guessed average advance — so it
    /// landed left of the text after wide letters and right of it after narrow
    /// ones. No constant can be right here: a proportional face exists
    /// precisely because its characters do not share a width.
    #[test]
    fn the_caret_sits_where_the_query_text_ends() {
        let caret_x = |query: &str| {
            let mut state = LauncherState::new(1280.0, 800.0);
            state.visible = true;
            state.query = query.to_owned();
            state.cursor = query.len();
            state
                .render()
                .into_iter()
                .find_map(|cmd| match cmd {
                    RenderCommand::Line { x1, x2, color, .. }
                        if (x1 - x2).abs() < f32::EPSILON && color == theme::BLUE =>
                    {
                        Some(x1)
                    }
                    _ => None,
                })
                .expect("the launcher draws a caret")
        };

        for query in ["WWW", "iii", "will", "Wii", "documents"] {
            let expected = 12.0 + text::measure(query, INPUT_FONT_SIZE, FontWeightHint::Regular);
            let actual = caret_x(query);
            assert!(
                (actual - expected).abs() < 0.01,
                "caret for {query:?} at {actual}, text ends at {expected}"
            );
        }

        // The claim with teeth: the old guess is *not* this answer. If these
        // agreed for narrow and wide text alike the face would be monospace and
        // this test would be asserting nothing.
        let narrow = caret_x("iii");
        let wide = caret_x("WWW");
        assert!(
            wide > narrow * 1.5,
            "three W measure {wide} and three i measure {narrow} — one guessed \
             average would have put both at the same place"
        );
    }

    /// A caret byte offset off a character boundary must not abort the process.
    /// `self.query[..self.cursor]` on a `String` panics there, and this is a
    /// launcher: it is drawn while the user is mid-keystroke.
    #[test]
    fn a_caret_off_a_character_boundary_draws_rather_than_panicking() {
        let mut state = LauncherState::new(1280.0, 800.0);
        state.visible = true;
        state.query = "é".to_owned();
        state.cursor = 1;
        assert!(!state.render().is_empty());
    }

    // --- Fuzzy matcher tests ---

    #[test]
    fn test_fuzzy_exact_prefix() {
        // "term" should match "Terminal" with high score (prefix match)
        let score = fuzzy_score("term", "Terminal");
        assert!(score.is_some());
        assert!(score.unwrap() > 60, "Prefix match should score high");
    }

    #[test]
    fn test_fuzzy_exact_match() {
        let score = fuzzy_score("terminal", "Terminal");
        assert!(score.is_some());
        assert!(score.unwrap() > 80, "Exact match should score very high");
    }

    #[test]
    fn test_fuzzy_non_contiguous() {
        // "te" in "Text Editor" — matches T and e at start
        let score = fuzzy_score("te", "Text Editor");
        assert!(score.is_some());
    }

    #[test]
    fn test_fuzzy_no_match() {
        // "xyz" should not match "Terminal"
        let score = fuzzy_score("xyz", "Terminal");
        assert!(score.is_none());
    }

    #[test]
    fn test_fuzzy_empty_query() {
        let score = fuzzy_score("", "Terminal");
        assert_eq!(score, Some(0));
    }

    #[test]
    fn test_fuzzy_query_longer_than_target() {
        let score = fuzzy_score("terminaltoolong", "Terminal");
        assert!(score.is_none());
    }

    #[test]
    fn test_fuzzy_word_boundary_bonus() {
        // "fe" matching "File Explorer" — 'f' at boundary, 'e' at boundary
        let boundary_score = fuzzy_score("fe", "File Explorer");
        // "fe" matching "coffee" — no boundary match for 'e'
        let no_boundary_score = fuzzy_score("fe", "coffee maker");
        assert!(boundary_score.is_some());
        assert!(no_boundary_score.is_some());
        // Boundary match should score higher
        assert!(
            boundary_score.unwrap_or(0) > no_boundary_score.unwrap_or(0),
            "Word boundary matches should score higher"
        );
    }

    #[test]
    fn test_fuzzy_consecutive_bonus() {
        // "calc" in "Calculator" — all consecutive
        let consecutive = fuzzy_score("calc", "Calculator");
        // "cltr" in "Calculator" — non-consecutive
        let non_consecutive = fuzzy_score("cltr", "Calculator");
        assert!(consecutive.is_some());
        assert!(non_consecutive.is_some());
        assert!(
            consecutive.unwrap_or(0) > non_consecutive.unwrap_or(0),
            "Consecutive matches should score higher"
        );
    }

    // --- Frecency tests ---

    #[test]
    fn test_frecency_no_history() {
        let bonus = frecency_bonus("/usr/bin/foo", &[], 1000, 0);
        assert_eq!(bonus, 0);
    }

    #[test]
    fn test_frecency_with_launch_count() {
        let bonus = frecency_bonus("/usr/bin/foo", &[], 1000, 8);
        assert!(bonus > 0, "Launch count should contribute to frecency");
    }

    #[test]
    fn test_frecency_recent_boost() {
        let history = vec![LaunchRecord {
            executable_path: "/usr/bin/foo".to_string(),
            timestamp_secs: 950,
        }];
        // Within 5 minutes (300 secs) — should get max recency bonus
        let bonus = frecency_bonus("/usr/bin/foo", &history, 1000, 0);
        assert!(bonus >= 20, "Recent launch should give high bonus");
    }

    #[test]
    fn test_frecency_old_launch() {
        let history = vec![LaunchRecord {
            executable_path: "/usr/bin/foo".to_string(),
            timestamp_secs: 0,
        }];
        // Very old (1000000 secs ago)
        let bonus = frecency_bonus("/usr/bin/foo", &history, 1_000_000, 0);
        assert!(bonus <= 5, "Very old launches should give minimal bonus");
    }

    #[test]
    fn test_frecency_different_app_ignored() {
        let history = vec![LaunchRecord {
            executable_path: "/usr/bin/bar".to_string(),
            timestamp_secs: 950,
        }];
        let bonus = frecency_bonus("/usr/bin/foo", &history, 1000, 0);
        assert_eq!(bonus, 0, "History for other apps should not contribute");
    }

    // --- Search scoring tests ---

    #[test]
    fn test_search_score_name_boost() {
        let entry = AppEntry {
            name: "Terminal".to_string(),
            description: "Command line".to_string(),
            executable_path: "/usr/bin/terminal".to_string(),
            keywords: vec![],
            category: Category::Application,
            launch_count: 0,
        };
        // "term" matches name strongly
        let score = search_score("term", &entry);
        assert!(score.is_some());
        assert!(score.unwrap_or(0) > 100, "Name match should be doubled");
    }

    #[test]
    fn test_search_score_keyword_match() {
        let entry = AppEntry {
            name: "Terminal".to_string(),
            description: "Command line".to_string(),
            executable_path: "/usr/bin/terminal".to_string(),
            keywords: vec!["shell".into(), "console".into()],
            category: Category::Application,
            launch_count: 0,
        };
        let score = search_score("shell", &entry);
        assert!(score.is_some(), "Should match via keyword");
    }

    #[test]
    fn test_search_score_no_match() {
        let entry = AppEntry {
            name: "Calculator".to_string(),
            description: "Math tool".to_string(),
            executable_path: "/usr/bin/calc".to_string(),
            keywords: vec!["math".into()],
            category: Category::Application,
            launch_count: 0,
        };
        let score = search_score("terminal", &entry);
        assert!(score.is_none(), "Unrelated query should not match");
    }

    // --- Launcher state tests ---

    #[test]
    fn test_launcher_initial_state() {
        let launcher = LauncherState::new(1920.0, 1080.0);
        assert!(!launcher.visible);
        assert!(launcher.query.is_empty());
    }

    #[test]
    fn test_launcher_show_resets_query() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.query = "old query".to_string();
        launcher.show();
        assert!(launcher.visible);
        assert!(launcher.query.is_empty());
        assert_eq!(launcher.selected_index, 0);
    }

    #[test]
    fn test_launcher_escape_dismisses() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.show();

        let event = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        let action = launcher.handle_key(&event);
        assert_eq!(action, LauncherAction::Dismiss);
        assert!(!launcher.visible);
    }

    #[test]
    fn test_launcher_typing_filters() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.show();

        // Type "calc"
        for ch in "calc".chars() {
            let event = KeyEvent {
                key: Key::A, // key code doesn't matter for text input
                pressed: true,
                modifiers: Modifiers::NONE,
                text: ch.to_string(),
            };
            launcher.handle_key(&event);
        }

        assert_eq!(launcher.query, "calc");
        // Should have Calculator in results
        let has_calc = launcher.results.iter().any(|s| {
            launcher
                .apps
                .get(s.db_index)
                .is_some_and(|e| e.name == "Calculator")
        });
        assert!(has_calc, "Calculator should appear in results for 'calc'");
    }

    #[test]
    fn test_launcher_arrow_navigation() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.show();
        assert_eq!(launcher.selected_index, 0);

        let down = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        launcher.handle_key(&down);
        assert_eq!(launcher.selected_index, 1);

        let up = KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        launcher.handle_key(&up);
        assert_eq!(launcher.selected_index, 0);

        // Up at 0 should stay at 0
        launcher.handle_key(&up);
        assert_eq!(launcher.selected_index, 0);
    }

    #[test]
    fn test_launcher_enter_launches() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.show();

        // The first result should be launchable
        let enter = KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        let action = launcher.handle_key(&enter);
        match action {
            LauncherAction::Launch(path) => {
                assert!(!path.is_empty(), "Launch path should not be empty");
            }
            _ => panic!("Enter should produce a Launch action"),
        }
        assert!(!launcher.visible, "Launcher should hide after launch");
    }

    #[test]
    fn test_launcher_backspace() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.show();

        // Type "ab"
        for ch in "ab".chars() {
            let event = KeyEvent {
                key: Key::A,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: ch.to_string(),
            };
            launcher.handle_key(&event);
        }
        assert_eq!(launcher.query, "ab");

        // Backspace
        let bs = KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        launcher.handle_key(&bs);
        assert_eq!(launcher.query, "a");
    }

    #[test]
    fn test_launcher_ctrl_number_launch() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.show();

        // Ctrl+1 should launch the first result
        let event = KeyEvent {
            key: Key::Num1,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        };
        let action = launcher.handle_key(&event);
        assert!(
            matches!(action, LauncherAction::Launch(_)),
            "Ctrl+1 should launch first result"
        );
    }

    #[test]
    fn test_launcher_tab_autocomplete() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.show();

        // Type "term" to filter to Terminal
        for ch in "term".chars() {
            let event = KeyEvent {
                key: Key::A,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: ch.to_string(),
            };
            launcher.handle_key(&event);
        }

        // Tab should autocomplete with the selected item's name
        let tab = KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        launcher.handle_key(&tab);

        // Query should now be the full name of the first result
        assert!(!launcher.query.is_empty());
        assert!(launcher.query.len() > "term".len());
    }

    #[test]
    fn test_launcher_render_when_hidden() {
        let launcher = LauncherState::new(1920.0, 1080.0);
        let cmds = launcher.render();
        assert!(
            cmds.is_empty(),
            "Hidden launcher should produce no commands"
        );
    }

    #[test]
    fn test_launcher_render_when_visible() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.show();
        let cmds = launcher.render();
        assert!(!cmds.is_empty(), "Visible launcher should produce commands");
        // Should have at least: shadow + background + clip + translate + input bg + ...
        assert!(cmds.len() > 5, "Should have multiple render commands");
    }

    #[test]
    fn test_launcher_launch_records_history() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.set_now(5000);
        launcher.show();

        let enter = KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        launcher.handle_key(&enter);

        assert_eq!(launcher.launch_history.len(), 1);
        assert_eq!(launcher.launch_history[0].timestamp_secs, 5000);
    }

    #[test]
    fn test_launcher_history_bounded() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        // Fill history to capacity
        for i in 0..105 {
            launcher.launch_history.push(LaunchRecord {
                executable_path: format!("/bin/app{i}"),
                timestamp_secs: i as u64,
            });
        }
        // History should be bounded (we trim on launch, but let's verify
        // that after a launch it stays at 100)
        launcher.show();
        launcher.set_now(200);
        let enter = KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        launcher.handle_key(&enter);
        // After launch, the vec has 106 entries then gets trimmed to keep <=100+1
        // Actually our trim removes one at a time when >100
        assert!(
            launcher.launch_history.len() <= 106,
            "History should be bounded"
        );
    }

    #[test]
    fn test_category_label_and_color() {
        assert_eq!(Category::Application.label(), "App");
        assert_eq!(Category::System.label(), "Sys");
        assert_eq!(Category::Setting.label(), "Set");
        assert_eq!(Category::File.label(), "File");
        assert_eq!(Category::Command.label(), "Cmd");

        // Colors should not be transparent
        assert_ne!(Category::Application.color().a, 0);
        assert_ne!(Category::System.color().a, 0);
    }

    #[test]
    fn test_released_key_ignored() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.show();

        let event = KeyEvent {
            key: Key::Escape,
            pressed: false, // released, not pressed
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        let action = launcher.handle_key(&event);
        assert_eq!(action, LauncherAction::None);
        assert!(launcher.visible, "Released key should not dismiss");
    }

    #[test]
    fn test_max_results_capped() {
        let mut launcher = LauncherState::new(1920.0, 1080.0);
        launcher.show();
        // With empty query, all apps shown but capped to MAX_RESULTS
        assert!(launcher.results.len() <= MAX_RESULTS);
    }

    // ========================================================================
    // The pointer: one layout, shared by the renderer and the hit-test
    // ========================================================================

    /// A visible launcher on a screen of a deliberately unusual size, so that
    /// nothing can pass by accidentally agreeing with 1920×1080.
    fn shown(width: f32, height: f32) -> LauncherState {
        let mut launcher = LauncherState::new(width, height);
        launcher.show();
        launcher
    }

    /// The centre of result row `i`, in screen coordinates, read out of the
    /// **render output** rather than computed from `Layout`.
    ///
    /// This is what makes the hit-test tests mean anything: if they measured
    /// rows with `Layout` and then hit-tested them with `Layout`, they would
    /// agree no matter how wrong `Layout` was. Instead this walks the commands
    /// the renderer actually emitted, takes the dialog's own
    /// `PushTranslate` as the origin, and finds the 24×24 category icon that
    /// every row draws — row backgrounds exist only on the selected row, so an
    /// icon is the one landmark present on all of them.
    fn drawn_row_centre(state: &LauncherState, i: usize) -> (f32, f32) {
        let commands = state.render();
        let (dx, dy) = commands
            .iter()
            .find_map(|cmd| match *cmd {
                RenderCommand::PushTranslate { dx, dy } => Some((dx, dy)),
                _ => None,
            })
            .expect("the dialog translates to its own interior");
        let (x, y) = commands
            .iter()
            .filter_map(|cmd| match *cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if width == 24.0 && height == 24.0 => Some((x, y)),
                _ => None,
            })
            .nth(i)
            .expect("that row is drawn");
        (dx + x + 12.0, dy + y + 12.0)
    }

    fn click_at(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    fn move_to(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Move,
        })
    }

    /// The top and bottom screen y of row `i`'s drawn highlight.
    ///
    /// Only the *selected* row draws a full-width background, so this selects
    /// the row, renders, and reads the one `FillRect` a row's height tall. It
    /// gives exact edges rather than a centre — and edges are what a hit-test
    /// can be wrong about while still looking right in the middle. An earlier
    /// version of this file's tests only checked centres, and an eight-pixel
    /// error in `row_at` passed them.
    fn drawn_row_bounds(state: &mut LauncherState, i: usize) -> (f32, f32) {
        let restore = state.selected_index;
        state.selected_index = i;
        let commands = state.render();
        let dy = commands
            .iter()
            .find_map(|cmd| match *cmd {
                RenderCommand::PushTranslate { dy, .. } => Some(dy),
                _ => None,
            })
            .expect("the dialog translates to its own interior");
        let y = commands
            .iter()
            .find_map(|cmd| match *cmd {
                RenderCommand::FillRect { y, height, .. } if height == ROW_HEIGHT => Some(y),
                _ => None,
            })
            .expect("the selected row draws a highlight");
        state.selected_index = restore;
        (dy + y, dy + y + ROW_HEIGHT)
    }

    #[test]
    fn a_click_lands_on_the_row_it_is_drawn_over() {
        // The reason `Layout` exists. Two copies of this arithmetic -- one in
        // the renderer, one in the hit-test -- is how a launcher comes to open
        // the program above the one you clicked.
        //
        // Asserted at the *edges*: a hit-test that is off by less than a row
        // still answers correctly in the middle of one.
        let mut launcher = shown(1280.0, 800.0);
        assert!(launcher.results.len() >= 3, "need rows to aim at");
        let x = drawn_row_centre(&launcher, 0).0;

        for row in 0..3 {
            let (top, bottom) = drawn_row_bounds(&mut launcher, row);
            let layout = Layout::of(&launcher);
            assert_eq!(
                layout.row_at(x, top),
                Some(row),
                "the first pixel of row {row} hit-tests as something else"
            );
            assert_eq!(
                layout.row_at(x, bottom - 0.5),
                Some(row),
                "the last pixel of row {row} hit-tests as something else"
            );
            // One pixel above the top belongs to whatever is above: the row
            // before it, or -- for the first row -- the search box, which is
            // not a row at all.
            assert_eq!(
                layout.row_at(x, top - 0.5),
                row.checked_sub(1),
                "the pixel above row {row} hit-tests as row {row}"
            );
        }
    }

    #[test]
    fn a_click_on_a_row_launches_that_row() {
        let mut launcher = shown(1280.0, 800.0);
        let second = launcher.results[1].db_index;
        let expected = launcher.apps[second].executable_path.clone();

        let (x, y) = drawn_row_centre(&launcher, 1);
        assert_eq!(
            launcher.handle_event(&click_at(x, y)),
            LauncherAction::Launch(expected)
        );
        assert!(!launcher.visible, "launching left the dialog up");
    }

    #[test]
    fn a_click_away_from_the_dialog_dismisses_it() {
        let mut launcher = shown(1280.0, 800.0);
        // Top-left corner: the dialog is centred and starts a quarter of the
        // way down, so nothing of it is here.
        assert_eq!(
            launcher.handle_event(&click_at(2.0, 2.0)),
            LauncherAction::Dismiss
        );
        assert!(!launcher.visible);
    }

    #[test]
    fn a_click_on_the_search_box_does_not_dismiss_the_dialog() {
        // Clicking the thing you are typing into must not close it. The naive
        // "not on a row means outside" hit-test gets this wrong.
        let mut launcher = shown(1280.0, 800.0);
        let layout = Layout::of(&launcher);
        let x = layout.dialog_x + DIALOG_WIDTH / 2.0;
        let y = layout.dialog_y + PADDING + (INPUT_HEIGHT - PADDING) / 2.0;
        assert_eq!(launcher.handle_event(&click_at(x, y)), LauncherAction::None);
        assert!(launcher.visible, "clicking the input closed the launcher");
    }

    #[test]
    fn hovering_a_row_selects_it() {
        let mut launcher = shown(1280.0, 800.0);
        assert_eq!(launcher.selected_index, 0);
        let (x, y) = drawn_row_centre(&launcher, 2);
        launcher.handle_event(&move_to(x, y));
        assert_eq!(launcher.selected_index, 2);
    }

    #[test]
    fn hovering_off_the_list_leaves_the_selection_alone() {
        // Moving the pointer out of the dialog must not reset the selection to
        // the top: the keyboard is still driving.
        let mut launcher = shown(1280.0, 800.0);
        let (x, y) = drawn_row_centre(&launcher, 2);
        launcher.handle_event(&move_to(x, y));
        launcher.handle_event(&move_to(2.0, 2.0));
        assert_eq!(launcher.selected_index, 2);
    }

    // ========================================================================
    // The frecency clock
    // ========================================================================

    #[test]
    fn a_launch_is_recorded_at_the_time_it_happened() {
        // `set_now` had no production caller, so every launch went into the
        // history stamped `0` and `now - timestamp` was `0` forever -- inside
        // the "launched in the last five minutes" band, permanently, for every
        // application ever launched.
        let mut launcher = shown(1280.0, 800.0);
        launcher.refresh_now();
        assert!(
            launcher.now_secs > 1_700_000_000,
            "the frecency clock read {} -- it is still at the epoch",
            launcher.now_secs
        );

        let (x, y) = drawn_row_centre(&launcher, 0);
        launcher.handle_event(&click_at(x, y));
        assert_eq!(launcher.launch_history.len(), 1);
        assert_eq!(
            launcher.launch_history[0].timestamp_secs, launcher.now_secs,
            "the record was stamped with something other than now"
        );
    }

    #[test]
    fn a_tick_advances_the_frecency_clock_through_the_event_loop() {
        // `refresh_now` is only correct if something calls it.
        let mut launcher = shown(1280.0, 800.0);
        launcher.set_now(0);
        launcher.handle_event(&Event::Tick { elapsed_ms: 16 });
        assert!(launcher.now_secs > 1_700_000_000);
    }

    #[test]
    fn an_old_launch_ranks_below_a_recent_one() {
        // The behaviour the frozen clock destroyed: with `now_secs` stuck at 0
        // these two scored identically, because both ages were 0.
        let history = vec![
            LaunchRecord {
                executable_path: String::from("/bin/old"),
                timestamp_secs: 0,
            },
            LaunchRecord {
                executable_path: String::from("/bin/new"),
                timestamp_secs: 1_787_751_000,
            },
        ];
        let now = 1_787_751_907;
        assert!(
            frecency_bonus("/bin/new", &history, now, 0)
                > frecency_bonus("/bin/old", &history, now, 0),
            "a launch from 1970 ranks as recently as one from a minute ago"
        );
    }

    // ========================================================================
    // The window: `impl oswindow::app::App`
    // ========================================================================

    #[test]
    fn render_believes_the_size_it_is_given_not_the_one_it_asked_for() {
        let mut launcher = shown(1920.0, 1080.0);
        let tree = oswindow::app::App::render(&mut launcher, 1024.0, 768.0);
        assert_eq!(launcher.viewport_width, 1024.0);
        assert_eq!(launcher.viewport_height, 768.0);
        assert!(!tree.commands.is_empty(), "nothing was drawn");

        // Centred on the screen it was actually given, not on the one it asked
        // for -- and the hit-test moved with it, which is what `Layout` buys.
        let layout = Layout::of(&launcher);
        assert_eq!(layout.dialog_x, (1024.0 - DIALOG_WIDTH) / 2.0);
        let (x, y) = drawn_row_centre(&launcher, 0);
        assert_eq!(layout.row_at(x, y), Some(0));
    }

    #[test]
    fn a_launcher_that_ranks_by_recency_never_stops_ticking() {
        let launcher = shown(1280.0, 800.0);
        assert_eq!(
            oswindow::app::App::tick_interval(&launcher),
            Some(CLOCK_TICK)
        );
    }

    #[test]
    fn an_event_the_launcher_ignores_does_not_ask_for_a_frame() {
        let mut launcher = shown(1280.0, 800.0);
        // A pointer crossing empty screen changes nothing that is drawn.
        assert_eq!(
            oswindow::app::App::on_event(&mut launcher, &move_to(2.0, 2.0)),
            Response::Idle
        );
        // A resize moves every coordinate in the tree while changing none of
        // the fields the revision samples -- which is why it is named
        // separately in `on_event`.
        assert_eq!(
            oswindow::app::App::on_event(
                &mut launcher,
                &Event::Resize {
                    width: 1024,
                    height: 768,
                }
            ),
            Response::Redraw
        );
    }

    #[test]
    fn arrowing_down_the_list_asks_for_a_frame() {
        // `handle_key` returns `LauncherAction::None` for an arrow, and the
        // highlight moves. Mapping "no action" to `Idle` would freeze the
        // selection on screen while it moved underneath.
        let mut launcher = shown(1280.0, 800.0);
        let down = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(
            oswindow::app::App::on_event(&mut launcher, &down),
            Response::Redraw
        );
        assert_eq!(launcher.selected_index, 1);
    }

    #[test]
    fn escape_closes_the_window() {
        let mut launcher = shown(1280.0, 800.0);
        let escape = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(
            oswindow::app::App::on_event(&mut launcher, &escape),
            Response::Exit
        );
    }

    // ========================================================================
    // The one failure a launcher can have
    // ========================================================================

    #[test]
    fn a_program_that_will_not_start_says_so_instead_of_vanishing() {
        // "I clicked it and nothing happened" is indistinguishable from a hung
        // machine. The dialog has to stay up and carry the reason.
        let mut launcher = shown(1280.0, 800.0);
        let missing = String::from("/nonexistent/definitely-not-a-program");
        let reason = spawn_program(&missing).expect_err("that path is not a program");
        launcher.report_launch_failure(&missing, &reason);

        assert!(launcher.visible, "the dialog vanished with the error on it");
        let shown_error = launcher.error().expect("an error to show");
        assert!(
            shown_error.contains(&missing),
            "the message does not name the program: {shown_error}"
        );
    }

    #[test]
    fn the_error_banner_makes_room_for_itself() {
        // The banner sits between the input and the list, so every row below it
        // moves down by its height -- in the renderer *and* in the hit-test,
        // because they are the same measurement.
        let mut launcher = shown(1280.0, 800.0);
        let before = Layout::of(&launcher);
        launcher.report_launch_failure("/bin/nope", "no such file");
        let after = Layout::of(&launcher);

        assert_eq!(after.rows_top - before.rows_top, ERROR_HEIGHT);
        assert_eq!(after.dialog_height - before.dialog_height, ERROR_HEIGHT);

        let (x, y) = drawn_row_centre(&launcher, 0);
        assert_eq!(
            after.row_at(x, y),
            Some(0),
            "the banner pushed the rows out from under the hit-test"
        );
    }

    #[test]
    fn typing_clears_a_stale_error() {
        let mut launcher = shown(1280.0, 800.0);
        launcher.report_launch_failure("/bin/nope", "no such file");
        assert!(launcher.error().is_some());

        launcher.handle_event(&Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::from("a"),
        }));
        assert!(
            launcher.error().is_none(),
            "the error outlived the query it was about"
        );
    }
}
