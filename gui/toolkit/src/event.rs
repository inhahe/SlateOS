//! Event types for the GUI toolkit.
//!
//! Events flow from the compositor/backend into the widget tree.
//! Widgets consume or propagate events up the tree.

/// Input event from the windowing system.
///
/// `PartialEq` but not `Eq`: `Tick`/`ScaleChanged` and the mouse coordinates
/// carry floats. It is derived so that a codec can assert a round trip returns
/// what it was given — comparing field by field instead would let an encoder
/// drop a field and still pass its own test.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Mouse event (click, move, scroll).
    Mouse(MouseEvent),
    /// Keyboard event.
    Key(KeyEvent),
    /// Window resized.
    Resize { width: u32, height: u32 },
    /// Window moved: the new screen position of its top-left corner.
    ///
    /// Sent for moves the client did not initiate as well as ones it did — a
    /// user dragging the title bar, or the compositor rearranging windows when
    /// a monitor is unplugged. Without it a window can only ever know where it
    /// last *asked* to be, which is not the same thing, and anything that must
    /// be placed in screen coordinates — a menu, a tooltip, a window position
    /// remembered across runs — would be computed against a stale answer.
    Moved { x: i32, y: i32 },
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
#[derive(Clone, Debug, PartialEq)]
pub struct MouseEvent {
    /// Mouse X position relative to widget.
    pub x: f32,
    /// Mouse Y position relative to widget.
    pub y: f32,
    /// Type of mouse event.
    pub kind: MouseEventKind,
}

/// Mouse event kind.
#[derive(Clone, Debug, PartialEq)]
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
    /// Scroll wheel, in **notches** — *not* pixels.
    ///
    /// `1.0` per detent of an ordinary wheel, positive away from the user, and
    /// a fraction of one for a high-resolution wheel or a precision trackpad.
    /// This is what the compositor's `wheel_delta()` produces (`raw / 120`),
    /// and it is the only thing in the tree that produces one.
    ///
    /// **Do not multiply this by a pixel constant.** It said "pixels" here
    /// once, and twelve consumers each picked a different constant to convert
    /// the imaginary pixels into something useful — 1, 20 and 40 px per notch
    /// all appeared, and the one handler that divided by a line height scrolled
    /// nothing at all, ever, because a notch over a line height truncates to
    /// zero. Use [`wheel::Accumulator`] to turn these into rows (it keeps the
    /// fractions a trackpad sends, which rounding each event would discard), or
    /// [`wheel::pixels`] for a genuinely continuous view.
    ///
    /// **`dx` and `dy` do not share a sign convention**, so `dx` needs
    /// [`wheel::pixels_x`], not [`wheel::pixels`]. `dy` is positive away from
    /// the user, which scrolls *towards the start*; `dx` is positive to the
    /// right, which scrolls *towards the end*. Both arrive through the same
    /// `wheel_delta()`, so the difference is in what the two Windows messages
    /// mean, not in how either is decoded — and a handler that reaches for
    /// `pixels` on the `dx` because the name looks right scrolls backwards.
    ///
    /// [`wheel::Accumulator`]: crate::wheel::Accumulator
    /// [`wheel::pixels`]: crate::wheel::pixels
    /// [`wheel::pixels_x`]: crate::wheel::pixels_x
    Scroll { dx: f32, dy: f32 },
    /// Double-click.
    DoubleClick(MouseButton),
}

/// Key event data.
///
/// Note what is *not* here: the physical scancode. A widget reacting to a key
/// wants to know it was `Key::Left`, not which switch on the board closed, and
/// putting the scancode here would oblige all 500-odd construction sites in the
/// tree to invent one. It rides on the wire event instead — see
/// `guiremote::input::InputEvent::scancode`.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyEvent {
    /// Key code (virtual key).
    pub key: Key,
    /// Whether this is a press or release.
    pub pressed: bool,
    /// Modifier keys held.
    pub modifiers: Modifiers,
    /// The text this keystroke produced — empty for the keys that produce
    /// none, and for every release.
    ///
    /// A string rather than the `Option<char>` this was until dead keys
    /// arrived, because one keystroke can produce **none, one, or several**
    /// characters and only the first two of those fit in an `Option`:
    ///
    /// * a dead key (`´` on a German board) types nothing on its own — it
    ///   waits for the next key and composes with it;
    /// * `´` then `e` types the single character `é`;
    /// * `´` then `x` — a pair that composes to nothing — types **`´x`**,
    ///   two characters, because the alternative is to discard silently what
    ///   someone typed. See `design-decisions.md` §550.
    ///
    /// Every real system agrees on the shape even where it disagrees on that
    /// last case: X11's `XLookupString` returns a character *count*, Wayland's
    /// text-input commits a *string*, and Windows sends as many `WM_CHAR`
    /// messages as the keystroke produced.
    ///
    /// **Most callers want [`typed`](Self::typed), not this field.** A widget
    /// that inserts text should insert all of it *and* drop the control
    /// characters, which is exactly what `typed` does; reading `text` raw is
    /// right only where control characters are the point (the terminal, which
    /// forwards them to a shell). One that is matching a single character —
    /// type-ahead in a list, a mnemonic — wants [`single_char`](Self::single_char),
    /// which answers `None` for the multi-character case rather than acting on
    /// half of it.
    pub text: String,
}

impl KeyEvent {
    /// The one character this keystroke produced, if it produced exactly one.
    ///
    /// `None` both for a keystroke that produced no text and for one that
    /// produced several. Deliberately not "the first character": a caller
    /// reaching for this is choosing *between* characters — jumping to a list
    /// item, matching a mnemonic — and acting on the first half of `´x` would
    /// be acting on something the user did not type as a unit.
    #[must_use]
    pub fn single_char(&self) -> Option<char> {
        let mut chars = self.text.chars();
        let first = chars.next()?;
        chars.next().is_none().then_some(first)
    }

    /// The characters this keystroke typed that belong in a text field:
    /// [`text`](Self::text) with control characters dropped.
    ///
    /// Every text-entry site in the tree wants exactly this and used to spell
    /// it out itself — thirty-odd copies of `if let Some(ch) = k.text && !ch
    /// .is_control()`, which is thirty chances to forget the second half. The
    /// filter is not optional politeness: on most layouts Enter, Tab, Escape
    /// and Backspace all *produce* text (`\r`, `\t`, `\x1b`, `\x08`), so a
    /// field that appends whatever arrives fills up with unprintable bytes the
    /// moment someone presses Escape.
    ///
    /// Yields nothing for a release, for a dead key awaiting its next
    /// keystroke, and for a key that types only a control character. Pair it
    /// with [`types_text`](Self::types_text) when the answer to "did this
    /// keystroke belong to the text field at all?" decides whether the event is
    /// consumed.
    pub fn typed(&self) -> impl Iterator<Item = char> + '_ {
        self.text.chars().filter(|c| !c.is_control())
    }

    /// Whether [`typed`](Self::typed) would yield anything.
    ///
    /// Separate from `typed().next().is_some()` only in reading better at the
    /// call site, where it is nearly always the condition on which a widget
    /// decides to claim the keystroke.
    #[must_use]
    pub fn types_text(&self) -> bool {
        self.typed().next().is_some()
    }
}

/// Virtual key codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    // Letters
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    // Numbers
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    // Function keys
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    // Navigation
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    // Editing
    Backspace,
    Delete,
    Insert,
    Enter,
    Tab,
    Escape,
    Space,
    // Modifiers (as key events)
    LeftShift,
    RightShift,
    LeftCtrl,
    RightCtrl,
    LeftAlt,
    RightAlt,
    LeftSuper,
    RightSuper,
    // Punctuation
    Comma,
    Period,
    Semicolon,
    Colon,
    Slash,
    Backslash,
    LeftBracket,
    RightBracket,
    Minus,
    Equals,
    Apostrophe,
    Grave,
    // Other
    PrintScreen,
    ScrollLock,
    Pause,
    CapsLock,
    NumLock,
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

#[cfg(test)]
mod tests {
    use super::{Key, KeyEvent, Modifiers};

    /// The five cases `text` exists to tell apart, built by hand so each test
    /// below reads as the keystroke it is about.
    fn typing(text: &str) -> KeyEvent {
        KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: text.to_string(),
        }
    }

    #[test]
    fn an_ordinary_keystroke_types_its_one_character() {
        assert_eq!(typing("a").typed().collect::<String>(), "a");
        assert!(typing("a").types_text());
        assert_eq!(typing("a").single_char(), Some('a'));
    }

    #[test]
    fn a_dead_key_types_nothing_and_is_not_the_same_as_a_key_that_types_nothing() {
        // Both spell as an empty string here — the *distinction* lives in the
        // compositor, which knows it is holding a pending accent. What matters
        // to a widget is that neither belongs in a text field, and both agree.
        let dead = typing("");
        assert!(!dead.types_text());
        assert_eq!(dead.typed().count(), 0);
        assert_eq!(dead.single_char(), None);
    }

    #[test]
    fn a_composition_that_succeeded_types_one_character_not_two() {
        // `´` then `e`. The compositor has already done the composing; the
        // event carries the result, so a text field inserts one character and
        // one Backspace removes it.
        let composed = typing("é");
        assert_eq!(composed.typed().collect::<String>(), "é");
        assert_eq!(composed.single_char(), Some('é'));
    }

    #[test]
    fn a_composition_that_failed_types_both_characters() {
        // `´` then `x`, which composes to nothing. Following Windows and macOS
        // rather than X11, both are typed — see `design-decisions.md` §550.
        // The accent is wrong but *visible*, and one Backspace from right;
        // discarding it would lose a keystroke with nothing on screen to say so.
        let failed = typing("´x");
        assert_eq!(failed.typed().collect::<String>(), "´x");
    }

    /// The distinction that makes `single_char` worth having over
    /// `text.chars().next()`.
    #[test]
    fn single_char_refuses_a_keystroke_that_typed_two_rather_than_taking_the_first() {
        // A type-ahead list jumping to `´` would select an entry the user never
        // named. `None` is the honest answer: two characters named no one item.
        assert_eq!(typing("´x").single_char(), None);
    }

    #[test]
    fn control_characters_are_not_text_however_they_arrive() {
        // Our own compositor reports no text for these, but a `guiremote` peer
        // running someone else's keymap does report them, and every text field
        // in the tree decides whether to claim a keystroke on this answer.
        for control in ["\r", "\n", "\t", "\x1b", "\x08", "\x7f"] {
            let ev = typing(control);
            assert!(
                !ev.types_text(),
                "{:?} must not count as text",
                control.escape_debug().to_string()
            );
            assert_eq!(ev.typed().count(), 0);
        }
    }

    #[test]
    fn a_control_mixed_into_real_text_loses_only_the_control() {
        // Rejecting the whole run would lose the part the user meant; taking
        // the whole run would put an unprintable byte in the field.
        assert_eq!(typing("a\x1bb").typed().collect::<String>(), "ab");
        assert!(typing("a\x1bb").types_text());
    }

    #[test]
    fn a_release_carries_no_text() {
        let release = KeyEvent {
            key: Key::A,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        assert!(!release.types_text());
    }

    /// `single_char` counts *characters*, not bytes — the multi-byte case must
    /// not read as "several".
    #[test]
    fn a_multi_byte_character_is_still_one_character() {
        assert_eq!(typing("ü").single_char(), Some('ü'));
        assert_eq!(typing("€").single_char(), Some('€'));
    }
}
