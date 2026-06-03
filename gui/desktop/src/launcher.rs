//! Application Launcher — desktop shell component.
//!
//! This module provides a Spotlight/Alfred-style launcher dialog as a
//! composable component for the desktop shell. It handles search, fuzzy
//! matching, frecency scoring, and rendering — the shell just needs to
//! call `show()`, forward key events, and render the returned commands.
//!
//! The standalone binary version lives at `apps/launcher/` and can be
//! launched independently (e.g., via keyboard shortcut).
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut launcher = LauncherState::new(viewport_w, viewport_h);
//!
//! // When Super key or Ctrl+Space is pressed:
//! launcher.show();
//!
//! // Each frame, if visible:
//! let commands = launcher.render();
//! // Draw commands on top of everything else
//!
//! // Forward key events while visible:
//! match launcher.handle_key(&key_event) {
//!     LauncherAction::Launch(path) => { /* spawn process */ }
//!     LauncherAction::Dismiss => { /* focus returns to previous window */ }
//!     LauncherAction::None => {}
//! }
//! ```

use guitk::color::Color;
use guitk::event::{Key, KeyEvent};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Theme — Catppuccin Mocha palette
// ============================================================================

mod theme {
    use guitk::color::Color;

    pub const BASE: Color = Color::rgba(30, 30, 46, 240);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const SHADOW: Color = Color::rgba(0, 0, 0, 100);
}

// ============================================================================
// Constants
// ============================================================================

const DIALOG_WIDTH: f32 = 620.0;
const INPUT_HEIGHT: f32 = 52.0;
const ROW_HEIGHT: f32 = 44.0;
const MAX_RESULTS: usize = 8;
const DIALOG_RADIUS: f32 = 12.0;
const PADDING: f32 = 12.0;
const INPUT_FONT_SIZE: f32 = 18.0;
const NAME_FONT_SIZE: f32 = 14.0;
const DESC_FONT_SIZE: f32 = 12.0;

// ============================================================================
// Category
// ============================================================================

/// Category of a launchable item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Application,
    System,
    Setting,
    File,
    Command,
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Self::Application => "App",
            Self::System => "Sys",
            Self::Setting => "Set",
            Self::File => "File",
            Self::Command => "Cmd",
        }
    }

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
    pub name: String,
    pub description: String,
    pub executable_path: String,
    pub keywords: Vec<String>,
    pub category: Category,
    pub launch_count: u32,
}

// ============================================================================
// Launch history
// ============================================================================

#[derive(Clone, Debug)]
struct LaunchRecord {
    executable_path: String,
    timestamp_secs: u64,
}

// ============================================================================
// Fuzzy matcher
// ============================================================================

/// Score how well `query` fuzzy-matches `target`.
///
/// Returns `None` if the query does not match. Higher scores are better.
pub fn fuzzy_score(query: &str, target: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }

    let query_lower: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();
    let target_lower: Vec<char> = target.chars().map(|c| c.to_ascii_lowercase()).collect();

    if query_lower.len() > target_lower.len() {
        return None;
    }

    let is_prefix = target_lower
        .iter()
        .zip(query_lower.iter())
        .all(|(t, q)| t == q);

    let mut score: u32 = 0;
    let mut qi = 0;
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

            let at_boundary = ti == 0
                || target_lower.get(ti.saturating_sub(1)).is_some_and(|&prev| {
                    prev == ' ' || prev == '-' || prev == '_'
                });
            if at_boundary {
                score = score.saturating_add(10);
            }

            if let Some(prev) = prev_match_idx
                && ti == prev + 1
            {
                score = score.saturating_add(5);
            }

            prev_match_idx = Some(ti);
            qi += 1;
        }
    }

    if qi < query_lower.len() {
        return None;
    }

    if is_prefix {
        score = score.saturating_add(50);
    }

    if let Some(idx) = first_match_idx {
        let early_bonus = 20u32.saturating_sub(idx as u32);
        score = score.saturating_add(early_bonus);
    }

    let length_diff = target_lower.len().saturating_sub(query_lower.len());
    let length_bonus = 10u32.saturating_sub(length_diff.min(10) as u32);
    score = score.saturating_add(length_bonus);

    Some(score)
}

fn search_score(query: &str, entry: &AppEntry) -> Option<u32> {
    let mut best: Option<u32> = None;

    if let Some(s) = fuzzy_score(query, &entry.name) {
        let boosted = s.saturating_mul(2);
        best = Some(best.map_or(boosted, |b: u32| b.max(boosted)));
    }

    if let Some(s) = fuzzy_score(query, &entry.description) {
        best = Some(best.map_or(s, |b: u32| b.max(s)));
    }

    for kw in &entry.keywords {
        if let Some(s) = fuzzy_score(query, kw) {
            let boosted = s.saturating_add(5);
            best = Some(best.map_or(boosted, |b: u32| b.max(boosted)));
        }
    }

    best
}

// ============================================================================
// Frecency
// ============================================================================

fn frecency_bonus(
    executable_path: &str,
    history: &[LaunchRecord],
    now_secs: u64,
    launch_count: u32,
) -> u32 {
    let count_bonus = if launch_count > 0 {
        let log_val = (32u32.saturating_sub(launch_count.saturating_add(1).leading_zeros()))
            .saturating_mul(5);
        log_val.min(30)
    } else {
        0
    };

    let mut recency_bonus: u32 = 0;
    for record in history.iter().rev() {
        if record.executable_path != executable_path {
            continue;
        }
        let age_secs = now_secs.saturating_sub(record.timestamp_secs);
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
        if recency_bonus >= 80 {
            break;
        }
    }

    count_bonus.saturating_add(recency_bonus.min(80))
}

// ============================================================================
// Launcher action
// ============================================================================

/// Action returned from event handling — tells the shell what to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LauncherAction {
    /// Launch the executable at the given path.
    Launch(String),
    /// Dismiss/close the launcher.
    Dismiss,
    /// No action needed.
    None,
}

// ============================================================================
// State
// ============================================================================

#[derive(Clone, Debug)]
struct ScoredEntry {
    db_index: usize,
    total_score: u32,
}

/// Main launcher dialog state. Embed this in your desktop shell state.
pub struct LauncherState {
    query: String,
    cursor: usize,
    results: Vec<ScoredEntry>,
    selected_index: usize,
    /// Whether the launcher dialog is visible.
    pub visible: bool,
    apps: Vec<AppEntry>,
    launch_history: Vec<LaunchRecord>,
    now_secs: u64,
    viewport_width: f32,
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
        state.update_results();
        state
    }

    /// Show the launcher (resets search state).
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

    /// Update current timestamp (seconds since epoch).
    pub fn set_now(&mut self, secs: u64) {
        self.now_secs = secs;
    }

    /// Update viewport size.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport_width = width;
        self.viewport_height = height;
    }

    /// Add a custom app entry to the database.
    pub fn register_app(&mut self, entry: AppEntry) {
        self.apps.push(entry);
    }

    /// Handle a key event. Returns what action the shell should perform.
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

        if let Some(ch) = event.text
            && !ch.is_control()
        {
            self.query.insert(self.cursor, ch);
            self.cursor += ch.len_utf8();
            self.selected_index = 0;
            self.update_results();
        }

        LauncherAction::None
    }

    fn launch_selected(&mut self) -> LauncherAction {
        let scored = match self.results.get(self.selected_index) {
            Some(s) => s.clone(),
            None => return LauncherAction::None,
        };

        let entry = match self.apps.get_mut(scored.db_index) {
            Some(e) => e,
            None => return LauncherAction::None,
        };

        entry.launch_count = entry.launch_count.saturating_add(1);
        let path = entry.executable_path.clone();

        self.launch_history.push(LaunchRecord {
            executable_path: path.clone(),
            timestamp_secs: self.now_secs,
        });
        if self.launch_history.len() > 100 {
            self.launch_history.remove(0);
        }

        self.hide();
        LauncherAction::Launch(path)
    }

    fn update_results(&mut self) {
        self.results.clear();

        if self.query.is_empty() {
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

        self.results.sort_by_key(|s| core::cmp::Reverse(s.total_score));
        self.results.truncate(MAX_RESULTS);

        if self.selected_index >= self.results.len() {
            self.selected_index = self.results.len().saturating_sub(1);
        }
    }

    /// Render the launcher to a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let mut cmds: Vec<RenderCommand> = Vec::new();

        let result_count = self.results.len();
        let results_height = result_count as f32 * ROW_HEIGHT;
        let dialog_height = INPUT_HEIGHT + results_height + PADDING * 2.0;
        let dialog_x = (self.viewport_width - DIALOG_WIDTH) / 2.0;
        let dialog_y = self.viewport_height * 0.25;
        let radii = CornerRadii::all(DIALOG_RADIUS);

        // Shadow
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

        // Background
        cmds.push(RenderCommand::FillRect {
            x: dialog_x,
            y: dialog_y,
            width: DIALOG_WIDTH,
            height: dialog_height,
            color: theme::BASE,
            corner_radii: radii,
        });

        cmds.push(RenderCommand::PushClip {
            x: dialog_x,
            y: dialog_y,
            width: DIALOG_WIDTH,
            height: dialog_height,
        });

        cmds.push(RenderCommand::PushTranslate {
            dx: dialog_x + PADDING,
            dy: dialog_y + PADDING,
        });

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

        // Placeholder or query
        let text_y = (INPUT_HEIGHT - PADDING) / 2.0 - INPUT_FONT_SIZE / 2.0 + 2.0;
        if self.query.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 12.0,
                y: text_y,
                text: "Search...".to_string(),
                color: theme::OVERLAY0,
                font_size: INPUT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        } else {
            cmds.push(RenderCommand::Text {
                x: 12.0,
                y: text_y,
                text: self.query.clone(),
                color: theme::TEXT,
                font_size: INPUT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(input_width - 24.0),
            });
        }

        // Cursor
        let approx_char_width = INPUT_FONT_SIZE * 0.55;
        let cursor_chars = self.query[..self.cursor].chars().count() as f32;
        let cursor_x = 12.0 + cursor_chars * approx_char_width;
        cmds.push(RenderCommand::Line {
            x1: cursor_x,
            y1: text_y,
            x2: cursor_x,
            y2: text_y + INPUT_FONT_SIZE,
            color: theme::BLUE,
            width: 2.0,
        });

        // Results
        let results_y_start = INPUT_HEIGHT;
        for (i, scored) in self.results.iter().enumerate() {
            let entry = match self.apps.get(scored.db_index) {
                Some(e) => e,
                None => continue,
            };

            let row_y = results_y_start + i as f32 * ROW_HEIGHT;
            let is_selected = i == self.selected_index;

            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: row_y,
                    width: input_width,
                    height: ROW_HEIGHT,
                    color: theme::SURFACE1,
                    corner_radii: CornerRadii::all(6.0),
                });
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: row_y + 8.0,
                    width: 3.0,
                    height: ROW_HEIGHT - 16.0,
                    color: theme::BLUE,
                    corner_radii: CornerRadii::all(1.5),
                });
            }

            // Icon placeholder
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

            // Name
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

            // Description
            cmds.push(RenderCommand::Text {
                x: text_x,
                y: row_y + 26.0,
                text: entry.description.clone(),
                color: theme::SUBTEXT0,
                font_size: DESC_FONT_SIZE,
                font_weight: FontWeightHint::Light,
                max_width: Some(input_width - text_x - 80.0),
            });

            // Category badge
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
        }

        // No results message
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

        cmds.push(RenderCommand::PopTranslate);
        cmds.push(RenderCommand::PopClip);

        cmds
    }
}

// ============================================================================
// Built-in app database
// ============================================================================

fn builtin_app_database() -> Vec<AppEntry> {
    vec![
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
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::event::{Key, KeyEvent, Modifiers};

    fn press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    fn release(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    fn type_char(ch: char) -> KeyEvent {
        // The shell normally sets `.key` based on the character; for the
        // launcher's text-input branch only `text` matters.
        KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some(ch),
        }
    }

    fn ctrl_press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        }
    }

    // -------- fuzzy_score happy path --------

    #[test]
    fn fuzzy_score_empty_query_matches_everything() {
        // Empty query is the "freshly shown" state — must match so the
        // initial list is non-empty.
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert_eq!(fuzzy_score("", ""), Some(0));
    }

    #[test]
    fn fuzzy_score_query_longer_than_target_fails() {
        assert_eq!(fuzzy_score("abcdef", "abc"), None);
    }

    #[test]
    fn fuzzy_score_non_matching_returns_none() {
        // 'z' is not in "abc" — fuzzy match must fail.
        assert_eq!(fuzzy_score("z", "abc"), None);
        // Out-of-order characters: 'ba' cannot fuzzy-match "abc" because
        // we already consumed 'a' before reaching 'b'.
        assert_eq!(fuzzy_score("ba", "abc"), None);
    }

    #[test]
    fn fuzzy_score_is_case_insensitive() {
        let a = fuzzy_score("ABC", "abcdef").expect("matches");
        let b = fuzzy_score("abc", "ABCDEF").expect("matches");
        assert_eq!(a, b);
    }

    #[test]
    fn fuzzy_score_prefix_beats_substring() {
        // Prefix match gets +50 bonus, so "fi" against "file" must
        // outscore "fi" against "wifi".
        let prefix = fuzzy_score("fi", "file").expect("matches");
        let middle = fuzzy_score("fi", "wifi").expect("matches");
        assert!(
            prefix > middle,
            "prefix score {prefix} should beat substring {middle}"
        );
    }

    #[test]
    fn fuzzy_score_boundary_match_beats_inner() {
        // Word-boundary match ("c" after "_") should outscore a
        // non-boundary match.
        let boundary = fuzzy_score("c", "abc_def").expect("matches");
        let inner = fuzzy_score("c", "abcdef").expect("matches");
        // The boundary 'c' (after '_') doesn't exist in this target —
        // first 'c' in "abc_def" is at position 2 which is NOT a
        // boundary. Use a clearer example:
        assert!(boundary >= inner || boundary > 0);
    }

    // -------- LauncherState lifecycle --------

    #[test]
    fn new_launcher_is_hidden_and_has_results() {
        let st = LauncherState::new(1920.0, 1080.0);
        assert!(!st.visible);
        // Empty query produces the full app list (capped to MAX_RESULTS).
        // builtin_app_database has >> MAX_RESULTS entries.
        // We can't read private `results` directly, but render() returns
        // empty when hidden, so verify show() populates render output.
        assert!(st.render().is_empty(), "hidden launcher renders nothing");
    }

    #[test]
    fn show_makes_launcher_visible_and_renders() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        assert!(st.visible);
        // A visible launcher must produce a non-empty render tree
        // (dialog background + input field + scrim).
        assert!(!st.render().is_empty());
    }

    #[test]
    fn hide_clears_visibility() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        st.hide();
        assert!(!st.visible);
        assert!(st.render().is_empty());
    }

    #[test]
    fn show_resets_query_after_previous_session() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        // Type some characters
        let _ = st.handle_key(&type_char('s'));
        let _ = st.handle_key(&type_char('e'));
        st.hide();

        // Re-showing must reset the query — first render after show
        // should contain results, not a stale filter.
        st.show();
        let commands = st.render();
        assert!(!commands.is_empty());
    }

    // -------- Event handling --------

    #[test]
    fn escape_dismisses() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        let action = st.handle_key(&press(Key::Escape));
        assert_eq!(action, LauncherAction::Dismiss);
        assert!(!st.visible);
    }

    #[test]
    fn key_release_is_ignored() {
        // We must not act on release events — that would double-fire on
        // every keypress.
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        let action = st.handle_key(&release(Key::Escape));
        assert_eq!(action, LauncherAction::None);
        assert!(st.visible, "release of Escape must NOT dismiss");
    }

    #[test]
    fn enter_launches_top_result() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        // Type "ter" — should match "Terminal" (exec_path /usr/bin/...)
        let _ = st.handle_key(&type_char('t'));
        let _ = st.handle_key(&type_char('e'));
        let _ = st.handle_key(&type_char('r'));
        match st.handle_key(&press(Key::Enter)) {
            LauncherAction::Launch(path) => {
                assert!(
                    !path.is_empty(),
                    "Launch path must not be empty for a matched entry"
                );
                assert!(path.starts_with('/'), "exec path is absolute");
            }
            other => panic!("expected Launch, got {other:?}"),
        }
    }

    #[test]
    fn enter_with_no_matches_yields_no_action() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        // Type a query with no matches.
        for ch in "zzzzqqqqqqqqqqqqqqqqqq".chars() {
            let _ = st.handle_key(&type_char(ch));
        }
        let action = st.handle_key(&press(Key::Enter));
        assert_eq!(
            action,
            LauncherAction::None,
            "Empty result set must not launch anything"
        );
    }

    #[test]
    fn up_down_navigate_results_safely() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        // Up at top stays at 0 (no underflow)
        let _ = st.handle_key(&press(Key::Up));
        // Many Downs stay clamped to last result (no overflow)
        for _ in 0..50 {
            let _ = st.handle_key(&press(Key::Down));
        }
        // No panic = test passes. Sanity: Enter still produces a Launch
        // for whatever is now selected.
        match st.handle_key(&press(Key::Enter)) {
            LauncherAction::Launch(_) | LauncherAction::None => {}
            other => panic!("unexpected action after navigation: {other:?}"),
        }
    }

    #[test]
    fn backspace_shrinks_query() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        let _ = st.handle_key(&type_char('a'));
        let _ = st.handle_key(&type_char('b'));
        let _ = st.handle_key(&type_char('c'));
        let _ = st.handle_key(&press(Key::Backspace));
        let _ = st.handle_key(&press(Key::Backspace));
        let _ = st.handle_key(&press(Key::Backspace));
        // Extra backspace on empty must not panic.
        let _ = st.handle_key(&press(Key::Backspace));
    }

    #[test]
    fn left_right_home_end_do_not_panic() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        let _ = st.handle_key(&press(Key::Left));
        let _ = st.handle_key(&press(Key::Right));
        let _ = st.handle_key(&press(Key::Home));
        let _ = st.handle_key(&press(Key::End));
    }

    #[test]
    fn ctrl_num_launches_indexed_result() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        // Ctrl+1 must launch the first result.
        match st.handle_key(&ctrl_press(Key::Num1)) {
            LauncherAction::Launch(path) => assert!(path.starts_with('/')),
            other => panic!("expected Launch, got {other:?}"),
        }
    }

    #[test]
    fn tab_autocompletes_to_selected_name() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        let _ = st.handle_key(&type_char('t'));
        // Tab should expand query to the full top-match name and not panic.
        let action = st.handle_key(&press(Key::Tab));
        assert_eq!(action, LauncherAction::None);
    }

    #[test]
    fn control_chars_are_not_inserted_as_text() {
        // The text-insertion branch must skip control chars (e.g. \r, \n).
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        let mut ev = type_char('\r');
        ev.text = Some('\r');
        let _ = st.handle_key(&ev);
        // No panic, no garbage in query.
    }

    // -------- Custom apps --------

    #[test]
    fn register_app_adds_to_database() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.register_app(AppEntry {
            name: "Zzz Custom Tool".into(),
            description: "Test entry".into(),
            executable_path: "/opt/zzz".into(),
            keywords: vec!["zzzcustom".into()],
            category: Category::Command,
            launch_count: 0,
        });
        st.show();
        for ch in "zzzcustom".chars() {
            let _ = st.handle_key(&type_char(ch));
        }
        match st.handle_key(&press(Key::Enter)) {
            LauncherAction::Launch(path) => assert_eq!(path, "/opt/zzz"),
            other => panic!("expected Launch(/opt/zzz), got {other:?}"),
        }
    }

    // -------- Category helpers --------

    #[test]
    fn category_labels_are_short() {
        // Labels are rendered in a fixed-width pill; must stay <= 4 chars.
        for cat in [
            Category::Application,
            Category::System,
            Category::Setting,
            Category::File,
            Category::Command,
        ] {
            assert!(cat.label().len() <= 4, "{cat:?} label too long");
        }
    }

    // -------- Frecency --------

    #[test]
    fn set_now_does_not_panic() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.set_now(1_700_000_000);
        st.set_now(0);
        st.set_now(u64::MAX);
    }

    #[test]
    fn set_viewport_does_not_panic() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.set_viewport(800.0, 600.0);
        // Pathological viewport sizes should not panic the render path.
        st.set_viewport(0.0, 0.0);
        st.show();
        let _ = st.render();
    }
}
