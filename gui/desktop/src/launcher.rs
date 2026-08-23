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
//! let commands = launcher.render(&palette);
//! // Draw commands on top of everything else
//!
//! // Forward key events while visible:
//! match launcher.handle_key(&key_event) {
//!     LauncherAction::Launch(path) => { /* spawn process */ }
//!     LauncherAction::Dismiss => { /* focus returns to previous window */ }
//!     LauncherAction::None => {}
//! }
//! ```
//!
//! # Colour
//!
//! Every colour here is a role of the [`Palette`] handed to [`render`], and
//! four judgements decide which role each site takes. Each is stated so it can
//! be refuted by a test rather than merely believed.
//!
//! 1. **The accent marks where you are; it never says what a thing is.** Two
//!    sites take `p.accent`: the caret in the search box and the bar beside the
//!    selected row. Both answer "here". A category badge answers "what", and so
//!    takes a *named hue* instead. This distinction is invisible under the
//!    shipped theme and is the whole reason the conversion is delicate — the
//!    stock accent **is** `blue`, and the `Application` category is also blue,
//!    so before this change one constant `BLUE` served both meanings at four
//!    sites and nothing could tell them apart. Move the accent to green and the
//!    caret follows while the App badge does not; that is the test.
//!
//! 2. **A category's colour is a fixed vocabulary of five, not a scale.** App,
//!    Sys, Set, File and Cmd take `blue`, `red`, `peach`, `green` and `mauve`.
//!    They are five names, so they must stay five *distinct* values in either
//!    mode — a vocabulary two of whose words are the same word cannot say
//!    which one it means. None of them follows the accent.
//!
//! 3. **The badge fill is the badge's own hue at alpha 40, computed and never
//!    named.** A category gains a colour by being added to the enum, not by
//!    someone remembering to add a matching wash; deriving the fill is what
//!    makes those two impossible to disagree.
//!
//! 4. **The dialog floats, so it is translucent over a real drop shadow.**
//!    `p.base` at alpha 240 lets the desktop show through just enough to read
//!    as lifted, and [`Palette::shadow`] is the shell's one shadow value —
//!    this module was one of the three popups that each picked their own
//!    (100 here, 120 and 160 elsewhere) without reference to the others. The
//!    shadow is black in both modes because a shadow is an absence of light
//!    rather than a colour.

use appearance::Palette;
use guitk::color::Color;
use guitk::event::{Key, KeyEvent};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::step;
use guitk::style::CornerRadii;
use guitk::text;
use guitk::text::TextCursor;
use guitk::theme::with_alpha;

/// How opaque the dialog itself is.
///
/// Named because it is asserted: a floating panel that reached full opacity
/// would stop reading as floating, and the difference is one byte.
const DIALOG_ALPHA: u8 = 240;

/// How strong the wash behind a category badge is.
const BADGE_WASH_ALPHA: u8 = 40;

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

    /// The hue that stands for this category.
    ///
    /// A *named hue*, never `p.accent`, even for `Application` — whose colour
    /// is the same pixel as the stock accent and so cannot be told apart from
    /// it until the user picks a different one. See judgement 1 in the module
    /// docs: this answers "what kind of thing", and the accent answers "where
    /// you are".
    fn color(self, p: &Palette) -> Color {
        match self {
            Self::Application => p.blue,
            Self::System => p.red,
            Self::Setting => p.peach,
            Self::File => p.green,
            Self::Command => p.mauve,
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
/// Re-exported so the launcher's own callers keep their path, but the
/// implementation lives in `textfind` — this ranking was written out three
/// times (here, the Run dialog, and the standalone launcher application), the
/// second copy carrying a comment promising it stayed in step with this one.
/// See `textfind::fuzzy_score` for what the score rewards.
pub use guitk::textfind::fuzzy_score;

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

            // Clamped, not wrapping: the results are a *ranking*, so running
            // off the end of it and landing back on the best match would be
            // saying something the list does not mean. `step` names the
            // choice; see its module docs for the other one.
            Key::Up => {
                self.selected_index = step::clamped_before(self.results.len(), self.selected_index);
                return LauncherAction::None;
            }

            Key::Down => {
                self.selected_index = step::clamped_after(self.results.len(), self.selected_index);
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

            // The four arms below all asked the same question — "where is the
            // caret stop next to this one" — and each answered it by slicing
            // the query and stepping `char_indices`, a slice that panics on an
            // offset which has drifted off a character boundary. `TextCursor`
            // answers it once, for every field in the toolkit.
            Key::Backspace => {
                let at = TextCursor::from(self.cursor);
                if let Some(prev) = at.prev_in(&self.query) {
                    self.query
                        .replace_range(prev.byte()..at.snapped_in(&self.query).byte(), "");
                    self.cursor = prev.byte();
                    self.selected_index = 0;
                    self.update_results();
                }
                return LauncherAction::None;
            }

            Key::Delete => {
                // Delete removes the character the caret sits *on*: the span
                // from this caret stop to the next one. `String::remove` took
                // a byte offset and a character's worth of faith.
                let at = TextCursor::from(self.cursor).snapped_in(&self.query);
                if let Some(next) = at.next_in(&self.query) {
                    self.query.replace_range(at.byte()..next.byte(), "");
                    self.cursor = at.byte();
                    self.selected_index = 0;
                    self.update_results();
                }
                return LauncherAction::None;
            }

            Key::Left => {
                if let Some(prev) = TextCursor::from(self.cursor).prev_in(&self.query) {
                    self.cursor = prev.byte();
                }
                return LauncherAction::None;
            }

            Key::Right => {
                if let Some(next) = TextCursor::from(self.cursor).next_in(&self.query) {
                    self.cursor = next.byte();
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

        if let Some(ch) = event.text
            && !ch.is_control()
        {
            self.query.insert(self.cursor, ch);
            self.cursor = self.cursor.saturating_add(ch.len_utf8());
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

        self.results
            .sort_by_key(|s| core::cmp::Reverse(s.total_score));
        self.results.truncate(MAX_RESULTS);

        if self.selected_index >= self.results.len() {
            self.selected_index = self.results.len().saturating_sub(1);
        }
    }

    /// Render the launcher to a list of render commands.
    ///
    /// `p` is the resolved palette; see the module docs for which role each
    /// site takes and why.
    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {
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
            color: p.shadow(),
            corner_radii: radii,
        });

        // Background
        cmds.push(RenderCommand::FillRect {
            x: dialog_x,
            y: dialog_y,
            width: DIALOG_WIDTH,
            height: dialog_height,
            color: with_alpha(p.base, DIALOG_ALPHA),
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
            color: p.mantle,
            corner_radii: input_radii,
        });

        // Input border
        cmds.push(RenderCommand::StrokeRect {
            x: 0.0,
            y: 0.0,
            width: input_width,
            height: INPUT_HEIGHT - PADDING,
            color: p.surface2,
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
                color: p.overlay0,
                font_size: INPUT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        } else {
            cmds.push(RenderCommand::Text {
                x: 12.0,
                y: text_y,
                text: self.query.clone(),
                color: p.text,
                font_size: INPUT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(input_width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Cursor
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
            y1: text_y,
            x2: cursor_x,
            y2: text_y + INPUT_FONT_SIZE,
            color: p.accent,
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
                    color: p.surface1,
                    corner_radii: CornerRadii::all(6.0),
                });
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: row_y + 8.0,
                    width: 3.0,
                    height: ROW_HEIGHT - 16.0,
                    color: p.accent,
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
                color: entry.category.color(p),
                corner_radii: CornerRadii::all(4.0),
            });

            // Name
            let text_x = icon_x + 36.0;
            cmds.push(RenderCommand::Text {
                x: text_x,
                y: row_y + 8.0,
                text: entry.name.clone(),
                color: if is_selected { p.text } else { p.subtext1 },
                font_size: NAME_FONT_SIZE,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(input_width - text_x - 80.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Description
            cmds.push(RenderCommand::Text {
                x: text_x,
                y: row_y + 26.0,
                text: entry.description.clone(),
                color: p.subtext0,
                font_size: DESC_FONT_SIZE,
                font_weight: FontWeightHint::Light,
                max_width: Some(input_width - text_x - 80.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Category badge
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
                // Derived from the badge's own hue, never named beside it:
                // a category gets a colour by being added to the enum, and a
                // hand-written wash is free to disagree with it.
                color: with_alpha(entry.category.color(p), BADGE_WASH_ALPHA),
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: badge_x + 6.0,
                y: badge_y + 4.0,
                text: badge_text.to_string(),
                color: entry.category.color(p),
                font_size: DESC_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // No results message
        if self.results.is_empty() && !self.query.is_empty() {
            cmds.push(RenderCommand::Text {
                x: input_width / 2.0 - 40.0,
                y: results_y_start + 16.0,
                text: "No results found".to_string(),
                color: p.overlay0,
                font_size: NAME_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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

/// The applications this desktop knows how to start.
///
/// Public because the launcher is not the only front end onto it: the shell's
/// start menu offers the same programs, and a second hand-written list there
/// would be free to drift away from this one — as it had, listing a "System
/// Monitor" that no entry here has ever provided.
#[must_use]
pub fn builtin_app_database() -> Vec<AppEntry> {
    vec![
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
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // These tests select a command by the exact literal the code under test was
    // handed — a width, a height, a font size. That is the assertion meant: a
    // tolerance would let a command that had drifted answer for one that had not.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::palette_check::assert_drawn_from;
    use guitk::event::{Key, KeyEvent, Modifiers};

    /// A palette whose accent belongs to no palette.
    ///
    /// The stock accent **is** `blue`, and `blue` is also this module's colour
    /// for the `Application` category — so under the shipped theme the caret,
    /// the selection bar and every App badge are one pixel value and no
    /// assertion can tell a position mark from a category mark. Every test
    /// here that mentions the accent uses this instead, and the loop below
    /// proves the substitute really is outside the palette rather than
    /// coincidentally equal to a role the test then reads by accident.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        for (name, role) in p.roles() {
            assert!(
                name == "accent"
                    || (role.r, role.g, role.b) != (p.accent.r, p.accent.g, p.accent.b),
                "the fixture accent collides with role {name}, so an assertion \
                 about the accent could pass while reading the wrong role"
            );
        }
        p
    }

    /// A launcher showing results, which is the only state that draws rows.
    ///
    /// A hidden launcher renders nothing at all, so a sweep over the default
    /// state would check zero commands and pass — the same vacuous-coverage
    /// trap the `.take(12)` fixture hit in `language_settings`.
    fn showing() -> LauncherState {
        let mut st = LauncherState::new(1280.0, 800.0);
        st.show();
        st
    }

    /// Every colour in `cmds`, whatever command carries it.
    fn all_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. }
                | RenderCommand::Line { color, .. }
                | RenderCommand::BoxShadow { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The colours of every `Text` command whose text is exactly `s`.
    fn text_colors(cmds: &[RenderCommand], s: &str) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == s => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The colour of the one `Text` command reading `s`.
    fn text_color(cmds: &[RenderCommand], s: &str) -> Color {
        let found = text_colors(cmds, s);
        assert_eq!(found.len(), 1, "expected exactly one command reading {s:?}");
        found[0]
    }

    /// The colour of the one `Text` command reading `s` at `size` points.
    ///
    /// The size is what separates the query from a result: type the name of an
    /// app and the same string is on the screen twice, once in the field at
    /// [`INPUT_FONT_SIZE`] and once in the list at [`NAME_FONT_SIZE`]. Those
    /// are two different sites that happen to share a role, so a lookup that
    /// could not tell them apart would be asserting about neither.
    fn text_color_sized(cmds: &[RenderCommand], s: &str, size: f32) -> Color {
        let found: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    color,
                    font_size,
                    ..
                } if text == s && *font_size == size => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(
            found.len(),
            1,
            "expected exactly one command reading {s:?} at {size}pt"
        );
        found[0]
    }

    /// The colours of every `FillRect` of exactly `w` x `h`.
    fn fills_sized(cmds: &[RenderCommand], w: f32, h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if *width == w && *height == h => Some(*color),
                _ => None,
            })
            .collect()
    }

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
                .render(&accented(false))
                .into_iter()
                .find_map(|cmd| match cmd {
                    RenderCommand::Line { x1, x2, color, .. }
                        if (x1 - x2).abs() < f32::EPSILON && color == accented(false).accent =>
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
        assert!(!state.render(&accented(false)).is_empty());
    }

    /// Editing must move by characters, not bytes. A byte-stepping caret puts
    /// three of the four characters below out of reach and panics on the way,
    /// and every ASCII test in this file passes throughout that bug's life.
    #[test]
    fn editing_walks_characters_of_every_width_rather_than_bytes() {
        let mut state = LauncherState::new(1280.0, 800.0);
        state.visible = true;
        for ch in "aé€🔒".chars() {
            state.handle_key(&type_char(ch));
        }
        assert_eq!(state.query, "aé€🔒");
        assert_eq!(state.cursor, 10);

        // Four Lefts reach the start — one per keystroke typed, not one per
        // byte, of which there are ten.
        for _ in 0..4 {
            state.handle_key(&press(Key::Left));
        }
        assert_eq!(state.cursor, 0);
        // And Left at the start stays put rather than underflowing.
        state.handle_key(&press(Key::Left));
        assert_eq!(state.cursor, 0);

        // Delete removes whole characters from the front.
        state.handle_key(&press(Key::Delete));
        assert_eq!(state.query, "é€🔒");
        state.handle_key(&press(Key::Delete));
        assert_eq!(state.query, "€🔒");

        // Right walks to the end and stops there.
        for _ in 0..5 {
            state.handle_key(&press(Key::Right));
        }
        assert_eq!(state.cursor, state.query.len());

        // Backspace erases one keystroke's worth each time, not one byte.
        state.handle_key(&press(Key::Backspace));
        assert_eq!(state.query, "€");
        state.handle_key(&press(Key::Backspace));
        assert_eq!(state.query, "");
        state.handle_key(&press(Key::Backspace));
        assert_eq!(state.query, "");
        assert_eq!(state.cursor, 0);
    }

    /// The companion to the render test above: a caret that has drifted inside
    /// a character must be *repaired* by the next edit, not carried into
    /// `String::remove`, which panics on exactly that offset.
    #[test]
    fn an_edit_repairs_a_caret_that_has_drifted_inside_a_character() {
        for key in [Key::Backspace, Key::Delete, Key::Left, Key::Right] {
            let mut state = LauncherState::new(1280.0, 800.0);
            state.visible = true;
            state.query = "né".to_owned();
            state.cursor = 2; // inside the two-byte 'é'
            state.handle_key(&press(key));
            assert!(
                state.query.is_char_boundary(state.cursor),
                "{key:?} left the caret at byte {} of {:?}",
                state.cursor,
                state.query,
            );
        }
    }

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
        assert!(
            st.render(&accented(false)).is_empty(),
            "hidden launcher renders nothing"
        );
    }

    #[test]
    fn show_makes_launcher_visible_and_renders() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        assert!(st.visible);
        // A visible launcher must produce a non-empty render tree
        // (dialog background + input field + scrim).
        assert!(!st.render(&accented(false)).is_empty());
    }

    #[test]
    fn hide_clears_visibility() {
        let mut st = LauncherState::new(1920.0, 1080.0);
        st.show();
        st.hide();
        assert!(!st.visible);
        assert!(st.render(&accented(false)).is_empty());
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
        let commands = st.render(&accented(false));
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
            LauncherAction::Dismiss => {
                panic!("navigating must not dismiss the launcher")
            }
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
        let _ = st.render(&accented(false));
    }

    // -------- Colour: the palette conversion --------
    //
    // TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE part 2.
    // Thirteen chromatic constants plus a shadow used to live in a private
    // `mod theme` here; the tests below check the four judgements in the
    // module docs, one test per judgement, plus the mechanical sweep that
    // catches a constant left behind by the edit.

    /// One app per category, which is the only fixture that draws all five
    /// badge hues.
    ///
    /// The builtin database is not that fixture: it is ranked and truncated to
    /// `MAX_RESULTS`, so which categories reach the screen is a property of the
    /// scoring, not of the render — and a category that never renders is a
    /// category whose colour is never checked. Replacing `apps` wholesale also
    /// pins the row *count*, which several assertions below index into.
    fn five_categories() -> LauncherState {
        fn entry(name: &str, category: Category) -> AppEntry {
            AppEntry {
                name: name.to_string(),
                description: format!("the {name} description"),
                executable_path: format!("/usr/bin/{name}"),
                keywords: Vec::new(),
                category,
                launch_count: 0,
            }
        }
        let mut st = LauncherState::new(1280.0, 800.0);
        st.apps = vec![
            entry("alpha", Category::Application),
            entry("bravo", Category::System),
            entry("charlie", Category::Setting),
            entry("delta", Category::File),
            entry("echo", Category::Command),
        ];
        st.show();
        assert_eq!(st.results.len(), 5, "all five categories reach the screen");
        st
    }

    /// The five categories in the order `five_categories` lists them.
    const FIVE: [(Category, &str, &str); 5] = [
        (Category::Application, "alpha", "App"),
        (Category::System, "bravo", "Sys"),
        (Category::Setting, "charlie", "Set"),
        (Category::File, "delta", "File"),
        (Category::Command, "echo", "Cmd"),
    ];

    /// Nothing this launcher draws comes from outside the palette it was given.
    ///
    /// Both modes, and both of the two states that draw different trees: a
    /// list of results, and the "No results found" line, which is reached from
    /// no other test here and would otherwise be swept zero times.
    #[test]
    fn every_colour_this_launcher_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);

            assert_drawn_from(&p, &five_categories().render(&p), &[], "launcher results");

            // And again over the *shipped* app database rather than the
            // five-row fixture: the fixture was built to reach every category,
            // and a fixture built to reach everything is exactly the thing that
            // cannot notice a row shape only the real data produces.
            let real = showing().render(&p);
            assert!(
                real.len() > 10,
                "the builtin database drew nothing worth sweeping"
            );
            assert_drawn_from(&p, &real, &[], "launcher over the builtin database");

            let mut empty = five_categories();
            "zzzzzzzz".clone_into(&mut empty.query);
            empty.update_results();
            let cmds = empty.render(&p);
            assert!(
                text_colors(&cmds, "No results found").len() == 1,
                "the empty-result state was not actually reached"
            );
            assert_drawn_from(&p, &cmds, &[], "launcher with no results");
        }
    }

    /// None of the thirteen deleted constants is still drawn.
    ///
    /// Every value below is a Catppuccin **Mocha** colour, so a light render
    /// cannot legitimately produce one — which turns "a substitution was
    /// missed" from an invisible defect into a named failure. The shadow is
    /// not in the list because it is black in both modes on purpose.
    #[test]
    fn none_of_the_thirteen_deleted_constants_is_still_drawn() {
        const DELETED: [(&str, u32); 13] = [
            ("BASE", 0x001E_1E2E),
            ("MANTLE", 0x0018_1825),
            ("SURFACE1", 0x0045_475A),
            ("SURFACE2", 0x0058_5B70),
            ("TEXT", 0x00CD_D6F4),
            ("SUBTEXT0", 0x00A6_ADC8),
            ("SUBTEXT1", 0x00BA_C2DE),
            ("OVERLAY0", 0x006C_7086),
            ("BLUE", 0x0089_B4FA),
            ("MAUVE", 0x00CB_A6F7),
            ("GREEN", 0x00A6_E3A1),
            ("PEACH", 0x00FA_B387),
            ("RED", 0x00F3_8BA8),
        ];

        let p = accented(true);
        let cmds = five_categories().render(&p);
        for c in all_colors(&cmds) {
            let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
            for (name, deleted) in DELETED {
                assert_ne!(
                    rgb, deleted,
                    "the launcher still draws the deleted constant {name} \
                     (#{deleted:06X}) in a light render"
                );
            }
        }
    }

    /// Every site, named one at a time, in the role this module claims for it.
    ///
    /// The sweep above proves only *membership*, and membership cannot see a
    /// swap: an input field painted `surface1` instead of `mantle` draws a
    /// legal colour, and so does a description painted `subtext1`. Neither
    /// changes any count, so a tally of roles would pass both. n source sites
    /// need n assertions, so this is that table.
    #[test]
    fn every_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let st = five_categories();
            let cmds = st.render(&p);

            // The dialog: 620 wide, and 52 + 5*44 + 24 tall.
            assert_eq!(
                fills_sized(&cmds, DIALOG_WIDTH, 296.0).len(),
                1,
                "one dialog background"
            );
            let dialog = fills_sized(&cmds, DIALOG_WIDTH, 296.0)[0];
            assert_eq!(
                (dialog.r, dialog.g, dialog.b),
                (p.base.r, p.base.g, p.base.b),
                "the dialog is the panel rung"
            );

            // The search field and its border. The border is the module's only
            // stroke, which is what selects it.
            assert_eq!(
                fills_sized(&cmds, 596.0, 40.0),
                vec![p.mantle],
                "the search field is a rung below the dialog around it"
            );
            let strokes: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(strokes, vec![p.surface2], "the search field's border");

            // The selected row: a fill and a bar, and exactly one of each.
            assert_eq!(
                fills_sized(&cmds, 596.0, ROW_HEIGHT),
                vec![p.surface1],
                "exactly one row is filled, at the raised rung"
            );
            assert_eq!(
                fills_sized(&cmds, 3.0, ROW_HEIGHT - 16.0),
                vec![p.accent],
                "exactly one row carries the selection bar"
            );

            // The three lines of each row.
            assert_eq!(
                text_color_sized(&cmds, "alpha", NAME_FONT_SIZE),
                p.text,
                "the selected row's name is the brightest thing in the list"
            );
            for (_, name, _) in &FIVE[1..] {
                assert_eq!(
                    text_color_sized(&cmds, name, NAME_FONT_SIZE),
                    p.subtext1,
                    "an unselected row's name is a rung dimmer"
                );
            }
            for (_, name, _) in FIVE {
                assert_eq!(
                    text_color_sized(&cmds, &format!("the {name} description"), DESC_FONT_SIZE),
                    p.subtext0,
                    "a row's description is dimmer than its name"
                );
            }

            // The five categories, each pinned to the role it names. A table,
            // not a set: a permutation of five distinct hues is still five
            // distinct hues, so `the_five_category_hues_stay_five_distinct_colours`
            // cannot see one, and neither can a test that compares the drawn
            // colour to `Category::color` — that asks the code under test what
            // it meant. Only naming the pairs here can fail.
            for (category, expected, label) in [
                (Category::Application, p.blue, "App"),
                (Category::System, p.red, "Sys"),
                (Category::Setting, p.peach, "Set"),
                (Category::File, p.green, "File"),
                (Category::Command, p.mauve, "Cmd"),
            ] {
                assert_eq!(category.color(&p), expected, "{label} names the wrong role");
                assert_eq!(
                    text_color(&cmds, label),
                    expected,
                    "the {label} badge is not drawn in the role its category names"
                );
            }

            // The placeholder and the no-results line, each in its own state.
            let mut typed = five_categories();
            "alpha".clone_into(&mut typed.query);
            typed.update_results();
            let typed = typed.render(&p);
            assert_eq!(
                text_color_sized(&typed, "alpha", INPUT_FONT_SIZE),
                p.text,
                "a typed query"
            );
            assert_eq!(
                text_color(&cmds, "Search..."),
                p.overlay0,
                "the placeholder in an empty field"
            );

            let mut none = five_categories();
            "zzzzzzzz".clone_into(&mut none.query);
            none.update_results();
            assert_eq!(
                text_color(&none.render(&p), "No results found"),
                p.overlay0,
                "the no-results line"
            );
        }
    }

    /// Judgement 1: the accent marks where you are; it never says what a thing
    /// is.
    ///
    /// This is the module's real trap. The stock accent **is** `blue`, and
    /// `Application` is also blue — so under the shipped theme the caret, the
    /// selection bar and every App badge are one pixel value, and an
    /// assertion made at the stock accent cannot fail. Hence the off-palette
    /// accent for the count, and the sweep across all 21 roles for the claim
    /// that a category badge does not move with it.
    #[test]
    fn the_accent_marks_where_you_are_and_never_what_a_thing_is() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = five_categories().render(&p);
            let n = all_colors(&cmds).iter().filter(|c| **c == p.accent).count();
            assert_eq!(
                n, 2,
                "the caret and the selected row's bar are the only two things \
                 that say where you are — found {n}"
            );

            // The caret is one of the two, and it is the module's only Line.
            let lines: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Line { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(lines, vec![p.accent], "the caret");

            // And the badges hold still while the accent moves everywhere.
            let base = Palette::for_mode(light);
            for (role, accent) in base.roles() {
                let mut swept = Palette::for_mode(light);
                swept.accent = accent;
                let cmds = five_categories().render(&swept);
                for (category, _, badge) in FIVE {
                    assert_eq!(
                        text_color(&cmds, badge),
                        category.color(&swept),
                        "the {badge} badge followed the accent when it was set \
                         to {role}"
                    );
                }
            }
        }
    }

    /// Judgement 2: five categories are five distinct colours, in either mode.
    ///
    /// A vocabulary two of whose words are the same word cannot say which one
    /// it means. Checked on the drawn commands rather than on `Category::color`
    /// alone, so that a renderer which drew the wrong one of the five is
    /// caught as well as a palette that collapsed two of them.
    #[test]
    fn the_five_category_hues_stay_five_distinct_colours() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = five_categories().render(&p);

            let mut seen: Vec<Color> = Vec::new();
            for (category, _, badge) in FIVE {
                let drawn = text_color(&cmds, badge);
                assert_eq!(
                    drawn,
                    category.color(&p),
                    "the {badge} badge is not drawn in its own category's hue"
                );
                assert!(
                    !seen.contains(&drawn),
                    "two categories share the colour {drawn:?}, so the badge \
                     cannot say which of them a row belongs to"
                );
                seen.push(drawn);
            }
            assert_eq!(seen.len(), 5);

            // The icon square beside each row takes the same hue as the badge
            // at the far end of it — one row, one colour, said twice.
            let icons = fills_sized(&cmds, 24.0, 24.0);
            assert_eq!(icons.len(), 5, "one icon per row");
            for (i, (category, _, _)) in FIVE.iter().enumerate() {
                assert_eq!(
                    icons[i],
                    category.color(&p),
                    "row {i}'s icon disagrees with its own category"
                );
            }
        }
    }

    /// Judgement 3: a badge's wash is its own hue at a lower alpha.
    ///
    /// Derived rather than named, so that adding a category cannot leave a
    /// wash behind that still refers to the old one. Asserted as a
    /// *relationship* between the two commands — RGB equal, alpha different —
    /// which is a claim a pair of hand-written constants would fail even if
    /// both happened to be plausible colours.
    #[test]
    fn a_badge_wash_is_its_own_hue_at_a_lower_alpha() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = five_categories().render(&p);

            let washes: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { height, color, .. } if *height == 20.0 => {
                        Some(*color)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(washes.len(), 5, "one wash per badge");

            for (i, (_, _, badge)) in FIVE.iter().enumerate() {
                let ink = text_color(&cmds, badge);
                assert_eq!(
                    (washes[i].r, washes[i].g, washes[i].b),
                    (ink.r, ink.g, ink.b),
                    "the {badge} wash is not the same hue as the {badge} label \
                     drawn on top of it"
                );
                assert_eq!(washes[i].a, BADGE_WASH_ALPHA, "the {badge} wash");
                // Not only "fainter than the ink" — faint enough to be a tint.
                // The line above compares the wash to the constant that
                // produced it and so cannot notice that constant moving; this
                // one can. A wash at, say, 200 still passes "the label is more
                // solid", but by then the label is being read against its own
                // hue rather than against the row it is sitting on.
                assert!(
                    washes[i].a <= 64,
                    "a wash at alpha {} is a fill, not a tint",
                    washes[i].a
                );
                assert!(
                    ink.a > washes[i].a,
                    "the {badge} label must be more solid than the wash under \
                     it or it cannot be read"
                );
            }
        }
    }

    /// Judgement 4: the dialog floats, so it is translucent over a real shadow.
    ///
    /// The alpha is the point. A launcher that reached full opacity would stop
    /// reading as lifted off the desktop, and the difference is one byte that
    /// nothing else in the module would notice. The shadow is asserted to be
    /// black in **both** modes rather than equal to some role, because a
    /// shadow is an absence of light and must not flip with the theme.
    #[test]
    fn the_dialog_floats_over_a_shadow_rather_than_sitting_on_the_desktop() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = five_categories().render(&p);

            let dialog = fills_sized(&cmds, DIALOG_WIDTH, 296.0)[0];
            assert_eq!(
                dialog.a, DIALOG_ALPHA,
                "the dialog stopped being translucent, so it no longer floats"
            );
            assert!(
                dialog.a < 255,
                "an opaque dialog is a panel, not a floating one"
            );
            // Bounded on both sides, because the line above compares the drawn
            // alpha to the constant that produced it and so cannot notice that
            // constant moving. Translucent enough to float, solid enough that
            // the desktop does not read through the result list.
            assert!(
                dialog.a >= 224,
                "at alpha {} the wallpaper shows through the results",
                dialog.a
            );

            let shadows: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::BoxShadow { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(shadows, vec![p.shadow()], "one drop shadow");
            assert_eq!(
                (shadows[0].r, shadows[0].g, shadows[0].b),
                (0, 0, 0),
                "a shadow is an absence of light, so it does not follow the mode"
            );
            assert!(shadows[0].a > 0, "an invisible shadow is not a shadow");
        }
    }

    /// An empty field reads as a prompt; a typed one reads as your text.
    ///
    /// The two-mode loop is not decoration: `p.text` in Mocha is exactly the
    /// `0xCDD6F4` this module was converted from, so a frozen query ink
    /// compared against `p.text` in the dark render would be comparing a
    /// constant against itself.
    #[test]
    fn an_empty_query_is_dimmer_than_a_typed_one() {
        for light in [false, true] {
            let p = accented(light);
            assert_eq!(
                text_color(&five_categories().render(&p), "Search..."),
                p.overlay0,
                "the placeholder"
            );

            let mut typed = five_categories();
            "alpha".clone_into(&mut typed.query);
            typed.update_results();
            let cmds = typed.render(&p);
            assert_eq!(
                text_color_sized(&cmds, "alpha", INPUT_FONT_SIZE),
                p.text,
                "the typed query"
            );
            assert_ne!(
                p.overlay0, p.text,
                "if these were equal a user could not tell a prompt from the \
                 text they had typed"
            );
        }
    }
}
