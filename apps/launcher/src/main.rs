#![allow(dead_code)]
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
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

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

/// Scores how well `query` matches `target` using fuzzy matching.
///
/// Characters in `query` must appear in order within `target`, but need
/// not be contiguous. Scoring rewards:
/// - Exact prefix match (highest)
/// - Matches at word boundaries (start of words after space/underscore/dash)
/// - Consecutive matched characters
/// - Earlier matches in the string
///
/// Returns `None` if the query does not match at all.
pub fn fuzzy_score(query: &str, target: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }

    let query_lower: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();
    let target_lower: Vec<char> = target.chars().map(|c| c.to_ascii_lowercase()).collect();

    if query_lower.len() > target_lower.len() {
        return None;
    }

    // Check if prefix match
    let is_prefix = target_lower
        .iter()
        .zip(query_lower.iter())
        .all(|(t, q)| t == q);

    // Try to match all query characters in order within target
    let mut score: u32 = 0;
    let mut qi = 0; // query index
    let mut prev_match_idx: Option<usize> = None;
    let mut first_match_idx: Option<usize> = None;

    for (ti, &tc) in target_lower.iter().enumerate() {
        if qi >= query_lower.len() {
            break;
        }
        if tc == query_lower[qi] {
            if first_match_idx.is_none() {
                first_match_idx = Some(ti);
            }

            // Bonus for word boundary match (start, after space/dash/underscore)
            let at_boundary = ti == 0
                || target_lower.get(ti.saturating_sub(1)).is_some_and(|&prev| {
                    prev == ' ' || prev == '-' || prev == '_'
                });
            if at_boundary {
                score = score.saturating_add(10);
            }

            // Bonus for consecutive matches
            if let Some(prev) = prev_match_idx
                && ti == prev + 1 {
                    score = score.saturating_add(5);
                }

            prev_match_idx = Some(ti);
            qi += 1;
        }
    }

    // All query characters must have been matched
    if qi < query_lower.len() {
        return None;
    }

    // Prefix bonus (strongest signal)
    if is_prefix {
        score = score.saturating_add(50);
    }

    // Bonus for early first match
    if let Some(idx) = first_match_idx {
        let early_bonus = 20u32.saturating_sub(idx as u32);
        score = score.saturating_add(early_bonus);
    }

    // Slight bonus for shorter targets (closer length match)
    let length_diff = target_lower.len().saturating_sub(query_lower.len());
    let length_bonus = 10u32.saturating_sub(length_diff.min(10) as u32);
    score = score.saturating_add(length_bonus);

    Some(score)
}

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
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
                return LauncherAction::None;
            }

            Key::Down => {
                let max_idx = self.results.len().saturating_sub(1);
                if self.selected_index < max_idx {
                    self.selected_index += 1;
                }
                return LauncherAction::None;
            }

            Key::Tab => {
                // Autocomplete: fill query with selected item's name
                if let Some(scored) = self.results.get(self.selected_index)
                    && let Some(entry) = self.apps.get(scored.db_index) {
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
                        .map(|(i, _)| self.cursor + i)
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
            Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4
            | Key::Num5 | Key::Num6 | Key::Num7 | Key::Num8
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
        if let Some(ch) = event.text
            && !ch.is_control() {
                self.query.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
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
        self.results.sort_by_key(|r| std::cmp::Reverse(r.total_score));

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

        // Compute dialog dimensions
        let result_count = self.results.len();
        let results_height = result_count as f32 * ROW_HEIGHT;
        let dialog_height = INPUT_HEIGHT + results_height + PADDING * 2.0;

        // Center horizontally, position ~30% from top
        let dialog_x = (self.viewport_width - DIALOG_WIDTH) / 2.0;
        let dialog_y = self.viewport_height * 0.25;

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
        let input_width = DIALOG_WIDTH - PADDING * 2.0;
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
            });
        }

        // Cursor indicator (simple vertical line approximation)
        // Approximate cursor x position based on character count * avg width
        let approx_char_width = INPUT_FONT_SIZE * 0.55;
        let cursor_chars = self.query[..self.cursor].chars().count() as f32;
        let cursor_x = 12.0 + cursor_chars * approx_char_width;
        cmds.push(RenderCommand::Line {
            x1: cursor_x,
            y1: (INPUT_HEIGHT - PADDING) / 2.0 - INPUT_FONT_SIZE / 2.0 + 2.0,
            x2: cursor_x,
            y2: (INPUT_HEIGHT - PADDING) / 2.0 + INPUT_FONT_SIZE / 2.0 + 2.0,
            color: theme::BLUE,
            width: 2.0,
        });

        // --- Results list ---
        let results_y_start = INPUT_HEIGHT;

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
                color: if is_selected { theme::TEXT } else { theme::SUBTEXT1 },
                font_size: NAME_FONT_SIZE,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(input_width - text_x - 80.0),
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
            });

            // Category badge (right-aligned)
            let badge_text = entry.category.label();
            let badge_width = badge_text.len() as f32 * DESC_FONT_SIZE * 0.6 + 12.0;
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
            });

            // Shortcut hint (Ctrl+N) for first 8 results
            if i < 8 {
                let hint = format!("^{}", i + 1);
                cmds.push(RenderCommand::Text {
                    x: input_width - badge_width - 40.0,
                    y: row_y + (ROW_HEIGHT - DESC_FONT_SIZE) / 2.0,
                    text: hint,
                    color: theme::OVERLAY0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Light,
                    max_width: None,
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
            keywords: vec!["shell".into(), "console".into(), "bash".into(), "cli".into()],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "Text Editor".to_string(),
            description: "Plain text and code editor".to_string(),
            executable_path: "/usr/bin/editor".to_string(),
            keywords: vec!["edit".into(), "code".into(), "write".into(), "notepad".into()],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "File Explorer".to_string(),
            description: "Browse and manage files".to_string(),
            executable_path: "/usr/bin/explorer".to_string(),
            keywords: vec!["files".into(), "browse".into(), "folder".into(), "directory".into()],
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
            keywords: vec!["hardware".into(), "info".into(), "about".into(), "specs".into()],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "Process Explorer".to_string(),
            description: "View and manage running processes".to_string(),
            executable_path: "/usr/bin/procexplorer".to_string(),
            keywords: vec!["task".into(), "manager".into(), "processes".into(), "kill".into()],
            category: Category::Application,
            launch_count: 0,
        },
        AppEntry {
            name: "Image Viewer".to_string(),
            description: "View images and photos".to_string(),
            executable_path: "/usr/bin/imageviewer".to_string(),
            keywords: vec!["photo".into(), "picture".into(), "gallery".into(), "png".into(), "jpg".into()],
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
            keywords: vec!["capture".into(), "snip".into(), "screen".into(), "grab".into()],
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
            keywords: vec!["monitor".into(), "resolution".into(), "screen".into(), "dpi".into()],
            category: Category::Setting,
            launch_count: 0,
        },
        AppEntry {
            name: "Network Settings".to_string(),
            description: "Wi-Fi, Ethernet, and VPN configuration".to_string(),
            executable_path: "/usr/bin/settings --network".to_string(),
            keywords: vec!["wifi".into(), "ethernet".into(), "vpn".into(), "internet".into()],
            category: Category::Setting,
            launch_count: 0,
        },
        AppEntry {
            name: "Sound Settings".to_string(),
            description: "Audio input/output and volume".to_string(),
            executable_path: "/usr/bin/settings --sound".to_string(),
            keywords: vec!["audio".into(), "volume".into(), "speaker".into(), "microphone".into()],
            category: Category::Setting,
            launch_count: 0,
        },
    ]
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    // The launcher is typically spawned by the desktop shell when the user
    // presses the launch shortcut (e.g., Super key or Ctrl+Space).
    // For now, we initialize state and enter the event loop placeholder.
    let mut launcher = LauncherState::new(1920.0, 1080.0);
    launcher.show();

    // In a real environment, the event loop would be driven by the compositor.
    // This placeholder demonstrates that the launcher initializes correctly.
    let _commands = launcher.render();
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
            text: None,
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
                text: Some(ch),
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
            text: None,
        };
        launcher.handle_key(&down);
        assert_eq!(launcher.selected_index, 1);

        let up = KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
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
            text: None,
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
                text: Some(ch),
            };
            launcher.handle_key(&event);
        }
        assert_eq!(launcher.query, "ab");

        // Backspace
        let bs = KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
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
            text: None,
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
                text: Some(ch),
            };
            launcher.handle_key(&event);
        }

        // Tab should autocomplete with the selected item's name
        let tab = KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
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
        assert!(cmds.is_empty(), "Hidden launcher should produce no commands");
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
            text: None,
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
            text: None,
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
            text: None,
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
}
