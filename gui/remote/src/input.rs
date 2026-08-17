//! Input-event protocol — the return direction of the SlateOS display
//! protocol.
//!
//! Until this module existed, [`crate`] was one-directional: it encoded a
//! [`RenderTree`](guitk::render::RenderTree) into bytes and back, and that was
//! its entire vocabulary. A compositor could therefore show a client's window
//! and had no way to tell the client that anything had been *done* to it. The
//! two halves were both finished and the seam between them was missing: the
//! compositor already hit-tests, tracks focus, and builds correctly addressed
//! per-window key and mouse events — and then dropped them, because there was
//! no wire type to send them as. This module is that wire type.
//!
//! ## What travels
//!
//! An [`InputEvent`] is a [`guitk::event::Event`] plus the window it is
//! addressed to. Keeping the payload the *client-facing* event type is
//! deliberate: the receiving app can hand what it decodes straight to its
//! widget tree, with no per-app translation layer to get subtly different from
//! every other app's.
//!
//! ## Wire format
//!
//! ```text
//! magic    : [u8;4] = b"INPT"
//! version  : u8     = INPUT_VERSION
//! flags    : u8     = 0 (reserved)
//! n_events : u32                       event count, little-endian
//!   per event:
//!     window : u64                     addressee window id
//!     tag    : u8                      EventTag
//!     payload: variable                see the per-tag encoders below
//! ```
//!
//! Scalars are little-endian and `f32` is the `to_le_bytes` of its IEEE-754
//! bits, matching the draw-command codec exactly — there is one set of
//! primitive conventions in this crate, not two.
//!
//! ## Robustness
//!
//! As with the draw-command decoder, malformed input is an error and never a
//! panic. Every read is bounds-checked; unknown tags, unknown key codes,
//! reserved modifier bits and non-scalar-value Unicode codepoints are all
//! reported as [`DecodeError`] variants naming the offending byte.

use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::{DecodeError, Reader, write_f32, write_u32, write_u64};

/// Input-frame magic: `b"INPT"`.
pub const INPUT_MAGIC: [u8; 4] = *b"INPT";

/// Input protocol version. Bump on any incompatible layout change; never reuse
/// a number.
pub const INPUT_VERSION: u8 = 1;

/// Input-frame header: magic + version + flags + event count.
const INPUT_HEADER_LEN: usize = 4 + 1 + 1 + 4;

/// Upper bound on events in a single frame, so a hostile sender cannot make the
/// decoder pre-allocate unboundedly. A frame is one compositor tick's worth of
/// input; even a pathological mouse burst is a few hundred.
pub const MAX_EVENTS_PER_FRAME: u32 = 1 << 16;

// ============================================================================
// The event as it crosses the wire
// ============================================================================

/// One input event, addressed to a window.
#[derive(Clone, Debug, PartialEq)]
pub struct InputEvent {
    /// The window this event belongs to. Mouse coordinates inside `event` are
    /// already window-local, so the client needs no knowledge of where it sits
    /// on screen to interpret them.
    pub window: u64,

    /// The event, in exactly the form the client's widget tree consumes.
    pub event: Event,

    /// The physical key position that produced a key event, if any.
    ///
    /// This is redundant for almost every client, and that is the point. The
    /// compositor translates scancodes to [`Key`] centrally (see
    /// `design-decisions.md` §456) so that one system keymap governs every app
    /// and a layout change takes effect everywhere at once. Carrying the raw
    /// code alongside costs four bytes and keeps the door open for the clients
    /// that genuinely want physical positions rather than letters — games that
    /// mean "the key left of S", and remapping utilities — so the central
    /// choice forecloses nothing.
    ///
    /// `None` for every non-key event. An `Option` that correlates with a
    /// variant of another field is not lovely, but the alternative — a
    /// `scancode` field on [`KeyEvent`] itself — would force a value at all
    /// ~500 places in this tree that build a `KeyEvent`, nearly all of them
    /// tests for which a scancode is meaningless noise.
    pub scancode: Option<u32>,
}

impl InputEvent {
    /// A non-keyboard event addressed to `window`.
    #[must_use]
    pub fn new(window: u64, event: Event) -> Self {
        Self {
            window,
            event,
            scancode: None,
        }
    }

    /// A key event addressed to `window`, carrying the physical key position.
    #[must_use]
    pub fn key(window: u64, event: KeyEvent, scancode: u32) -> Self {
        Self {
            window,
            event: Event::Key(event),
            scancode: Some(scancode),
        }
    }
}

// ============================================================================
// Tags
// ============================================================================

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EventTag {
    Mouse = 0x01,
    Key = 0x02,
    Resize = 0x03,
    FocusIn = 0x04,
    FocusOut = 0x05,
    CloseRequested = 0x06,
    Tick = 0x07,
    ScaleChanged = 0x08,
    Moved = 0x09,
}

impl EventTag {
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::Mouse),
            0x02 => Some(Self::Key),
            0x03 => Some(Self::Resize),
            0x04 => Some(Self::FocusIn),
            0x05 => Some(Self::FocusOut),
            0x06 => Some(Self::CloseRequested),
            0x07 => Some(Self::Tick),
            0x08 => Some(Self::ScaleChanged),
            0x09 => Some(Self::Moved),
            _ => None,
        }
    }
}

const MOUSE_PRESS: u8 = 0x01;
const MOUSE_RELEASE: u8 = 0x02;
const MOUSE_MOVE: u8 = 0x03;
const MOUSE_ENTER: u8 = 0x04;
const MOUSE_LEAVE: u8 = 0x05;
const MOUSE_SCROLL: u8 = 0x06;
const MOUSE_DOUBLE_CLICK: u8 = 0x07;

const BUTTON_LEFT: u8 = 0x01;
const BUTTON_RIGHT: u8 = 0x02;
const BUTTON_MIDDLE: u8 = 0x03;
const BUTTON_BACK: u8 = 0x04;
const BUTTON_FORWARD: u8 = 0x05;

/// Reserved for [`Key::Unknown`], whose raw code follows as a `u32`. Zero
/// rather than one past the table so that the table can grow without moving it.
const KEY_UNKNOWN: u8 = 0x00;

const MOD_SHIFT: u8 = 1 << 0;
const MOD_CTRL: u8 = 1 << 1;
const MOD_ALT: u8 = 1 << 2;
const MOD_SUPER: u8 = 1 << 3;
/// Bits with no meaning yet. Rejected rather than ignored: when a later version
/// adds a modifier, an old client should say so loudly rather than silently
/// treat `Ctrl+AltGr+K` as `Ctrl+K`.
const MOD_KNOWN: u8 = MOD_SHIFT | MOD_CTRL | MOD_ALT | MOD_SUPER;

// ============================================================================
// The key table
// ============================================================================

/// Generates both directions of the [`Key`] ↔ wire-byte mapping from a single
/// table.
///
/// Writing the two matches out by hand would be 88 lines each and one typo away
/// from a codec that encodes `Home` and decodes `End` — a bug that no round-trip
/// test written against the same wrong table would catch. Deriving both from one
/// list makes disagreement unrepresentable, and makes two further mistakes
/// compile errors rather than runtime surprises: a duplicated code byte is an
/// unreachable match arm, and a `Key` variant added later without a code makes
/// `key_code`'s match non-exhaustive.
macro_rules! key_table {
    ($($code:literal => $variant:ident,)*) => {
        /// The wire byte for a key, or `None` for [`Key::Unknown`], which
        /// carries its raw code separately.
        const fn key_code(k: Key) -> Option<u8> {
            match k {
                $(Key::$variant => Some($code),)*
                Key::Unknown(_) => None,
            }
        }

        /// The key for a wire byte, or `None` if this decoder does not know it.
        const fn key_from_code(b: u8) -> Option<Key> {
            match b {
                $($code => Some(Key::$variant),)*
                _ => None,
            }
        }
    };
}

key_table! {
    // Letters
    0x01 => A, 0x02 => B, 0x03 => C, 0x04 => D, 0x05 => E, 0x06 => F,
    0x07 => G, 0x08 => H, 0x09 => I, 0x0A => J, 0x0B => K, 0x0C => L,
    0x0D => M, 0x0E => N, 0x0F => O, 0x10 => P, 0x11 => Q, 0x12 => R,
    0x13 => S, 0x14 => T, 0x15 => U, 0x16 => V, 0x17 => W, 0x18 => X,
    0x19 => Y, 0x1A => Z,
    // Digits
    0x1B => Num0, 0x1C => Num1, 0x1D => Num2, 0x1E => Num3, 0x1F => Num4,
    0x20 => Num5, 0x21 => Num6, 0x22 => Num7, 0x23 => Num8, 0x24 => Num9,
    // Function keys
    0x25 => F1, 0x26 => F2, 0x27 => F3, 0x28 => F4, 0x29 => F5, 0x2A => F6,
    0x2B => F7, 0x2C => F8, 0x2D => F9, 0x2E => F10, 0x2F => F11, 0x30 => F12,
    // Navigation
    0x31 => Left, 0x32 => Right, 0x33 => Up, 0x34 => Down,
    0x35 => Home, 0x36 => End, 0x37 => PageUp, 0x38 => PageDown,
    // Editing
    0x39 => Backspace, 0x3A => Delete, 0x3B => Insert, 0x3C => Enter,
    0x3D => Tab, 0x3E => Escape, 0x3F => Space,
    // Modifiers as keys in their own right
    0x40 => LeftShift, 0x41 => RightShift, 0x42 => LeftCtrl, 0x43 => RightCtrl,
    0x44 => LeftAlt, 0x45 => RightAlt, 0x46 => LeftSuper, 0x47 => RightSuper,
    // Punctuation
    0x48 => Comma, 0x49 => Period, 0x4A => Semicolon, 0x4B => Colon,
    0x4C => Slash, 0x4D => Backslash, 0x4E => LeftBracket, 0x4F => RightBracket,
    0x50 => Minus, 0x51 => Equals, 0x52 => Apostrophe, 0x53 => Grave,
    // Locks and the rest
    0x54 => PrintScreen, 0x55 => ScrollLock, 0x56 => Pause, 0x57 => CapsLock,
    0x58 => NumLock,
}

// ============================================================================
// Encoding
// ============================================================================

/// Encode a batch of input events into a self-contained frame.
///
/// Batched rather than one frame per event because input arrives in bursts —
/// a mouse drag is a stream of moves — and a header per move would be mostly
/// header.
#[must_use]
pub fn encode_input_frame(events: &[InputEvent]) -> Vec<u8> {
    let mut out = Vec::with_capacity(INPUT_HEADER_LEN + events.len() * 24);
    encode_input_frame_into(&mut out, events);
    out
}

/// Encode into a caller-provided buffer, appending to whatever it holds. Lets a
/// sender reuse one allocation across frames.
pub fn encode_input_frame_into(out: &mut Vec<u8>, events: &[InputEvent]) {
    out.extend_from_slice(&INPUT_MAGIC);
    out.push(INPUT_VERSION);
    out.push(0); // flags
    // Saturating rather than panicking: a caller that somehow assembled four
    // billion events gets a truncated frame, not a downed compositor. The
    // decoder rejects anything past MAX_EVENTS_PER_FRAME regardless.
    write_u32(out, u32::try_from(events.len()).unwrap_or(u32::MAX));
    for ev in events {
        encode_event(out, ev);
    }
}

fn encode_event(out: &mut Vec<u8>, ev: &InputEvent) {
    write_u64(out, ev.window);
    match &ev.event {
        Event::Mouse(m) => {
            out.push(EventTag::Mouse as u8);
            write_f32(out, m.x);
            write_f32(out, m.y);
            encode_mouse_kind(out, &m.kind);
        }
        Event::Key(k) => {
            out.push(EventTag::Key as u8);
            encode_key(out, k.key);
            out.push(u8::from(k.pressed));
            out.push(encode_modifiers(k.modifiers));
            encode_optional_char(out, k.text);
            // Absent is legal here even though the compositor always supplies
            // one: a synthetic key event (a macro, a test, an accessibility
            // tool) has no physical key behind it and should not have to invent
            // a plausible-looking lie.
            match ev.scancode {
                Some(code) => {
                    out.push(1);
                    write_u32(out, code);
                }
                None => out.push(0),
            }
        }
        Event::Resize { width, height } => {
            out.push(EventTag::Resize as u8);
            write_u32(out, *width);
            write_u32(out, *height);
        }
        Event::FocusIn => out.push(EventTag::FocusIn as u8),
        Event::FocusOut => out.push(EventTag::FocusOut as u8),
        Event::CloseRequested => out.push(EventTag::CloseRequested as u8),
        Event::Tick { elapsed_ms } => {
            out.push(EventTag::Tick as u8);
            write_u64(out, *elapsed_ms);
        }
        Event::ScaleChanged { scale } => {
            out.push(EventTag::ScaleChanged as u8);
            write_f32(out, *scale);
        }
        Event::Moved { x, y } => {
            out.push(EventTag::Moved as u8);
            // Screen coordinates are signed — a window can sit left of or above
            // the origin on a multi-monitor desktop. Encode the raw
            // two's-complement bits, as the scene codec does for the same
            // reason.
            write_u32(out, x.cast_unsigned());
            write_u32(out, y.cast_unsigned());
        }
    }
}

fn encode_mouse_kind(out: &mut Vec<u8>, kind: &MouseEventKind) {
    match kind {
        MouseEventKind::Press(b) => {
            out.push(MOUSE_PRESS);
            out.push(button_code(*b));
        }
        MouseEventKind::Release(b) => {
            out.push(MOUSE_RELEASE);
            out.push(button_code(*b));
        }
        MouseEventKind::Move => out.push(MOUSE_MOVE),
        MouseEventKind::Enter => out.push(MOUSE_ENTER),
        MouseEventKind::Leave => out.push(MOUSE_LEAVE),
        MouseEventKind::Scroll { dx, dy } => {
            out.push(MOUSE_SCROLL);
            write_f32(out, *dx);
            write_f32(out, *dy);
        }
        MouseEventKind::DoubleClick(b) => {
            out.push(MOUSE_DOUBLE_CLICK);
            out.push(button_code(*b));
        }
    }
}

const fn button_code(b: MouseButton) -> u8 {
    match b {
        MouseButton::Left => BUTTON_LEFT,
        MouseButton::Right => BUTTON_RIGHT,
        MouseButton::Middle => BUTTON_MIDDLE,
        MouseButton::Back => BUTTON_BACK,
        MouseButton::Forward => BUTTON_FORWARD,
    }
}

const fn button_from_code(b: u8) -> Option<MouseButton> {
    match b {
        BUTTON_LEFT => Some(MouseButton::Left),
        BUTTON_RIGHT => Some(MouseButton::Right),
        BUTTON_MIDDLE => Some(MouseButton::Middle),
        BUTTON_BACK => Some(MouseButton::Back),
        BUTTON_FORWARD => Some(MouseButton::Forward),
        _ => None,
    }
}

fn encode_key(out: &mut Vec<u8>, key: Key) {
    match key {
        // The one variant `key_code` cannot name, because its wire form is
        // data rather than an identity.
        Key::Unknown(raw) => {
            out.push(KEY_UNKNOWN);
            write_u32(out, raw);
        }
        // `key_code` is total over every other variant — its match is
        // exhaustive, so a `Key` added without a table entry is a compile
        // error rather than something that reaches this arm. It is still
        // written as an unknown key rather than as nothing, because emitting
        // no bytes would desynchronise every event after it in the frame: a
        // key nobody pressed is a bug, a shifted frame is a catastrophe.
        other => {
            if let Some(code) = key_code(other) {
                out.push(code);
            } else {
                out.push(KEY_UNKNOWN);
                write_u32(out, 0);
            }
        }
    }
}

const fn encode_modifiers(m: Modifiers) -> u8 {
    let mut bits = 0u8;
    if m.shift {
        bits |= MOD_SHIFT;
    }
    if m.ctrl {
        bits |= MOD_CTRL;
    }
    if m.alt {
        bits |= MOD_ALT;
    }
    if m.super_key {
        bits |= MOD_SUPER;
    }
    bits
}

fn encode_optional_char(out: &mut Vec<u8>, c: Option<char>) {
    match c {
        Some(ch) => {
            out.push(1);
            write_u32(out, ch as u32);
        }
        None => out.push(0),
    }
}

// ============================================================================
// Decoding
// ============================================================================

/// Decode exactly one input frame. Returns the events and the bytes consumed.
pub fn decode_input_frame(input: &[u8]) -> Result<(Vec<InputEvent>, usize), DecodeError> {
    decode_internal(input)
}

/// Streaming decode: `Ok(None)` when the buffer holds only part of a frame, so
/// a caller reading from a socket can simply read more. Errors are reserved for
/// genuine corruption.
pub fn try_decode_input_frame(
    input: &[u8],
) -> Result<Option<(Vec<InputEvent>, usize)>, DecodeError> {
    match decode_internal(input) {
        Ok(v) => Ok(Some(v)),
        Err(DecodeError::UnexpectedEof) => Ok(None),
        Err(e) => Err(e),
    }
}

fn decode_internal(input: &[u8]) -> Result<(Vec<InputEvent>, usize), DecodeError> {
    let mut r = Reader::new(input);
    r.need(INPUT_HEADER_LEN)?;
    let magic = [r.buf[0], r.buf[1], r.buf[2], r.buf[3]];
    if magic != INPUT_MAGIC {
        return Err(DecodeError::BadMagic);
    }
    r.pos = 4;
    let ver = r.read_u8()?;
    if ver != INPUT_VERSION {
        return Err(DecodeError::UnsupportedVersion(ver));
    }
    let flags = r.read_u8()?;
    if flags != 0 {
        return Err(DecodeError::ReservedFlags(flags));
    }
    let n = r.read_u32()?;
    if n > MAX_EVENTS_PER_FRAME {
        return Err(DecodeError::TooManyEvents(n));
    }
    // Capacity from the *declared* count is safe only because the count is
    // bounded above; without that check this line is the allocation a hostile
    // sender would aim at.
    let mut events = Vec::with_capacity(n as usize);
    for _ in 0..n {
        events.push(decode_event(&mut r)?);
    }
    Ok((events, r.pos))
}

fn decode_event(r: &mut Reader<'_>) -> Result<InputEvent, DecodeError> {
    let window = r.read_u64()?;
    let tag_byte = r.read_u8()?;
    let tag = EventTag::from_byte(tag_byte).ok_or(DecodeError::BadTag(tag_byte))?;
    let (event, scancode) = match tag {
        EventTag::Mouse => {
            let x = r.read_f32()?;
            let y = r.read_f32()?;
            let kind = decode_mouse_kind(r)?;
            (Event::Mouse(MouseEvent { x, y, kind }), None)
        }
        EventTag::Key => {
            let key = decode_key(r)?;
            let pressed = r.read_u8()? != 0;
            let modifiers = decode_modifiers(r.read_u8()?)?;
            let text = decode_optional_char(r)?;
            let scancode = match r.read_u8()? {
                0 => None,
                1 => Some(r.read_u32()?),
                other => return Err(DecodeError::BadTag(other)),
            };
            (
                Event::Key(KeyEvent {
                    key,
                    pressed,
                    modifiers,
                    text,
                }),
                scancode,
            )
        }
        EventTag::Resize => {
            let width = r.read_u32()?;
            let height = r.read_u32()?;
            (Event::Resize { width, height }, None)
        }
        EventTag::FocusIn => (Event::FocusIn, None),
        EventTag::FocusOut => (Event::FocusOut, None),
        EventTag::CloseRequested => (Event::CloseRequested, None),
        EventTag::Tick => (
            Event::Tick {
                elapsed_ms: r.read_u64()?,
            },
            None,
        ),
        EventTag::ScaleChanged => (
            Event::ScaleChanged {
                scale: r.read_f32()?,
            },
            None,
        ),
        EventTag::Moved => (
            Event::Moved {
                x: r.read_u32()?.cast_signed(),
                y: r.read_u32()?.cast_signed(),
            },
            None,
        ),
    };
    Ok(InputEvent {
        window,
        event,
        scancode,
    })
}

fn decode_mouse_kind(r: &mut Reader<'_>) -> Result<MouseEventKind, DecodeError> {
    let kind = r.read_u8()?;
    Ok(match kind {
        MOUSE_PRESS => MouseEventKind::Press(decode_button(r)?),
        MOUSE_RELEASE => MouseEventKind::Release(decode_button(r)?),
        MOUSE_MOVE => MouseEventKind::Move,
        MOUSE_ENTER => MouseEventKind::Enter,
        MOUSE_LEAVE => MouseEventKind::Leave,
        MOUSE_SCROLL => MouseEventKind::Scroll {
            dx: r.read_f32()?,
            dy: r.read_f32()?,
        },
        MOUSE_DOUBLE_CLICK => MouseEventKind::DoubleClick(decode_button(r)?),
        other => return Err(DecodeError::BadMouseKind(other)),
    })
}

fn decode_button(r: &mut Reader<'_>) -> Result<MouseButton, DecodeError> {
    let b = r.read_u8()?;
    button_from_code(b).ok_or(DecodeError::BadMouseButton(b))
}

fn decode_key(r: &mut Reader<'_>) -> Result<Key, DecodeError> {
    let code = r.read_u8()?;
    if code == KEY_UNKNOWN {
        return Ok(Key::Unknown(r.read_u32()?));
    }
    key_from_code(code).ok_or(DecodeError::BadKey(code))
}

const fn decode_modifiers(bits: u8) -> Result<Modifiers, DecodeError> {
    if bits & !MOD_KNOWN != 0 {
        return Err(DecodeError::ReservedFlags(bits));
    }
    Ok(Modifiers {
        shift: bits & MOD_SHIFT != 0,
        ctrl: bits & MOD_CTRL != 0,
        alt: bits & MOD_ALT != 0,
        super_key: bits & MOD_SUPER != 0,
    })
}

fn decode_optional_char(r: &mut Reader<'_>) -> Result<Option<char>, DecodeError> {
    match r.read_u8()? {
        0 => Ok(None),
        1 => {
            let raw = r.read_u32()?;
            // Surrogates and out-of-range values are rejected rather than
            // replaced: silently substituting U+FFFD would insert a character
            // the user never typed into whatever document is focused.
            char::from_u32(raw)
                .map(Some)
                .ok_or(DecodeError::BadChar(raw))
        }
        other => Err(DecodeError::BadTag(other)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use super::*;

    fn roundtrip(events: &[InputEvent]) -> Vec<InputEvent> {
        let bytes = encode_input_frame(events);
        let (decoded, consumed) = decode_input_frame(&bytes).unwrap();
        assert_eq!(
            consumed,
            bytes.len(),
            "decoder must consume exactly the frame it was given"
        );
        decoded
    }

    fn key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    #[test]
    fn an_empty_frame_is_a_header_and_nothing_else() {
        let bytes = encode_input_frame(&[]);
        assert_eq!(bytes.len(), INPUT_HEADER_LEN);
        let (events, consumed) = decode_input_frame(&bytes).unwrap();
        assert!(events.is_empty());
        assert_eq!(consumed, INPUT_HEADER_LEN);
    }

    #[test]
    fn every_key_in_the_table_survives_the_round_trip() {
        // The point of the exhaustive sweep is the encode/decode *pairing*: a
        // table entry that maps two keys to one byte, or two bytes to one key,
        // shows up here as a key coming back as a different key.
        let mut events = Vec::new();
        for code in 1..=u8::MAX {
            if let Some(k) = key_from_code(code) {
                events.push(InputEvent::key(1, key(k), u32::from(code)));
            }
        }
        assert_eq!(events.len(), 88, "the key table should have 88 named keys");
        let decoded = roundtrip(&events);
        assert_eq!(decoded, events);
    }

    #[test]
    fn an_unmapped_key_carries_its_raw_code_through() {
        let ev = InputEvent::key(7, key(Key::Unknown(0xDEAD_BEEF)), 42);
        assert_eq!(roundtrip(std::slice::from_ref(&ev)), vec![ev]);
    }

    #[test]
    fn every_mouse_kind_and_button_survives_the_round_trip() {
        let buttons = [
            MouseButton::Left,
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Back,
            MouseButton::Forward,
        ];
        let mut kinds = vec![
            MouseEventKind::Move,
            MouseEventKind::Enter,
            MouseEventKind::Leave,
            MouseEventKind::Scroll { dx: -2.5, dy: 13.0 },
        ];
        for b in buttons {
            kinds.push(MouseEventKind::Press(b));
            kinds.push(MouseEventKind::Release(b));
            kinds.push(MouseEventKind::DoubleClick(b));
        }
        let events: Vec<_> = kinds
            .into_iter()
            .map(|kind| {
                InputEvent::new(
                    3,
                    Event::Mouse(MouseEvent {
                        x: 12.5,
                        y: -4.25,
                        kind,
                    }),
                )
            })
            .collect();
        assert_eq!(roundtrip(&events), events);
    }

    #[test]
    fn every_non_input_event_variant_survives_the_round_trip() {
        let events: Vec<_> = [
            Event::Resize {
                width: 1920,
                height: 1080,
            },
            Event::FocusIn,
            Event::FocusOut,
            Event::CloseRequested,
            Event::Tick { elapsed_ms: 16 },
            Event::ScaleChanged { scale: 1.5 },
            Event::Moved { x: 100, y: 200 },
        ]
        .into_iter()
        .map(|e| InputEvent::new(9, e))
        .collect();
        assert_eq!(roundtrip(&events), events);
    }

    #[test]
    fn a_window_left_of_the_origin_is_not_reported_as_a_distant_one() {
        // Screen coordinates are signed: on a multi-monitor desktop the primary
        // display's origin is not the leftmost point. Read as unsigned, x = -1
        // would come back as 4294967295.
        let events = vec![InputEvent::new(1, Event::Moved { x: -1920, y: -12 })];
        assert_eq!(roundtrip(&events), events);
    }

    #[test]
    fn every_modifier_combination_survives_the_round_trip() {
        // All sixteen, not a sample: a bit-shift typo that swapped ctrl and alt
        // would pass any test that only ever set one of them.
        let mut events = Vec::new();
        for bits in 0..16u8 {
            let modifiers = Modifiers {
                shift: bits & 1 != 0,
                ctrl: bits & 2 != 0,
                alt: bits & 4 != 0,
                super_key: bits & 8 != 0,
            };
            events.push(InputEvent::key(
                1,
                KeyEvent {
                    key: Key::K,
                    pressed: true,
                    modifiers,
                    text: None,
                },
                0,
            ));
        }
        assert_eq!(roundtrip(&events), events);
    }

    #[test]
    fn a_typed_character_survives_including_astral_ones() {
        for ch in ['a', 'é', '中', '🙂', '\u{10FFFF}'] {
            let ev = InputEvent::key(
                1,
                KeyEvent {
                    key: Key::A,
                    pressed: true,
                    modifiers: Modifiers::shift(),
                    text: Some(ch),
                },
                30,
            );
            assert_eq!(
                roundtrip(std::slice::from_ref(&ev)),
                vec![ev],
                "char {ch:?}"
            );
        }
    }

    #[test]
    fn a_key_release_is_distinguishable_from_a_press() {
        let press = InputEvent::key(1, key(Key::Space), 57);
        let mut release = press.clone();
        release.event = Event::Key(KeyEvent {
            key: Key::Space,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        });
        let decoded = roundtrip(&[press.clone(), release.clone()]);
        assert_eq!(decoded, vec![press, release]);
    }

    #[test]
    fn a_synthetic_key_event_may_have_no_scancode() {
        let ev = InputEvent {
            window: 4,
            event: Event::Key(key(Key::Enter)),
            scancode: None,
        };
        assert_eq!(roundtrip(std::slice::from_ref(&ev)), vec![ev]);
    }

    #[test]
    fn window_ids_are_preserved_per_event_not_per_frame() {
        let events: Vec<_> = [1u64, 2, u64::MAX]
            .into_iter()
            .map(|w| InputEvent::new(w, Event::FocusIn))
            .collect();
        let decoded = roundtrip(&events);
        assert_eq!(
            decoded.iter().map(|e| e.window).collect::<Vec<_>>(),
            vec![1, 2, u64::MAX]
        );
    }

    #[test]
    fn frames_decode_back_to_back_from_one_buffer() {
        // What a socket reader actually does: two frames concatenated, and the
        // consumed count is the only thing telling it where the second starts.
        let mut buf = encode_input_frame(&[InputEvent::new(1, Event::FocusIn)]);
        let first_len = buf.len();
        encode_input_frame_into(&mut buf, &[InputEvent::new(2, Event::FocusOut)]);

        let (a, used_a) = decode_input_frame(&buf).unwrap();
        assert_eq!(used_a, first_len);
        assert_eq!(a[0].window, 1);
        let (b, used_b) = decode_input_frame(&buf[used_a..]).unwrap();
        assert_eq!(used_a + used_b, buf.len());
        assert_eq!(b[0].window, 2);
    }

    #[test]
    fn a_partial_frame_is_incomplete_rather_than_corrupt() {
        let bytes = encode_input_frame(&[InputEvent::key(1, key(Key::A), 30)]);
        for cut in 0..bytes.len() {
            assert_eq!(
                try_decode_input_frame(&bytes[..cut]),
                Ok(None),
                "truncating to {cut} bytes should read as incomplete"
            );
        }
        assert!(try_decode_input_frame(&bytes).unwrap().is_some());
    }

    #[test]
    fn a_foreign_frame_is_rejected_by_magic() {
        // Draw frames travel the other way over a duplex transport; a mixed-up
        // direction must fail loudly at the first four bytes, not decode into
        // plausible nonsense.
        let mut bytes = encode_input_frame(&[]);
        bytes[..4].copy_from_slice(&crate::MAGIC);
        assert_eq!(decode_input_frame(&bytes), Err(DecodeError::BadMagic));
    }

    #[test]
    fn a_future_version_is_rejected_rather_than_guessed_at() {
        let mut bytes = encode_input_frame(&[]);
        bytes[4] = INPUT_VERSION + 1;
        assert_eq!(
            decode_input_frame(&bytes),
            Err(DecodeError::UnsupportedVersion(INPUT_VERSION + 1))
        );
    }

    #[test]
    fn reserved_flag_bits_are_rejected() {
        let mut bytes = encode_input_frame(&[]);
        bytes[5] = 0x80;
        assert_eq!(
            decode_input_frame(&bytes),
            Err(DecodeError::ReservedFlags(0x80))
        );
    }

    #[test]
    fn an_absurd_event_count_is_rejected_before_allocating() {
        let mut bytes = encode_input_frame(&[]);
        bytes[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode_input_frame(&bytes),
            Err(DecodeError::TooManyEvents(u32::MAX))
        );
    }

    #[test]
    fn an_unknown_event_tag_names_the_byte() {
        let mut bytes = encode_input_frame(&[InputEvent::new(1, Event::FocusIn)]);
        // window id occupies the eight bytes after the header.
        let tag_at = INPUT_HEADER_LEN + 8;
        bytes[tag_at] = 0xFE;
        assert_eq!(decode_input_frame(&bytes), Err(DecodeError::BadTag(0xFE)));
    }

    #[test]
    fn an_unknown_key_code_names_the_byte() {
        let bytes = encode_input_frame(&[InputEvent::key(1, key(Key::A), 30)]);
        let mut bytes = bytes;
        // header, window (8), event tag (1), then the key code.
        let key_at = INPUT_HEADER_LEN + 8 + 1;
        bytes[key_at] = 0xF0;
        assert_eq!(decode_input_frame(&bytes), Err(DecodeError::BadKey(0xF0)));
    }

    #[test]
    fn an_unknown_mouse_button_names_the_byte() {
        let mut bytes = encode_input_frame(&[InputEvent::new(
            1,
            Event::Mouse(MouseEvent {
                x: 0.0,
                y: 0.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        )]);
        // header, window (8), tag (1), x (4), y (4), kind (1), then the button.
        let button_at = INPUT_HEADER_LEN + 8 + 1 + 4 + 4 + 1;
        bytes[button_at] = 0x09;
        assert_eq!(
            decode_input_frame(&bytes),
            Err(DecodeError::BadMouseButton(0x09))
        );
    }

    #[test]
    fn an_unknown_mouse_kind_names_the_byte() {
        let mut bytes = encode_input_frame(&[InputEvent::new(
            1,
            Event::Mouse(MouseEvent {
                x: 0.0,
                y: 0.0,
                kind: MouseEventKind::Move,
            }),
        )]);
        let kind_at = INPUT_HEADER_LEN + 8 + 1 + 4 + 4;
        bytes[kind_at] = 0x7F;
        assert_eq!(
            decode_input_frame(&bytes),
            Err(DecodeError::BadMouseKind(0x7F))
        );
    }

    #[test]
    fn an_unassigned_modifier_bit_is_rejected_not_ignored() {
        // A frame from a future compositor that added AltGr must not silently
        // arrive as the same chord without it.
        assert_eq!(
            decode_modifiers(0x10),
            Err(DecodeError::ReservedFlags(0x10))
        );
        assert_eq!(
            decode_modifiers(MOD_KNOWN).unwrap(),
            Modifiers {
                shift: true,
                ctrl: true,
                alt: true,
                super_key: true,
            }
        );
    }

    #[test]
    fn a_non_character_codepoint_is_rejected_not_substituted() {
        let mut bytes = encode_input_frame(&[InputEvent::key(
            1,
            KeyEvent {
                key: Key::A,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: Some('a'),
            },
            30,
        )]);
        // header, window (8), tag (1), key (1), pressed (1), mods (1),
        // text-present (1), then the codepoint.
        let ch_at = INPUT_HEADER_LEN + 8 + 1 + 1 + 1 + 1 + 1;
        // A lone surrogate: valid UTF-16, never a Rust `char`.
        bytes[ch_at..ch_at + 4].copy_from_slice(&0xD800u32.to_le_bytes());
        assert_eq!(
            decode_input_frame(&bytes),
            Err(DecodeError::BadChar(0xD800))
        );
    }

    #[test]
    fn no_truncation_of_any_frame_ever_panics() {
        // The decoder is the first thing to touch bytes from another process,
        // so "never panics on malformed input" is a security property, not a
        // tidiness one.
        let events = vec![
            InputEvent::key(1, key(Key::A), 30),
            InputEvent::new(
                2,
                Event::Mouse(MouseEvent {
                    x: 1.0,
                    y: 2.0,
                    kind: MouseEventKind::Scroll { dx: 1.0, dy: 2.0 },
                }),
            ),
            InputEvent::new(3, Event::ScaleChanged { scale: 2.0 }),
        ];
        let bytes = encode_input_frame(&events);
        for cut in 0..=bytes.len() {
            let _ = try_decode_input_frame(&bytes[..cut]);
        }
        // And with every single byte corrupted in turn.
        for i in 0..bytes.len() {
            let mut damaged = bytes.clone();
            damaged[i] = damaged[i].wrapping_add(0x55);
            let _ = decode_input_frame(&damaged);
        }
    }
}
