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
//! A glyph and a tooltip; the name of the program it belongs to
//! ([`TrayIcon::app_id`]); and, if the program gives one, the name of an icon
//! from the icon theme ([`IconName`]) for the shell to draw in the glyph's
//! place.
//!
//! **A name, not a bitmap.** The pictures a tray most wants -- a battery, a
//! network, a speaker -- are emoji to a font, and no font this system has
//! draws emoji, so a glyph alone leaves them boxes. An icon name follows the
//! user's theme and its light or dark mode, costs a few bytes, and lets the
//! shell upload each picture once; pixels would be one more image format on
//! this wire and a copy per program. The glyph stays, as what the shell draws
//! when the theme has no such icon.
//!
//! **Not a colour**: the tray is shell chrome and its contrast is the shell's
//! problem — a program that chose its own would be choosing against a palette
//! it cannot see, which is the defect `Palette::ink` exists to prevent. The
//! shell draws a named icon in its own ink, as it draws the glyph.
//!
//! **Not a position, either.** Order is the shell's, which is what lets a user
//! rearrange the tray; a program that could pin itself leftmost would take that
//! from them. The program's name is what lets the shell *remember* an
//! arrangement: an owner is a new number every time the program starts.

use crate::{DecodeError, Reader, capacity_hint, write_string, write_u32, write_u64};

/// Tray-list frame magic: `b"TRAY"`.
pub const TRAY_MAGIC: [u8; 4] = *b"TRAY";

/// The version of the tray frame this build writes and understands.
///
/// **2** — each icon gained the program's name ([`TrayIcon::app_id`]) and an
/// icon name ([`TrayIcon::icon_name`]), written after its tooltip. Moves
/// bytes: a version-1 decoder would read the app id's length as the next
/// icon's owner, so a version-1 frame is refused rather than read.
pub const TRAY_VERSION: u8 = 2;

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

/// The longest tooltip an icon keeps, in bytes.
///
/// A tooltip is a line or two -- "Battery: 87% (2 h 10 min left)" -- and a
/// kilobyte holds several sentences in any script. The compositor keeps the
/// first kilobyte of a longer one, cut on a character boundary as a glyph is.
///
/// A bound because the registry keeps every icon's text for as long as its
/// program runs. Without one a tooltip may be as long as any string on this
/// wire, 4 MiB, and one program's icons could hold the compositor's memory
/// by the gigabyte.
pub const MAX_TOOLTIP_BYTES: usize = 1024;

/// The most icons one client may have in the tray at once.
///
/// A share rather than only the tray's [`MAX_TRAY_ICONS`]: with that alone,
/// one program registering icon after icon fills the tray, and every program
/// started after it is refused. A program shows one icon, or a few -- a
/// status, a second for a download in progress -- and 32 leaves a program
/// that hosts several indicators room to spare.
///
/// Counted per connection, which is what the compositor knows a client by.
/// Replacing an icon a client already has is never refused; only a new id
/// past the share is.
pub const MAX_TRAY_ICONS_PER_CLIENT: u32 = 32;

/// The longest program name an icon keeps, in bytes.
///
/// A program's name is its executable's stem, or a reverse-domain name
/// (`org.example.Notes`); 255 bytes is the bound freedesktop's names share
/// with D-Bus's, and more than any program has. The compositor keeps the
/// first 255 bytes of a longer one, cut on a character boundary as it cuts a
/// glyph or a tooltip: too long is cut, never refused, for every text an
/// icon carries.
pub const MAX_APP_ID_BYTES: usize = 255;

/// The name of an icon in the icon theme: `battery-caution`,
/// `network-offline`, `audio-volume-muted`.
///
/// Known to be a name and not a path: 1 to [`IconName::MAX_LEN`] bytes of
/// `a`-`z`, `0`-`9`, `-` and `_`, not starting with `-` -- the names
/// `appearance::icons::is_valid_name` accepts, within a length. Nothing that
/// holds one can hold a `/`, a `.` or a `..`, so the shell can join it to a
/// theme's folder without checking it again, and a program that names
/// something else finds out when it builds the name rather than when the
/// icon fails to appear.
///
/// Held inline, so it is `Copy` and the compositor allocates nothing for the
/// names its clients send.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct IconName {
    len: u8,
    // Zero past `len`, always: what makes the derived equality and hash those
    // of the name.
    bytes: [u8; IconName::MAX_LEN],
}

impl IconName {
    /// The longest name, in bytes. The longest name in the freedesktop icon
    /// naming specification is under forty.
    pub const MAX_LEN: usize = 64;

    /// `name` as an icon name, or `None` if it is not one: empty, longer than
    /// [`Self::MAX_LEN`] bytes, starting with `-`, or holding anything but
    /// `a`-`z`, `0`-`9`, `-` and `_`.
    ///
    /// `None` rather than an error, so a program can hand it straight to
    /// [`TraySpec::with_icon_name`]: a name that is not one leaves the icon
    /// its glyph, which is what the shell would draw for a name its theme
    /// lacks anyway.
    #[must_use]
    pub fn new(name: &str) -> Option<Self> {
        Self::from_bytes(name.as_bytes())
    }

    /// [`Self::new`] for bytes off the wire. Every byte a name may hold is
    /// ASCII, so bytes that pass are text.
    pub(crate) fn from_bytes(raw: &[u8]) -> Option<Self> {
        let named = raw.len() <= Self::MAX_LEN
            && raw.first().is_some_and(|&b| b != b'-')
            && raw
                .iter()
                .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
        if !named {
            return None;
        }
        let mut bytes = [0u8; Self::MAX_LEN];
        bytes.get_mut(..raw.len())?.copy_from_slice(raw);
        Some(Self {
            len: u8::try_from(raw.len()).ok()?,
            bytes,
        })
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Built only from bytes `from_bytes` passed, every one ASCII, so the
        // empty answer is unreachable.
        self.bytes
            .get(..usize::from(self.len))
            .and_then(|b| core::str::from_utf8(b).ok())
            .unwrap_or("")
    }
}

impl core::fmt::Debug for IconName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "IconName({:?})", self.as_str())
    }
}

impl core::fmt::Display for IconName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialOrd for IconName {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// By the names' text: the derived order would put every short name before
/// every long one, since the length is the first field.
impl Ord for IconName {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

/// An icon name on the wire: its length in one byte, `0` for none, then its
/// bytes. Here because two codecs carry it -- the request a program sends
/// (`control`) and the list a shell is sent (this module's frame) -- and two
/// spellings of one field is how the two ends of a wire come to disagree.
pub(crate) fn write_icon_name(out: &mut Vec<u8>, name: Option<IconName>) {
    let bytes = name.as_ref().map_or(&[][..], |n| n.as_str().as_bytes());
    // At most `IconName::MAX_LEN`, 64, bytes, so the length always fits the
    // byte; the fallback is unreachable, and a length it wrote would be
    // refused by the reader rather than misread.
    out.push(u8::try_from(bytes.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(bytes);
}

/// [`write_icon_name`]'s reader: the length, the bytes, and
/// [`IconName::new`]'s verdict on them -- the one validation, so a name that
/// crossed the wire is a name a program could have made.
pub(crate) fn read_icon_name(r: &mut Reader<'_>) -> Result<Option<IconName>, DecodeError> {
    let len = r.read_u8()?;
    if len == 0 {
        return Ok(None);
    }
    let bytes = r.take(usize::from(len))?;
    IconName::from_bytes(bytes)
        .map(Some)
        .ok_or(DecodeError::BadIconName)
}

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
    /// The character to draw, when there is no [`icon_name`](Self::icon_name)
    /// or the theme has no such icon.
    pub glyph: String,
    /// What to show when the pointer rests on it.
    pub tooltip: String,
    /// Which program this icon belongs to: the name it declares for itself,
    /// as for its windows ([`WindowInfo::app_id`](crate::window_list::WindowInfo::app_id))
    /// -- conventionally its executable's stem, lower-cased. Empty when the
    /// program did not say.
    ///
    /// What a user's arrangement of the tray is remembered by: the
    /// [`owner`](Self::owner) is a new number every time the program starts,
    /// and this is not.
    ///
    /// The client's word, and unverified: fine for grouping and remembering,
    /// never for a permission. Any program may call itself anything.
    pub app_id: String,
    /// An icon from the icon theme to draw instead of the glyph, if the
    /// program named one.
    pub icon_name: Option<IconName>,
}

impl TrayIcon {
    /// An icon `id` of process `owner`, drawn as `glyph`, with `tooltip`; of
    /// no named program and naming no theme icon until
    /// [`with_app_id`](Self::with_app_id) and
    /// [`with_icon_name`](Self::with_icon_name) say otherwise.
    ///
    /// The way to build one outside this crate. A struct literal names every
    /// field, so each field added to the icon would break every literal in
    /// another lane's tree; a constructor plus a builder per new field breaks
    /// none.
    #[must_use]
    pub fn new(owner: u64, id: u32, glyph: impl Into<String>, tooltip: impl Into<String>) -> Self {
        Self {
            owner,
            id,
            glyph: glyph.into(),
            tooltip: tooltip.into(),
            app_id: String::new(),
            icon_name: None,
        }
    }

    /// This icon, belonging to the program named `app_id`.
    #[must_use]
    pub fn with_app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = app_id.into();
        self
    }

    /// This icon, drawn as the theme's `name` -- or as its glyph, for `None`.
    ///
    /// Takes an [`IconName`] or an `Option` of one, so
    /// `with_icon_name(IconName::new("battery-low"))` reads as it should.
    #[must_use]
    pub fn with_icon_name(mut self, name: impl Into<Option<IconName>>) -> Self {
        self.icon_name = name.into();
        self
    }
}

/// What a program asks the tray to show under one of its ids: everything in a
/// [`TrayIcon`] but the owner, which the compositor fills in from the
/// connection, and the id, which names the icon this is for.
///
/// The request's half of the icon, as a
/// [`WindowSpec`](crate::control::WindowSpec) is a window's. Built with
/// [`new`](Self::new) and the `with_` builders, so a field added later breaks
/// no program.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraySpec {
    /// The character to draw. See [`TrayIcon::glyph`].
    pub glyph: String,
    /// What to show when the pointer rests on it.
    pub tooltip: String,
    /// The program's name. See [`TrayIcon::app_id`]. Left empty, `oswindow`
    /// fills in the name the program declared for its windows.
    pub app_id: String,
    /// An icon from the theme to draw instead of the glyph.
    pub icon_name: Option<IconName>,
}

impl TraySpec {
    /// An icon drawn as `glyph`, with `tooltip`.
    #[must_use]
    pub fn new(glyph: impl Into<String>, tooltip: impl Into<String>) -> Self {
        Self {
            glyph: glyph.into(),
            tooltip: tooltip.into(),
            ..Self::default()
        }
    }

    /// This icon, as the program named `app_id`'s.
    #[must_use]
    pub fn with_app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = app_id.into();
        self
    }

    /// This icon, drawn as the theme's `name` -- or as its glyph, for `None`.
    /// See [`TrayIcon::with_icon_name`].
    #[must_use]
    pub fn with_icon_name(mut self, name: impl Into<Option<IconName>>) -> Self {
        self.icon_name = name.into();
        self
    }
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
    // 25 fixed bytes per icon -- owner and id, three 4-byte string lengths and
    // the icon name's length byte -- so 64 leaves room for a glyph, a short
    // tooltip and a program's name before the vector grows.
    let mut out = Vec::with_capacity(capacity_hint(TRAY_HEADER_LEN, list.icons.len(), 64));
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
        write_string(out, &icon.app_id);
        write_icon_name(out, icon.icon_name);
    }
}

/// Decode a `TRAY` frame.
///
/// # Errors
///
/// [`DecodeError::BadMagic`] if the frame is not `TRAY`,
/// [`DecodeError::UnsupportedVersion`] for a version this build does not know,
/// [`DecodeError::ReservedFlags`] for reserved bits set,
/// [`DecodeError::TooManyTrayIcons`] for a count past [`MAX_TRAY_ICONS`],
/// [`DecodeError::BadIconName`] for an icon name [`IconName::new`] refuses,
/// and [`DecodeError::UnexpectedEof`] for a frame that ends early.
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
    // Checked before reserving: a count is 4 bytes and an icon is at least 25,
    // so a frame claiming a million icons is short long before it is big, and
    // reserving for the claim first is how a length field becomes an
    // allocation primitive for whoever sends it.
    let mut icons = Vec::new();
    for _ in 0..count {
        let owner = r.read_u64()?;
        let id = r.read_u32()?;
        let glyph = r.read_string()?;
        let tooltip = r.read_string()?;
        let app_id = r.read_string()?;
        let icon_name = read_icon_name(&mut r)?;
        icons.push(TrayIcon {
            owner,
            id,
            glyph,
            tooltip,
            app_id,
            icon_name,
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
        TrayIcon::new(owner, id, glyph, tooltip)
    }

    fn name(text: &str) -> IconName {
        IconName::new(text).expect("a name")
    }

    #[test]
    fn the_constructor_fills_every_field_it_is_given() {
        let built = TrayIcon::new(7, 3, "B", "Battery: 87%");
        assert_eq!(
            built,
            TrayIcon {
                owner: 7,
                id: 3,
                glyph: "B".to_string(),
                tooltip: "Battery: 87%".to_string(),
                app_id: String::new(),
                icon_name: None,
            }
        );
        let named = built
            .with_app_id("powerd")
            .with_icon_name(name("battery-good"));
        assert_eq!(named.app_id, "powerd");
        assert_eq!(named.icon_name, Some(name("battery-good")));
        assert_eq!(named.with_icon_name(None).icon_name, None);
    }

    #[test]
    fn a_spec_is_built_the_same_way() {
        let spec = TraySpec::new("B", "Battery: 12%")
            .with_app_id("powerd")
            .with_icon_name(IconName::new("battery-caution"));
        assert_eq!(
            spec,
            TraySpec {
                glyph: "B".to_string(),
                tooltip: "Battery: 12%".to_string(),
                app_id: "powerd".to_string(),
                icon_name: Some(name("battery-caution")),
            }
        );
        // A name that is not one leaves the icon its glyph, with no
        // separate failure for a program to handle.
        let unnamed = TraySpec::new("B", "").with_icon_name(IconName::new("../battery"));
        assert_eq!(unnamed.icon_name, None);
    }

    #[test]
    fn a_list_survives_the_wire() {
        let list = TrayList {
            icons: vec![
                icon(7, 1, "B", "Battery: 87%")
                    .with_app_id("powerd")
                    .with_icon_name(name("battery-good")),
                icon(7, 2, "W", "Wi-Fi: Home").with_app_id("powerd"),
                icon(9, 1, "M", "Music").with_icon_name(name("audio-volume-high")),
                icon(9, 2, "\u{2709}", "Mail").with_app_id("org.example.Mail"),
            ],
        };
        let bytes = encode_tray_list(&list);
        assert_eq!(decode_tray_list(&bytes).expect("round trip").0, list);
    }

    /// The names `appearance::icons::is_valid_name` accepts, within 64 bytes,
    /// and nothing that could be a path.
    #[test]
    fn an_icon_name_is_a_name_and_never_a_path() {
        for good in [
            "battery-caution",
            "network-offline",
            "audio-volume-muted",
            "a",
            "0",
            "x_y",
            "_private",
            "go-next-symbolic",
            "trailing-",
        ] {
            assert_eq!(
                IconName::new(good).map(|n| n.to_string()),
                Some(good.to_string()),
                "{good:?}"
            );
        }
        for bad in [
            "",
            "-leading",
            "Upper",
            "battery/low",
            "../etc/passwd",
            "..",
            ".",
            "a.png",
            "with space",
            "back\\slash",
            "nul\0",
            "caf\u{e9}",
            "\u{1F50B}",
        ] {
            assert_eq!(IconName::new(bad), None, "{bad:?} was taken as a name");
        }
        let longest = "a".repeat(IconName::MAX_LEN);
        assert_eq!(
            IconName::new(&longest).map(|n| n.to_string()),
            Some(longest.clone())
        );
        assert_eq!(IconName::new(&format!("{longest}a")), None);
    }

    /// Equal names are equal and hash alike however they were built, and
    /// names sort as their text -- not short before long.
    #[test]
    fn icon_names_compare_as_their_text() {
        use std::collections::HashSet;
        let names: HashSet<IconName> = ["b", "b", "ab"].into_iter().map(name).collect();
        assert_eq!(names.len(), 2);
        assert!(
            name("b") > name("ab"),
            "a short name sorted before a long one"
        );
        assert!(name("ab") < name("abc"));
        assert_eq!(format!("{:?}", name("x-y")), "IconName(\"x-y\")");
    }

    /// A name off the wire is held to the rule a program's is: a frame naming
    /// a path is refused, not passed to the shell to join to a folder.
    #[test]
    fn a_frame_naming_something_that_is_not_an_icon_is_refused() {
        let list = TrayList {
            icons: vec![icon(1, 1, "A", "").with_icon_name(name("aa-bb"))],
        };
        let bytes = encode_tray_list(&list);
        let at = bytes
            .windows(5)
            .position(|w| w == b"aa-bb")
            .expect("the name is in the frame");
        for (with, what) in [
            (*b"../bb", "a path"),
            (*b"AA-BB", "capitals"),
            (*b"-a-bb", "a leading -"),
        ] {
            let mut bad = bytes.clone();
            bad[at..at + 5].copy_from_slice(&with);
            assert_eq!(
                decode_tray_list(&bad).map(|(l, _)| l),
                Err(DecodeError::BadIconName),
                "a name of {what} decoded"
            );
        }
        // A length past the bound is refused, whatever the bytes.
        let mut long = bytes.clone();
        long[at - 1] = u8::try_from(IconName::MAX_LEN + 1).expect("small");
        long.extend(std::iter::repeat_n(b'a', IconName::MAX_LEN));
        assert_eq!(
            decode_tray_list(&long).map(|(l, _)| l),
            Err(DecodeError::BadIconName)
        );
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
