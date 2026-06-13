//! SlateOS Automator -- Keyboard/Mouse Automation & Macro Recorder
//!
//! A desktop automation tool with:
//! - **Macro recording**: capture keyboard and mouse actions with timestamps
//! - **Action types**: key press/release, mouse click/double-click, mouse move,
//!   scroll, type text, delay/wait
//! - **Playback**: execute macros at configurable speeds (0.5x, 1x, 2x, 5x, instant)
//! - **Repeat options**: play once, N times, or loop until stopped
//! - **Macro editor**: view, edit, reorder, delete individual actions
//! - **Script language**: text-based macro format with variables, labels, goto, conditionals
//! - **Trigger system**: assign hotkeys to start/stop macros
//! - **Macro library**: save, load, and organize macros with names and descriptions
//! - **Pixel color check**: conditional execution based on screen pixel color
//! - **Multi-panel UI**: sidebar, action list, properties panel, toolbar
//! - **Import/Export**: save/load macros as text files
//! - **Recording indicator**: visual feedback during recording
//!
//! Uses the guitk library for UI rendering with a Catppuccin Mocha dark theme.

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
#![allow(clippy::doc_markdown)]

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::BTreeMap;

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
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1000.0;
const WINDOW_HEIGHT: f32 = 700.0;
const HEADER_HEIGHT: f32 = 44.0;
const TOOLBAR_HEIGHT: f32 = 38.0;
const SIDEBAR_WIDTH: f32 = 240.0;
const PROPERTIES_WIDTH: f32 = 260.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const ROW_HEIGHT: f32 = 30.0;
const PADDING: f32 = 10.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const BUTTON_HEIGHT: f32 = 28.0;
const CORNER_RADIUS: f32 = 4.0;
const RECORDING_PULSE_PERIOD_MS: u64 = 1000;

// ============================================================================
// Mouse button (for macro actions -- distinct from guitk's MouseButton)
// ============================================================================

/// Mouse button for recorded macro actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacroMouseButton {
    Left,
    Right,
    Middle,
}

impl MacroMouseButton {
    fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Middle => "Middle",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "middle" => Some(Self::Middle),
            _ => None,
        }
    }
}

// ============================================================================
// Scroll direction
// ============================================================================

/// Direction for scroll actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

impl ScrollDirection {
    fn label(self) -> &'static str {
        match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

// ============================================================================
// Macro action
// ============================================================================

/// A single recordable/playable action in a macro.
#[derive(Clone, Debug, PartialEq)]
pub enum MacroAction {
    /// Press a key down.
    KeyPress { key_name: String },
    /// Release a key.
    KeyRelease { key_name: String },
    /// Mouse click (press + release).
    MouseClick {
        x: f32,
        y: f32,
        button: MacroMouseButton,
    },
    /// Mouse double-click.
    MouseDoubleClick {
        x: f32,
        y: f32,
        button: MacroMouseButton,
    },
    /// Move the mouse pointer.
    MouseMove { x: f32, y: f32 },
    /// Scroll the mouse wheel.
    Scroll {
        direction: ScrollDirection,
        amount: i32,
    },
    /// Type a string of text.
    TypeText { text: String },
    /// Wait for a duration in milliseconds.
    Delay { ms: u64 },
    /// Check a pixel color at (x,y) and skip next action if it does not match.
    IfPixelColor {
        x: f32,
        y: f32,
        r: u8,
        g: u8,
        b: u8,
        tolerance: u8,
    },
}

impl MacroAction {
    /// Short human-readable label for the action list.
    fn label(&self) -> String {
        match self {
            Self::KeyPress { key_name } => format!("Key Down: {key_name}"),
            Self::KeyRelease { key_name } => format!("Key Up: {key_name}"),
            Self::MouseClick { x, y, button } => {
                format!("{} Click ({x:.0}, {y:.0})", button.label())
            }
            Self::MouseDoubleClick { x, y, button } => {
                format!("{} DblClick ({x:.0}, {y:.0})", button.label())
            }
            Self::MouseMove { x, y } => format!("Move ({x:.0}, {y:.0})"),
            Self::Scroll { direction, amount } => {
                format!("Scroll {} x{amount}", direction.label())
            }
            Self::TypeText { text } => {
                let preview: String = text.chars().take(20).collect();
                if text.len() > 20 {
                    format!("Type \"{preview}...\"")
                } else {
                    format!("Type \"{preview}\"")
                }
            }
            Self::Delay { ms } => format!("Wait {ms}ms"),
            Self::IfPixelColor { x, y, r, g, b, .. } => {
                format!("If pixel ({x:.0},{y:.0}) = #{r:02X}{g:02X}{b:02X}")
            }
        }
    }

    /// Icon/badge character for the action type.
    fn icon(&self) -> &'static str {
        match self {
            Self::KeyPress { .. } | Self::KeyRelease { .. } => "KB",
            Self::MouseClick { .. } | Self::MouseDoubleClick { .. } => "CL",
            Self::MouseMove { .. } => "MV",
            Self::Scroll { .. } => "SC",
            Self::TypeText { .. } => "TX",
            Self::Delay { .. } => "DL",
            Self::IfPixelColor { .. } => "IF",
        }
    }

    /// Badge color for the action type.
    fn badge_color(&self) -> Color {
        match self {
            Self::KeyPress { .. } | Self::KeyRelease { .. } => BLUE,
            Self::MouseClick { .. } | Self::MouseDoubleClick { .. } => GREEN,
            Self::MouseMove { .. } => TEAL,
            Self::Scroll { .. } => PEACH,
            Self::TypeText { .. } => LAVENDER,
            Self::Delay { .. } => YELLOW,
            Self::IfPixelColor { .. } => RED,
        }
    }
}

// ============================================================================
// Timed action (action + delay before it)
// ============================================================================

/// An action paired with its delay from the previous action (in milliseconds).
#[derive(Clone, Debug, PartialEq)]
pub struct TimedAction {
    pub action: MacroAction,
    pub delay_ms: u64,
}

impl TimedAction {
    pub fn new(action: MacroAction, delay_ms: u64) -> Self {
        Self { action, delay_ms }
    }

    pub fn immediate(action: MacroAction) -> Self {
        Self {
            action,
            delay_ms: 0,
        }
    }
}

// ============================================================================
// Playback speed
// ============================================================================

/// Speed multiplier for macro playback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlaybackSpeed {
    Half,
    Normal,
    Double,
    Quintuple,
    Instant,
}

impl PlaybackSpeed {
    fn label(self) -> &'static str {
        match self {
            Self::Half => "0.5x",
            Self::Normal => "1x",
            Self::Double => "2x",
            Self::Quintuple => "5x",
            Self::Instant => "Instant",
        }
    }

    fn multiplier(self) -> f64 {
        match self {
            Self::Half => 2.0,
            Self::Normal => 1.0,
            Self::Double => 0.5,
            Self::Quintuple => 0.2,
            Self::Instant => 0.0,
        }
    }

    fn all() -> &'static [PlaybackSpeed] {
        &[
            Self::Half,
            Self::Normal,
            Self::Double,
            Self::Quintuple,
            Self::Instant,
        ]
    }

    /// Cycle to the next speed.
    fn next(self) -> Self {
        match self {
            Self::Half => Self::Normal,
            Self::Normal => Self::Double,
            Self::Double => Self::Quintuple,
            Self::Quintuple => Self::Instant,
            Self::Instant => Self::Half,
        }
    }
}

// ============================================================================
// Repeat mode
// ============================================================================

/// How many times a macro should play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatMode {
    Once,
    Times(u32),
    Forever,
}

impl RepeatMode {
    fn label(self) -> String {
        match self {
            Self::Once => "Once".to_string(),
            Self::Times(n) => format!("{n}x"),
            Self::Forever => "Loop".to_string(),
        }
    }
}

// ============================================================================
// Hotkey trigger
// ============================================================================

/// A hotkey combination that triggers a macro.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key_name: String,
}

impl Hotkey {
    fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(&self.key_name);
        parts.join("+")
    }

    fn from_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        if parts.is_empty() {
            return None;
        }
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut key_name = String::new();

        for (idx, part) in parts.iter().enumerate() {
            let lower = part.to_ascii_lowercase();
            if lower == "ctrl" {
                ctrl = true;
            } else if lower == "alt" {
                alt = true;
            } else if lower == "shift" {
                shift = true;
            } else if idx == parts.len().saturating_sub(1) {
                key_name = (*part).to_string();
            } else {
                return None;
            }
        }

        if key_name.is_empty() {
            return None;
        }

        Some(Self {
            ctrl,
            alt,
            shift,
            key_name,
        })
    }
}

// ============================================================================
// Macro definition
// ============================================================================

/// A named macro containing a sequence of timed actions.
#[derive(Clone, Debug)]
pub struct Macro {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub actions: Vec<TimedAction>,
    pub trigger: Option<Hotkey>,
    pub repeat_mode: RepeatMode,
    pub speed: PlaybackSpeed,
    pub created_at_ms: u64,
    pub modified_at_ms: u64,
}

impl Macro {
    pub fn new(id: u64, name: &str, now_ms: u64) -> Self {
        Self {
            id,
            name: name.to_string(),
            description: String::new(),
            actions: Vec::new(),
            trigger: None,
            repeat_mode: RepeatMode::Once,
            speed: PlaybackSpeed::Normal,
            created_at_ms: now_ms,
            modified_at_ms: now_ms,
        }
    }

    /// Total duration of the macro in milliseconds (sum of all delays).
    pub fn total_duration_ms(&self) -> u64 {
        self.actions
            .iter()
            .fold(0u64, |acc, ta| acc.saturating_add(ta.delay_ms))
    }

    /// Number of actions in the macro.
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Move an action from one index to another.
    pub fn move_action(&mut self, from: usize, to: usize) -> bool {
        if from >= self.actions.len() || to >= self.actions.len() {
            return false;
        }
        let item = self.actions.remove(from);
        self.actions.insert(to, item);
        true
    }

    /// Remove an action at the given index.
    pub fn remove_action(&mut self, idx: usize) -> Option<TimedAction> {
        if idx >= self.actions.len() {
            return None;
        }
        Some(self.actions.remove(idx))
    }

    /// Insert an action at the given index.
    pub fn insert_action(&mut self, idx: usize, action: TimedAction) {
        let clamped = idx.min(self.actions.len());
        self.actions.insert(clamped, action);
    }
}

// ============================================================================
// Script parser -- text-based macro language
// ============================================================================

/// Script parse error.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptError {
    pub line: usize,
    pub message: String,
}

impl core::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Line {}: {}", self.line, self.message)
    }
}

/// Parse a script string into a list of timed actions.
///
/// Supported commands:
/// - `key <name>` -- press and release a key
/// - `keydown <name>` -- press a key
/// - `keyup <name>` -- release a key
/// - `click <x> <y> [button]` -- mouse click
/// - `dblclick <x> <y> [button]` -- mouse double-click
/// - `move <x> <y>` -- mouse move
/// - `scroll <direction> [amount]` -- scroll
/// - `type <text>` -- type text
/// - `wait <ms>` -- delay
/// - `if_pixel <x> <y> <r> <g> <b> [tolerance]` -- conditional on pixel color
/// - `# comment` -- comment line
/// - blank lines are ignored
pub fn parse_script(source: &str) -> Result<Vec<TimedAction>, ScriptError> {
    let mut actions = Vec::new();
    let mut variables: BTreeMap<String, String> = BTreeMap::new();

    for (line_idx, raw_line) in source.lines().enumerate() {
        let line_num = line_idx.saturating_add(1);
        let line = raw_line.trim();

        // Skip blank lines and comments.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Variable assignment: $var = value
        if line.starts_with('$') {
            if let Some(eq_pos) = line.find('=') {
                let var_name = line[1..eq_pos].trim().to_string();
                let var_value = line[eq_pos.saturating_add(1)..].trim().to_string();
                if var_name.is_empty() {
                    return Err(ScriptError {
                        line: line_num,
                        message: "Empty variable name".to_string(),
                    });
                }
                variables.insert(var_name, var_value);
                continue;
            }
            return Err(ScriptError {
                line: line_num,
                message: "Variable line missing '='".to_string(),
            });
        }

        // Label: :labelname (stored but not used for goto in this simple version)
        if line.starts_with(':') {
            // Labels are recognized but no-ops in the linear action list.
            continue;
        }

        // Substitute variables in the line.
        let expanded = substitute_vars(line, &variables);
        let parts: Vec<&str> = expanded.split_whitespace().collect();

        if parts.is_empty() {
            continue;
        }

        let cmd = parts.first().map_or("", |p| *p).to_ascii_lowercase();
        let action = match cmd.as_str() {
            "key" => {
                let key_name = require_arg(&parts, 1, line_num, "key name")?;
                // key = keydown + keyup
                actions.push(TimedAction::immediate(MacroAction::KeyPress {
                    key_name: key_name.clone(),
                }));
                Some(TimedAction::new(MacroAction::KeyRelease { key_name }, 50))
            }
            "keydown" => {
                let key_name = require_arg(&parts, 1, line_num, "key name")?;
                Some(TimedAction::immediate(MacroAction::KeyPress { key_name }))
            }
            "keyup" => {
                let key_name = require_arg(&parts, 1, line_num, "key name")?;
                Some(TimedAction::immediate(MacroAction::KeyRelease { key_name }))
            }
            "click" => {
                let x = parse_f32_arg(&parts, 1, line_num, "x")?;
                let y = parse_f32_arg(&parts, 2, line_num, "y")?;
                let button = parts
                    .get(3)
                    .and_then(|s| MacroMouseButton::from_str(s))
                    .unwrap_or(MacroMouseButton::Left);
                Some(TimedAction::immediate(MacroAction::MouseClick {
                    x,
                    y,
                    button,
                }))
            }
            "dblclick" => {
                let x = parse_f32_arg(&parts, 1, line_num, "x")?;
                let y = parse_f32_arg(&parts, 2, line_num, "y")?;
                let button = parts
                    .get(3)
                    .and_then(|s| MacroMouseButton::from_str(s))
                    .unwrap_or(MacroMouseButton::Left);
                Some(TimedAction::immediate(MacroAction::MouseDoubleClick {
                    x,
                    y,
                    button,
                }))
            }
            "move" => {
                let x = parse_f32_arg(&parts, 1, line_num, "x")?;
                let y = parse_f32_arg(&parts, 2, line_num, "y")?;
                Some(TimedAction::immediate(MacroAction::MouseMove { x, y }))
            }
            "scroll" => {
                let dir_str = require_arg(&parts, 1, line_num, "direction")?;
                let direction = ScrollDirection::from_str(&dir_str).ok_or_else(|| ScriptError {
                    line: line_num,
                    message: format!("Unknown scroll direction: {dir_str}"),
                })?;
                let amount = parts
                    .get(2)
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(3);
                Some(TimedAction::immediate(MacroAction::Scroll {
                    direction,
                    amount,
                }))
            }
            "type" => {
                // Everything after "type " is the text.
                let rest = expanded
                    .strip_prefix(parts.first().map_or("", |p| *p))
                    .unwrap_or("")
                    .trim_start();
                let text = rest
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(rest)
                    .to_string();
                Some(TimedAction::immediate(MacroAction::TypeText { text }))
            }
            "wait" | "delay" | "sleep" => {
                let ms = parse_u64_arg(&parts, 1, line_num, "milliseconds")?;
                Some(TimedAction::immediate(MacroAction::Delay { ms }))
            }
            "if_pixel" => {
                let x = parse_f32_arg(&parts, 1, line_num, "x")?;
                let y = parse_f32_arg(&parts, 2, line_num, "y")?;
                let r = parse_u8_arg(&parts, 3, line_num, "red")?;
                let g = parse_u8_arg(&parts, 4, line_num, "green")?;
                let b = parse_u8_arg(&parts, 5, line_num, "blue")?;
                let tolerance = parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(10);
                Some(TimedAction::immediate(MacroAction::IfPixelColor {
                    x,
                    y,
                    r,
                    g,
                    b,
                    tolerance,
                }))
            }
            "repeat" | "loop" | "goto" => {
                // These control-flow commands are recognized but resolve to no-ops
                // in the linear action list. Full control flow would need a VM.
                None
            }
            _ => {
                return Err(ScriptError {
                    line: line_num,
                    message: format!("Unknown command: {cmd}"),
                });
            }
        };

        if let Some(a) = action {
            actions.push(a);
        }
    }

    Ok(actions)
}

/// Substitute `$varname` references in a line using the variables map.
fn substitute_vars(line: &str, vars: &BTreeMap<String, String>) -> String {
    let mut result = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars.get(i).copied() == Some('$') {
            let start = i.saturating_add(1);
            let mut end = start;
            while end < len
                && chars
                    .get(end)
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            {
                end = end.saturating_add(1);
            }
            if end > start {
                let var_name: String = chars.get(start..end).map(|s| s.iter().collect()).unwrap_or_default();
                if let Some(val) = vars.get(&var_name) {
                    result.push_str(val);
                } else {
                    // Unresolved variable: keep as-is.
                    result.push('$');
                    result.push_str(&var_name);
                }
                i = end;
            } else {
                result.push('$');
                i = start;
            }
        } else {
            if let Some(&ch) = chars.get(i) {
                result.push(ch);
            }
            i = i.saturating_add(1);
        }
    }

    result
}

/// Require a string argument at a given position.
fn require_arg(parts: &[&str], idx: usize, line: usize, name: &str) -> Result<String, ScriptError> {
    parts
        .get(idx)
        .map(|s| (*s).to_string())
        .ok_or_else(|| ScriptError {
            line,
            message: format!("Missing argument: {name}"),
        })
}

/// Parse an f32 argument at a given position.
fn parse_f32_arg(parts: &[&str], idx: usize, line: usize, name: &str) -> Result<f32, ScriptError> {
    let s = require_arg(parts, idx, line, name)?;
    s.parse::<f32>().map_err(|_| ScriptError {
        line,
        message: format!("Invalid number for {name}: {s}"),
    })
}

/// Parse a u64 argument at a given position.
fn parse_u64_arg(parts: &[&str], idx: usize, line: usize, name: &str) -> Result<u64, ScriptError> {
    let s = require_arg(parts, idx, line, name)?;
    s.parse::<u64>().map_err(|_| ScriptError {
        line,
        message: format!("Invalid number for {name}: {s}"),
    })
}

/// Parse a u8 argument at a given position.
fn parse_u8_arg(parts: &[&str], idx: usize, line: usize, name: &str) -> Result<u8, ScriptError> {
    let s = require_arg(parts, idx, line, name)?;
    s.parse::<u8>().map_err(|_| ScriptError {
        line,
        message: format!("Invalid number for {name}: {s}"),
    })
}

// ============================================================================
// Script serializer -- convert actions back to script text
// ============================================================================

/// Serialize a list of timed actions back to the text-based script format.
pub fn serialize_script(actions: &[TimedAction]) -> String {
    let mut lines = Vec::new();
    lines.push("# Slate OS Automator Macro Script".to_string());
    lines.push(String::new());

    for ta in actions {
        // If the action has a delay, emit a wait command first.
        if ta.delay_ms > 0 {
            lines.push(format!("wait {}", ta.delay_ms));
        }
        let line = match &ta.action {
            MacroAction::KeyPress { key_name } => format!("keydown {key_name}"),
            MacroAction::KeyRelease { key_name } => format!("keyup {key_name}"),
            MacroAction::MouseClick { x, y, button } => {
                format!(
                    "click {x:.0} {y:.0} {}",
                    button.label().to_ascii_lowercase()
                )
            }
            MacroAction::MouseDoubleClick { x, y, button } => {
                format!(
                    "dblclick {x:.0} {y:.0} {}",
                    button.label().to_ascii_lowercase()
                )
            }
            MacroAction::MouseMove { x, y } => format!("move {x:.0} {y:.0}"),
            MacroAction::Scroll { direction, amount } => {
                format!("scroll {} {amount}", direction.label().to_ascii_lowercase())
            }
            MacroAction::TypeText { text } => format!("type \"{text}\""),
            MacroAction::Delay { ms } => format!("wait {ms}"),
            MacroAction::IfPixelColor {
                x,
                y,
                r,
                g,
                b,
                tolerance,
            } => {
                format!("if_pixel {x:.0} {y:.0} {r} {g} {b} {tolerance}")
            }
        };
        lines.push(line);
    }

    lines.join("\n")
}

// ============================================================================
// Macro library (collection of named macros)
// ============================================================================

/// A library of macros, keyed by ID.
#[derive(Clone, Debug)]
pub struct MacroLibrary {
    macros: Vec<Macro>,
    next_id: u64,
}

impl MacroLibrary {
    pub fn new() -> Self {
        Self {
            macros: Vec::new(),
            next_id: 1,
        }
    }

    /// Create a new empty macro with the given name.
    pub fn create_macro(&mut self, name: &str, now_ms: u64) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.macros.push(Macro::new(id, name, now_ms));
        id
    }

    /// Get a macro by ID.
    pub fn get(&self, id: u64) -> Option<&Macro> {
        self.macros.iter().find(|m| m.id == id)
    }

    /// Get a mutable reference to a macro by ID.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Macro> {
        self.macros.iter_mut().find(|m| m.id == id)
    }

    /// Remove a macro by ID.
    pub fn remove(&mut self, id: u64) -> bool {
        let initial = self.macros.len();
        self.macros.retain(|m| m.id != id);
        self.macros.len() != initial
    }

    /// List all macros.
    pub fn list(&self) -> &[Macro] {
        &self.macros
    }

    /// Number of macros.
    pub fn count(&self) -> usize {
        self.macros.len()
    }

    /// Find a macro whose trigger matches the given hotkey.
    pub fn find_by_hotkey(&self, hotkey: &Hotkey) -> Option<u64> {
        self.macros
            .iter()
            .find(|m| m.trigger.as_ref() == Some(hotkey))
            .map(|m| m.id)
    }

    /// Duplicate a macro.
    pub fn duplicate(&mut self, id: u64, now_ms: u64) -> Option<u64> {
        let source = self.get(id)?.clone();
        let new_id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let mut copy = source;
        copy.id = new_id;
        copy.name = format!("{} (copy)", copy.name);
        copy.trigger = None; // Don't duplicate hotkey triggers.
        copy.created_at_ms = now_ms;
        copy.modified_at_ms = now_ms;
        self.macros.push(copy);
        Some(new_id)
    }
}

impl Default for MacroLibrary {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Recording state
// ============================================================================

/// State of the macro recorder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordingState {
    Idle,
    Recording,
    Paused,
}

impl RecordingState {
    fn label(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Recording => "Recording",
            Self::Paused => "Paused",
        }
    }

    fn is_recording(&self) -> bool {
        matches!(self, Self::Recording)
    }
}

// ============================================================================
// Playback state
// ============================================================================

/// State of macro playback.
#[derive(Clone, Debug, PartialEq)]
pub enum PlaybackState {
    Stopped,
    Playing {
        macro_id: u64,
        action_idx: usize,
        elapsed_ms: u64,
        repeat_count: u32,
    },
    PausedPlayback {
        macro_id: u64,
        action_idx: usize,
        elapsed_ms: u64,
        repeat_count: u32,
    },
}

impl PlaybackState {
    fn is_playing(&self) -> bool {
        matches!(self, Self::Playing { .. })
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Playing { .. } => "Playing",
            Self::PausedPlayback { .. } => "Paused",
        }
    }
}

// ============================================================================
// Active tab
// ============================================================================

/// Which panel/tab is active in the main view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveTab {
    Editor,
    Script,
}

impl ActiveTab {
    fn label(self) -> &'static str {
        match self {
            Self::Editor => "Editor",
            Self::Script => "Script",
        }
    }
}

// ============================================================================
// Application state
// ============================================================================

/// The main application state.
pub struct AutomatorApp {
    library: MacroLibrary,
    selected_macro_id: Option<u64>,
    selected_action_idx: Option<usize>,
    recording_state: RecordingState,
    recording_target_id: Option<u64>,
    recording_last_event_ms: u64,
    playback_state: PlaybackState,
    active_tab: ActiveTab,
    script_text: String,
    script_error: Option<String>,
    sidebar_scroll: f32,
    action_list_scroll: f32,
    elapsed_ms: u64,
    status_message: String,
}

impl AutomatorApp {
    pub fn new() -> Self {
        Self {
            library: MacroLibrary::new(),
            selected_macro_id: None,
            selected_action_idx: None,
            recording_state: RecordingState::Idle,
            recording_target_id: None,
            recording_last_event_ms: 0,
            playback_state: PlaybackState::Stopped,
            active_tab: ActiveTab::Editor,
            script_text: String::new(),
            script_error: None,
            sidebar_scroll: 0.0,
            action_list_scroll: 0.0,
            elapsed_ms: 0,
            status_message: "Ready".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Macro management
    // -----------------------------------------------------------------------

    /// Create a new macro and select it.
    pub fn new_macro(&mut self, name: &str) -> u64 {
        let id = self.library.create_macro(name, self.elapsed_ms);
        self.selected_macro_id = Some(id);
        self.selected_action_idx = None;
        self.status_message = format!("Created macro: {name}");
        id
    }

    /// Delete the selected macro.
    pub fn delete_selected_macro(&mut self) -> bool {
        if let Some(id) = self.selected_macro_id
            && self.library.remove(id)
        {
            self.selected_macro_id = None;
            self.selected_action_idx = None;
            self.status_message = "Macro deleted".to_string();
            return true;
        }
        false
    }

    /// Duplicate the selected macro.
    pub fn duplicate_selected_macro(&mut self) -> Option<u64> {
        let id = self.selected_macro_id?;
        let new_id = self.library.duplicate(id, self.elapsed_ms)?;
        self.selected_macro_id = Some(new_id);
        self.status_message = "Macro duplicated".to_string();
        Some(new_id)
    }

    /// Select a macro by its index in the library list.
    pub fn select_macro_by_index(&mut self, idx: usize) {
        if let Some(mac) = self.library.list().get(idx) {
            self.selected_macro_id = Some(mac.id);
            self.selected_action_idx = None;
            // Update script text to reflect the selected macro.
            if let Some(m) = self.library.get(mac.id) {
                self.script_text = serialize_script(&m.actions);
            }
        }
    }

    /// Select an action in the current macro.
    pub fn select_action(&mut self, idx: usize) {
        self.selected_action_idx = Some(idx);
    }

    // -----------------------------------------------------------------------
    // Recording
    // -----------------------------------------------------------------------

    /// Start recording into the selected macro (or create a new one).
    pub fn start_recording(&mut self) {
        let target_id = self
            .selected_macro_id
            .unwrap_or_else(|| self.new_macro("Recorded Macro"));
        self.recording_target_id = Some(target_id);
        self.recording_state = RecordingState::Recording;
        self.recording_last_event_ms = self.elapsed_ms;
        self.status_message = "Recording...".to_string();
    }

    /// Stop recording.
    pub fn stop_recording(&mut self) {
        self.recording_state = RecordingState::Idle;
        self.recording_target_id = None;
        self.status_message = "Recording stopped".to_string();
        // Refresh script text.
        if let Some(id) = self.selected_macro_id
            && let Some(m) = self.library.get(id)
        {
            self.script_text = serialize_script(&m.actions);
        }
    }

    /// Pause recording.
    pub fn pause_recording(&mut self) {
        if self.recording_state == RecordingState::Recording {
            self.recording_state = RecordingState::Paused;
            self.status_message = "Recording paused".to_string();
        }
    }

    /// Resume recording.
    pub fn resume_recording(&mut self) {
        if self.recording_state == RecordingState::Paused {
            self.recording_state = RecordingState::Recording;
            self.recording_last_event_ms = self.elapsed_ms;
            self.status_message = "Recording resumed".to_string();
        }
    }

    /// Record an action (called by the system event handler while recording).
    pub fn record_action(&mut self, action: MacroAction) {
        if !self.recording_state.is_recording() {
            return;
        }
        let target_id = match self.recording_target_id {
            Some(id) => id,
            None => return,
        };
        let delay = self.elapsed_ms.saturating_sub(self.recording_last_event_ms);
        self.recording_last_event_ms = self.elapsed_ms;
        if let Some(mac) = self.library.get_mut(target_id) {
            mac.actions.push(TimedAction::new(action, delay));
            mac.modified_at_ms = self.elapsed_ms;
        }
    }

    // -----------------------------------------------------------------------
    // Playback
    // -----------------------------------------------------------------------

    /// Start playing the selected macro.
    pub fn start_playback(&mut self) {
        if let Some(id) = self.selected_macro_id
            && self.library.get(id).is_some_and(|m| !m.actions.is_empty())
        {
            self.playback_state = PlaybackState::Playing {
                macro_id: id,
                action_idx: 0,
                elapsed_ms: 0,
                repeat_count: 0,
            };
            self.status_message = "Playing macro...".to_string();
        }
    }

    /// Stop playback.
    pub fn stop_playback(&mut self) {
        self.playback_state = PlaybackState::Stopped;
        self.status_message = "Playback stopped".to_string();
    }

    /// Pause playback.
    pub fn pause_playback(&mut self) {
        if let PlaybackState::Playing {
            macro_id,
            action_idx,
            elapsed_ms,
            repeat_count,
        } = self.playback_state
        {
            self.playback_state = PlaybackState::PausedPlayback {
                macro_id,
                action_idx,
                elapsed_ms,
                repeat_count,
            };
            self.status_message = "Playback paused".to_string();
        }
    }

    /// Resume playback.
    pub fn resume_playback(&mut self) {
        if let PlaybackState::PausedPlayback {
            macro_id,
            action_idx,
            elapsed_ms,
            repeat_count,
        } = self.playback_state
        {
            self.playback_state = PlaybackState::Playing {
                macro_id,
                action_idx,
                elapsed_ms,
                repeat_count,
            };
            self.status_message = "Playback resumed".to_string();
        }
    }

    /// Advance playback by one tick. Returns the action to execute, if any.
    pub fn tick_playback(&mut self, delta_ms: u64) -> Option<MacroAction> {
        let (macro_id, action_idx, elapsed_ms, repeat_count) = match &self.playback_state {
            PlaybackState::Playing {
                macro_id,
                action_idx,
                elapsed_ms,
                repeat_count,
            } => (*macro_id, *action_idx, *elapsed_ms, *repeat_count),
            _ => return None,
        };

        let (total_actions, speed, repeat_mode, timed_action) = {
            let mac = self.library.get(macro_id)?;
            let ta = mac.actions.get(action_idx)?;
            (mac.action_count(), mac.speed, mac.repeat_mode, ta.clone())
        };

        let adjusted_delay = (timed_action.delay_ms as f64 * speed.multiplier()) as u64;
        let new_elapsed = elapsed_ms.saturating_add(delta_ms);

        if new_elapsed >= adjusted_delay {
            // Fire the action.
            let next_idx = action_idx.saturating_add(1);
            if next_idx >= total_actions {
                // End of macro -- check repeat mode.
                let new_repeat = repeat_count.saturating_add(1);
                match repeat_mode {
                    RepeatMode::Once => {
                        self.playback_state = PlaybackState::Stopped;
                        self.status_message = "Playback complete".to_string();
                    }
                    RepeatMode::Times(n) => {
                        if new_repeat >= n {
                            self.playback_state = PlaybackState::Stopped;
                            self.status_message = "Playback complete".to_string();
                        } else {
                            self.playback_state = PlaybackState::Playing {
                                macro_id,
                                action_idx: 0,
                                elapsed_ms: 0,
                                repeat_count: new_repeat,
                            };
                        }
                    }
                    RepeatMode::Forever => {
                        self.playback_state = PlaybackState::Playing {
                            macro_id,
                            action_idx: 0,
                            elapsed_ms: 0,
                            repeat_count: new_repeat,
                        };
                    }
                }
            } else {
                self.playback_state = PlaybackState::Playing {
                    macro_id,
                    action_idx: next_idx,
                    elapsed_ms: 0,
                    repeat_count,
                };
            }
            return Some(timed_action.action);
        }

        // Not yet time to fire.
        self.playback_state = PlaybackState::Playing {
            macro_id,
            action_idx,
            elapsed_ms: new_elapsed,
            repeat_count,
        };
        None
    }

    // -----------------------------------------------------------------------
    // Script tab
    // -----------------------------------------------------------------------

    /// Apply the current script text to the selected macro.
    pub fn apply_script(&mut self) -> bool {
        let id = if let Some(id) = self.selected_macro_id {
            id
        } else {
            self.script_error = Some("No macro selected".to_string());
            return false;
        };

        match parse_script(&self.script_text) {
            Ok(actions) => {
                if let Some(mac) = self.library.get_mut(id) {
                    mac.actions = actions;
                    mac.modified_at_ms = self.elapsed_ms;
                }
                self.script_error = None;
                self.status_message = "Script applied".to_string();
                true
            }
            Err(e) => {
                self.script_error = Some(e.to_string());
                self.status_message = format!("Script error: {e}");
                false
            }
        }
    }

    /// Set the script text (e.g. from user editing).
    pub fn set_script_text(&mut self, text: &str) {
        self.script_text = text.to_string();
    }

    // -----------------------------------------------------------------------
    // Action editing
    // -----------------------------------------------------------------------

    /// Delete the selected action from the current macro.
    pub fn delete_selected_action(&mut self) -> bool {
        let mac_id = match self.selected_macro_id {
            Some(id) => id,
            None => return false,
        };
        let idx = match self.selected_action_idx {
            Some(i) => i,
            None => return false,
        };

        if let Some(mac) = self.library.get_mut(mac_id)
            && mac.remove_action(idx).is_some()
        {
            // Adjust selection.
            if mac.actions.is_empty() {
                self.selected_action_idx = None;
            } else if idx >= mac.actions.len() {
                self.selected_action_idx = Some(mac.actions.len().saturating_sub(1));
            }
            mac.modified_at_ms = self.elapsed_ms;
            self.status_message = "Action deleted".to_string();
            return true;
        }
        false
    }

    /// Move the selected action up.
    pub fn move_action_up(&mut self) -> bool {
        let mac_id = match self.selected_macro_id {
            Some(id) => id,
            None => return false,
        };
        let idx = match self.selected_action_idx {
            Some(i) if i > 0 => i,
            _ => return false,
        };

        if let Some(mac) = self.library.get_mut(mac_id)
            && mac.move_action(idx, idx.saturating_sub(1))
        {
            self.selected_action_idx = Some(idx.saturating_sub(1));
            mac.modified_at_ms = self.elapsed_ms;
            return true;
        }
        false
    }

    /// Move the selected action down.
    pub fn move_action_down(&mut self) -> bool {
        let mac_id = match self.selected_macro_id {
            Some(id) => id,
            None => return false,
        };
        let idx = match self.selected_action_idx {
            Some(i) => i,
            None => return false,
        };

        if let Some(mac) = self.library.get_mut(mac_id) {
            let next = idx.saturating_add(1);
            if next < mac.actions.len() && mac.move_action(idx, next) {
                self.selected_action_idx = Some(next);
                mac.modified_at_ms = self.elapsed_ms;
                return true;
            }
        }
        false
    }

    /// Add a manual action to the current macro at the end.
    pub fn add_action(&mut self, action: MacroAction, delay_ms: u64) -> bool {
        let mac_id = match self.selected_macro_id {
            Some(id) => id,
            None => return false,
        };
        if let Some(mac) = self.library.get_mut(mac_id) {
            mac.actions.push(TimedAction::new(action, delay_ms));
            mac.modified_at_ms = self.elapsed_ms;
            self.status_message = "Action added".to_string();
            return true;
        }
        false
    }

    /// Set the trigger hotkey for the selected macro.
    pub fn set_trigger(&mut self, hotkey: Option<Hotkey>) {
        if let Some(id) = self.selected_macro_id
            && let Some(mac) = self.library.get_mut(id)
        {
            mac.trigger = hotkey;
            mac.modified_at_ms = self.elapsed_ms;
            self.status_message = "Trigger updated".to_string();
        }
    }

    /// Set the playback speed for the selected macro.
    pub fn set_speed(&mut self, speed: PlaybackSpeed) {
        if let Some(id) = self.selected_macro_id
            && let Some(mac) = self.library.get_mut(id)
        {
            mac.speed = speed;
        }
    }

    /// Cycle the playback speed for the selected macro.
    pub fn cycle_speed(&mut self) {
        if let Some(id) = self.selected_macro_id
            && let Some(mac) = self.library.get_mut(id)
        {
            mac.speed = mac.speed.next();
            self.status_message = format!("Speed: {}", mac.speed.label());
        }
    }

    /// Set the repeat mode for the selected macro.
    pub fn set_repeat_mode(&mut self, mode: RepeatMode) {
        if let Some(id) = self.selected_macro_id
            && let Some(mac) = self.library.get_mut(id)
        {
            mac.repeat_mode = mode;
        }
    }

    /// Advance elapsed time.
    pub fn tick(&mut self, delta_ms: u64) {
        self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// Render the entire UI, returning a list of `RenderCommand`s.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut cmds, width);
        self.render_toolbar(&mut cmds, width);

        let content_y = HEADER_HEIGHT + TOOLBAR_HEIGHT;
        let content_h = height - content_y - STATUS_BAR_HEIGHT;

        self.render_sidebar(&mut cmds, content_y, content_h);
        self.render_action_list(&mut cmds, content_y, content_h, width);
        self.render_properties_panel(&mut cmds, content_y, content_h, width);
        self.render_status_bar(&mut cmds, width, height);

        cmds
    }

    /// Render the title header bar.
    fn render_header(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        // Header background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: HEADER_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 14.0,
            text: "Automator".to_string(),
            color: TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Recording indicator.
        if self.recording_state.is_recording() {
            let pulse = (self.elapsed_ms % RECORDING_PULSE_PERIOD_MS) as f32
                / RECORDING_PULSE_PERIOD_MS as f32;
            let alpha = if pulse < 0.5 {
                (pulse * 2.0 * 255.0) as u8
            } else {
                ((1.0 - (pulse - 0.5) * 2.0) * 255.0) as u8
            };
            let indicator_color = Color::rgba(RED.r, RED.g, RED.b, alpha);

            cmds.push(RenderCommand::FillRect {
                x: 110.0,
                y: 14.0,
                width: 12.0,
                height: 12.0,
                color: indicator_color,
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: 128.0,
                y: 14.0,
                text: "REC".to_string(),
                color: RED,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Playback state indicator.
        if self.playback_state.is_playing() {
            let play_x = if self.recording_state.is_recording() {
                175.0
            } else {
                110.0
            };
            cmds.push(RenderCommand::Text {
                x: play_x,
                y: 14.0,
                text: format!("PLAYING ({})", self.playback_state.label()),
                color: GREEN,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Tab bar at the right side of the header.
        let tabs = [ActiveTab::Editor, ActiveTab::Script];
        let tab_w = 80.0;
        let tab_start_x = width - (tabs.len() as f32 * tab_w) - PADDING;
        for (i, tab) in tabs.iter().enumerate() {
            let tx = tab_start_x + i as f32 * tab_w;
            let selected = *tab == self.active_tab;
            let bg = if selected { SURFACE0 } else { CRUST };
            let fg = if selected { BLUE } else { SUBTEXT0 };

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: 10.0,
                width: tab_w - 4.0,
                height: 26.0,
                color: bg,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            cmds.push(RenderCommand::Text {
                x: tx + 12.0,
                y: 15.0,
                text: tab.label().to_string(),
                color: fg,
                font_size: FONT_SIZE,
                font_weight: if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_w - 20.0),
            });
        }
    }

    /// Render the toolbar with recording/playback controls.
    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        let y = HEADER_HEIGHT;

        // Toolbar background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width,
            height: TOOLBAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator line.
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TOOLBAR_HEIGHT - 1.0,
            x2: width,
            y2: y + TOOLBAR_HEIGHT - 1.0,
            color: SURFACE0,
            width: 1.0,
        });

        let btn_y = y + 5.0;
        let mut bx = PADDING;

        // Record button.
        let rec_color = if self.recording_state.is_recording() {
            RED
        } else {
            SURFACE1
        };
        bx = self.render_toolbar_button(cmds, bx, btn_y, "Record", rec_color, TEXT);

        // Stop recording.
        bx = self.render_toolbar_button(cmds, bx, btn_y, "Stop Rec", SURFACE1, TEXT);

        // Separator.
        bx += 8.0;
        cmds.push(RenderCommand::Line {
            x1: bx,
            y1: btn_y,
            x2: bx,
            y2: btn_y + BUTTON_HEIGHT,
            color: SURFACE2,
            width: 1.0,
        });
        bx += 8.0;

        // Play button.
        let play_color = if self.playback_state.is_playing() {
            GREEN
        } else {
            SURFACE1
        };
        bx = self.render_toolbar_button(cmds, bx, btn_y, "Play", play_color, TEXT);

        // Pause.
        bx = self.render_toolbar_button(cmds, bx, btn_y, "Pause", SURFACE1, TEXT);

        // Stop.
        bx = self.render_toolbar_button(cmds, bx, btn_y, "Stop", SURFACE1, TEXT);

        // Separator.
        bx += 8.0;
        cmds.push(RenderCommand::Line {
            x1: bx,
            y1: btn_y,
            x2: bx,
            y2: btn_y + BUTTON_HEIGHT,
            color: SURFACE2,
            width: 1.0,
        });
        bx += 8.0;

        // Speed label.
        let speed_label = self
            .selected_macro_id
            .and_then(|id| self.library.get(id))
            .map_or("1x", |m| m.speed.label());
        bx = self.render_toolbar_button(
            cmds,
            bx,
            btn_y,
            &format!("Speed: {speed_label}"),
            SURFACE1,
            PEACH,
        );

        // Repeat mode label.
        let repeat_label = self
            .selected_macro_id
            .and_then(|id| self.library.get(id))
            .map_or_else(|| "Once".to_string(), |m| m.repeat_mode.label());
        let _ = self.render_toolbar_button(
            cmds,
            bx,
            btn_y,
            &format!("Repeat: {repeat_label}"),
            SURFACE1,
            LAVENDER,
        );
    }

    /// Render a toolbar button, returning the x position after the button.
    fn render_toolbar_button(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        label: &str,
        bg: Color,
        fg: Color,
    ) -> f32 {
        let btn_w = label.len() as f32 * 7.5 + 16.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: btn_w,
            height: BUTTON_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 7.0,
            text: label.to_string(),
            color: fg,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(btn_w - 12.0),
        });
        x + btn_w + 6.0
    }

    /// Render the macro list sidebar.
    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, content_y: f32, content_h: f32) {
        // Sidebar background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: content_y,
            width: SIDEBAR_WIDTH,
            height: content_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Sidebar header.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: content_y,
            width: SIDEBAR_WIDTH,
            height: ROW_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: content_y + 8.0,
            text: format!("Macros ({})", self.library.count()),
            color: TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 2.0 * PADDING),
        });

        // Macro list.
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_y + ROW_HEIGHT,
            width: SIDEBAR_WIDTH,
            height: content_h - ROW_HEIGHT,
        });

        let list_y_start = content_y + ROW_HEIGHT;
        for (i, mac) in self.library.list().iter().enumerate() {
            let row_y = list_y_start + i as f32 * ROW_HEIGHT - self.sidebar_scroll;
            if row_y + ROW_HEIGHT < list_y_start || row_y > content_y + content_h {
                continue;
            }

            let selected = self.selected_macro_id == Some(mac.id);
            let bg = if selected { SURFACE0 } else { MANTLE };
            let fg = if selected { TEXT } else { SUBTEXT1 };

            cmds.push(RenderCommand::FillRect {
                x: 4.0,
                y: row_y,
                width: SIDEBAR_WIDTH - 8.0,
                height: ROW_HEIGHT - 2.0,
                color: bg,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            // Macro name.
            cmds.push(RenderCommand::Text {
                x: PADDING + 4.0,
                y: row_y + 4.0,
                text: mac.name.clone(),
                color: fg,
                font_size: FONT_SIZE,
                font_weight: if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - 3.0 * PADDING),
            });

            // Action count badge.
            let count_text = format!("{}", mac.action_count());
            cmds.push(RenderCommand::Text {
                x: SIDEBAR_WIDTH - 40.0,
                y: row_y + 4.0,
                text: count_text,
                color: OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(30.0),
            });

            // Trigger indicator.
            if mac.trigger.is_some() {
                cmds.push(RenderCommand::FillRect {
                    x: SIDEBAR_WIDTH - 56.0,
                    y: row_y + 8.0,
                    width: 10.0,
                    height: 10.0,
                    color: PEACH,
                    corner_radii: CornerRadii::all(5.0),
                });
            }
        }

        cmds.push(RenderCommand::PopClip);

        // Sidebar border (right edge).
        cmds.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: content_y,
            x2: SIDEBAR_WIDTH,
            y2: content_y + content_h,
            color: SURFACE0,
            width: 1.0,
        });

        // New / Delete buttons at bottom of sidebar.
        let btn_area_y = content_y + content_h - 36.0;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: btn_area_y,
            width: SIDEBAR_WIDTH,
            height: 36.0,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let half = (SIDEBAR_WIDTH - 3.0 * PADDING) / 2.0;
        // New button.
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: btn_area_y + 4.0,
            width: half,
            height: BUTTON_HEIGHT,
            color: BLUE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: PADDING + half / 2.0 - 12.0,
            y: btn_area_y + 11.0,
            text: "+ New".to_string(),
            color: CRUST,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(half - 8.0),
        });

        // Delete button.
        let del_x = PADDING * 2.0 + half;
        cmds.push(RenderCommand::FillRect {
            x: del_x,
            y: btn_area_y + 4.0,
            width: half,
            height: BUTTON_HEIGHT,
            color: SURFACE1,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: del_x + half / 2.0 - 16.0,
            y: btn_area_y + 11.0,
            text: "Delete".to_string(),
            color: RED,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(half - 8.0),
        });
    }

    /// Render the action list (center panel).
    fn render_action_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_y: f32,
        content_h: f32,
        width: f32,
    ) {
        let list_x = SIDEBAR_WIDTH;
        let list_w = width - SIDEBAR_WIDTH - PROPERTIES_WIDTH;

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: list_x,
            y: content_y,
            width: list_w,
            height: content_h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Panel header.
        cmds.push(RenderCommand::FillRect {
            x: list_x,
            y: content_y,
            width: list_w,
            height: ROW_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let header_text = match self.active_tab {
            ActiveTab::Editor => "Actions",
            ActiveTab::Script => "Script",
        };
        cmds.push(RenderCommand::Text {
            x: list_x + PADDING,
            y: content_y + 8.0,
            text: header_text.to_string(),
            color: TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(list_w - 2.0 * PADDING),
        });

        match self.active_tab {
            ActiveTab::Editor => {
                self.render_action_editor(
                    cmds,
                    list_x,
                    content_y + ROW_HEIGHT,
                    list_w,
                    content_h - ROW_HEIGHT,
                );
            }
            ActiveTab::Script => {
                self.render_script_editor(
                    cmds,
                    list_x,
                    content_y + ROW_HEIGHT,
                    list_w,
                    content_h - ROW_HEIGHT,
                );
            }
        }

        // Right border.
        cmds.push(RenderCommand::Line {
            x1: list_x + list_w,
            y1: content_y,
            x2: list_x + list_w,
            y2: content_y + content_h,
            color: SURFACE0,
            width: 1.0,
        });
    }

    /// Render the visual action editor.
    fn render_action_editor(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        let actions = if let Some(mac) = self.selected_macro_id.and_then(|id| self.library.get(id))
        {
            &mac.actions
        } else {
            // Empty state.
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 80.0,
                y: y + h / 2.0 - 10.0,
                text: "Select a macro to edit".to_string(),
                color: OVERLAY0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 2.0 * PADDING),
            });
            return;
        };

        if actions.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 100.0,
                y: y + h / 2.0 - 10.0,
                text: "No actions. Start recording or add manually.".to_string(),
                color: OVERLAY0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 2.0 * PADDING),
            });
            return;
        }

        // Clip to panel area.
        cmds.push(RenderCommand::PushClip {
            x,
            y,
            width: w,
            height: h,
        });

        for (i, timed) in actions.iter().enumerate() {
            let row_y = y + i as f32 * ROW_HEIGHT - self.action_list_scroll;
            if row_y + ROW_HEIGHT < y || row_y > y + h {
                continue;
            }

            let selected = self.selected_action_idx == Some(i);
            let bg = if selected { SURFACE0 } else { BASE };
            let fg = if selected { TEXT } else { SUBTEXT1 };

            // Row background.
            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: row_y,
                width: w - 8.0,
                height: ROW_HEIGHT - 2.0,
                color: bg,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            // Index number.
            let num_text = format!("{:>3}", i.saturating_add(1));
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: row_y + 8.0,
                text: num_text,
                color: OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(30.0),
            });

            // Action type badge.
            let badge_color = timed.action.badge_color();
            cmds.push(RenderCommand::FillRect {
                x: x + 38.0,
                y: row_y + 5.0,
                width: 24.0,
                height: 18.0,
                color: badge_color,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 41.0,
                y: row_y + 8.0,
                text: timed.action.icon().to_string(),
                color: CRUST,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(20.0),
            });

            // Action label.
            cmds.push(RenderCommand::Text {
                x: x + 70.0,
                y: row_y + 8.0,
                text: timed.action.label(),
                color: fg,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 160.0),
            });

            // Delay badge.
            if timed.delay_ms > 0 {
                let delay_text = format_duration_ms(timed.delay_ms);
                cmds.push(RenderCommand::Text {
                    x: x + w - 70.0,
                    y: row_y + 8.0,
                    text: delay_text,
                    color: YELLOW,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(60.0),
                });
            }

            // Playback position indicator.
            if let PlaybackState::Playing { action_idx, .. } = &self.playback_state
                && *action_idx == i
            {
                cmds.push(RenderCommand::FillRect {
                    x: x + 2.0,
                    y: row_y,
                    width: 3.0,
                    height: ROW_HEIGHT - 2.0,
                    color: GREEN,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        cmds.push(RenderCommand::PopClip);

        // Action toolbar at bottom (move up/down, delete).
        let btn_y = y + h - 36.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y: btn_y,
            width: w,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let mut bx = x + PADDING;
        let small_btn_w = 50.0;

        // Move Up button.
        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: btn_y + 4.0,
            width: small_btn_w,
            height: BUTTON_HEIGHT,
            color: SURFACE1,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: bx + 10.0,
            y: btn_y + 11.0,
            text: "Up".to_string(),
            color: TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(small_btn_w - 12.0),
        });
        bx += small_btn_w + 4.0;

        // Move Down button.
        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: btn_y + 4.0,
            width: small_btn_w,
            height: BUTTON_HEIGHT,
            color: SURFACE1,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: bx + 10.0,
            y: btn_y + 11.0,
            text: "Down".to_string(),
            color: TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(small_btn_w - 12.0),
        });
        bx += small_btn_w + 4.0;

        // Delete button.
        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: btn_y + 4.0,
            width: small_btn_w + 10.0,
            height: BUTTON_HEIGHT,
            color: SURFACE1,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: bx + 8.0,
            y: btn_y + 11.0,
            text: "Delete".to_string(),
            color: RED,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(small_btn_w),
        });
    }

    /// Render the script text editor tab.
    fn render_script_editor(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        // Script text area background.
        cmds.push(RenderCommand::FillRect {
            x: x + 4.0,
            y: y + 4.0,
            width: w - 8.0,
            height: h - 44.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Render script lines.
        cmds.push(RenderCommand::PushClip {
            x: x + 4.0,
            y: y + 4.0,
            width: w - 8.0,
            height: h - 44.0,
        });

        let line_height = 18.0;
        for (i, line) in self.script_text.lines().enumerate() {
            let ly = y + 8.0 + i as f32 * line_height;
            if ly > y + h - 44.0 {
                break;
            }

            // Line number.
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: ly,
                text: format!("{:>3}", i.saturating_add(1)),
                color: OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(30.0),
            });

            // Line content -- color based on content type.
            let line_color = if line.starts_with('#') {
                OVERLAY0
            } else if line.starts_with('$') {
                PEACH
            } else if line.starts_with(':') {
                YELLOW
            } else {
                TEXT
            };

            cmds.push(RenderCommand::Text {
                x: x + 42.0,
                y: ly,
                text: line.to_string(),
                color: line_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 54.0),
            });
        }

        cmds.push(RenderCommand::PopClip);

        // Error display.
        if let Some(ref err) = self.script_error {
            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: y + h - 62.0,
                width: w - 8.0,
                height: 20.0,
                color: Color::rgba(RED.r, RED.g, RED.b, 40),
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: y + h - 58.0,
                text: err.clone(),
                color: RED,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 16.0),
            });
        }

        // Apply button.
        let btn_y = y + h - 36.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y: btn_y,
            width: w,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let apply_w = 100.0;
        cmds.push(RenderCommand::FillRect {
            x: x + PADDING,
            y: btn_y + 4.0,
            width: apply_w,
            height: BUTTON_HEIGHT,
            color: BLUE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: x + PADDING + 18.0,
            y: btn_y + 11.0,
            text: "Apply Script".to_string(),
            color: CRUST,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(apply_w - 12.0),
        });
    }

    /// Render the properties panel (right side).
    fn render_properties_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_y: f32,
        content_h: f32,
        width: f32,
    ) {
        let panel_x = width - PROPERTIES_WIDTH;

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: panel_x,
            y: content_y,
            width: PROPERTIES_WIDTH,
            height: content_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Panel header.
        cmds.push(RenderCommand::FillRect {
            x: panel_x,
            y: content_y,
            width: PROPERTIES_WIDTH,
            height: ROW_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: panel_x + PADDING,
            y: content_y + 8.0,
            text: "Properties".to_string(),
            color: TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PROPERTIES_WIDTH - 2.0 * PADDING),
        });

        let prop_y = content_y + ROW_HEIGHT + PADDING;

        // Show macro properties if a macro is selected.
        if let Some(mac) = self.selected_macro_id.and_then(|id| self.library.get(id)) {
            let mut cy = prop_y;

            // Macro name.
            cy = self.render_property_row(cmds, panel_x, cy, "Name", &mac.name);

            // Description.
            let desc = if mac.description.is_empty() {
                "(none)"
            } else {
                &mac.description
            };
            cy = self.render_property_row(cmds, panel_x, cy, "Description", desc);

            // Action count.
            cy = self.render_property_row(
                cmds,
                panel_x,
                cy,
                "Actions",
                &mac.action_count().to_string(),
            );

            // Total duration.
            cy = self.render_property_row(
                cmds,
                panel_x,
                cy,
                "Duration",
                &format_duration_ms(mac.total_duration_ms()),
            );

            // Speed.
            cy = self.render_property_row(cmds, panel_x, cy, "Speed", mac.speed.label());

            // Repeat mode.
            cy = self.render_property_row(cmds, panel_x, cy, "Repeat", &mac.repeat_mode.label());

            // Trigger.
            let trigger_text = mac
                .trigger
                .as_ref()
                .map_or_else(|| "(none)".to_string(), Hotkey::label);
            cy = self.render_property_row(cmds, panel_x, cy, "Trigger", &trigger_text);

            // Separator.
            cy += 8.0;
            cmds.push(RenderCommand::Line {
                x1: panel_x + PADDING,
                y1: cy,
                x2: panel_x + PROPERTIES_WIDTH - PADDING,
                y2: cy,
                color: SURFACE0,
                width: 1.0,
            });
            cy += 8.0;

            // Selected action properties.
            if let Some(action_idx) = self.selected_action_idx {
                if let Some(timed) = mac.actions.get(action_idx) {
                    cmds.push(RenderCommand::Text {
                        x: panel_x + PADDING,
                        y: cy,
                        text: "Action Properties".to_string(),
                        color: BLUE,
                        font_size: FONT_SIZE,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(PROPERTIES_WIDTH - 2.0 * PADDING),
                    });
                    cy += 22.0;

                    cy = self.render_property_row(cmds, panel_x, cy, "Type", timed.action.icon());
                    cy = self.render_property_row(
                        cmds,
                        panel_x,
                        cy,
                        "Delay",
                        &format!("{}ms", timed.delay_ms),
                    );

                    // Type-specific properties.
                    match &timed.action {
                        MacroAction::KeyPress { key_name }
                        | MacroAction::KeyRelease { key_name } => {
                            self.render_property_row(cmds, panel_x, cy, "Key", key_name);
                        }
                        MacroAction::MouseClick { x, y, button }
                        | MacroAction::MouseDoubleClick { x, y, button } => {
                            cy = self.render_property_row(
                                cmds,
                                panel_x,
                                cy,
                                "Position",
                                &format!("({x:.0}, {y:.0})"),
                            );
                            self.render_property_row(cmds, panel_x, cy, "Button", button.label());
                        }
                        MacroAction::MouseMove { x, y } => {
                            self.render_property_row(
                                cmds,
                                panel_x,
                                cy,
                                "Target",
                                &format!("({x:.0}, {y:.0})"),
                            );
                        }
                        MacroAction::Scroll { direction, amount } => {
                            cy = self.render_property_row(
                                cmds,
                                panel_x,
                                cy,
                                "Direction",
                                direction.label(),
                            );
                            self.render_property_row(
                                cmds,
                                panel_x,
                                cy,
                                "Amount",
                                &amount.to_string(),
                            );
                        }
                        MacroAction::TypeText { text } => {
                            let preview: String = text.chars().take(30).collect();
                            self.render_property_row(cmds, panel_x, cy, "Text", &preview);
                        }
                        MacroAction::Delay { ms } => {
                            self.render_property_row(cmds, panel_x, cy, "Wait", &format!("{ms}ms"));
                        }
                        MacroAction::IfPixelColor {
                            x,
                            y,
                            r,
                            g,
                            b,
                            tolerance,
                        } => {
                            cy = self.render_property_row(
                                cmds,
                                panel_x,
                                cy,
                                "Pixel",
                                &format!("({x:.0}, {y:.0})"),
                            );
                            cy = self.render_property_row(
                                cmds,
                                panel_x,
                                cy,
                                "Color",
                                &format!("#{r:02X}{g:02X}{b:02X}"),
                            );
                            self.render_property_row(
                                cmds,
                                panel_x,
                                cy,
                                "Tolerance",
                                &tolerance.to_string(),
                            );
                        }
                    }
                }
            } else {
                cmds.push(RenderCommand::Text {
                    x: panel_x + PADDING,
                    y: cy,
                    text: "Select an action to see details".to_string(),
                    color: OVERLAY0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(PROPERTIES_WIDTH - 2.0 * PADDING),
                });
            }
        } else {
            // No macro selected.
            cmds.push(RenderCommand::Text {
                x: panel_x + PADDING,
                y: prop_y + 20.0,
                text: "No macro selected".to_string(),
                color: OVERLAY0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(PROPERTIES_WIDTH - 2.0 * PADDING),
            });
        }

        // Speed control section at bottom.
        let speed_section_y = content_y + content_h - 100.0;
        cmds.push(RenderCommand::Line {
            x1: panel_x + PADDING,
            y1: speed_section_y,
            x2: panel_x + PROPERTIES_WIDTH - PADDING,
            y2: speed_section_y,
            color: SURFACE0,
            width: 1.0,
        });

        cmds.push(RenderCommand::Text {
            x: panel_x + PADDING,
            y: speed_section_y + 8.0,
            text: "Playback Speed".to_string(),
            color: TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PROPERTIES_WIDTH - 2.0 * PADDING),
        });

        let selected_speed = self
            .selected_macro_id
            .and_then(|id| self.library.get(id))
            .map_or(PlaybackSpeed::Normal, |m| m.speed);

        let speed_btn_w = 44.0;
        let speed_y = speed_section_y + 28.0;
        for (i, speed) in PlaybackSpeed::all().iter().enumerate() {
            let sx = panel_x + PADDING + i as f32 * (speed_btn_w + 4.0);
            let is_selected = *speed == selected_speed;
            let bg = if is_selected { BLUE } else { SURFACE1 };
            let fg = if is_selected { CRUST } else { TEXT };

            cmds.push(RenderCommand::FillRect {
                x: sx,
                y: speed_y,
                width: speed_btn_w,
                height: 24.0,
                color: bg,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: sx + 6.0,
                y: speed_y + 5.0,
                text: speed.label().to_string(),
                color: fg,
                font_size: FONT_SIZE_SMALL,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(speed_btn_w - 8.0),
            });
        }

        // Repeat mode.
        cmds.push(RenderCommand::Text {
            x: panel_x + PADDING,
            y: speed_y + 32.0,
            text: "Repeat Mode".to_string(),
            color: TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PROPERTIES_WIDTH - 2.0 * PADDING),
        });

        let selected_repeat = self
            .selected_macro_id
            .and_then(|id| self.library.get(id))
            .map_or(RepeatMode::Once, |m| m.repeat_mode);
        let repeat_modes = [RepeatMode::Once, RepeatMode::Times(5), RepeatMode::Forever];
        let repeat_y = speed_y + 52.0;
        for (i, mode) in repeat_modes.iter().enumerate() {
            let rx = panel_x + PADDING + i as f32 * 76.0;
            let is_selected = *mode == selected_repeat;
            let bg = if is_selected { LAVENDER } else { SURFACE1 };
            let fg = if is_selected { CRUST } else { TEXT };

            cmds.push(RenderCommand::FillRect {
                x: rx,
                y: repeat_y,
                width: 70.0,
                height: 24.0,
                color: bg,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: rx + 8.0,
                y: repeat_y + 5.0,
                text: mode.label(),
                color: fg,
                font_size: FONT_SIZE_SMALL,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(60.0),
            });
        }
    }

    /// Render a single property row (label + value). Returns next y position.
    fn render_property_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        panel_x: f32,
        y: f32,
        label: &str,
        value: &str,
    ) -> f32 {
        cmds.push(RenderCommand::Text {
            x: panel_x + PADDING,
            y,
            text: label.to_string(),
            color: SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });
        cmds.push(RenderCommand::Text {
            x: panel_x + 90.0,
            y,
            text: value.to_string(),
            color: TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(PROPERTIES_WIDTH - 100.0),
        });
        y + 20.0
    }

    /// Render the status bar at the bottom.
    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        let bar_y = height - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Status message.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: bar_y + 6.0,
            text: self.status_message.clone(),
            color: SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width / 2.0),
        });

        // Recording state on right.
        let state_text = format!(
            "Rec: {} | Play: {}",
            self.recording_state.label(),
            self.playback_state.label()
        );
        cmds.push(RenderCommand::Text {
            x: width - 200.0,
            y: bar_y + 6.0,
            text: state_text,
            color: OVERLAY0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
        });
    }
}

impl Default for AutomatorApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Format a millisecond duration as a human-readable string.
fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let secs = ms / 1000;
    let rem_ms = ms % 1000;
    if secs < 60 {
        if rem_ms > 0 {
            return format!("{secs}.{rem_ms:03}s");
        }
        return format!("{secs}s");
    }
    let mins = secs / 60;
    let rem_secs = secs % 60;
    format!("{mins}m {rem_secs}s")
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let mut app = AutomatorApp::new();

    // Create some demo macros.
    let id = app.new_macro("Login Sequence");
    app.add_action(
        MacroAction::MouseClick {
            x: 500.0,
            y: 300.0,
            button: MacroMouseButton::Left,
        },
        0,
    );
    app.add_action(
        MacroAction::TypeText {
            text: "admin".to_string(),
        },
        100,
    );
    app.add_action(
        MacroAction::KeyPress {
            key_name: "Tab".to_string(),
        },
        50,
    );
    app.add_action(
        MacroAction::KeyRelease {
            key_name: "Tab".to_string(),
        },
        50,
    );
    app.add_action(
        MacroAction::TypeText {
            text: "password123".to_string(),
        },
        100,
    );
    app.add_action(
        MacroAction::KeyPress {
            key_name: "Enter".to_string(),
        },
        200,
    );
    app.add_action(
        MacroAction::KeyRelease {
            key_name: "Enter".to_string(),
        },
        50,
    );

    // Set trigger for the login macro (using Hotkey::from_str for text-based configuration).
    app.set_trigger(Hotkey::from_str("Ctrl+Alt+L"));

    let _ = id;
    let _id2 = app.new_macro("Screenshot Workflow");
    app.add_action(MacroAction::Delay { ms: 500 }, 0);
    app.add_action(
        MacroAction::KeyPress {
            key_name: "PrintScreen".to_string(),
        },
        500,
    );
    app.add_action(
        MacroAction::KeyRelease {
            key_name: "PrintScreen".to_string(),
        },
        50,
    );
    app.add_action(MacroAction::MouseMove { x: 100.0, y: 100.0 }, 200);
    app.add_action(
        MacroAction::MouseClick {
            x: 100.0,
            y: 100.0,
            button: MacroMouseButton::Left,
        },
        100,
    );

    // Select the first macro.
    app.select_macro_by_index(0);
    app.select_action(2);

    let cmds = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
    let _ = cmds.len();
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

    // -----------------------------------------------------------------------
    // MacroMouseButton tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_macro_mouse_button_label() {
        assert_eq!(MacroMouseButton::Left.label(), "Left");
        assert_eq!(MacroMouseButton::Right.label(), "Right");
        assert_eq!(MacroMouseButton::Middle.label(), "Middle");
    }

    #[test]
    fn test_macro_mouse_button_from_str() {
        assert_eq!(
            MacroMouseButton::from_str("left"),
            Some(MacroMouseButton::Left)
        );
        assert_eq!(
            MacroMouseButton::from_str("RIGHT"),
            Some(MacroMouseButton::Right)
        );
        assert_eq!(
            MacroMouseButton::from_str("Middle"),
            Some(MacroMouseButton::Middle)
        );
        assert_eq!(MacroMouseButton::from_str("unknown"), None);
    }

    // -----------------------------------------------------------------------
    // ScrollDirection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_scroll_direction_label() {
        assert_eq!(ScrollDirection::Up.label(), "Up");
        assert_eq!(ScrollDirection::Down.label(), "Down");
        assert_eq!(ScrollDirection::Left.label(), "Left");
        assert_eq!(ScrollDirection::Right.label(), "Right");
    }

    #[test]
    fn test_scroll_direction_from_str() {
        assert_eq!(ScrollDirection::from_str("up"), Some(ScrollDirection::Up));
        assert_eq!(
            ScrollDirection::from_str("DOWN"),
            Some(ScrollDirection::Down)
        );
        assert_eq!(ScrollDirection::from_str("bad"), None);
    }

    // -----------------------------------------------------------------------
    // MacroAction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_action_label_key_press() {
        let a = MacroAction::KeyPress {
            key_name: "A".to_string(),
        };
        assert_eq!(a.label(), "Key Down: A");
    }

    #[test]
    fn test_action_label_mouse_click() {
        let a = MacroAction::MouseClick {
            x: 100.0,
            y: 200.0,
            button: MacroMouseButton::Left,
        };
        assert_eq!(a.label(), "Left Click (100, 200)");
    }

    #[test]
    fn test_action_label_type_text_short() {
        let a = MacroAction::TypeText {
            text: "Hello".to_string(),
        };
        assert_eq!(a.label(), "Type \"Hello\"");
    }

    #[test]
    fn test_action_label_type_text_long() {
        let a = MacroAction::TypeText {
            text: "This is a very long text that exceeds twenty characters".to_string(),
        };
        assert!(a.label().contains("..."));
    }

    #[test]
    fn test_action_label_delay() {
        let a = MacroAction::Delay { ms: 500 };
        assert_eq!(a.label(), "Wait 500ms");
    }

    #[test]
    fn test_action_icon() {
        assert_eq!(
            MacroAction::KeyPress {
                key_name: "A".to_string()
            }
            .icon(),
            "KB"
        );
        assert_eq!(MacroAction::MouseMove { x: 0.0, y: 0.0 }.icon(), "MV");
        assert_eq!(
            MacroAction::TypeText {
                text: String::new()
            }
            .icon(),
            "TX"
        );
        assert_eq!(MacroAction::Delay { ms: 0 }.icon(), "DL");
    }

    // -----------------------------------------------------------------------
    // TimedAction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_timed_action_immediate() {
        let ta = TimedAction::immediate(MacroAction::Delay { ms: 100 });
        assert_eq!(ta.delay_ms, 0);
    }

    #[test]
    fn test_timed_action_with_delay() {
        let ta = TimedAction::new(MacroAction::Delay { ms: 100 }, 250);
        assert_eq!(ta.delay_ms, 250);
    }

    // -----------------------------------------------------------------------
    // PlaybackSpeed tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_playback_speed_labels() {
        assert_eq!(PlaybackSpeed::Half.label(), "0.5x");
        assert_eq!(PlaybackSpeed::Normal.label(), "1x");
        assert_eq!(PlaybackSpeed::Double.label(), "2x");
        assert_eq!(PlaybackSpeed::Quintuple.label(), "5x");
        assert_eq!(PlaybackSpeed::Instant.label(), "Instant");
    }

    #[test]
    fn test_playback_speed_multiplier() {
        assert!((PlaybackSpeed::Normal.multiplier() - 1.0).abs() < f64::EPSILON);
        assert!((PlaybackSpeed::Double.multiplier() - 0.5).abs() < f64::EPSILON);
        assert!((PlaybackSpeed::Instant.multiplier()).abs() < f64::EPSILON);
    }

    #[test]
    fn test_playback_speed_cycle() {
        assert_eq!(PlaybackSpeed::Half.next(), PlaybackSpeed::Normal);
        assert_eq!(PlaybackSpeed::Normal.next(), PlaybackSpeed::Double);
        assert_eq!(PlaybackSpeed::Instant.next(), PlaybackSpeed::Half);
    }

    #[test]
    fn test_playback_speed_all() {
        let all = PlaybackSpeed::all();
        assert_eq!(all.len(), 5);
    }

    // -----------------------------------------------------------------------
    // RepeatMode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_repeat_mode_label() {
        assert_eq!(RepeatMode::Once.label(), "Once");
        assert_eq!(RepeatMode::Times(3).label(), "3x");
        assert_eq!(RepeatMode::Forever.label(), "Loop");
    }

    // -----------------------------------------------------------------------
    // Hotkey tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_hotkey_label() {
        let hk = Hotkey {
            ctrl: true,
            alt: true,
            shift: false,
            key_name: "F5".to_string(),
        };
        assert_eq!(hk.label(), "Ctrl+Alt+F5");
    }

    #[test]
    fn test_hotkey_label_all_modifiers() {
        let hk = Hotkey {
            ctrl: true,
            alt: true,
            shift: true,
            key_name: "A".to_string(),
        };
        assert_eq!(hk.label(), "Ctrl+Alt+Shift+A");
    }

    #[test]
    fn test_hotkey_from_str() {
        let hk = Hotkey::from_str("Ctrl+Alt+F5").unwrap();
        assert!(hk.ctrl);
        assert!(hk.alt);
        assert!(!hk.shift);
        assert_eq!(hk.key_name, "F5");
    }

    #[test]
    fn test_hotkey_from_str_no_modifiers() {
        let hk = Hotkey::from_str("F1").unwrap();
        assert!(!hk.ctrl);
        assert!(!hk.alt);
        assert!(!hk.shift);
        assert_eq!(hk.key_name, "F1");
    }

    #[test]
    fn test_hotkey_from_str_empty() {
        assert!(Hotkey::from_str("").is_none());
    }

    // -----------------------------------------------------------------------
    // Macro tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_macro_new() {
        let m = Macro::new(1, "Test", 1000);
        assert_eq!(m.id, 1);
        assert_eq!(m.name, "Test");
        assert_eq!(m.created_at_ms, 1000);
        assert!(m.actions.is_empty());
    }

    #[test]
    fn test_macro_total_duration() {
        let mut m = Macro::new(1, "Test", 0);
        m.actions
            .push(TimedAction::new(MacroAction::Delay { ms: 100 }, 50));
        m.actions
            .push(TimedAction::new(MacroAction::Delay { ms: 200 }, 100));
        assert_eq!(m.total_duration_ms(), 150);
    }

    #[test]
    fn test_macro_move_action() {
        let mut m = Macro::new(1, "Test", 0);
        m.actions
            .push(TimedAction::immediate(MacroAction::Delay { ms: 100 }));
        m.actions
            .push(TimedAction::immediate(MacroAction::Delay { ms: 200 }));
        m.actions
            .push(TimedAction::immediate(MacroAction::Delay { ms: 300 }));

        assert!(m.move_action(0, 2));
        if let MacroAction::Delay { ms } = &m.actions[0].action {
            assert_eq!(*ms, 200);
        }
    }

    #[test]
    fn test_macro_move_action_invalid() {
        let mut m = Macro::new(1, "Test", 0);
        m.actions
            .push(TimedAction::immediate(MacroAction::Delay { ms: 100 }));
        assert!(!m.move_action(0, 5));
    }

    #[test]
    fn test_macro_remove_action() {
        let mut m = Macro::new(1, "Test", 0);
        m.actions
            .push(TimedAction::immediate(MacroAction::Delay { ms: 100 }));
        m.actions
            .push(TimedAction::immediate(MacroAction::Delay { ms: 200 }));
        assert!(m.remove_action(0).is_some());
        assert_eq!(m.actions.len(), 1);
    }

    #[test]
    fn test_macro_remove_action_invalid() {
        let mut m = Macro::new(1, "Test", 0);
        assert!(m.remove_action(0).is_none());
    }

    #[test]
    fn test_macro_insert_action() {
        let mut m = Macro::new(1, "Test", 0);
        m.actions
            .push(TimedAction::immediate(MacroAction::Delay { ms: 100 }));
        m.actions
            .push(TimedAction::immediate(MacroAction::Delay { ms: 300 }));
        m.insert_action(1, TimedAction::immediate(MacroAction::Delay { ms: 200 }));
        assert_eq!(m.actions.len(), 3);
        if let MacroAction::Delay { ms } = &m.actions[1].action {
            assert_eq!(*ms, 200);
        }
    }

    // -----------------------------------------------------------------------
    // MacroLibrary tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_library_create_and_get() {
        let mut lib = MacroLibrary::new();
        let id = lib.create_macro("Test", 0);
        assert!(lib.get(id).is_some());
        assert_eq!(lib.get(id).unwrap().name, "Test");
    }

    #[test]
    fn test_library_remove() {
        let mut lib = MacroLibrary::new();
        let id = lib.create_macro("Test", 0);
        assert!(lib.remove(id));
        assert!(lib.get(id).is_none());
    }

    #[test]
    fn test_library_remove_nonexistent() {
        let mut lib = MacroLibrary::new();
        assert!(!lib.remove(999));
    }

    #[test]
    fn test_library_count() {
        let mut lib = MacroLibrary::new();
        assert_eq!(lib.count(), 0);
        lib.create_macro("A", 0);
        lib.create_macro("B", 0);
        assert_eq!(lib.count(), 2);
    }

    #[test]
    fn test_library_find_by_hotkey() {
        let mut lib = MacroLibrary::new();
        let id = lib.create_macro("Test", 0);
        let hk = Hotkey {
            ctrl: true,
            alt: false,
            shift: false,
            key_name: "F1".to_string(),
        };
        lib.get_mut(id).unwrap().trigger = Some(hk.clone());
        assert_eq!(lib.find_by_hotkey(&hk), Some(id));
    }

    #[test]
    fn test_library_find_by_hotkey_not_found() {
        let lib = MacroLibrary::new();
        let hk = Hotkey {
            ctrl: true,
            alt: false,
            shift: false,
            key_name: "F1".to_string(),
        };
        assert_eq!(lib.find_by_hotkey(&hk), None);
    }

    #[test]
    fn test_library_duplicate() {
        let mut lib = MacroLibrary::new();
        let id = lib.create_macro("Original", 0);
        lib.get_mut(id)
            .unwrap()
            .actions
            .push(TimedAction::immediate(MacroAction::Delay { ms: 100 }));
        let new_id = lib.duplicate(id, 1000).unwrap();
        assert_ne!(id, new_id);
        let dup = lib.get(new_id).unwrap();
        assert!(dup.name.contains("copy"));
        assert_eq!(dup.actions.len(), 1);
    }

    #[test]
    fn test_library_duplicate_no_trigger() {
        let mut lib = MacroLibrary::new();
        let id = lib.create_macro("Original", 0);
        lib.get_mut(id).unwrap().trigger = Some(Hotkey {
            ctrl: true,
            alt: false,
            shift: false,
            key_name: "F1".to_string(),
        });
        let new_id = lib.duplicate(id, 0).unwrap();
        assert!(lib.get(new_id).unwrap().trigger.is_none());
    }

    // -----------------------------------------------------------------------
    // Script parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_empty_script() {
        let result = parse_script("");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_comment_only() {
        let result = parse_script("# just a comment\n# another");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_key_command() {
        let result = parse_script("key Enter").unwrap();
        assert_eq!(result.len(), 2); // keydown + keyup
    }

    #[test]
    fn test_parse_keydown_keyup() {
        let result = parse_script("keydown A\nkeyup A").unwrap();
        assert_eq!(result.len(), 2);
        assert!(matches!(&result[0].action, MacroAction::KeyPress { key_name } if key_name == "A"));
        assert!(
            matches!(&result[1].action, MacroAction::KeyRelease { key_name } if key_name == "A")
        );
    }

    #[test]
    fn test_parse_click() {
        let result = parse_script("click 100 200").unwrap();
        assert_eq!(result.len(), 1);
        if let MacroAction::MouseClick { x, y, button } = &result[0].action {
            assert!((x - 100.0).abs() < f32::EPSILON);
            assert!((y - 200.0).abs() < f32::EPSILON);
            assert_eq!(*button, MacroMouseButton::Left);
        } else {
            panic!("Expected MouseClick");
        }
    }

    #[test]
    fn test_parse_click_with_button() {
        let result = parse_script("click 50 60 right").unwrap();
        if let MacroAction::MouseClick { button, .. } = &result[0].action {
            assert_eq!(*button, MacroMouseButton::Right);
        } else {
            panic!("Expected MouseClick");
        }
    }

    #[test]
    fn test_parse_dblclick() {
        let result = parse_script("dblclick 10 20 left").unwrap();
        assert!(matches!(
            &result[0].action,
            MacroAction::MouseDoubleClick { .. }
        ));
    }

    #[test]
    fn test_parse_move() {
        let result = parse_script("move 300 400").unwrap();
        if let MacroAction::MouseMove { x, y } = &result[0].action {
            assert!((x - 300.0).abs() < f32::EPSILON);
            assert!((y - 400.0).abs() < f32::EPSILON);
        } else {
            panic!("Expected MouseMove");
        }
    }

    #[test]
    fn test_parse_scroll() {
        let result = parse_script("scroll down 5").unwrap();
        if let MacroAction::Scroll { direction, amount } = &result[0].action {
            assert_eq!(*direction, ScrollDirection::Down);
            assert_eq!(*amount, 5);
        } else {
            panic!("Expected Scroll");
        }
    }

    #[test]
    fn test_parse_type() {
        let result = parse_script("type \"Hello World\"").unwrap();
        if let MacroAction::TypeText { text } = &result[0].action {
            assert_eq!(text, "Hello World");
        } else {
            panic!("Expected TypeText");
        }
    }

    #[test]
    fn test_parse_wait() {
        let result = parse_script("wait 500").unwrap();
        if let MacroAction::Delay { ms } = &result[0].action {
            assert_eq!(*ms, 500);
        } else {
            panic!("Expected Delay");
        }
    }

    #[test]
    fn test_parse_delay_alias() {
        let result = parse_script("delay 100").unwrap();
        assert!(matches!(&result[0].action, MacroAction::Delay { ms: 100 }));
    }

    #[test]
    fn test_parse_if_pixel() {
        let result = parse_script("if_pixel 100 200 255 0 0 10").unwrap();
        if let MacroAction::IfPixelColor {
            x,
            y,
            r,
            g,
            b,
            tolerance,
        } = &result[0].action
        {
            assert!((x - 100.0).abs() < f32::EPSILON);
            assert!((y - 200.0).abs() < f32::EPSILON);
            assert_eq!(*r, 255);
            assert_eq!(*g, 0);
            assert_eq!(*b, 0);
            assert_eq!(*tolerance, 10);
        } else {
            panic!("Expected IfPixelColor");
        }
    }

    #[test]
    fn test_parse_variables() {
        let script = "$x = 100\n$y = 200\nclick $x $y";
        let result = parse_script(script).unwrap();
        assert_eq!(result.len(), 1);
        if let MacroAction::MouseClick { x, y, .. } = &result[0].action {
            assert!((x - 100.0).abs() < f32::EPSILON);
            assert!((y - 200.0).abs() < f32::EPSILON);
        } else {
            panic!("Expected MouseClick");
        }
    }

    #[test]
    fn test_parse_unknown_command() {
        let result = parse_script("foobar 1 2 3");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Unknown command"));
    }

    #[test]
    fn test_parse_missing_arg() {
        let result = parse_script("click 100");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_number() {
        let result = parse_script("click abc 200");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_labels_ignored() {
        let result = parse_script(":start\nwait 100\n:end").unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_parse_empty_variable_name() {
        let result = parse_script("$ = value");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_variable_missing_equals() {
        let result = parse_script("$foo value");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Serialize/deserialize round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn test_serialize_script() {
        let actions = vec![
            TimedAction::new(
                MacroAction::MouseClick {
                    x: 100.0,
                    y: 200.0,
                    button: MacroMouseButton::Left,
                },
                50,
            ),
            TimedAction::immediate(MacroAction::TypeText {
                text: "hello".to_string(),
            }),
        ];
        let text = serialize_script(&actions);
        assert!(text.contains("click"));
        assert!(text.contains("type"));
        assert!(text.contains("wait 50"));
    }

    // -----------------------------------------------------------------------
    // AutomatorApp tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_app_new_macro() {
        let mut app = AutomatorApp::new();
        let id = app.new_macro("Test");
        assert!(app.library.get(id).is_some());
        assert_eq!(app.selected_macro_id, Some(id));
    }

    #[test]
    fn test_app_delete_selected_macro() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        assert!(app.delete_selected_macro());
        assert!(app.selected_macro_id.is_none());
    }

    #[test]
    fn test_app_delete_no_selection() {
        let mut app = AutomatorApp::new();
        assert!(!app.delete_selected_macro());
    }

    #[test]
    fn test_app_duplicate() {
        let mut app = AutomatorApp::new();
        let id = app.new_macro("Orig");
        app.add_action(MacroAction::Delay { ms: 50 }, 0);
        let new_id = app.duplicate_selected_macro().unwrap();
        assert_ne!(id, new_id);
        assert_eq!(app.selected_macro_id, Some(new_id));
    }

    #[test]
    fn test_app_recording_lifecycle() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.start_recording();
        assert_eq!(app.recording_state, RecordingState::Recording);

        app.record_action(MacroAction::KeyPress {
            key_name: "A".to_string(),
        });
        app.tick(100);
        app.record_action(MacroAction::KeyRelease {
            key_name: "A".to_string(),
        });

        app.stop_recording();
        assert_eq!(app.recording_state, RecordingState::Idle);

        let mac = app.library.get(app.selected_macro_id.unwrap()).unwrap();
        assert_eq!(mac.actions.len(), 2);
    }

    #[test]
    fn test_app_recording_pause_resume() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.start_recording();
        app.pause_recording();
        assert_eq!(app.recording_state, RecordingState::Paused);
        app.resume_recording();
        assert_eq!(app.recording_state, RecordingState::Recording);
    }

    #[test]
    fn test_app_record_while_idle() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        // Recording not started -- should be a no-op.
        app.record_action(MacroAction::Delay { ms: 100 });
        let mac = app.library.get(app.selected_macro_id.unwrap()).unwrap();
        assert!(mac.actions.is_empty());
    }

    #[test]
    fn test_app_playback_lifecycle() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.add_action(MacroAction::Delay { ms: 100 }, 0);
        app.add_action(MacroAction::Delay { ms: 200 }, 50);

        app.start_playback();
        assert!(app.playback_state.is_playing());

        // Tick past first action (delay 0).
        let result = app.tick_playback(10);
        assert!(result.is_some());

        // Tick through second action (delay 50).
        let result2 = app.tick_playback(60);
        assert!(result2.is_some());
    }

    #[test]
    fn test_app_playback_repeat_forever() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.add_action(MacroAction::Delay { ms: 100 }, 0);
        app.set_repeat_mode(RepeatMode::Forever);
        app.start_playback();

        // First execution.
        app.tick_playback(1);
        // Should still be playing (looped).
        assert!(app.playback_state.is_playing());
    }

    #[test]
    fn test_app_playback_no_actions() {
        let mut app = AutomatorApp::new();
        app.new_macro("Empty");
        app.start_playback();
        // Should not start playing if no actions.
        assert!(!app.playback_state.is_playing());
    }

    #[test]
    fn test_app_move_actions() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.add_action(MacroAction::Delay { ms: 100 }, 0);
        app.add_action(MacroAction::Delay { ms: 200 }, 0);
        app.add_action(MacroAction::Delay { ms: 300 }, 0);
        app.select_action(0);

        assert!(app.move_action_down());
        assert_eq!(app.selected_action_idx, Some(1));
    }

    #[test]
    fn test_app_delete_action() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.add_action(MacroAction::Delay { ms: 100 }, 0);
        app.add_action(MacroAction::Delay { ms: 200 }, 0);
        app.select_action(0);

        assert!(app.delete_selected_action());
        let mac = app.library.get(app.selected_macro_id.unwrap()).unwrap();
        assert_eq!(mac.actions.len(), 1);
    }

    #[test]
    fn test_app_apply_script() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.set_script_text("click 100 200\nwait 500");
        assert!(app.apply_script());

        let mac = app.library.get(app.selected_macro_id.unwrap()).unwrap();
        assert_eq!(mac.actions.len(), 2);
    }

    #[test]
    fn test_app_apply_script_error() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.set_script_text("badcommand xyz");
        assert!(!app.apply_script());
        assert!(app.script_error.is_some());
    }

    #[test]
    fn test_app_apply_script_no_macro() {
        let mut app = AutomatorApp::new();
        app.set_script_text("wait 100");
        assert!(!app.apply_script());
    }

    #[test]
    fn test_app_set_trigger() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        let hk = Hotkey {
            ctrl: true,
            alt: false,
            shift: false,
            key_name: "F5".to_string(),
        };
        app.set_trigger(Some(hk));
        let mac = app.library.get(app.selected_macro_id.unwrap()).unwrap();
        assert!(mac.trigger.is_some());
    }

    #[test]
    fn test_app_cycle_speed() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.cycle_speed();
        let mac = app.library.get(app.selected_macro_id.unwrap()).unwrap();
        assert_eq!(mac.speed, PlaybackSpeed::Double);
    }

    #[test]
    fn test_app_render_empty() {
        let app = AutomatorApp::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_with_macros() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test Macro");
        app.add_action(MacroAction::Delay { ms: 100 }, 0);
        app.select_action(0);
        let cmds = app.render(1000.0, 700.0);
        assert!(!cmds.is_empty());
    }

    // -----------------------------------------------------------------------
    // format_duration_ms tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_duration_ms_millis() {
        assert_eq!(format_duration_ms(500), "500ms");
    }

    #[test]
    fn test_format_duration_ms_seconds() {
        assert_eq!(format_duration_ms(3000), "3s");
    }

    #[test]
    fn test_format_duration_ms_seconds_with_millis() {
        assert_eq!(format_duration_ms(3500), "3.500s");
    }

    #[test]
    fn test_format_duration_ms_minutes() {
        assert_eq!(format_duration_ms(125000), "2m 5s");
    }

    // -----------------------------------------------------------------------
    // substitute_vars tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_substitute_vars_basic() {
        let mut vars = BTreeMap::new();
        vars.insert("x".to_string(), "100".to_string());
        assert_eq!(substitute_vars("click $x 200", &vars), "click 100 200");
    }

    #[test]
    fn test_substitute_vars_multiple() {
        let mut vars = BTreeMap::new();
        vars.insert("a".to_string(), "10".to_string());
        vars.insert("b".to_string(), "20".to_string());
        assert_eq!(substitute_vars("move $a $b", &vars), "move 10 20");
    }

    #[test]
    fn test_substitute_vars_unresolved() {
        let vars = BTreeMap::new();
        assert_eq!(substitute_vars("click $x 200", &vars), "click $x 200");
    }

    #[test]
    fn test_substitute_vars_no_vars() {
        let vars = BTreeMap::new();
        assert_eq!(substitute_vars("click 100 200", &vars), "click 100 200");
    }

    #[test]
    fn test_script_error_display() {
        let e = ScriptError {
            line: 5,
            message: "bad input".to_string(),
        };
        assert_eq!(format!("{e}"), "Line 5: bad input");
    }

    #[test]
    fn test_recording_state_label() {
        assert_eq!(RecordingState::Idle.label(), "Idle");
        assert_eq!(RecordingState::Recording.label(), "Recording");
        assert_eq!(RecordingState::Paused.label(), "Paused");
    }

    #[test]
    fn test_playback_state_label() {
        assert_eq!(PlaybackState::Stopped.label(), "Stopped");
    }

    #[test]
    fn test_active_tab_label() {
        assert_eq!(ActiveTab::Editor.label(), "Editor");
        assert_eq!(ActiveTab::Script.label(), "Script");
    }

    #[test]
    fn test_action_badge_colors_unique() {
        let actions: Vec<MacroAction> = vec![
            MacroAction::KeyPress {
                key_name: "A".to_string(),
            },
            MacroAction::MouseClick {
                x: 0.0,
                y: 0.0,
                button: MacroMouseButton::Left,
            },
            MacroAction::MouseMove { x: 0.0, y: 0.0 },
            MacroAction::Scroll {
                direction: ScrollDirection::Up,
                amount: 1,
            },
            MacroAction::TypeText {
                text: String::new(),
            },
            MacroAction::Delay { ms: 0 },
            MacroAction::IfPixelColor {
                x: 0.0,
                y: 0.0,
                r: 0,
                g: 0,
                b: 0,
                tolerance: 0,
            },
        ];
        // Just verify we get a color for each without panicking.
        for a in &actions {
            let _ = a.badge_color();
        }
    }

    #[test]
    fn test_macro_library_default() {
        let lib = MacroLibrary::default();
        assert_eq!(lib.count(), 0);
    }

    #[test]
    fn test_app_default() {
        let app = AutomatorApp::default();
        assert!(app.selected_macro_id.is_none());
    }

    #[test]
    fn test_app_select_macro_by_index() {
        let mut app = AutomatorApp::new();
        app.new_macro("First");
        app.new_macro("Second");
        app.select_macro_by_index(0);
        let mac = app.library.get(app.selected_macro_id.unwrap()).unwrap();
        assert_eq!(mac.name, "First");
    }

    #[test]
    fn test_app_playback_pause_resume() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.add_action(MacroAction::Delay { ms: 100 }, 0);
        app.add_action(MacroAction::Delay { ms: 200 }, 500);
        app.start_playback();
        app.tick_playback(1); // Fire first action.
        app.pause_playback();
        assert!(matches!(
            app.playback_state,
            PlaybackState::PausedPlayback { .. }
        ));
        app.resume_playback();
        assert!(app.playback_state.is_playing());
    }

    #[test]
    fn test_scroll_default_amount() {
        let result = parse_script("scroll up").unwrap();
        if let MacroAction::Scroll { amount, .. } = &result[0].action {
            assert_eq!(*amount, 3);
        } else {
            panic!("Expected Scroll");
        }
    }

    #[test]
    fn test_if_pixel_default_tolerance() {
        let result = parse_script("if_pixel 10 20 255 0 0").unwrap();
        if let MacroAction::IfPixelColor { tolerance, .. } = &result[0].action {
            assert_eq!(*tolerance, 10);
        } else {
            panic!("Expected IfPixelColor");
        }
    }

    #[test]
    fn test_serialize_empty() {
        let text = serialize_script(&[]);
        assert!(text.contains("Slate OS Automator"));
    }

    #[test]
    fn test_app_start_recording_creates_macro() {
        let mut app = AutomatorApp::new();
        // No macro selected, so start_recording should create one.
        app.start_recording();
        assert!(app.selected_macro_id.is_some());
        assert_eq!(app.recording_state, RecordingState::Recording);
    }

    #[test]
    fn test_app_move_action_up() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.add_action(MacroAction::Delay { ms: 100 }, 0);
        app.add_action(MacroAction::Delay { ms: 200 }, 0);
        app.select_action(1);
        assert!(app.move_action_up());
        assert_eq!(app.selected_action_idx, Some(0));
    }

    #[test]
    fn test_app_move_action_up_at_top() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.add_action(MacroAction::Delay { ms: 100 }, 0);
        app.select_action(0);
        assert!(!app.move_action_up());
    }

    #[test]
    fn test_playback_repeat_times() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.add_action(MacroAction::Delay { ms: 100 }, 0);
        app.set_repeat_mode(RepeatMode::Times(2));
        app.start_playback();

        // First pass.
        app.tick_playback(1);
        // Should still be playing (one more repeat left).
        assert!(app.playback_state.is_playing());

        // Second pass.
        app.tick_playback(1);
        // Should be stopped now.
        assert!(!app.playback_state.is_playing());
    }

    #[test]
    fn test_app_render_recording_indicator() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.start_recording();
        let cmds = app.render(800.0, 600.0);
        // Should have REC text somewhere.
        let has_rec = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "REC"));
        assert!(has_rec);
    }

    #[test]
    fn test_app_render_script_tab() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test");
        app.active_tab = ActiveTab::Script;
        app.set_script_text("wait 100\nclick 50 60");
        let cmds = app.render(1000.0, 700.0);
        assert!(!cmds.is_empty());
    }
}
