//! Event types for the GUI toolkit.
//!
//! Events flow from the compositor/backend into the widget tree.
//! Widgets consume or propagate events up the tree.

/// Input event from the windowing system.
#[derive(Clone, Debug)]
pub enum Event {
    /// Mouse event (click, move, scroll).
    Mouse(MouseEvent),
    /// Keyboard event.
    Key(KeyEvent),
    /// Window resized.
    Resize { width: u32, height: u32 },
    /// Window focus gained.
    FocusIn,
    /// Window focus lost.
    FocusOut,
    /// Window close requested.
    CloseRequested,
    /// Timer tick (for animations, polling).
    Tick { elapsed_ms: u64 },
    /// DPI/scale factor changed.
    ScaleChanged { scale: f32 },
}

/// Mouse button identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

/// Mouse event data.
#[derive(Clone, Debug)]
pub struct MouseEvent {
    /// Mouse X position relative to widget.
    pub x: f32,
    /// Mouse Y position relative to widget.
    pub y: f32,
    /// Type of mouse event.
    pub kind: MouseEventKind,
}

/// Mouse event kind.
#[derive(Clone, Debug)]
pub enum MouseEventKind {
    /// Button pressed down.
    Press(MouseButton),
    /// Button released.
    Release(MouseButton),
    /// Mouse moved (with optional held button).
    Move,
    /// Mouse entered widget bounds.
    Enter,
    /// Mouse left widget bounds.
    Leave,
    /// Scroll wheel (dx, dy in pixels).
    Scroll { dx: f32, dy: f32 },
    /// Double-click.
    DoubleClick(MouseButton),
}

/// Key event data.
#[derive(Clone, Debug)]
pub struct KeyEvent {
    /// Key code (virtual key).
    pub key: Key,
    /// Whether this is a press or release.
    pub pressed: bool,
    /// Modifier keys held.
    pub modifiers: Modifiers,
    /// Character generated (if applicable, for text input).
    pub text: Option<char>,
}

/// Virtual key codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    // Letters
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    // Numbers
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    // Navigation
    Left, Right, Up, Down,
    Home, End, PageUp, PageDown,
    // Editing
    Backspace, Delete, Insert,
    Enter, Tab, Escape, Space,
    // Modifiers (as key events)
    LeftShift, RightShift,
    LeftCtrl, RightCtrl,
    LeftAlt, RightAlt,
    LeftSuper, RightSuper,
    // Punctuation
    Comma, Period, Semicolon, Colon,
    Slash, Backslash,
    LeftBracket, RightBracket,
    Minus, Equals, Apostrophe, Grave,
    // Other
    PrintScreen, ScrollLock, Pause,
    CapsLock, NumLock,
    /// Unknown/unmapped key.
    Unknown(u32),
}

/// Modifier key state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
        alt: false,
        super_key: false,
    };

    pub fn ctrl() -> Self {
        Self {
            ctrl: true,
            ..Self::NONE
        }
    }

    pub fn shift() -> Self {
        Self {
            shift: true,
            ..Self::NONE
        }
    }

    pub fn alt() -> Self {
        Self {
            alt: true,
            ..Self::NONE
        }
    }
}

/// Result of handling an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResult {
    /// Event was consumed (don't propagate further).
    Consumed,
    /// Event was ignored (propagate to parent).
    Ignored,
}
