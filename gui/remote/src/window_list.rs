//! Window-list protocol — how a shell learns what is on the desktop.
//!
//! Every other frame this crate defines is about *one* window: `ORDR` and
//! `SURF` carry a picture for one, `INPT` carries an event addressed to one,
//! `CREQ`/`CRSP` ask about one. That vocabulary is complete for an application,
//! which has no business knowing that anyone else's windows exist, and it is
//! exactly one thing short for a **shell** — a taskbar has to list the windows
//! it did not open, and had no way to ask.
//!
//! Stacking layers ([`Layer`](crate::control::Layer)) fixed the other half of
//! the same problem: a taskbar can now *stay* in front of the windows it is
//! supposed to list. This module is what lets it know what they are.
//!
//! ## Not [`scene`](crate::scene), which looks similar and is not
//!
//! A `SCEN` frame also carries "all the windows", and is not a substitute. It
//! is a *remote viewer's* frame: it carries each window's geometry and its
//! **draw commands**, because the viewer's job is to reproduce the pixels. A
//! taskbar wants none of that and does want what `SCEN` has no room for — the
//! title, whether the window is minimised, which one has focus. The two frames
//! answer different questions about the same window set, and collapsing them
//! would give each consumer most of a frame it has to skip past.
//!
//! ## Push, not poll
//!
//! A client subscribes once, with
//! [`RequestBody::SubscribeWindowList`](crate::control::RequestBody::SubscribeWindowList),
//! and the compositor sends a `WLST` frame whenever the list it would send
//! differs from the one that client last received. There is no query form.
//!
//! A poll would have to run at some rate, and every choice is wrong: slower
//! than the change rate and the taskbar lags, faster and an idle desktop pays
//! for a round trip per frame to be told nothing happened. Pushing on change
//! costs nothing at all when nothing changes, which is what a desktop mostly
//! does.
//!
//! Each frame is a **complete snapshot**, not a delta. The list is tens of
//! entries at the very most and a delta stream has to be applied in order
//! against state the two ends must agree about; a snapshot is idempotent, and a
//! receiver that drops one is corrected by the next rather than desynchronised
//! forever.
//!
//! ## Wire format
//!
//! ```text
//! magic    : [u8;4] = b"WLST"
//! version  : u8     = WINDOW_LIST_VERSION
//! flags    : u8     = 0 (reserved)
//! n_win    : u32                       window count, bottom→top stacking order
//!   per window:
//!     id      : u64                    compositor's window id
//!     pid     : u64                    owning process
//!     layer   : u8                     Layer::as_byte
//!     state   : u8                     bitfield, see STATE_* below
//!     title   : u32 length + UTF-8 bytes
//! ```
//!
//! Scalars are little-endian, matching every other codec in this crate.

use crate::control::Layer;
use crate::{DecodeError, Reader, capacity_hint, write_string, write_u32, write_u64};

/// Window-list frame magic: `b"WLST"`.
pub const WINDOW_LIST_MAGIC: [u8; 4] = *b"WLST";

/// Window-list protocol version. Bump on any incompatible layout change; never
/// reuse a number.
pub const WINDOW_LIST_VERSION: u8 = 1;

/// Window-list header: magic + version + flags + window count.
const WINDOW_LIST_HEADER_LEN: usize = 4 + 1 + 1 + 4;

/// Upper bound on entries in one list, so a hostile or corrupt sender cannot
/// make the decoder pre-allocate unboundedly.
///
/// Deliberately the same order as [`scene::MAX_WINDOWS_PER_FRAME`] — both are
/// bounding the same population — but its own constant, because the two frames
/// are free to diverge and a shared constant would silently couple them.
///
/// [`scene::MAX_WINDOWS_PER_FRAME`]: crate::scene::MAX_WINDOWS_PER_FRAME
pub const MAX_WINDOWS_PER_LIST: u32 = 1 << 16;

const STATE_VISIBLE: u8 = 1 << 0;
const STATE_MINIMIZED: u8 = 1 << 1;
const STATE_MAXIMIZED: u8 = 1 << 2;
const STATE_FOCUSED: u8 = 1 << 3;

/// Bits with a meaning. A set bit outside this mask is rejected rather than
/// ignored, for the reason `input::MOD_KNOWN` gives: when a later version adds
/// a state, an older shell should say so loudly rather than quietly draw a
/// window as though it did not have it.
const STATE_KNOWN: u8 = STATE_VISIBLE | STATE_MINIMIZED | STATE_MAXIMIZED | STATE_FOCUSED;

/// One window, as the rest of the desktop sees it.
///
/// Everything a taskbar or window switcher needs to draw a row, and nothing
/// else. Notably absent is geometry: a shell that wants to move a window asks
/// the compositor to, and one that wants to draw a live thumbnail needs the
/// pixels, which is [`scene`](crate::scene)'s job and not this frame's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    /// The compositor's id for the window, as used by every other request.
    pub id: u64,
    /// The process that owns it, so a shell can group a program's windows.
    pub pid: u64,
    /// Which stacking band it lives in.
    ///
    /// The field that makes the list usable: a taskbar must not list itself,
    /// the wallpaper, or the start menu, and `Layer` is how it tells those
    /// apart from the application windows it exists to list.
    pub layer: Layer,
    /// The window's title, as last set by its client.
    pub title: String,
    /// Whether the window is mapped at all. A hidden window keeps its id and
    /// its place in the stack, and should not get a taskbar button.
    pub visible: bool,
    /// Whether it is minimised.
    pub minimized: bool,
    /// Whether it is maximised.
    pub maximized: bool,
    /// Whether it currently holds keyboard focus. At most one entry has this.
    pub focused: bool,
}

impl WindowInfo {
    /// An ordinary visible, unfocused window — the base a builder-ish caller
    /// (mostly a test) can adjust.
    #[must_use]
    pub fn new(id: u64, pid: u64, title: impl Into<String>) -> Self {
        Self {
            id,
            pid,
            layer: Layer::Normal,
            title: title.into(),
            visible: true,
            minimized: false,
            maximized: false,
            focused: false,
        }
    }

    fn state_byte(&self) -> u8 {
        let mut bits = 0u8;
        if self.visible {
            bits |= STATE_VISIBLE;
        }
        if self.minimized {
            bits |= STATE_MINIMIZED;
        }
        if self.maximized {
            bits |= STATE_MAXIMIZED;
        }
        if self.focused {
            bits |= STATE_FOCUSED;
        }
        bits
    }
}

// ============================================================================
// Encoding
// ============================================================================

/// Encode a window list as one self-contained `WLST` frame.
///
/// `windows` is in bottom-to-top stacking order, which is the order
/// [`scene`](crate::scene) uses and the order the compositor keeps internally.
/// A shell that wants most-recent-first reverses it; one that wants a stable
/// button order sorts by [`WindowInfo::id`], which is monotonic in creation.
#[must_use]
pub fn encode_window_list(windows: &[WindowInfo]) -> Vec<u8> {
    let mut out = Vec::with_capacity(capacity_hint(WINDOW_LIST_HEADER_LEN, windows.len(), 40));
    encode_window_list_into(&mut out, windows);
    out
}

/// Encode into a caller-provided buffer, appending to whatever it holds. Lets
/// the compositor reuse one allocation across ticks.
pub fn encode_window_list_into(out: &mut Vec<u8>, windows: &[WindowInfo]) {
    out.extend_from_slice(&WINDOW_LIST_MAGIC);
    out.push(WINDOW_LIST_VERSION);
    out.push(0); // flags
    // Saturating rather than panicking, as everywhere else in this crate: the
    // decoder rejects anything past MAX_WINDOWS_PER_LIST regardless.
    write_u32(out, u32::try_from(windows.len()).unwrap_or(u32::MAX));
    for win in windows {
        write_u64(out, win.id);
        write_u64(out, win.pid);
        out.push(win.layer.as_byte());
        out.push(win.state_byte());
        write_string(out, &win.title);
    }
}

// ============================================================================
// Decoding
// ============================================================================

/// Decode a window-list frame, returning it and the bytes it occupied.
///
/// # Errors
///
/// [`DecodeError::BadMagic`] if the frame is not `WLST`,
/// [`DecodeError::UnsupportedVersion`] for a version this build does not know,
/// [`DecodeError::ReservedFlags`] for reserved bits set in either the frame
/// flags or a window's state byte, [`DecodeError::BadTag`] for a stacking layer
/// this build does not know, [`DecodeError::TooManyListedWindows`] for a count
/// that would allocate absurdly, and [`DecodeError::UnexpectedEof`] for a short
/// buffer.
pub fn decode_window_list(input: &[u8]) -> Result<(Vec<WindowInfo>, usize), DecodeError> {
    decode_internal(input)
}

/// Streaming form: `Ok(None)` when the buffer holds only part of a frame, so a
/// caller reading from a transport can simply read more.
///
/// # Errors
///
/// As [`decode_window_list`], except that a short buffer is `Ok(None)`.
pub fn try_decode_window_list(
    input: &[u8],
) -> Result<Option<(Vec<WindowInfo>, usize)>, DecodeError> {
    match decode_internal(input) {
        Ok(v) => Ok(Some(v)),
        Err(DecodeError::UnexpectedEof) => Ok(None),
        Err(e) => Err(e),
    }
}

fn decode_internal(input: &[u8]) -> Result<(Vec<WindowInfo>, usize), DecodeError> {
    let mut r = Reader::new(input);
    // The whole header is required before any of it is read, so a buffer
    // holding part of one reports `UnexpectedEof` — "incomplete, read more" —
    // rather than failing on whichever field the truncation landed in.
    r.need(WINDOW_LIST_HEADER_LEN)?;
    r.expect_magic(WINDOW_LIST_MAGIC)?;
    let ver = r.read_u8()?;
    if ver != WINDOW_LIST_VERSION {
        return Err(DecodeError::UnsupportedVersion(ver));
    }
    let flags = r.read_u8()?;
    if flags != 0 {
        return Err(DecodeError::ReservedFlags(flags));
    }
    let n = r.read_u32()?;
    if n > MAX_WINDOWS_PER_LIST {
        return Err(DecodeError::TooManyListedWindows(n));
    }
    // Capacity from the declared count is only safe because the count was just
    // bounded; without that check this line is what a hostile sender aims at.
    let mut windows = Vec::with_capacity(n as usize);
    for _ in 0..n {
        windows.push(decode_one(&mut r)?);
    }
    Ok((windows, r.position()))
}

fn decode_one(r: &mut Reader<'_>) -> Result<WindowInfo, DecodeError> {
    let id = r.read_u64()?;
    let pid = r.read_u64()?;
    let layer_byte = r.read_u8()?;
    let layer = Layer::from_byte(layer_byte).ok_or(DecodeError::BadTag(layer_byte))?;
    let state = r.read_u8()?;
    if state & !STATE_KNOWN != 0 {
        return Err(DecodeError::ReservedFlags(state));
    }
    let title = r.read_string()?;
    Ok(WindowInfo {
        id,
        pid,
        layer,
        title,
        visible: state & STATE_VISIBLE != 0,
        minimized: state & STATE_MINIMIZED != 0,
        maximized: state & STATE_MAXIMIZED != 0,
        focused: state & STATE_FOCUSED != 0,
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    fn sample() -> Vec<WindowInfo> {
        vec![
            WindowInfo {
                id: 1,
                pid: 100,
                layer: Layer::Background,
                title: "Wallpaper".to_string(),
                visible: true,
                minimized: false,
                maximized: false,
                focused: false,
            },
            WindowInfo {
                id: 2,
                pid: 200,
                layer: Layer::Normal,
                title: "Editor — notes.txt".to_string(),
                visible: true,
                minimized: false,
                maximized: true,
                focused: true,
            },
            WindowInfo {
                id: 3,
                pid: 200,
                layer: Layer::Normal,
                title: String::new(),
                visible: false,
                minimized: true,
                maximized: false,
                focused: false,
            },
            WindowInfo {
                id: 4,
                pid: 300,
                layer: Layer::Overlay,
                title: "Taskbar".to_string(),
                visible: true,
                minimized: false,
                maximized: false,
                focused: false,
            },
        ]
    }

    #[test]
    fn a_list_survives_the_round_trip_field_for_field() {
        // Every boolean is set on at least one entry and clear on another, so a
        // codec that dropped one — or transposed two — cannot pass.
        let windows = sample();
        let bytes = encode_window_list(&windows);
        let (back, used) = decode_window_list(&bytes).expect("decodes");
        assert_eq!(back, windows);
        assert_eq!(used, bytes.len(), "the frame consumes exactly its bytes");
    }

    #[test]
    fn an_empty_list_is_a_legal_frame() {
        // A desktop with nothing open is a real state, and is the state a shell
        // must be told about when the last window closes. Encoding it as
        // "nothing to send" would leave the taskbar showing the last window
        // forever.
        let bytes = encode_window_list(&[]);
        let (back, used) = decode_window_list(&bytes).expect("decodes");
        assert!(back.is_empty());
        assert_eq!(used, bytes.len());
    }

    #[test]
    fn a_non_ascii_title_is_not_mangled() {
        let windows = vec![WindowInfo::new(9, 1, "日本語 — файл — 🗔")];
        let bytes = encode_window_list(&windows);
        let (back, _) = decode_window_list(&bytes).expect("decodes");
        assert_eq!(back[0].title, "日本語 — файл — 🗔");
    }

    #[test]
    fn every_prefix_reads_as_incomplete_rather_than_corrupt() {
        // What a transport actually delivers on its first read of a big frame.
        let bytes = encode_window_list(&sample());
        for n in 0..bytes.len() {
            assert!(
                matches!(try_decode_window_list(&bytes[..n]), Ok(None)),
                "a {n}-byte prefix must read as incomplete"
            );
        }
        assert!(try_decode_window_list(&bytes).unwrap().is_some());
    }

    #[test]
    fn a_foreign_magic_is_rejected() {
        let mut bytes = encode_window_list(&sample());
        bytes[0] = b'X';
        assert_eq!(decode_window_list(&bytes), Err(DecodeError::BadMagic));
    }

    #[test]
    fn a_version_this_build_does_not_know_is_refused() {
        let mut bytes = encode_window_list(&sample());
        bytes[4] = WINDOW_LIST_VERSION + 1;
        assert_eq!(
            decode_window_list(&bytes),
            Err(DecodeError::UnsupportedVersion(WINDOW_LIST_VERSION + 1))
        );
    }

    #[test]
    fn a_state_bit_with_no_meaning_is_refused_rather_than_ignored() {
        // The point of STATE_KNOWN. A newer compositor that grows a fifth state
        // must not have an older shell silently drop it.
        let windows = vec![WindowInfo::new(1, 1, "W")];
        let mut bytes = encode_window_list(&windows);
        // header (10) + id (8) + pid (8) + layer (1) = state byte at 27.
        let state_at = WINDOW_LIST_HEADER_LEN + 8 + 8 + 1;
        assert_eq!(bytes[state_at], STATE_VISIBLE, "located the state byte");
        bytes[state_at] |= 1 << 6;
        assert!(matches!(
            decode_window_list(&bytes),
            Err(DecodeError::ReservedFlags(_))
        ));
    }

    #[test]
    fn a_layer_this_build_does_not_know_is_refused() {
        let windows = vec![WindowInfo::new(1, 1, "W")];
        let mut bytes = encode_window_list(&windows);
        let layer_at = WINDOW_LIST_HEADER_LEN + 8 + 8;
        bytes[layer_at] = 0x7F;
        assert_eq!(decode_window_list(&bytes), Err(DecodeError::BadTag(0x7F)));
    }

    #[test]
    fn an_absurd_count_is_refused_before_anything_is_allocated() {
        // Ten bytes claiming four billion windows: the allocation a hostile
        // sender would aim at if the bound were not checked first.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&WINDOW_LIST_MAGIC);
        bytes.push(WINDOW_LIST_VERSION);
        bytes.push(0);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode_window_list(&bytes),
            Err(DecodeError::TooManyListedWindows(u32::MAX))
        );
    }

    #[test]
    fn encoding_into_a_shared_buffer_appends_rather_than_replaces() {
        // How the compositor uses it: one buffer, many frames.
        let mut buf = b"already here".to_vec();
        encode_window_list_into(&mut buf, &sample());
        let (back, used) = decode_window_list(&buf[12..]).expect("decodes");
        assert_eq!(back, sample());
        assert_eq!(12 + used, buf.len());
    }
}
