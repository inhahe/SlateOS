//! Icon themes: the pictures a desktop draws beside a name, found by name in
//! the theme the user chose.
//!
//! # Where an icon comes from
//!
//! A theme's icons are SVG files in an `icons` directory inside the theme's own
//! folder -- beside the `theme.yaml` of [`crate::themes`], under the same two
//! roots, the user's first -- one file per icon, `<name>.svg`. The names are
//! the freedesktop Icon Naming Specification's (`folder`, `user-home`,
//! `utilities-terminal`, `preferences-system`, ...), so an icon set drawn for
//! another desktop can be dropped in, and a program asking for an icon uses a
//! name every theme author already knows.
//!
//! A name the theme lacks is asked for again with its last `-part` dropped --
//! `folder-documents` falls back to `folder` -- which is the specification's
//! own rule, and then the built-in theme's icon of that name. The built-in
//! icons are compiled in, so a desktop with no theme installed, or a theme
//! that draws only a few icons, still draws every one.
//!
//! # Colour
//!
//! An icon drawn in `currentColor` takes the colour it is drawn in -- the text
//! beside it, the accent -- so a monochrome set follows the theme without a
//! file per colour (`roadmap-detailed.md` §3.4 Tier 1, "colour token
//! substitution"). An icon with colours of its own keeps them.
//!
//! # Trust
//!
//! A theme's files are the user's, or whoever installed the theme's: a file
//! larger than [`MAX_ICON_BYTES`], one that is not an SVG this renderer can
//! read, or one that draws nothing is passed over for the next place to look,
//! and an icon is never rendered larger than [`MAX_ICON_PX`] square.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use guitk::color::Color;
use guitk::svg::SvgDocument;

use crate::themes::{self, ThemeDirs};

/// The directory inside a theme's folder that holds its icons.
pub const ICONS_DIR: &str = "icons";

/// The largest icon file read. An icon is a few hundred bytes of path data; a
/// file of megabytes is not an icon, and is not parsed to find out.
pub const MAX_ICON_BYTES: u64 = 256 * 1024;

/// The largest size an icon is drawn at, in pixels square.
pub const MAX_ICON_PX: u32 = 512;

/// The built-in theme's icons: `(name, SVG)`. Several names share one picture
/// -- a place is drawn as what it holds, so `folder-documents` is the document.
const BUILT_IN: &[(&str, &str)] = &[
    ("folder", include_str!("../themes/aero/icons/folder.svg")),
    (
        "text-x-generic",
        include_str!("../themes/aero/icons/text-x-generic.svg"),
    ),
    (
        "folder-documents",
        include_str!("../themes/aero/icons/text-x-generic.svg"),
    ),
    (
        "image-x-generic",
        include_str!("../themes/aero/icons/image-x-generic.svg"),
    ),
    (
        "folder-pictures",
        include_str!("../themes/aero/icons/image-x-generic.svg"),
    ),
    (
        "audio-x-generic",
        include_str!("../themes/aero/icons/audio-x-generic.svg"),
    ),
    (
        "folder-music",
        include_str!("../themes/aero/icons/audio-x-generic.svg"),
    ),
    (
        "folder-download",
        include_str!("../themes/aero/icons/folder-download.svg"),
    ),
    (
        "user-home",
        include_str!("../themes/aero/icons/user-home.svg"),
    ),
    (
        "preferences-system",
        include_str!("../themes/aero/icons/preferences-system.svg"),
    ),
    (
        "utilities-terminal",
        include_str!("../themes/aero/icons/utilities-terminal.svg"),
    ),
    (
        "system-shutdown",
        include_str!("../themes/aero/icons/system-shutdown.svg"),
    ),
    (
        "application-x-executable",
        include_str!("../themes/aero/icons/application-x-executable.svg"),
    ),
    (
        "computer",
        include_str!("../themes/aero/icons/computer.svg"),
    ),
    (
        "user-trash",
        include_str!("../themes/aero/icons/user-trash.svg"),
    ),
    (
        "system-search",
        include_str!("../themes/aero/icons/system-search.svg"),
    ),
    (
        "drive-harddisk",
        include_str!("../themes/aero/icons/drive-harddisk.svg"),
    ),
    (
        "emblem-symbolic-link",
        include_str!("../themes/aero/icons/emblem-symbolic-link.svg"),
    ),
    (
        "x-office-document",
        include_str!("../themes/aero/icons/text-x-generic.svg"),
    ),
];

/// The names the built-in theme draws, for a caller that lists them.
#[must_use]
pub fn built_in_names() -> Vec<&'static str> {
    BUILT_IN.iter().map(|(name, _)| *name).collect()
}

/// Whether `name` can name an icon: lower-case letters, digits and `-` (and
/// `_`, which some sets use), not empty and not starting with `-`. Anything
/// else -- a `/`, a `.`, a `..` -- would be a path rather than a name.
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// `name`, then each shorter name the specification's fallback rule gives:
/// `folder-documents` then `folder`.
fn candidates(name: &str) -> impl Iterator<Item = &str> {
    let mut next = Some(name);
    std::iter::from_fn(move || {
        let this = next?;
        next = this
            .rfind('-')
            .and_then(|cut| this.get(..cut))
            .filter(|s| !s.is_empty());
        Some(this)
    })
}

/// An icon, drawn: `size` pixels square, straight-alpha `0xAARRGGBB`, row by
/// row -- the form `imagecodec` decodes to and the compositor uploads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    /// The side of the square, in pixels.
    pub size: u32,
    /// The pixels, `size * size` of them.
    pub argb: Vec<u32>,
}

/// A theme's icons.
///
/// The theme is named by its folder, which need not be text
/// (`design-decisions.md` §426) -- an `OsString`, as a colour theme's is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconTheme {
    id: OsString,
    /// Where to look, or `None` for the standard directories -- read when an
    /// icon is looked up, not when the theme is named, so that the setting
    /// is the name alone and two readings of one file compare equal.
    dirs: Option<ThemeDirs>,
}

impl Default for IconTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl IconTheme {
    /// The built-in theme, whose icons are compiled in -- read from the
    /// standard theme directories first, so an installed copy that has been
    /// edited is the one drawn.
    #[must_use]
    pub fn built_in() -> Self {
        Self::load(OsStr::new(themes::BUILT_IN))
    }

    /// The theme `id`, looked for in `dirs`.
    #[must_use]
    pub fn named(id: &OsStr, dirs: ThemeDirs) -> Self {
        Self {
            id: id.to_os_string(),
            dirs: Some(dirs),
        }
    }

    /// The theme `id`, looked for in the standard theme directories.
    #[must_use]
    pub fn load(id: &OsStr) -> Self {
        Self {
            id: id.to_os_string(),
            dirs: None,
        }
    }

    /// The theme's name -- its folder's.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// The SVG for `name`: the theme's own file, the user's copy before the
    /// system's, trying each shorter name in turn; then the built-in icon of
    /// the name or a shorter one. `None` for a name nothing draws, or one that
    /// is not a name at all.
    #[must_use]
    pub fn source(&self, name: &str) -> Option<Cow<'static, str>> {
        if !is_valid_name(name) {
            return None;
        }
        for candidate in candidates(name) {
            if let Some(text) = self.theme_file(candidate) {
                return Some(Cow::Owned(text));
            }
        }
        candidates(name).find_map(|candidate| {
            BUILT_IN
                .iter()
                .find(|(built, _)| *built == candidate)
                .map(|(_, svg)| Cow::Borrowed(*svg))
        })
    }

    /// The first readable, parseable file for exactly `name` in the theme's
    /// folders, user's first.
    fn theme_file(&self, name: &str) -> Option<String> {
        if !themes::is_valid_id(&self.id) {
            return None;
        }
        let file = format!("{name}.svg");
        let dirs = self.dirs.clone().unwrap_or_else(ThemeDirs::standard);
        dirs.roots().into_iter().find_map(|(root, _)| {
            let path: PathBuf = root.join(&self.id).join(ICONS_DIR).join(&file);
            read_icon(&path)
        })
    }

    /// `name` drawn `size` pixels square, `currentColor` as `color`. `None`
    /// when nothing draws the name, or `size` is zero; a size past
    /// [`MAX_ICON_PX`] is drawn at that.
    #[must_use]
    pub fn render(&self, name: &str, size: u32, color: Color) -> Option<Icon> {
        let size = size.min(MAX_ICON_PX);
        if size == 0 {
            return None;
        }
        let source = self.source(name)?;
        render_svg(&source, size, color)
    }
}

/// A theme's icon file, if it is one: small enough, text, and an SVG this
/// renderer reads. Anything else is passed over for the next place to look.
fn read_icon(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_ICON_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    SvgDocument::parse(&text).ok().map(|_| text)
}

/// `svg` drawn `size` pixels square, `currentColor` as `color` -- and the
/// whole icon faded by `color`'s alpha, so a translucent colour draws a
/// translucent icon (the ghost of a dragged icon, say) whatever colours the
/// icon's own shapes are in.
fn render_svg(svg: &str, size: u32, color: Color) -> Option<Icon> {
    let hex = format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b);
    let tinted = svg
        .replace("currentColor", &hex)
        .replace("currentcolor", &hex);
    let doc = SvgDocument::parse(&tinted).ok()?;
    let bytes = doc.render(size, size);
    // The renderer's pixels are `[r, g, b, a]`, straight alpha.
    let argb: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|p| match p {
            [r, g, b, a] => {
                // `a * color.a / 255`, rounded: both at most 255, so the
                // product fits and the quotient is a byte again.
                let faded = u16::from(*a).saturating_mul(u16::from(color.a));
                let alpha = u8::try_from(faded.saturating_add(127) / 255).unwrap_or(u8::MAX);
                u32::from_be_bytes([alpha, *r, *g, *b])
            }
            _ => 0,
        })
        .collect();
    let expected = usize::try_from(size)
        .ok()?
        .checked_mul(usize::try_from(size).ok()?)?;
    if argb.len() != expected || argb.iter().all(|px| px & 0xFF00_0000 == 0) {
        // Nothing drawn: an SVG of no visible shapes is not an icon.
        return None;
    }
    Some(Icon { size, argb })
}

#[cfg(test)]
#[path = "icons_tests.rs"]
mod tests;
