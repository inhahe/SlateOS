//! System-tray protocol — how a program puts an icon in the shell's tray.
//!
//! The tray is the strip at the end of the taskbar: clock, network, volume,
//! and whatever a running program wants to show there. Everything in it except
//! the shell's own items has to come from another process, and until now
//! nothing could put one there.
//!
//! ## Why this module exists, and what was already in the tree
//!
//! Four things modelled a tray icon and no two of them met: `apps/systray`
//! (which draws a tray with built-in icons and whose `register_icon` has no
//! caller but its own tests), `gui/desktop`'s taskbar tray (the shell's own
//! items), `gui/desktop/src/tray_dnd.rs` (drag, drop, pin, reorder, for icons
//! that could not exist), and `kernel/src/fs/systray.rs` (persistence). Every
//! *part* existed and no process boundary was crossed anywhere in it.
//!
//! `design-decisions.md` 842 settled where icons go: the shell's tray, because
//! 815's rule is "is this something the desktop shows you, or a screen you
//! open?" and a tray is on screen the whole time. This module is the road from
//! a program to that tray.
//!
//! ## Shaped after [`window_list`](crate::window_list), deliberately
//!
//! That module's first paragraph is this one with "windows" in it: a taskbar
//! has to list the windows it did not open and had no way to ask. A tray has
//! to show icons it did not create. The same answers apply:
//!
//! * **Push, not poll.** A shell subscribes once with
//!   [`RequestBody::SubscribeTrayIcons`](crate::control::RequestBody::SubscribeTrayIcons)
//!   and the compositor sends a `TRAY` frame whenever the list changes. There
//!   is no query form, so there is no way to ask and get a stale answer.
//! * **The compositor owns the registry**, not the shell. A shell that held it
//!   would lose every icon when it restarted, and a program would have to know
//!   the shell had come back. The compositor already outlives both.
//! * **A departing client's icons are reaped**, exactly as its windows are. A
//!   crash must not leave an icon nobody can remove — that is the tray
//!   equivalent of a window with no close button.
//!
//! ## What an icon carries, and what it deliberately does not
//!
//! A glyph and a tooltip. **Not a bitmap**: every icon this tree draws is a
//! text glyph, because the render protocol carries draw commands rather than
//! pixels and a picture would need the upload path. **Not a colour**: the tray
//! is shell chrome and its contrast is the shell's problem — a program that
//! chose its own would be choosing against a palette it cannot see, which is
//! the defect `Palette::ink` exists to prevent.
//!
//! **Not a position, either.** Order is the shell's, which is what lets a user
//! rearrange the tray; a program that could pin itself leftmost would take that
//! from them.

use crate::{DecodeError, Reader, capacity_hint, write_string, write_u32, write_u64};

/// Tray-list frame magic: `b"TRAY"`.
pub const TRAY_MAGIC: [u8; 4] = *b"TRAY";

/// The version of the tray frame this build writes and understands.
pub const TRAY_VERSION: u8 = 1;

/// Magic, version, flags, and the icon count.
pub const TRAY_HEADER_LEN: usize = 4 + 1 + 1 + 4;

/// The most icons one frame may describe.
///
/// A bound rather than a policy: the decoder refuses a count past this before
/// allocating for it, so a corrupt or hostile length cannot ask for a gigabyte.
/// 4096 is far more than a tray can show and far less than a problem.
pub const MAX_TRAY_ICONS: u32 = 4096;

/// The longest glyph a client may send, in bytes.
///
/// A glyph is one character to a user and up to a few code points to Unicode —
/// an emoji with a skin-tone modifier and a variation selector is four. 32
/// bytes holds any of those and refuses a client trying to draw a paragraph
/// into the taskbar.
pub const MAX_GLYPH_BYTES: usize = 32;

/// One icon in the tray.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayIcon {
    /// The process that owns it, assigned by the compositor from the
    /// connection rather than taken from the client.
    ///
    /// A client cannot name someone else's process here, which is the whole
    /// reason the compositor fills it in: an icon's owner decides who a click
    /// goes to and whose departure reaps it.
    pub owner: u64,
    /// The client's own name for this icon, unique within that client.
    ///
    /// Scoped to the owner, so two programs may both use id 1. A client
    /// updating an icon sends the same id again; the registry replaces rather
    /// than appends, which is what makes "set the battery to 20%" a set rather
    /// than a leak.
    pub id: u32,
    /// The character to draw.
    pub glyph: String,
    /// What to show when the pointer rests on it.
    pub tooltip: String,
}

/// Everything currently in the tray, in the order the shell should draw it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrayList {
    /// The icons, oldest registration first.
    ///
    /// Stable order matters more than any particular order: a tray whose icons
    /// move when an unrelated program registers one is a tray where the user's
    /// muscle memory is wrong. Registration order is the one the compositor can
    /// keep without the shell telling it anything.
    pub icons: Vec<TrayIcon>,
}

/// Encode a tray list as one self-contained `TRAY` frame.
#[must_use]
pub fn encode_tray_list(list: &TrayList) -> Vec<u8> {
    // 12 fixed bytes per icon (owner, id) plus two 4-byte string lengths, so 40
    // leaves room for a glyph and a short tooltip before the vector grows.
    let mut out = Vec::with_capacity(capacity_hint(TRAY_HEADER_LEN, list.icons.len(), 40));
    encode_tray_list_into(&mut out, list);
    out
}

/// Encode into a caller-provided buffer, appending to whatever it holds.
pub fn encode_tray_list_into(out: &mut Vec<u8>, list: &TrayList) {
    out.extend_from_slice(&TRAY_MAGIC);
    out.push(TRAY_VERSION);
    out.push(0); // flags
    // Saturating rather than panicking, as everywhere else in this crate: the
    // decoder rejects anything past MAX_TRAY_ICONS regardless.
    write_u32(out, u32::try_from(list.icons.len()).unwrap_or(u32::MAX));
    for icon in &list.icons {
        write_u64(out, icon.owner);
        write_u32(out, icon.id);
        write_string(out, &icon.glyph);
        write_string(out, &icon.tooltip);
    }
}

/// Decode a `TRAY` frame.
///
/// # Errors
///
/// [`DecodeError::BadMagic`] if the frame is not `TRAY`,
/// [`DecodeError::UnsupportedVersion`] for a version this build does not know,
/// [`DecodeError::ReservedFlags`] for reserved bits set,
/// [`DecodeError::TooManyTrayIcons`] for a count past [`MAX_TRAY_ICONS`], and
/// [`DecodeError::UnexpectedEof`] for a frame that ends early.
///
/// On success, answers the list and how many bytes it occupied.
pub fn decode_tray_list(input: &[u8]) -> Result<(TrayList, usize), DecodeError> {
    let mut r = Reader::new(input);
    if r.take_array::<4>()? != TRAY_MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let version = r.read_u8()?;
    if version != TRAY_VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }
    let flags = r.read_u8()?;
    if flags != 0 {
        return Err(DecodeError::ReservedFlags(flags));
    }
    let count = r.read_u32()?;
    if count > MAX_TRAY_ICONS {
        return Err(DecodeError::TooManyTrayIcons(count));
    }
    // Checked before reserving: a count is 4 bytes and an icon is at least 20,
    // so a frame claiming a million icons is short long before it is big, and
    // reserving for the claim first is how a length field becomes an
    // allocation primitive for whoever sends it.
    let mut icons = Vec::new();
    for _ in 0..count {
        let owner = r.read_u64()?;
        let id = r.read_u32()?;
        let glyph = r.read_string()?;
        let tooltip = r.read_string()?;
        icons.push(TrayIcon {
            owner,
            id,
            glyph,
            tooltip,
        });
    }
    // The position, not the input length: a `TRAY` frame arrives in a stream
    // with whatever follows it, and a caller that assumed the frame was the
    // whole buffer would swallow the next one.
    Ok((TrayList { icons }, r.position()))
}

#[cfg(test)]
mod tests {
    // The same list `window_list`'s tests carry, for the same reason: a test
    // that indexes out of range should fail loudly at the line that did it.
    #![allow(
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_used
    )]

    use super::*;

    fn icon(owner: u64, id: u32, glyph: &str, tooltip: &str) -> TrayIcon {
        TrayIcon {
            owner,
            id,
            glyph: glyph.to_string(),
            tooltip: tooltip.to_string(),
        }
    }

    #[test]
    fn a_list_survives_the_wire() {
        let list = TrayList {
            icons: vec![
                icon(7, 1, "B", "Battery: 87%"),
                icon(7, 2, "W", "Wi-Fi: Home"),
                icon(9, 1, "M", "Music"),
            ],
        };
        let bytes = encode_tray_list(&list);
        assert_eq!(decode_tray_list(&bytes).expect("round trip").0, list);
    }

    /// Two programs may both call an icon `1`, and they stay apart.
    ///
    /// The id is scoped to the owner. If it were global, the second program to
    /// register would silently replace the first one's icon -- and the first
    /// would have no way to know, because it never asked for a global name.
    #[test]
    fn an_id_is_scoped_to_its_owner() {
        let list = TrayList {
            icons: vec![icon(7, 1, "A", "first"), icon(9, 1, "B", "second")],
        };
        let back = decode_tray_list(&encode_tray_list(&list))
            .expect("round trip")
            .0;
        assert_eq!(back.icons.len(), 2, "same id, different owners, both kept");
        assert_ne!(back.icons[0].owner, back.icons[1].owner);
    }

    #[test]
    fn an_empty_tray_is_a_valid_frame() {
        let list = TrayList::default();
        assert_eq!(
            decode_tray_list(&encode_tray_list(&list))
                .expect("round trip")
                .0,
            list
        );
    }

    /// A glyph is text, and text is not ASCII.
    #[test]
    fn a_multi_code_point_glyph_survives() {
        let list = TrayList {
            icons: vec![icon(1, 1, "\u{1F50B}", "electricity")],
        };
        assert_eq!(
            decode_tray_list(&encode_tray_list(&list))
                .expect("round trip")
                .0,
            list
        );
    }

    #[test]
    fn a_frame_that_is_not_ours_is_refused() {
        let mut bytes = encode_tray_list(&TrayList::default());
        bytes[0] = b'X';
        assert_eq!(
            decode_tray_list(&bytes).map(|(l, _)| l),
            Err(DecodeError::BadMagic)
        );
    }

    #[test]
    fn a_version_this_build_does_not_know_is_refused() {
        let mut bytes = encode_tray_list(&TrayList::default());
        bytes[4] = TRAY_VERSION.wrapping_add(1);
        assert!(matches!(
            decode_tray_list(&bytes),
            Err(DecodeError::UnsupportedVersion(_))
        ));
    }

    /// A count nobody could have meant is refused before anything is allocated
    /// for it.
    ///
    /// The check is the point rather than the number: a length field a client
    /// controls is an allocation primitive for that client unless something
    /// bounds it first.
    #[test]
    fn an_impossible_count_is_refused_rather_than_reserved_for() {
        let mut bytes = encode_tray_list(&TrayList::default());
        let claim = (MAX_TRAY_ICONS + 1).to_le_bytes();
        bytes[6..10].copy_from_slice(&claim);
        assert_eq!(
            decode_tray_list(&bytes).map(|(l, _)| l),
            Err(DecodeError::TooManyTrayIcons(MAX_TRAY_ICONS + 1))
        );
    }

    /// A frame that stops in the middle of an icon is an error, not a short
    /// list.
    ///
    /// And a whole frame reports its own length, so a stream of them can be
    /// walked: the caller needs to know where the next one starts.
    #[test]
    fn a_truncated_frame_is_refused() {
        let bytes = encode_tray_list(&TrayList {
            icons: vec![icon(1, 1, "A", "a tooltip")],
        });
        assert_eq!(
            decode_tray_list(&bytes).expect("whole frame").1,
            bytes.len(),
            "a whole frame consumes exactly itself"
        );
        for cut in TRAY_HEADER_LEN..bytes.len() {
            assert!(
                decode_tray_list(&bytes[..cut]).is_err(),
                "a frame cut at {cut} decoded as though it were whole"
            );
        }
    }
}
