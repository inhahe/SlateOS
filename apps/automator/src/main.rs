//! Slate OS Automator -- Keyboard/Mouse Automation & Macro Recorder
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
//!
//! The window is real: a `Layout` is solved from the live size every frame, the
//! drawing pass records the hit box of everything it paints, and a click is
//! answered by the thing that was actually drawn under it. See the roadmap
//! entry for the faults wiring it exposed -- chief among them that not one of
//! the twenty-odd controls in the picture had ever been clickable, and that
//! playback had no clock to advance it.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
//
// There used to be thirteen crate-level `#![allow]`s here -- `too_many_lines`,
// four `cast_*`, `similar_names`, `must_use_candidate`, `missing_panics_doc`
// and the rest. A blanket allow is not a decision about a line of code; it is a
// decision not to look at any of them, and this file has enough arithmetic in
// it that the four `cast_*` allows alone were covering every f32/usize
// conversion in the layout. They are all gone.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

use std::collections::BTreeMap;
use std::process::ExitCode;

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
// Window and clock
// ============================================================================

/// The size the window opens at. Nothing else in the program may assume it.
///
/// It used to be the size the *whole picture* was drawn at: `render` measured
/// nothing vertical, so `HEADER_HEIGHT = 44`, `TOOLBAR_HEIGHT = 38`,
/// `SIDEBAR_WIDTH = 240` and `PROPERTIES_WIDTH = 260` were the layout in every
/// window there has ever been, and 240 + 260 of a 400-wide one is more than
/// there is.
const WINDOW_WIDTH: f32 = 1000.0;
const WINDOW_HEIGHT: f32 = 700.0;

const TITLE: &str = "Automator";

/// How often the window asks for a frame.
///
/// Two things need it and neither had it: the recording indicator pulses on
/// `elapsed_ms`, and playback advances on `tick_playback` -- which, in the
/// program as shipped, had no caller outside the tests. A macro that had been
/// started (had there been any way to start one) would have sat on its first
/// action for ever.
const TICK_MS: u64 = 16;

const RECORDING_PULSE_PERIOD_MS: u64 = 1000;

const CORNER_RADIUS: f32 = 4.0;

/// The narrowest a side panel may be before it is left out rather than drawn.
///
/// A panel squeezed to forty pixels shows nobody anything; it just takes forty
/// pixels off the list, which is the part of the window that is actually doing
/// work. The old layout had no such rule -- both panels were drawn at their
/// full fixed widths whatever was left over, so a 400-wide window got a centre
/// panel a hundred pixels *wide in the negative*.
const MIN_PANEL_W: f32 = 132.0;

/// The narrowest the action list may be squeezed to by the two side panels.
///
/// The list is the part of the window doing the work; a side panel that would
/// leave less than this is left out instead.
const MIN_LIST_W: f32 = 120.0;

// ============================================================================
// What a click can land on
// ============================================================================

/// Everything in the picture a click can reach.
///
/// The program used to have no answer to that question at all. There was no
/// `handle_event`, no mouse code and no key code: two tabs, eight transport
/// buttons, every macro row, every action row, New, Delete, Up, Down, the
/// action Delete, Apply Script, five speed buttons and three repeat buttons
/// were all painted, and not one of them could be pressed. Hit boxes are
/// recorded by the pass that paints them now, so a control is clickable exactly
/// where its ink is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The `i`th macro of the library, as the sidebar currently lists it.
    Macro(usize),
    /// The `i`th action of the selected macro, as the list currently shows it.
    Action(usize),
    Tab(ActiveTab),
    Button(Button),
    Speed(PlaybackSpeed),
    Repeat(RepeatMode),
    /// The help card itself, which swallows the click that dismisses it.
    Help,
}

/// A control that has both a button in the picture and a key on the keyboard.
///
/// The pairing is the point: `press` below is the single implementation, so a
/// button and its key cannot drift apart, and the button's own label is what
/// tells the user which key it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Record,
    StopRecording,
    Play,
    PausePlayback,
    StopPlayback,
    CycleSpeed,
    CycleRepeat,
    NewMacro,
    DeleteMacro,
    MoveActionUp,
    MoveActionDown,
    DeleteAction,
    ApplyScript,
    Help,
}

impl Button {
    /// The key that does the same thing, spelled the way the label spells it.
    fn key(self) -> Key {
        match self {
            Self::Record => Key::R,
            Self::StopRecording => Key::T,
            Self::Play => Key::P,
            Self::PausePlayback => Key::Space,
            Self::StopPlayback => Key::S,
            Self::CycleSpeed => Key::X,
            Self::CycleRepeat => Key::E,
            Self::NewMacro => Key::N,
            Self::DeleteMacro => Key::Delete,
            Self::MoveActionUp => Key::LeftBracket,
            Self::MoveActionDown => Key::RightBracket,
            Self::DeleteAction => Key::Backspace,
            Self::ApplyScript => Key::A,
            Self::Help => Key::F1,
        }
    }

    /// How the key reads on a button face.
    fn key_label(self) -> &'static str {
        match self {
            Self::Record => "R",
            Self::StopRecording => "T",
            Self::Play => "P",
            Self::PausePlayback => "Space",
            Self::StopPlayback => "S",
            Self::CycleSpeed => "X",
            Self::CycleRepeat => "E",
            Self::NewMacro => "N",
            Self::DeleteMacro => "Del",
            Self::MoveActionUp => "[",
            Self::MoveActionDown => "]",
            Self::DeleteAction => "Bksp",
            Self::ApplyScript => "A",
            Self::Help => "F1",
        }
    }

    /// What the button says it does.
    fn action_label(self) -> &'static str {
        match self {
            Self::Record => "Record",
            Self::StopRecording => "Stop Rec",
            Self::Play => "Play",
            Self::PausePlayback => "Pause",
            Self::StopPlayback => "Stop",
            Self::CycleSpeed => "Speed",
            Self::CycleRepeat => "Repeat",
            Self::NewMacro => "New",
            Self::DeleteMacro => "Delete macro",
            Self::MoveActionUp => "Move up",
            Self::MoveActionDown => "Move down",
            Self::DeleteAction => "Delete action",
            Self::ApplyScript => "Apply Script",
            Self::Help => "Help",
        }
    }

    /// Every button, in the order the help card lists them.
    fn all() -> &'static [Self] {
        &[
            Self::Record,
            Self::StopRecording,
            Self::Play,
            Self::PausePlayback,
            Self::StopPlayback,
            Self::CycleSpeed,
            Self::CycleRepeat,
            Self::NewMacro,
            Self::DeleteMacro,
            Self::MoveActionUp,
            Self::MoveActionDown,
            Self::DeleteAction,
            Self::ApplyScript,
            Self::Help,
        ]
    }
}

// ============================================================================
// Layout
// ============================================================================

/// Every rectangle in the picture, solved from the live window size.
///
/// What this replaced was eight compile-time sizes -- `HEADER_HEIGHT = 44`,
/// `SIDEBAR_WIDTH = 240`, `PROPERTIES_WIDTH = 260` and the rest -- and a
/// `render` that took a width and a height and then measured nothing vertical
/// with either. The centre panel was literally `width - 240.0 - 260.0`, which
/// is a *negative* width in any window narrower than five hundred pixels, and
/// the properties panel began at `width - 260.0`, which is off the left edge of
/// anything narrower than that.
#[derive(Clone, Copy, Debug)]
struct Layout {
    window: Rect,
    /// The title bar: name, recording and playback indicators, the tab pair.
    header: Rect,
    /// The transport strip: record, play, speed, repeat.
    toolbar: Rect,
    /// The macro library. Empty when the window cannot pay for one.
    sidebar: Rect,
    /// The action list or the script, whichever tab is up. Never empty.
    list: Rect,
    /// The read-out and the speed/repeat pads. Empty when there is no room.
    props: Rect,
    /// The one line of prose along the bottom.
    status: Rect,
    /// The height of one heading strip, one list row, one property row.
    row: f32,
    /// The height of a button, in the toolbar and in every panel footer.
    button: f32,
    pad: f32,
    heading: f32,
    font: f32,
    small: f32,
}

impl Layout {
    fn solve(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        let window = Rect::new(0.0, 0.0, w, h);

        let pad = (w.min(h) * 0.02).clamp(3.0, 12.0);
        let font = (h / 52.0).clamp(9.0, 15.0);
        let small = (font - 2.0).max(8.0);
        let heading = (font * 1.25).clamp(11.0, 20.0);
        let row = (font * 2.2).max(14.0);
        let button = (small * 2.4).max(12.0);

        let header = Rect::new(0.0, 0.0, w, (heading + pad * 2.2).min(h));
        let toolbar_h = (button + pad * 1.4).min((h - header.h).max(0.0));
        let toolbar = Rect::new(0.0, header.bottom(), w, toolbar_h);
        let status_h = (small + pad * 1.2).min((h - header.h - toolbar_h).max(0.0));
        let status = Rect::new(0.0, h - status_h, w, status_h);

        let content_y = toolbar.bottom();
        let content_h = (status.y - content_y).max(0.0);

        // Two panels flank a list, and all three want the same pixels. The
        // list is what is doing the work, so it is what is protected: a side
        // panel is taken only if it is wide enough to read *and* leaves the
        // list wide enough to read. Properties goes first when only one can
        // stay, because the sidebar is how a macro is chosen and the
        // properties panel only reports on the one already chosen.
        let mut sidebar_w = (w * 0.24).clamp(0.0, 240.0);
        if sidebar_w < MIN_PANEL_W || w - sidebar_w < MIN_LIST_W {
            sidebar_w = 0.0;
        }
        let mut props_w = (w * 0.26).clamp(0.0, 280.0);
        if props_w < MIN_PANEL_W || w - sidebar_w - props_w < MIN_LIST_W {
            props_w = 0.0;
        }

        let sidebar = if sidebar_w > 0.0 && content_h > 0.0 {
            Rect::new(0.0, content_y, sidebar_w, content_h)
        } else {
            Rect::EMPTY
        };
        let props = if props_w > 0.0 && content_h > 0.0 {
            Rect::new(w - props_w, content_y, props_w, content_h)
        } else {
            Rect::EMPTY
        };
        // The list takes what the panels *actually* took, not what they asked
        // for. A panel dropped for want of height still has a width, and
        // subtracting that width would leave a strip of window belonging to
        // nothing -- which is how a pane goes missing without any pane being
        // told to go missing.
        let list = Rect::new(
            sidebar.w,
            content_y,
            (w - sidebar.w - props.w).max(0.0),
            content_h,
        );

        Self {
            window,
            header,
            toolbar,
            sidebar,
            list,
            props,
            status,
            row,
            button,
            pad,
            heading,
            font,
            small,
        }
    }

    /// Split a panel into a heading strip, a footer strip and the body between.
    ///
    /// The footer is taken only if the panel can pay for it *and* still leave a
    /// body. The old sidebar drew its New/Delete bar at
    /// `content_y + content_h - 36.0` unconditionally and the action list drew
    /// its Up/Down/Delete bar the same way, so in a short window the bar was
    /// not under the list, it was on top of it -- and the rows it covered were
    /// drawn all the same, under a bar that hid them.
    fn split(panel: Rect, head_h: f32, foot_h: f32) -> (Rect, Rect, Rect) {
        if panel.is_empty() {
            return (Rect::EMPTY, Rect::EMPTY, Rect::EMPTY);
        }
        let head_h = head_h.min(panel.h);
        let rest = panel.h - head_h;
        // Half the remainder is the most a footer may take: a footer that eats
        // the body is a footer with nothing to be the foot of.
        let foot_h = foot_h.min(rest * 0.5).max(0.0);
        let head = Rect::new(panel.x, panel.y, panel.w, head_h);
        let body = Rect::new(panel.x, panel.y + head_h, panel.w, rest - foot_h);
        let foot = Rect::new(panel.x, panel.bottom() - foot_h, panel.w, foot_h);
        (head, body, foot)
    }
}

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
                let var_name: String = chars
                    .get(start..end)
                    .map(|s| s.iter().collect())
                    .unwrap_or_default();
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
pub enum ActiveTab {
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
    /// The first macro row the sidebar shows, as an index into the library.
    ///
    /// This and `action_scroll` used to be `f32` pixel offsets that nothing
    /// ever wrote: initialised to zero in `new`, subtracted by the drawing
    /// pass, and assigned nowhere else in the program. A row index cannot be
    /// half a row, and it can be clamped against the list it indexes.
    sidebar_scroll: usize,
    /// The first action row the list shows.
    action_scroll: usize,
    /// Whether the key card is up.
    show_help: bool,
    /// The size the last frame was drawn at.
    ///
    /// A click arrives in window coordinates and has to be tested against the
    /// picture the user was looking at, which is the one drawn at this size.
    size: (f32, f32),
    /// Wheel notches earned but not yet spent as whole rows.
    wheel: guitk::wheel::Accumulator,
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
            sidebar_scroll: 0,
            action_scroll: 0,
            show_help: false,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            wheel: guitk::wheel::Accumulator::default(),
            elapsed_ms: 0,
            status_message: "Ready".to_string(),
        }
    }

    /// A library with two macros in it, so the window opens with something to
    /// look at rather than an empty list and no clue what a macro looks like.
    ///
    /// This is what `main` used to be: eighteen `add_action` calls in the
    /// program's entry point, feeding a picture that was rendered once into a
    /// `Vec` and dropped on the next line. Naming it makes it a fixture a test
    /// can build.
    pub fn with_demo_library() -> Self {
        let mut app = Self::new();
        app.new_macro("Login Sequence");
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
        app.set_trigger(Hotkey::from_str("Ctrl+Alt+L"));

        app.new_macro("Screenshot Workflow");
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

        app.select_macro_by_index(0);
        app.select_action(2);
        app.status_message = "Ready".to_string();
        app
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
            self.status_message = format!("Repeat: {}", mac.repeat_mode.label());
        }
    }

    /// Step the repeat mode round its three settings.
    pub fn cycle_repeat_mode(&mut self) {
        if let Some(id) = self.selected_macro_id
            && let Some(mac) = self.library.get_mut(id)
        {
            mac.repeat_mode = match mac.repeat_mode {
                RepeatMode::Once => RepeatMode::Times(5),
                RepeatMode::Times(_) => RepeatMode::Forever,
                RepeatMode::Forever => RepeatMode::Once,
            };
            self.status_message = format!("Repeat: {}", mac.repeat_mode.label());
        }
    }

    /// Advance the clock, and with it whatever is running.
    ///
    /// Returns whether anything moved, so a window that has nothing to show can
    /// stay idle rather than repaint sixty times a second for ever.
    ///
    /// `tick_playback` used to have no caller anywhere but the tests. `tick`
    /// advanced `elapsed_ms` and stopped there, so playback -- had there been
    /// any way to start it, which there was not -- would have sat on its first
    /// action until the program was closed.
    pub fn tick(&mut self, delta_ms: u64) -> bool {
        self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);
        let was_playing = self.playback_state.is_playing();
        let fired = self.tick_playback(delta_ms).is_some();
        // The recording indicator pulses, so a recording window is always
        // asking for the next frame; nothing else is.
        fired || was_playing || self.recording_state.is_recording()
    }

    // -----------------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------------

    /// Draw the whole window, recording the hit box of everything drawn.
    ///
    /// One pass paints and hit-boxes together, so a control is clickable
    /// exactly where its ink is. The program this replaced had no hit boxes at
    /// all -- and no mouse handler to consult them.
    fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let l = Layout::solve(width, height);
        let mut f = Frame::new(width, height);
        fill(&mut f, l.window, BASE, CornerRadii::ZERO);
        self.draw_header(&mut f, &l);
        self.draw_toolbar(&mut f, &l);
        self.draw_sidebar(&mut f, &l);
        self.draw_list(&mut f, &l);
        self.draw_props(&mut f, &l);
        self.draw_status(&mut f, &l);
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    /// The title bar: the name, the two live indicators, and the tab pair.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        let head = l.header;
        if head.is_empty() {
            return;
        }
        fill(f, head, CRUST, CornerRadii::ZERO);

        // The tabs are laid out from the right edge inwards, and the title and
        // the indicators get what is left. The old header did the reverse and
        // did it with literals: the title at x = 10, the recording dot at
        // x = 110, "REC" at x = 128 and "PLAYING (...)" at either 110 or 175
        // depending on whether the dot was showing -- 175 being a guess at how
        // wide "REC" is. Nothing measured anything, so a longer indicator ran
        // straight through the tabs.
        let tabs = [ActiveTab::Editor, ActiveTab::Script];
        let tab_w = tabs
            .iter()
            .map(|t| text::padded_width(t.label(), l.pad * 1.6, l.font, FontWeightHint::Bold))
            .fold(0.0_f32, f32::max);
        let tab_h = (head.h - l.pad).max(0.0);
        let tab_y = head.y + (head.h - tab_h) / 2.0;
        let tabs_w = tab_w * 2.0 + l.pad;
        // Tabs are drawn only if they fit beside a readable title. A tab pad
        // squeezed over the title is two controls in one place.
        let mut tabs_x = head.right();
        if tabs_w + l.pad * 2.0 <= head.w * 0.55 {
            tabs_x = head.right() - l.pad - tabs_w;
            for (i, tab) in tabs.iter().enumerate() {
                let rect = Rect::new(
                    (tab_w + l.pad).mul_add(usize_f32(i), tabs_x),
                    tab_y,
                    tab_w,
                    tab_h,
                );
                let selected = *tab == self.active_tab;
                fill(
                    f,
                    rect,
                    if selected { SURFACE0 } else { CRUST },
                    CornerRadii::all(CORNER_RADIUS),
                );
                centred(
                    f,
                    rect.x,
                    rect.w,
                    rect.y + (rect.h - l.font) / 2.0,
                    tab.label(),
                    if selected { BLUE } else { SUBTEXT0 },
                    l.font,
                    if selected {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                );
                f.hit(Target::Tab(*tab), rect);
            }
        }

        // Everything on the left marches from a measurement, not a literal.
        let room_end = tabs_x - l.pad;
        let mut cx = head.x + l.pad;
        let ty = head.y + (head.h - l.heading) / 2.0;
        bounded(
            f,
            (cx, ty),
            room_end - cx,
            TITLE,
            TEXT,
            l.heading,
            FontWeightHint::Bold,
        );
        cx += text::measure(TITLE, l.heading, FontWeightHint::Bold) + l.pad * 1.5;

        let sy = head.y + (head.h - l.font) / 2.0;
        if self.recording_state.is_recording() {
            let dot = l.small * 0.9;
            let pulse = u64_f32(self.elapsed_ms % RECORDING_PULSE_PERIOD_MS)
                / u64_f32(RECORDING_PULSE_PERIOD_MS);
            // A triangle wave: bright at the half-period, dark at both ends.
            let level = 1.0 - (pulse * 2.0 - 1.0).abs();
            let alpha = f32_u8(level * 255.0);
            if cx + dot <= room_end {
                fill(
                    f,
                    Rect::new(cx, head.y + (head.h - dot) / 2.0, dot, dot),
                    Color::rgba(RED.r, RED.g, RED.b, alpha),
                    CornerRadii::all(dot / 2.0),
                );
                cx += dot + l.pad * 0.5;
            }
            bounded(
                f,
                (cx, sy),
                room_end - cx,
                "REC",
                RED,
                l.font,
                FontWeightHint::Bold,
            );
            cx += text::measure("REC", l.font, FontWeightHint::Bold) + l.pad;
        }
        if self.playback_state.is_playing() {
            let label = format!("PLAYING ({})", self.playback_state.label());
            bounded(
                f,
                (cx, sy),
                room_end - cx,
                &label,
                GREEN,
                l.font,
                FontWeightHint::Bold,
            );
        }
    }

    /// The transport strip.
    fn draw_toolbar(&self, f: &mut Frame<Target>, l: &Layout) {
        let bar = l.toolbar;
        if bar.is_empty() {
            return;
        }
        fill(f, bar, MANTLE, CornerRadii::ZERO);
        hline(f, bar.x, bar.right(), bar.bottom() - 1.0, SURFACE0);

        let btn_h = l.button.min(bar.h);
        let btn_y = bar.y + (bar.h - btn_h) / 2.0;
        let mut bx = bar.x + l.pad;
        for (button, label, bg, fg) in self.toolbar_buttons() {
            let w = text::padded_width(&label, l.pad * 1.4, l.small, FontWeightHint::Regular);
            // A button that does not fit is left out, and so is every button
            // after it. The old toolbar marched `bx` from `PADDING` through
            // eight buttons with no bound of any kind, so in a window narrower
            // than about five hundred and seventy pixels the last controls were
            // simply painted past the right-hand edge -- present in the command
            // list, absent from the screen.
            if bx + w > bar.right() - l.pad {
                break;
            }
            let rect = Rect::new(bx, btn_y, w, btn_h);
            fill(f, rect, bg, CornerRadii::all(CORNER_RADIUS));
            centred(
                f,
                rect.x,
                rect.w,
                rect.y + (rect.h - l.small) / 2.0,
                &label,
                fg,
                l.small,
                FontWeightHint::Regular,
            );
            f.hit(Target::Button(button), rect);
            bx += w + l.pad * 0.6;
        }
    }

    /// The toolbar's buttons, with the labels that name their keys.
    fn toolbar_buttons(&self) -> Vec<(Button, String, Color, Color)> {
        let selected = self.selected_macro_id.and_then(|id| self.library.get(id));
        let speed = selected.map_or(PlaybackSpeed::Normal, |m| m.speed);
        let repeat = selected.map_or(RepeatMode::Once, |m| m.repeat_mode);
        let rec_bg = if self.recording_state.is_recording() {
            RED
        } else {
            SURFACE1
        };
        let play_bg = if self.playback_state.is_playing() {
            GREEN
        } else {
            SURFACE1
        };
        vec![
            (Button::Record, faced(Button::Record), rec_bg, TEXT),
            (
                Button::StopRecording,
                faced(Button::StopRecording),
                SURFACE1,
                TEXT,
            ),
            (Button::Play, faced(Button::Play), play_bg, TEXT),
            (
                Button::PausePlayback,
                faced(Button::PausePlayback),
                SURFACE1,
                TEXT,
            ),
            (
                Button::StopPlayback,
                faced(Button::StopPlayback),
                SURFACE1,
                TEXT,
            ),
            (
                Button::CycleSpeed,
                format!(
                    "Speed: {} ({})",
                    speed.label(),
                    Button::CycleSpeed.key_label()
                ),
                SURFACE1,
                PEACH,
            ),
            (
                Button::CycleRepeat,
                format!(
                    "Repeat: {} ({})",
                    repeat.label(),
                    Button::CycleRepeat.key_label()
                ),
                SURFACE1,
                LAVENDER,
            ),
            (Button::Help, faced(Button::Help), SURFACE1, TEAL),
        ]
    }

    /// The macro library down the left-hand side.
    fn draw_sidebar(&self, f: &mut Frame<Target>, l: &Layout) {
        let panel = l.sidebar;
        if panel.is_empty() {
            return;
        }
        fill(f, panel, MANTLE, CornerRadii::ZERO);
        let (head, body, foot) = Layout::split(panel, l.row, l.button + l.pad);

        fill(f, head, CRUST, CornerRadii::ZERO);
        bounded(
            f,
            (head.x + l.pad, head.y + (head.h - l.font) / 2.0),
            head.w - l.pad * 2.0,
            &format!("Macros ({})", self.library.count()),
            TEXT,
            l.font,
            FontWeightHint::Bold,
        );

        let macros = self.library.list();
        let first = self.sidebar_scroll.min(macros.len());
        for (slot, (i, mac)) in macros.iter().enumerate().skip(first).enumerate() {
            let row_y = l.row.mul_add(usize_f32(slot), body.y);
            // The row is drawn only if the whole of it is inside the body. A
            // row half under the footer is a row whose hit box is half under
            // the footer, which is two controls claiming the same pixels.
            if row_y + l.row > body.bottom() + 0.01 {
                break;
            }
            let rect = Rect::new(body.x + 2.0, row_y, (body.w - 4.0).max(0.0), l.row - 2.0);
            let selected = self.selected_macro_id == Some(mac.id);
            fill(
                f,
                rect,
                if selected { SURFACE0 } else { MANTLE },
                CornerRadii::all(CORNER_RADIUS),
            );

            // The count sits at the right-hand end, measured; the trigger dot
            // beside it; the name gets what is left. The old row put the count
            // at `SIDEBAR_WIDTH - 40.0` and the dot at `SIDEBAR_WIDTH - 56.0`,
            // both literal offsets from a width that no longer exists.
            let count = format!("{}", mac.action_count());
            let count_w = text::measure(&count, l.small, FontWeightHint::Regular);
            let mut right = rect.right() - l.pad * 0.5;
            bounded(
                f,
                (right - count_w, rect.y + (rect.h - l.small) / 2.0),
                count_w,
                &count,
                OVERLAY0,
                l.small,
                FontWeightHint::Regular,
            );
            right -= count_w + l.pad * 0.5;
            if mac.trigger.is_some() {
                let dot = l.small * 0.8;
                fill(
                    f,
                    Rect::new(right - dot, rect.y + (rect.h - dot) / 2.0, dot, dot),
                    PEACH,
                    CornerRadii::all(dot / 2.0),
                );
                right -= dot + l.pad * 0.5;
            }
            let name_x = rect.x + l.pad * 0.6;
            bounded(
                f,
                (name_x, rect.y + (rect.h - l.font) / 2.0),
                right - name_x,
                &mac.name,
                if selected { TEXT } else { SUBTEXT1 },
                l.font,
                if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            );
            f.hit(Target::Macro(i), rect);
        }

        vline(f, panel.right(), panel.y, panel.bottom(), SURFACE0);

        fill(f, foot, CRUST, CornerRadii::ZERO);
        self.draw_button_row(f, l, foot, &[Button::NewMacro, Button::DeleteMacro]);
    }

    /// A row of equal buttons filling a panel footer.
    ///
    /// One implementation for the sidebar's pair, the action list's trio and
    /// the script tab's single Apply, so none of them can be the one that
    /// forgets to divide the room by the number of buttons in it.
    fn draw_button_row(&self, f: &mut Frame<Target>, l: &Layout, foot: Rect, buttons: &[Button]) {
        if foot.is_empty() || buttons.is_empty() {
            return;
        }
        let gaps = l.pad * usize_f32(buttons.len().saturating_sub(1));
        let each = (foot.w - l.pad * 2.0 - gaps) / usize_f32(buttons.len());
        if each <= 0.0 {
            return;
        }
        let h = l.button.min(foot.h);
        let y = foot.y + (foot.h - h) / 2.0;
        for (i, button) in buttons.iter().enumerate() {
            let rect = Rect::new(
                (each + l.pad).mul_add(usize_f32(i), foot.x + l.pad),
                y,
                each,
                h,
            );
            let (bg, fg) = match button {
                Button::NewMacro | Button::ApplyScript => (BLUE, CRUST),
                Button::DeleteMacro | Button::DeleteAction => (SURFACE1, RED),
                _ => (SURFACE1, TEXT),
            };
            fill(f, rect, bg, CornerRadii::all(CORNER_RADIUS));
            centred(
                f,
                rect.x,
                rect.w,
                rect.y + (rect.h - l.small) / 2.0,
                &faced(*button),
                fg,
                l.small,
                FontWeightHint::Bold,
            );
            f.hit(Target::Button(*button), rect);
        }
    }

    /// The centre panel: the action list or the script, whichever tab is up.
    fn draw_list(&self, f: &mut Frame<Target>, l: &Layout) {
        let panel = l.list;
        if panel.is_empty() {
            return;
        }
        fill(f, panel, BASE, CornerRadii::ZERO);
        let (head, body, foot) = Layout::split(panel, l.row, l.button + l.pad);

        fill(f, head, SURFACE0, CornerRadii::ZERO);
        bounded(
            f,
            (head.x + l.pad, head.y + (head.h - l.font) / 2.0),
            head.w - l.pad * 2.0,
            match self.active_tab {
                ActiveTab::Editor => "Actions",
                ActiveTab::Script => "Script",
            },
            TEXT,
            l.font,
            FontWeightHint::Bold,
        );

        match self.active_tab {
            ActiveTab::Editor => {
                self.draw_actions(f, l, body);
                fill(f, foot, SURFACE0, CornerRadii::ZERO);
                self.draw_button_row(
                    f,
                    l,
                    foot,
                    &[
                        Button::MoveActionUp,
                        Button::MoveActionDown,
                        Button::DeleteAction,
                    ],
                );
            }
            ActiveTab::Script => {
                self.draw_script(f, l, body);
                fill(f, foot, SURFACE0, CornerRadii::ZERO);
                self.draw_button_row(f, l, foot, &[Button::ApplyScript]);
            }
        }

        if !l.props.is_empty() {
            vline(f, panel.right(), panel.y, panel.bottom(), SURFACE0);
        }
    }

    /// The rows of the selected macro's actions.
    fn draw_actions(&self, f: &mut Frame<Target>, l: &Layout, body: Rect) {
        if body.is_empty() {
            return;
        }
        let Some(mac) = self.selected_macro_id.and_then(|id| self.library.get(id)) else {
            centred(
                f,
                body.x + l.pad,
                (body.w - l.pad * 2.0).max(0.0),
                body.y + (body.h - l.font) / 2.0,
                "Select a macro to edit",
                OVERLAY0,
                l.font,
                FontWeightHint::Regular,
            );
            return;
        };
        if mac.actions.is_empty() {
            centred(
                f,
                body.x + l.pad,
                (body.w - l.pad * 2.0).max(0.0),
                body.y + (body.h - l.font) / 2.0,
                "No actions. Start recording or add manually.",
                OVERLAY0,
                l.font,
                FontWeightHint::Regular,
            );
            return;
        }

        let first = self.action_scroll.min(mac.actions.len());
        for (slot, (i, timed)) in mac.actions.iter().enumerate().skip(first).enumerate() {
            let row_y = l.row.mul_add(usize_f32(slot), body.y);
            if row_y + l.row > body.bottom() + 0.01 {
                break;
            }
            let rect = Rect::new(body.x + 2.0, row_y, (body.w - 4.0).max(0.0), l.row - 2.0);
            let selected = self.selected_action_idx == Some(i);
            fill(
                f,
                rect,
                if selected { SURFACE0 } else { BASE },
                CornerRadii::all(CORNER_RADIUS),
            );

            if let PlaybackState::Playing { action_idx, .. } = &self.playback_state
                && *action_idx == i
            {
                fill(
                    f,
                    Rect::new(rect.x, rect.y, 3.0, rect.h),
                    GREEN,
                    CornerRadii::ZERO,
                );
            }

            // Four columns, every one of them measured. The old row put the
            // number at `x + 8`, the badge at `x + 38`, the label at `x + 70`
            // bounded by `w - 160`, and the delay at `x + w - 70` -- five
            // literals that between them assume a particular font at a
            // particular size in a particular panel width.
            let ty = rect.y + (rect.h - l.small) / 2.0;
            let mut cx = rect.x + l.pad * 0.6;

            // The right-hand column is placed first, because it is what tells
            // the left-hand columns where they must stop. Marching left to
            // right and only bounding the last column is how the number and
            // the badge came to be painted past the row's own right edge in a
            // narrow window: each of them was given the width it wanted.
            let mut right = rect.right() - l.pad * 0.6;
            if timed.delay_ms > 0 {
                let delay = format_duration_ms(timed.delay_ms);
                let delay_w = text::measure(&delay, l.small, FontWeightHint::Regular)
                    .min((right - cx).max(0.0));
                bounded(
                    f,
                    (right - delay_w, ty),
                    delay_w,
                    &delay,
                    YELLOW,
                    l.small,
                    FontWeightHint::Regular,
                );
                right -= delay_w + l.pad * 0.5;
            }

            let num = format!("{:>3}", i.saturating_add(1));
            let num_w =
                text::measure("000", l.small, FontWeightHint::Regular).min((right - cx).max(0.0));
            bounded(
                f,
                (cx, ty),
                num_w,
                &num,
                OVERLAY0,
                l.small,
                FontWeightHint::Regular,
            );
            cx += num_w + l.pad * 0.5;

            let badge_w = text::padded_width(
                timed.action.icon(),
                l.pad * 0.8,
                l.small,
                FontWeightHint::Bold,
            )
            .min((right - cx).max(0.0));
            let badge_h = (rect.h - 4.0).max(0.0);
            if badge_w > 0.0 && badge_h > 0.0 {
                let badge = Rect::new(cx, rect.y + 2.0, badge_w, badge_h);
                fill(f, badge, timed.action.badge_color(), CornerRadii::all(3.0));
                centred(
                    f,
                    badge.x,
                    badge.w,
                    ty,
                    timed.action.icon(),
                    CRUST,
                    l.small,
                    FontWeightHint::Bold,
                );
            }
            cx += badge_w + l.pad * 0.6;

            bounded(
                f,
                (cx, rect.y + (rect.h - l.font) / 2.0),
                right - cx,
                &timed.action.label(),
                if selected { TEXT } else { SUBTEXT1 },
                l.font,
                FontWeightHint::Regular,
            );
            f.hit(Target::Action(i), rect);
        }
    }

    /// The script tab's numbered lines and its parse error.
    fn draw_script(&self, f: &mut Frame<Target>, l: &Layout, body: Rect) {
        if body.is_empty() {
            return;
        }
        // The error message is a strip along the bottom of the body, taken out
        // of the body before the lines are laid out rather than painted over
        // them afterwards.
        let err_h = if self.script_error.is_some() {
            (l.small + l.pad).min(body.h * 0.5)
        } else {
            0.0
        };
        let text_area = Rect::new(
            body.x + 2.0,
            body.y + 2.0,
            (body.w - 4.0).max(0.0),
            (body.h - err_h - 4.0).max(0.0),
        );
        fill(f, text_area, MANTLE, CornerRadii::all(CORNER_RADIUS));

        let line_h = l.font * 1.35;
        // The gutter is bounded by the text area, not merely by how wide three
        // digits happen to be: in a window narrower than the line numbers, an
        // unbounded gutter puts the numbers past the right-hand edge and
        // leaves the lines themselves nowhere at all.
        let gutter = (text::measure("000", l.small, FontWeightHint::Regular) + l.pad)
            .min((text_area.w - l.pad).max(0.0));
        for (i, line) in self.script_text.lines().enumerate() {
            let ly = line_h.mul_add(usize_f32(i), text_area.y + l.pad * 0.4);
            if ly + line_h > text_area.bottom() + 0.01 {
                break;
            }
            bounded(
                f,
                (text_area.x + l.pad * 0.5, ly),
                gutter,
                &format!("{:>3}", i.saturating_add(1)),
                OVERLAY0,
                l.small,
                FontWeightHint::Regular,
            );
            let color = match line.as_bytes().first() {
                Some(b'#') => OVERLAY0,
                Some(b'$') => PEACH,
                Some(b':') => YELLOW,
                _ => TEXT,
            };
            let lx = text_area.x + l.pad * 0.5 + gutter;
            bounded(
                f,
                (lx, ly),
                text_area.right() - l.pad * 0.5 - lx,
                line,
                color,
                l.font,
                FontWeightHint::Regular,
            );
        }

        if let Some(err) = self.script_error.as_ref() {
            let strip = Rect::new(
                body.x + 2.0,
                text_area.bottom(),
                (body.w - 4.0).max(0.0),
                err_h,
            );
            fill(
                f,
                strip,
                Color::rgba(RED.r, RED.g, RED.b, 40),
                CornerRadii::all(CORNER_RADIUS),
            );
            bounded(
                f,
                (strip.x + l.pad * 0.5, strip.y + (strip.h - l.small) / 2.0),
                strip.w - l.pad,
                err,
                RED,
                l.small,
                FontWeightHint::Regular,
            );
        }
    }

    /// The read-out down the right-hand side, and the speed/repeat pads.
    fn draw_props(&self, f: &mut Frame<Target>, l: &Layout) {
        let panel = l.props;
        if panel.is_empty() {
            return;
        }
        fill(f, panel, MANTLE, CornerRadii::ZERO);
        // The pads are a *reserved* strip, so the rows above cannot run into
        // them. The old panel put the speed section at
        // `content_y + content_h - 100.0` and grew the property rows downwards
        // from the top with no idea the section was there, so a macro with a
        // selected action wrote its last rows straight over the heading; and
        // the repeat buttons, at `speed_section_y + 80.0` and 24 tall, ended
        // four pixels *below* the content area, in the status bar.
        let (head, body, pads) = Layout::split(panel, l.row, l.row * 4.0);

        fill(f, head, CRUST, CornerRadii::ZERO);
        bounded(
            f,
            (head.x + l.pad, head.y + (head.h - l.font) / 2.0),
            head.w - l.pad * 2.0,
            "Properties",
            TEXT,
            l.font,
            FontWeightHint::Bold,
        );

        let mut cy = body.y + l.pad * 0.5;

        if let Some(mac) = self.selected_macro_id.and_then(|id| self.library.get(id)) {
            let desc = if mac.description.is_empty() {
                "(none)"
            } else {
                mac.description.as_str()
            };
            let trigger = mac
                .trigger
                .as_ref()
                .map_or_else(|| "(none)".to_string(), Hotkey::label);
            let mut more = prop_row(f, l, body, &mut cy, "Name", &mac.name);
            more = more && prop_row(f, l, body, &mut cy, "Description", desc);
            more = more
                && prop_row(
                    f,
                    l,
                    body,
                    &mut cy,
                    "Actions",
                    &mac.action_count().to_string(),
                );
            more = more
                && prop_row(
                    f,
                    l,
                    body,
                    &mut cy,
                    "Duration",
                    &format_duration_ms(mac.total_duration_ms()),
                );
            more = more && prop_row(f, l, body, &mut cy, "Speed", mac.speed.label());
            more = more && prop_row(f, l, body, &mut cy, "Repeat", &mac.repeat_mode.label());
            more = more && prop_row(f, l, body, &mut cy, "Trigger", &trigger);

            if more {
                cy += l.pad * 0.5;
                hline(f, body.x + l.pad, body.right() - l.pad, cy, SURFACE0);
                cy += l.pad * 0.5;
            }
            match self
                .selected_action_idx
                .and_then(|i| mac.actions.get(i))
                .filter(|_| more)
            {
                Some(timed) => {
                    if prop_row(f, l, body, &mut cy, "Action", timed.action.icon()) {
                        let mut ok = prop_row(
                            f,
                            l,
                            body,
                            &mut cy,
                            "Delay",
                            &format!("{}ms", timed.delay_ms),
                        );
                        for (label, value) in action_rows(&timed.action) {
                            ok = ok && prop_row(f, l, body, &mut cy, label, &value);
                        }
                    }
                }
                None if more => {
                    prop_row(f, l, body, &mut cy, "Action", "(none selected)");
                }
                None => {}
            }
        } else {
            prop_row(f, l, body, &mut cy, "Macro", "(none selected)");
        }

        self.draw_pads(f, l, pads);
    }

    /// The speed and repeat pads at the foot of the properties panel.
    fn draw_pads(&self, f: &mut Frame<Target>, l: &Layout, pads: Rect) {
        if pads.is_empty() {
            return;
        }
        let selected = self.selected_macro_id.and_then(|id| self.library.get(id));
        let speed = selected.map_or(PlaybackSpeed::Normal, |m| m.speed);
        let repeat = selected.map_or(RepeatMode::Once, |m| m.repeat_mode);
        let quarter = pads.h / 4.0;

        bounded(
            f,
            (pads.x + l.pad, pads.y + (quarter - l.small) / 2.0),
            pads.w - l.pad * 2.0,
            "Playback Speed",
            TEXT,
            l.small,
            FontWeightHint::Bold,
        );
        let speeds = PlaybackSpeed::all();
        pad_row(
            f,
            l,
            Rect::new(pads.x, pads.y + quarter, pads.w, quarter),
            speeds.len(),
            |f, i, rect| {
                let Some(&s) = speeds.get(i) else { return };
                let on = s == speed;
                paint_pad(f, l, rect, s.label(), on, BLUE);
                f.hit(Target::Speed(s), rect);
            },
        );

        bounded(
            f,
            (
                pads.x + l.pad,
                quarter.mul_add(2.0, pads.y) + (quarter - l.small) / 2.0,
            ),
            pads.w - l.pad * 2.0,
            "Repeat Mode",
            TEXT,
            l.small,
            FontWeightHint::Bold,
        );
        let modes = [RepeatMode::Once, RepeatMode::Times(5), RepeatMode::Forever];
        pad_row(
            f,
            l,
            Rect::new(pads.x, quarter.mul_add(3.0, pads.y), pads.w, quarter),
            modes.len(),
            |f, i, rect| {
                let Some(&m) = modes.get(i) else { return };
                let on = m == repeat;
                paint_pad(f, l, rect, &m.label(), on, LAVENDER);
                f.hit(Target::Repeat(m), rect);
            },
        );
    }

    /// The one line of prose along the bottom.
    fn draw_status(&self, f: &mut Frame<Target>, l: &Layout) {
        let bar = l.status;
        if bar.is_empty() {
            return;
        }
        fill(f, bar, CRUST, CornerRadii::ZERO);
        let ty = bar.y + (bar.h - l.small) / 2.0;

        // The right-hand read-out is placed by measuring it, and the left-hand
        // message is bounded by what the read-out leaves. The old bar drew the
        // read-out at a literal `width - 200.0` -- off the left edge of any
        // window narrower than two hundred pixels -- and bounded the message at
        // `width / 2.0` regardless, so at four hundred wide the two overlapped.
        let state = format!(
            "Rec: {} | Play: {}",
            self.recording_state.label(),
            self.playback_state.label()
        );
        let state_w = text::measure(&state, l.small, FontWeightHint::Regular)
            .min((bar.w - l.pad * 2.0).max(0.0) * 0.6);
        let right = bar.right() - l.pad;
        bounded(
            f,
            (right - state_w, ty),
            state_w,
            &state,
            OVERLAY0,
            l.small,
            FontWeightHint::Regular,
        );
        let mx = bar.x + l.pad;
        bounded(
            f,
            (mx, ty),
            right - state_w - l.pad - mx,
            &self.status_message,
            SUBTEXT0,
            l.small,
            FontWeightHint::Regular,
        );
    }

    /// The card that lists every button and the key that does the same thing.
    fn draw_help(&self, f: &mut Frame<Target>, l: &Layout) {
        let rows = Button::all();
        let line = l.font * 1.5;
        let wanted_h = line.mul_add(usize_f32(rows.len()) + 2.0, l.pad * 2.0);
        let wanted_w = rows
            .iter()
            .map(|b| {
                text::measure(b.action_label(), l.font, FontWeightHint::Regular)
                    + text::measure(b.key_label(), l.font, FontWeightHint::Bold)
                    + l.pad * 4.0
            })
            .fold(0.0_f32, f32::max);
        let card = Rect::new(0.0, 0.0, wanted_w.min(l.window.w), wanted_h.min(l.window.h));
        let card = Rect::new(
            ((l.window.w - card.w) / 2.0).max(0.0),
            ((l.window.h - card.h) / 2.0).max(0.0),
            card.w,
            card.h,
        );
        fill(f, card, CRUST, CornerRadii::all(CORNER_RADIUS));
        outline(f, card, SURFACE2);

        let mut y = card.y + l.pad;
        centred(
            f,
            card.x + l.pad,
            (card.w - l.pad * 2.0).max(0.0),
            y,
            "Keys",
            TEAL,
            l.font,
            FontWeightHint::Bold,
        );
        y += line * 1.5;
        let key_w = (card.w - l.pad * 2.0) * 0.3;
        for button in rows {
            if y + line > card.bottom() {
                break;
            }
            bounded(
                f,
                (card.x + l.pad, y),
                key_w,
                button.key_label(),
                YELLOW,
                l.font,
                FontWeightHint::Bold,
            );
            let ax = card.x + l.pad + key_w;
            bounded(
                f,
                (ax, y),
                card.right() - l.pad - ax,
                button.action_label(),
                TEXT,
                l.font,
                FontWeightHint::Regular,
            );
            y += line;
        }
        // The card swallows the click that dismisses it, so a click meant for
        // the card is not also a click on whatever it is covering.
        f.hit(Target::Help, card);
    }

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    /// Answer one event from the window.
    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // A release is not a press. Handling both runs every binding twice,
        // which for a toggle means it toggles back before the frame is drawn.
        if !key.pressed {
            return EventResult::Ignored;
        }
        // A key with a modifier on it belongs to the window manager, not to us.
        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {
            return EventResult::Ignored;
        }
        if self.show_help {
            // Any key at all dismisses the card, so a user who opened it by
            // accident is not stuck reading it.
            self.show_help = false;
            self.status_message = "Ready".to_string();
            return EventResult::Consumed;
        }
        match key.key {
            Key::Escape => {
                self.show_help = false;
                EventResult::Consumed
            }
            Key::Tab => {
                self.active_tab = match self.active_tab {
                    ActiveTab::Editor => ActiveTab::Script,
                    ActiveTab::Script => ActiveTab::Editor,
                };
                self.status_message = format!("{} tab", self.active_tab.label());
                EventResult::Consumed
            }
            Key::Up => self.move_action_selection(-1),
            Key::Down => self.move_action_selection(1),
            Key::Left => self.move_macro_selection(-1),
            Key::Right => self.move_macro_selection(1),
            k => {
                for button in Button::all() {
                    if button.key() == k {
                        self.press(*button);
                        return EventResult::Consumed;
                    }
                }
                EventResult::Ignored
            }
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        let (w, h) = self.size;
        match mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let frame = self.frame(w, h);
                // A click is answered by what was drawn under it in the frame
                // this window is showing -- and by nothing if nothing was.
                let Some(target) = frame.hit_test(mouse.x, mouse.y) else {
                    return EventResult::Ignored;
                };
                self.click(target)
            }
            MouseEventKind::Scroll { dy, .. } => {
                let rows = self.wheel.rows(dy);
                if rows == 0 {
                    return EventResult::Ignored;
                }
                let l = Layout::solve(w, h);
                if l.sidebar.contains(mouse.x, mouse.y) {
                    self.scroll_sidebar(rows)
                } else if l.list.contains(mouse.x, mouse.y) {
                    self.scroll_actions(rows)
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Do what a click on `target` means.
    fn click(&mut self, target: Target) -> EventResult {
        match target {
            Target::Help => {
                self.show_help = false;
                EventResult::Consumed
            }
            Target::Tab(tab) => {
                self.active_tab = tab;
                self.status_message = format!("{} tab", tab.label());
                EventResult::Consumed
            }
            Target::Button(button) => {
                self.press(button);
                EventResult::Consumed
            }
            Target::Macro(i) => {
                self.select_macro_by_index(i);
                EventResult::Consumed
            }
            Target::Action(i) => {
                self.select_action(i);
                EventResult::Consumed
            }
            Target::Speed(speed) => {
                self.set_speed(speed);
                EventResult::Consumed
            }
            Target::Repeat(mode) => {
                self.set_repeat_mode(mode);
                EventResult::Consumed
            }
        }
    }

    /// Do what a button means -- the one implementation its key shares.
    ///
    /// A button and its key cannot drift apart because there is only one of
    /// them: `handle_key` looks the button up by its key and calls this.
    fn press(&mut self, button: Button) {
        match button {
            Button::Record => self.start_recording(),
            Button::StopRecording => self.stop_recording(),
            Button::Play => self.start_playback(),
            Button::PausePlayback => {
                if matches!(self.playback_state, PlaybackState::PausedPlayback { .. }) {
                    self.resume_playback();
                } else {
                    self.pause_playback();
                }
            }
            Button::StopPlayback => self.stop_playback(),
            Button::CycleSpeed => self.cycle_speed(),
            Button::CycleRepeat => self.cycle_repeat_mode(),
            Button::NewMacro => {
                let n = self.library.count().saturating_add(1);
                self.new_macro(&format!("Macro {n}"));
            }
            Button::DeleteMacro => {
                self.delete_selected_macro();
            }
            Button::MoveActionUp => {
                self.move_action_up();
            }
            Button::MoveActionDown => {
                self.move_action_down();
            }
            Button::DeleteAction => {
                self.delete_selected_action();
            }
            Button::ApplyScript => {
                self.active_tab = ActiveTab::Script;
                self.apply_script();
            }
            Button::Help => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.status_message = "Any key or click closes the card".to_string();
                }
            }
        }
        self.clamp_scrolls();
    }

    fn move_action_selection(&mut self, delta: isize) -> EventResult {
        let Some(n) = self
            .selected_macro_id
            .and_then(|id| self.library.get(id))
            .map(|m| m.actions.len())
        else {
            return EventResult::Ignored;
        };
        if n == 0 {
            return EventResult::Ignored;
        }
        let next = step(self.selected_action_idx, delta, n);
        if self.selected_action_idx == Some(next) {
            return EventResult::Ignored;
        }
        self.select_action(next);
        self.clamp_scrolls();
        EventResult::Consumed
    }

    fn move_macro_selection(&mut self, delta: isize) -> EventResult {
        let n = self.library.count();
        if n == 0 {
            return EventResult::Ignored;
        }
        let current = self
            .selected_macro_id
            .and_then(|id| self.library.list().iter().position(|m| m.id == id));
        let next = step(current, delta, n);
        if current == Some(next) {
            return EventResult::Ignored;
        }
        self.select_macro_by_index(next);
        self.clamp_scrolls();
        EventResult::Consumed
    }

    fn scroll_sidebar(&mut self, rows: isize) -> EventResult {
        let before = self.sidebar_scroll;
        self.sidebar_scroll = shift(self.sidebar_scroll, rows, self.library.count());
        self.clamp_scrolls();
        if self.sidebar_scroll == before {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    fn scroll_actions(&mut self, rows: isize) -> EventResult {
        let n = self
            .selected_macro_id
            .and_then(|id| self.library.get(id))
            .map_or(0, |m| m.actions.len());
        let before = self.action_scroll;
        self.action_scroll = shift(self.action_scroll, rows, n);
        self.clamp_scrolls();
        if self.action_scroll == before {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    /// Keep both scroll offsets pointing at rows that exist, and keep the
    /// selection on screen.
    ///
    /// The two offsets used to be `f32` fields written by nothing at all: they
    /// were read by the drawing pass, initialised to zero in `new`, and never
    /// assigned again anywhere in the program. A library with more macros than
    /// fit was permanently truncated at whatever the window could show.
    fn clamp_scrolls(&mut self) {
        self.sidebar_scroll = self
            .sidebar_scroll
            .min(self.library.count().saturating_sub(1));
        let actions = self
            .selected_macro_id
            .and_then(|id| self.library.get(id))
            .map_or(0, |m| m.actions.len());
        self.action_scroll = self.action_scroll.min(actions.saturating_sub(1));
        if let Some(i) = self.selected_action_idx
            && i < self.action_scroll
        {
            self.action_scroll = i;
        }
        if let Some(i) = self
            .selected_macro_id
            .and_then(|id| self.library.list().iter().position(|m| m.id == id))
            && i < self.sidebar_scroll
        {
            self.sidebar_scroll = i;
        }
    }
}

impl Default for AutomatorApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Drawing helpers
// ============================================================================

/// One filled rectangle.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii,
    });
}

/// A one-pixel horizontal rule.
fn hline(f: &mut Frame<Target>, x1: f32, x2: f32, y: f32, color: Color) {
    if x2 <= x1 {
        return;
    }
    f.push(RenderCommand::Line {
        x1,
        y1: y,
        x2,
        y2: y,
        color,
        width: 1.0,
    });
}

/// A one-pixel vertical rule.
fn vline(f: &mut Frame<Target>, x: f32, y1: f32, y2: f32, color: Color) {
    if y2 <= y1 {
        return;
    }
    f.push(RenderCommand::Line {
        x1: x,
        y1,
        x2: x,
        y2,
        color,
        width: 1.0,
    });
}

/// A one-pixel border round a rectangle.
fn outline(f: &mut Frame<Target>, r: Rect, color: Color) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width: 1.0,
        corner_radii: CornerRadii::all(CORNER_RADIUS),
    });
}

/// One run of text, cut with an ellipsis rather than allowed to run on.
fn bounded(
    f: &mut Frame<Target>,
    at: (f32, f32),
    width: f32,
    s: &str,
    color: Color,
    font_size: f32,
    font_weight: FontWeightHint,
) {
    // No room, no run. A caller squeezed to nothing hands on a width of zero
    // while its origin is still wherever the layout put it, which in a window
    // too small for the widget is outside the widget. Such a run paints
    // nothing either way, but a command outside the frame is a claim the
    // picture does not make, and it is the claim a test has to read.
    if width <= 0.0 || width.is_nan() {
        return;
    }
    f.push(RenderCommand::Text {
        x: at.0,
        y: at.1,
        text: String::from(s),
        color,
        font_size,
        font_weight,
        max_width: Some(width),
        overflow: TextOverflow::Ellipsis,
    });
}

/// A run of text centred in `[x, x + w)`, by measuring it.
///
/// The old program centred by subtracting a literal -- `x + w / 2.0 - 80.0`
/// for "Select a macro to edit" and `- 100.0` for the longer empty-state line,
/// each of them a guess at half of one particular string at one particular size
/// in a program that links `guitk::text`.
fn centred(
    f: &mut Frame<Target>,
    x: f32,
    w: f32,
    y: f32,
    s: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) {
    let measured = text::measure(s, size, weight);
    // The offset is floored at zero so a run too wide to centre starts at the
    // left edge rather than hanging off both sides, and the width handed on is
    // the room left from where the run starts, not the whole box -- a bound of
    // `w` measured from `x + offset` reaches `offset` pixels past the box's own
    // right edge.
    let offset = ((w - measured) / 2.0).max(0.0);
    bounded(
        f,
        (x + offset, y),
        (w - offset).max(0.0),
        s,
        color,
        size,
        weight,
    );
}

/// One `label: value` row of the properties panel.
///
/// Returns whether it was drawn -- a row that would fall past the bottom of the
/// body is left out, and so is every row after it. The old panel had no such
/// check: it grew rows downwards from the top of a panel whose bottom hundred
/// pixels were separately claimed by the speed and repeat sections, so a macro
/// with a selected action wrote its last rows over that heading.
fn prop_row(
    f: &mut Frame<Target>,
    l: &Layout,
    body: Rect,
    cy: &mut f32,
    label: &str,
    value: &str,
) -> bool {
    let row_h = l.small * 1.6;
    if *cy + row_h > body.bottom() + 0.01 {
        return false;
    }
    // The label column is a share of the panel, not a literal eighty pixels,
    // and the value starts where the label column ends rather than at a literal
    // `panel_x + 90.0` -- which in a panel narrower than a hundred pixels put
    // the value past the panel's own right edge.
    let label_w = (body.w - l.pad * 2.0) * 0.38;
    bounded(
        f,
        (body.x + l.pad, *cy),
        label_w,
        label,
        SUBTEXT0,
        l.small,
        FontWeightHint::Regular,
    );
    let vx = body.x + l.pad + label_w + l.pad * 0.4;
    bounded(
        f,
        (vx, *cy),
        body.right() - l.pad - vx,
        value,
        TEXT,
        l.small,
        FontWeightHint::Regular,
    );
    *cy += row_h;
    true
}

/// Lay `n` equal cells across a row and paint each through `each`.
fn pad_row<F>(f: &mut Frame<Target>, l: &Layout, row: Rect, n: usize, mut each: F)
where
    F: FnMut(&mut Frame<Target>, usize, Rect),
{
    if row.is_empty() || n == 0 {
        return;
    }
    let gap = l.pad * 0.4;
    let cell = (row.w - l.pad * 2.0 - gap * usize_f32(n.saturating_sub(1))) / usize_f32(n);
    if cell <= 0.0 {
        return;
    }
    let h = (row.h - 2.0).max(0.0);
    for i in 0..n {
        each(
            f,
            i,
            Rect::new(
                (cell + gap).mul_add(usize_f32(i), row.x + l.pad),
                row.y + (row.h - h) / 2.0,
                cell,
                h,
            ),
        );
    }
}

/// One cell of a speed or repeat pad.
fn paint_pad(f: &mut Frame<Target>, l: &Layout, rect: Rect, label: &str, on: bool, accent: Color) {
    fill(
        f,
        rect,
        if on { accent } else { SURFACE1 },
        CornerRadii::all(CORNER_RADIUS),
    );
    centred(
        f,
        rect.x,
        rect.w,
        rect.y + (rect.h - l.small) / 2.0,
        label,
        if on { CRUST } else { TEXT },
        l.small,
        if on {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        },
    );
}

/// A button's face: what it does, and the key that does the same thing.
///
/// The old program had no buttons at all -- every control was a key, and the
/// only record of which keys those were was nowhere. A label that names its own
/// key is what stops the two drifting apart in the user's head as well as in
/// the code.
fn faced(button: Button) -> String {
    format!("{} ({})", button.action_label(), button.key_label())
}

/// The property rows peculiar to one kind of action.
fn action_rows(action: &MacroAction) -> Vec<(&'static str, String)> {
    match action {
        MacroAction::KeyPress { key_name } | MacroAction::KeyRelease { key_name } => {
            vec![("Key", key_name.clone())]
        }
        MacroAction::MouseClick { x, y, button }
        | MacroAction::MouseDoubleClick { x, y, button } => {
            vec![
                ("Position", format!("({x:.0}, {y:.0})")),
                ("Button", button.label().to_string()),
            ]
        }
        MacroAction::MouseMove { x, y } => vec![("Target", format!("({x:.0}, {y:.0})"))],
        MacroAction::Scroll { direction, amount } => vec![
            ("Direction", direction.label().to_string()),
            ("Amount", amount.to_string()),
        ],
        MacroAction::TypeText { text } => {
            vec![("Text", text.chars().take(30).collect())]
        }
        MacroAction::Delay { ms } => vec![("Wait", format!("{ms}ms"))],
        MacroAction::IfPixelColor {
            x,
            y,
            r,
            g,
            b,
            tolerance,
        } => vec![
            ("Pixel", format!("({x:.0}, {y:.0})")),
            ("Color", format!("#{r:02X}{g:02X}{b:02X}")),
            ("Tolerance", tolerance.to_string()),
        ],
    }
}

/// Move a selection by `delta` within `0..n`, starting one from nothing.
///
/// Both ends are bounded here, once, rather than one end in a match guard and
/// the other in the arm body: two spellings of one rule is one spelling too
/// many for a rule that has to hold at both ends.
fn step(current: Option<usize>, delta: isize, n: usize) -> usize {
    let Some(i) = current else {
        return if delta < 0 { n.saturating_sub(1) } else { 0 };
    };
    if delta < 0 {
        i.saturating_sub(delta.unsigned_abs())
    } else {
        i.saturating_add(delta.unsigned_abs())
            .min(n.saturating_sub(1))
    }
}

/// Move a scroll offset by `rows` within `0..n`.
fn shift(current: usize, rows: isize, n: usize) -> usize {
    let moved = if rows < 0 {
        current.saturating_sub(rows.unsigned_abs())
    } else {
        current.saturating_add(rows.unsigned_abs())
    };
    moved.min(n.saturating_sub(1))
}

/// `usize` as `f32`, saturating rather than wrapping.
///
/// Every one of these used to be a bare `as f32` under a crate-level
/// `#![allow(clippy::cast_precision_loss)]`.
fn usize_f32(n: usize) -> f32 {
    u32::try_from(n).map_or(f32::from(u16::MAX), |n| n as f32)
}

/// `u64` milliseconds as `f32`, for the pulse phase.
fn u64_f32(n: u64) -> f32 {
    u32::try_from(n).map_or(f32::from(u16::MAX), |n| n as f32)
}

/// A `0.0..=255.0` level as a byte, with both ends bounded.
fn f32_u8(v: f32) -> u8 {
    if v.is_nan() {
        return 0;
    }
    let v = v.clamp(0.0, 255.0);
    // SAFETY-of-arithmetic: clamped to the byte range and not NaN, so the
    // truncation below cannot saturate or wrap.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 and checked for NaN on the line above"
    )]
    {
        v as u8
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Format a millisecond duration as a human-readable string.
///
/// Automator and Task Scheduler both report how long a step took, and both
/// had a copy of this. They rendered 5 250 ms as `5.250s` and `5.3s`
/// respectively — three decimals of a wall-clock measurement against one —
/// and neither had an hours field, so a 90-minute automation read `90m 0s`.
fn format_duration_ms(ms: u64) -> String {
    guitk::duration::units_ms(ms)
}

impl App for AutomatorApp {
    fn title(&self) -> String {
        String::from(TITLE)
    }

    fn app_id(&self) -> String {
        String::from("automator")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<std::time::Duration> {
        // Without a clock the recording indicator does not pulse and, more to
        // the point, playback does not advance: `tick_playback` had no caller
        // outside the tests.
        Some(std::time::Duration::from_millis(TICK_MS))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.size = (width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for AutomatorApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
        self.size = size;
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.size = size;
        self.handle_event(&Event::Key(key.clone()))
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() -> ExitCode {
    let mut app = AutomatorApp::with_demo_library();
    app::launch("automator", &mut app)
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
        let frame = app.frame(800.0, 600.0);
        assert!(!frame.commands().is_empty());
    }

    #[test]
    fn test_app_render_with_macros() {
        let mut app = AutomatorApp::new();
        app.new_macro("Test Macro");
        app.add_action(MacroAction::Delay { ms: 100 }, 0);
        app.select_action(0);
        let frame = app.frame(1000.0, 700.0);
        assert!(!frame.commands().is_empty());
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
        // Was "3s". A whole second now keeps its tenths place, so that a row
        // of run durations lines up instead of alternating "3s" and "3.5s".
        assert_eq!(format_duration_ms(3000), "3.0s");
    }

    #[test]
    fn test_format_duration_ms_seconds_with_millis() {
        // Was "3.500s" — three decimal places on a figure the scheduler's own
        // list rendered as "3.5s". Tenths is what a run duration can support.
        assert_eq!(format_duration_ms(3500), "3.5s");
    }

    #[test]
    fn test_format_duration_ms_has_an_hours_field() {
        // Regression: the old ladder stopped at minutes, so a 90-minute
        // automation reported "90m 0s".
        assert_eq!(format_duration_ms(5_400_000), "1h 30m 0s");
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
        let frame = app.frame(800.0, 600.0);
        // Should have REC text somewhere.
        let has_rec = frame
            .commands()
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
        let frame = app.frame(1000.0, 700.0);
        assert!(!frame.commands().is_empty());
    }

    // =======================================================================
    // The program in a window
    // =======================================================================
    //
    // Everything below is about Automator being a window rather than a
    // simulation: that the picture is drawn at the size it is given, that what
    // the pointer reaches is what the painter painted, that a button and its
    // key do the same thing, and that a running macro advances on a clock
    // rather than only when a test calls `tick_playback` by hand.

    /// The window widths every layout claim is checked at.
    ///
    /// A rule about `Layout::solve` is a rule at *every* size, so a handful of
    /// sampled sizes tests a handful of points and nothing else. The sizes that
    /// break a layout rule are the ones nobody would think to sample: 20 wide,
    /// where neither side panel can be afforded; 260 wide, where the sidebar
    /// fits and the properties panel does not; 1400 wide, where both are
    /// capped and the list takes the rest.
    const GRID_W: [f32; 8] = [0.0, 20.0, 60.0, 130.0, 260.0, 520.0, 1000.0, 1400.0];
    /// The window heights every layout claim is checked at.
    const GRID_H: [f32; 6] = [0.0, 18.0, 55.0, 140.0, 700.0, 1100.0];

    /// Every window size the layout claims sweep.
    fn sizes() -> impl Iterator<Item = (f32, f32)> {
        GRID_W.into_iter().flat_map(|w| GRID_H.map(move |h| (w, h)))
    }

    /// Is `inner` within `outer`, allowing for a pixel of rounding?
    ///
    /// A rectangle with no area is "inside" anything: it is the answer the
    /// layout gives when a panel does not fit and is left out, and a panel that
    /// was left out cannot hang off an edge.
    fn inside(outer: Rect, inner: Rect) -> bool {
        inner.is_empty()
            || (inner.x >= outer.x - 0.01
                && inner.y >= outer.y - 0.01
                && inner.right() <= outer.right() + 0.01
                && inner.bottom() <= outer.bottom() + 0.01)
    }

    /// Do these two rectangles share any area?
    fn overlaps(a: Rect, b: Rect) -> bool {
        a.intersect(b).is_some_and(|r| r.w > 0.01 && r.h > 0.01)
    }

    #[test]
    fn every_pane_stays_inside_the_window() {
        // The old layout was eight compile-time literals. The centre panel was
        // `width - 240.0 - 260.0`, which is negative below five hundred wide,
        // and the properties panel began at `width - 260.0`, which is off the
        // left edge of anything narrower than that.
        for (w, h) in sizes() {
            let l = Layout::solve(w, h);
            let at = format!("{w}x{h}");
            for (name, r) in [
                ("header", l.header),
                ("toolbar", l.toolbar),
                ("sidebar", l.sidebar),
                ("list", l.list),
                ("props", l.props),
                ("status", l.status),
            ] {
                assert!(inside(l.window, r), "{name} escapes the window at {at}");
                assert!(r.w >= -0.01 && r.h >= -0.01, "{name} is negative at {at}");
            }
        }
    }

    #[test]
    fn the_panes_are_stacked_and_do_not_overlap() {
        for (w, h) in sizes() {
            let l = Layout::solve(w, h);
            let at = format!("{w}x{h}");
            assert!(
                l.header.bottom() <= l.toolbar.y + 0.01,
                "header/toolbar {at}"
            );
            assert!(l.toolbar.bottom() <= l.list.y + 0.01, "toolbar/list {at}");
            assert!(l.list.bottom() <= l.status.y + 0.01, "list/status {at}");
            assert!(l.status.bottom() <= h + 0.01, "status/window {at}");
            for (a, an, b, bn) in [
                (l.sidebar, "sidebar", l.list, "list"),
                (l.list, "list", l.props, "props"),
                (l.sidebar, "sidebar", l.props, "props"),
                (l.sidebar, "sidebar", l.status, "status"),
                (l.props, "props", l.status, "status"),
                (l.sidebar, "sidebar", l.toolbar, "toolbar"),
                (l.props, "props", l.toolbar, "toolbar"),
            ] {
                assert!(!overlaps(a, b), "{an} overlaps {bn} at {at}");
            }
        }
    }

    #[test]
    fn the_list_is_never_given_up_for_a_side_panel() {
        // The list is what is doing the work, so a side panel is taken only if
        // it leaves the list wide enough to read. A window that can pay for
        // neither gets all of its width as list.
        for (w, h) in sizes() {
            let l = Layout::solve(w, h);
            let at = format!("{w}x{h}");
            if !l.sidebar.is_empty() || !l.props.is_empty() {
                assert!(
                    l.list.w >= MIN_LIST_W - 0.01,
                    "list squeezed to {} at {at}",
                    l.list.w
                );
            }
            for (name, r) in [("sidebar", l.sidebar), ("props", l.props)] {
                assert!(
                    r.is_empty() || r.w >= MIN_PANEL_W - 0.01,
                    "{name} squeezed to {} at {at}",
                    r.w
                );
            }
        }
    }

    #[test]
    fn the_panels_and_the_list_fill_the_width_between_them() {
        // Whatever the side panels do not take, the list takes: a gap between
        // them would be felt-coloured nothing, and an overlap would be one
        // panel painted over another.
        for (w, h) in sizes() {
            let l = Layout::solve(w, h);
            let at = format!("{w}x{h}");
            let sidebar_w = if l.sidebar.is_empty() {
                0.0
            } else {
                l.sidebar.w
            };
            let props_w = if l.props.is_empty() { 0.0 } else { l.props.w };
            assert!(
                (sidebar_w + l.list.w + props_w - w).abs() < 0.01,
                "widths sum to {} not {w} at {at}",
                sidebar_w + l.list.w + props_w
            );
        }
    }

    #[test]
    fn a_footer_never_eats_the_body_it_is_the_foot_of() {
        // The old sidebar drew its New/Delete bar at `content_y + content_h -
        // 36.0` unconditionally, and the action list drew its Up/Down/Delete
        // bar the same way. In a short window the bar was not under the list,
        // it was on top of it -- and the rows it covered were drawn all the
        // same, under a bar that hid them.
        for (w, h) in sizes() {
            let l = Layout::solve(w, h);
            let at = format!("{w}x{h}");
            for panel in [l.sidebar, l.list, l.props] {
                let (head, body, foot) = Layout::split(panel, l.row, l.button + l.pad);
                assert!(inside(panel, head), "head escapes at {at}");
                assert!(inside(panel, body), "body escapes at {at}");
                assert!(inside(panel, foot), "foot escapes at {at}");
                assert!(!overlaps(head, body), "head over body at {at}");
                assert!(!overlaps(body, foot), "body over foot at {at}");
                assert!(head.bottom() <= body.y + 0.01, "head below body at {at}");
                assert!(body.bottom() <= foot.y + 0.01, "body below foot at {at}");
                assert!(body.h >= -0.01, "body is negative at {at}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // The states the picture is drawn in
    // -----------------------------------------------------------------------
    //
    // A whole-frame invariant is only as wide as the states you draw. Half of
    // this program's picture -- the script tab, the help card, the empty-list
    // message, the recording indicator -- is drawn in states the demo library
    // is not in, and the strings those states centre are exactly the ones a
    // single fixture cannot see.

    /// A library with nothing in it: every list is empty and every read-out
    /// has nothing to read out.
    fn empty_app() -> AutomatorApp {
        AutomatorApp::new()
    }

    /// The demo library `main` used to build and throw away.
    fn demo_app() -> AutomatorApp {
        AutomatorApp::with_demo_library()
    }

    /// Recording, so the header's dot and "REC" are painted.
    fn recording_app() -> AutomatorApp {
        let mut app = demo_app();
        app.press(Button::Record);
        app
    }

    /// Playing, so the header's playing read-out is painted.
    fn playing_app() -> AutomatorApp {
        let mut app = demo_app();
        app.press(Button::Play);
        app
    }

    /// The script tab, whose body is a different drawing pass entirely.
    fn script_app() -> AutomatorApp {
        let mut app = demo_app();
        app.active_tab = ActiveTab::Script;
        app.set_script_text("wait 100\nclick 50 60\ntype hello world\nkey Enter\n");
        app
    }

    /// The script tab after a script that does not parse, so the error line is
    /// painted.
    fn error_app() -> AutomatorApp {
        let mut app = demo_app();
        app.active_tab = ActiveTab::Script;
        app.set_script_text("wait\nclick oops");
        app.press(Button::ApplyScript);
        app
    }

    /// The help card, which covers the picture and is drawn nowhere else.
    fn helping_app() -> AutomatorApp {
        let mut app = demo_app();
        app.press(Button::Help);
        app
    }

    /// Names and a status longer than any window is wide.
    fn wordy_app() -> AutomatorApp {
        let mut app = demo_app();
        app.new_macro(&"A macro with a preposterously long name ".repeat(3));
        app.add_action(
            MacroAction::TypeText {
                text: "the quick brown fox jumps over the lazy dog ".repeat(4),
            },
            0,
        );
        app.select_action(0);
        app.status_message = "Recorded a mouse move to a point far off the right edge ".repeat(3);
        app
    }

    /// Every state the picture is drawn in.
    fn states() -> Vec<(&'static str, AutomatorApp)> {
        vec![
            ("empty", empty_app()),
            ("demo", demo_app()),
            ("recording", recording_app()),
            ("playing", playing_app()),
            ("script", script_app()),
            ("error", error_app()),
            ("helping", helping_app()),
            ("wordy", wordy_app()),
        ]
    }

    #[test]
    fn no_text_runs_off_the_window_it_is_drawn_in() {
        // Every text the old program drew was `max_width: None`. A macro name,
        // an action's summary, the status message and the header's playing
        // read-out all ran straight off a narrow window, and the panel they
        // were nominally in did not bound them at all.
        for (name, app) in states() {
            for (w, h) in sizes() {
                for c in app.frame(w, h).commands() {
                    let RenderCommand::Text {
                        text,
                        x,
                        max_width,
                        font_size,
                        font_weight,
                        ..
                    } = c
                    else {
                        continue;
                    };
                    let bound =
                        max_width.unwrap_or_else(|| text::measure(text, *font_size, *font_weight));
                    assert!(
                        *x >= -0.01,
                        "{name}: {text:?} starts at {x} in a {w}x{h} window"
                    );
                    assert!(
                        x + bound <= w + 0.01,
                        "{name}: {text:?} runs off a {w}x{h} window: x {x} + {bound}"
                    );
                }
            }
        }
    }

    #[test]
    fn nothing_is_painted_outside_the_window() {
        // A rectangle drawn past the edge is a rectangle the compositor pays
        // to clip. More to the point it means the layout believed in room the
        // window does not have.
        for (name, app) in states() {
            for (w, h) in sizes() {
                let window = Rect::new(0.0, 0.0, w, h);
                for c in app.frame(w, h).commands() {
                    let (x, y, width, height) = match c {
                        RenderCommand::FillRect {
                            x,
                            y,
                            width,
                            height,
                            ..
                        }
                        | RenderCommand::StrokeRect {
                            x,
                            y,
                            width,
                            height,
                            ..
                        } => (*x, *y, *width, *height),
                        _ => continue,
                    };
                    let r = Rect::new(x, y, width, height);
                    assert!(
                        inside(window, r),
                        "{name}: a rect at ({x},{y}) {width}x{height} escapes a {w}x{h} window"
                    );
                }
            }
        }
    }

    #[test]
    fn every_hit_box_is_inside_the_window_and_has_area() {
        // A hit box outside the window can never be clicked, and a hit box
        // with no area is a control that is painted and unreachable -- which
        // is what every control in this program was.
        for (name, app) in states() {
            for (w, h) in sizes() {
                let window = Rect::new(0.0, 0.0, w, h);
                for (target, rect) in app.frame(w, h).hits() {
                    assert!(
                        rect.w > 0.0 && rect.h > 0.0,
                        "{name}: {target:?} has no area at {w}x{h}"
                    );
                    assert!(
                        inside(window, *rect),
                        "{name}: {target:?} is hit-boxed outside a {w}x{h} window"
                    );
                }
            }
        }
    }
}
