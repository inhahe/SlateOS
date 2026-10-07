//! A theme's window-decorations axis: its `window-decorations` section, read,
//! and the frame chosen from it ([`DecorationTheme`]).
//!
//! The section sets the shape of every window's frame
//! ([`crate::decorations::DecorationStyle`]): its title bar, the buttons on
//! it, its border and its shadow. The user picks which theme's to use as
//! `theme.decorations` in `appearance.yaml`, independently of the colours,
//! the icons, the controls and the motion -- the mix-and-match every axis
//! allows. The frame's colours are the colours axis's. `design-decisions.md`
//! §1456.
//!
//! ```yaml
//! window-decorations:
//!   title-bar:
//!     height: 30        # 20 to 56 pixels
//!     align: left       # left | center
//!     bold: false
//!     overflow: ellipsis   # clip | ellipsis | keep-tail
//!   buttons:
//!     side: right       # left | right
//!     order: [minimize, maximize, close]   # left to right, each once
//!     shape: rounded    # rounded | circle | square | glyph
//!     size: 20          # 12 to 40 pixels
//!     gap: 4            # 0 to 16 pixels
//!   border: 1           # 0 to 8 pixels
//!   shadow: 8           # 0 to 48 pixels
//! ```
//!
//! As with every section, a setting left out keeps the built-in theme's, and
//! a value that cannot be understood costs that value and is listed for the
//! theme's author; the rest of the section is used. A size outside its range
//! is taken as the nearer end, with a note. An `order` that does not name
//! each button exactly once is refused whole, since there is no telling
//! which button the author meant to leave out or put twice.

use super::values::{at, read_choice, read_flag, read_pixels};
use super::{
    BUILT_IN, DECORATIONS_SECTION as SECTION, FILE_NAME, ThemeDirs, ThemeError, ThemeFile,
    Warnings, is_valid_id, list, quoted, read_theme_file,
};
use crate::decorations::{ButtonShape, ButtonSide, DecorationStyle, TitleAlign, TitleButton};
use std::ffi::{OsStr, OsString};
use yamldoc::Document;

/// The parts a section may hold, as a warning offers them.
const PARTS: &str = "title-bar, buttons, border and shadow";

// ============================================================================
// Reading the section
// ============================================================================

/// The frame `doc`'s section sets, over the built-in one; `None` when the
/// section is absent or sets nothing usable.
pub(super) fn read(doc: &Document, warnings: &mut Warnings) -> Option<DecorationStyle> {
    let keys = doc.keys(&[SECTION]);
    if keys.is_empty() {
        if let Some(value) = doc.get_str(&[SECTION]).filter(|v| !v.trim().is_empty()) {
            warnings.push(format!(
                "`{SECTION}` is ignored: it holds settings ({PARTS}), not a value like `{}`",
                quoted(value.trim())
            ));
        }
        return None;
    }
    let mut style = DecorationStyle::AERO;
    let mut any = false;
    for key in keys {
        let path = [SECTION, key.as_str()];
        let set = match key.as_str() {
            "title-bar" => read_title_bar(doc, &mut style, warnings),
            "buttons" => read_buttons(doc, &mut style, warnings),
            "border" => read_pixels(
                doc,
                &path,
                0,
                DecorationStyle::MAX_BORDER,
                "a border",
                warnings,
            )
            .map(|px| style.border = px)
            .is_some(),
            "shadow" => read_pixels(
                doc,
                &path,
                0,
                DecorationStyle::MAX_SHADOW,
                "a shadow",
                warnings,
            )
            .map(|px| style.shadow = px)
            .is_some(),
            other => {
                warnings.push(format!(
                    "`{SECTION}.{}` is ignored: window decorations have no part called `{}` \
                     (they have {PARTS})",
                    quoted(other),
                    quoted(other)
                ));
                false
            }
        };
        any |= set;
    }
    any.then_some(style)
}

/// The `title-bar` part: its height, where the title goes, and whether it
/// is bold.
fn read_title_bar(doc: &Document, style: &mut DecorationStyle, warnings: &mut Warnings) -> bool {
    let mut any = false;
    for key in doc.keys(&[SECTION, "title-bar"]) {
        let path = [SECTION, "title-bar", key.as_str()];
        any |= match key.as_str() {
            "height" => read_pixels(
                doc,
                &path,
                DecorationStyle::MIN_TITLE_HEIGHT,
                DecorationStyle::MAX_TITLE_HEIGHT,
                "a title bar",
                warnings,
            )
            .map(|px| style.title_height = px)
            .is_some(),
            "align" => read_choice(
                doc,
                &path,
                &[("left", TitleAlign::Left), ("center", TitleAlign::Center)],
                warnings,
            )
            .map(|align| style.title_align = align)
            .is_some(),
            "bold" => read_flag(doc, &path, warnings)
                .map(|bold| style.title_bold = bold)
                .is_some(),
            "overflow" => read_choice(
                doc,
                &path,
                &guitk::text::Overflow::ALL.map(|o| (o.name(), o)),
                warnings,
            )
            .map(|overflow| style.title_overflow = overflow)
            .is_some(),
            other => {
                warnings.push(format!(
                    "`{}` is ignored: a title bar has no setting called `{}` \
                     (it has height, align, bold and overflow)",
                    at(&path),
                    quoted(other)
                ));
                false
            }
        };
    }
    any
}

/// The `buttons` part: which end, in what order, what shape, how big and
/// how far apart.
fn read_buttons(doc: &Document, style: &mut DecorationStyle, warnings: &mut Warnings) -> bool {
    let mut any = false;
    for key in doc.keys(&[SECTION, "buttons"]) {
        let path = [SECTION, "buttons", key.as_str()];
        any |= match key.as_str() {
            "side" => read_choice(
                doc,
                &path,
                &[("left", ButtonSide::Left), ("right", ButtonSide::Right)],
                warnings,
            )
            .map(|side| style.button_side = side)
            .is_some(),
            "order" => read_order(doc, &path, warnings)
                .map(|order| style.buttons = order)
                .is_some(),
            "shape" => read_choice(
                doc,
                &path,
                &[
                    ("rounded", ButtonShape::Rounded),
                    ("circle", ButtonShape::Circle),
                    ("square", ButtonShape::Square),
                    ("glyph", ButtonShape::Glyph),
                ],
                warnings,
            )
            .map(|shape| style.button_shape = shape)
            .is_some(),
            "size" => read_pixels(
                doc,
                &path,
                DecorationStyle::MIN_BUTTON_SIZE,
                DecorationStyle::MAX_BUTTON_SIZE,
                "a button",
                warnings,
            )
            .map(|px| style.button_size = px)
            .is_some(),
            "gap" => read_pixels(
                doc,
                &path,
                0,
                DecorationStyle::MAX_BUTTON_GAP,
                "the gap between buttons",
                warnings,
            )
            .map(|px| style.button_gap = px)
            .is_some(),
            other => {
                warnings.push(format!(
                    "`{}` is ignored: the buttons have no setting called `{}` \
                     (they have side, order, shape, size and gap)",
                    at(&path),
                    quoted(other)
                ));
                false
            }
        };
    }
    any
}

/// The buttons' order, left to right: each of the three named exactly once,
/// or the whole order is refused with what was wrong with it.
fn read_order(doc: &Document, path: &[&str], warnings: &mut Warnings) -> Option<[TitleButton; 3]> {
    let names = list(doc, path);
    if names.is_empty() {
        warnings.push(format!(
            "`{}` has no value: write the three buttons in order, like \
             [minimize, maximize, close]",
            at(path)
        ));
        return None;
    }
    let mut order: Vec<TitleButton> = Vec::with_capacity(3);
    for name in &names {
        let Some(button) = TitleButton::ALL.into_iter().find(|b| b.name() == name) else {
            warnings.push(format!(
                "`{}` is ignored: `{}` is not a button (they are minimize, maximize and close)",
                at(path),
                quoted(name)
            ));
            return None;
        };
        if order.contains(&button) {
            warnings.push(format!(
                "`{}` is ignored: it names `{}` twice -- each button goes in once",
                at(path),
                button.name()
            ));
            return None;
        }
        order.push(button);
    }
    if let Some(missing) = TitleButton::ALL.into_iter().find(|b| !order.contains(b)) {
        warnings.push(format!(
            "`{}` is ignored: it leaves out `{}` -- every window has its buttons in it",
            at(path),
            missing.name()
        ));
        return None;
    }
    order.try_into().ok()
}

// ============================================================================
// The frame in use
// ============================================================================

/// The window-decorations theme in use: which theme's frame was chosen, and
/// what reading it gave. [`super::AnimationTheme`]'s counterpart for this
/// axis, and one value for the same reason: the name and the frame cannot
/// disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecorationTheme {
    id: OsString,
    style: DecorationStyle,
    warnings: Vec<String>,
    problem: Option<String>,
}

impl Default for DecorationTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl DecorationTheme {
    /// The built-in theme's frame, [`DecorationStyle::AERO`].
    #[must_use]
    pub fn built_in() -> Self {
        Self {
            id: OsString::from(BUILT_IN),
            style: DecorationStyle::AERO,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The frame of the theme named `id`, read from the standard
    /// directories.
    ///
    /// Never fails. A theme that cannot be used -- not installed, unreadable,
    /// with no `window-decorations` section -- keeps its name, so saving the
    /// settings does not quietly forget the choice, and the built-in frame is
    /// used with a [`problem`](Self::problem) saying why.
    #[must_use]
    pub fn load(id: &OsStr) -> Self {
        Self::load_from(&ThemeDirs::standard(), id)
    }

    /// [`load`](Self::load), from the given directories.
    #[must_use]
    pub fn load_from(dirs: &ThemeDirs, id: &OsStr) -> Self {
        if id == OsStr::new(BUILT_IN) {
            return Self::built_in();
        }
        match read_for_decorations(dirs, id) {
            Ok((style, warnings)) => Self {
                id: id.to_owned(),
                style,
                warnings,
                problem: None,
            },
            Err(err) => Self {
                id: id.to_owned(),
                style: DecorationStyle::AERO,
                warnings: Vec::new(),
                problem: Some(format!(
                    "\"{}\" {err}, so the built-in window frames are used.",
                    pathcodec::display_os(id)
                )),
            },
        }
    }

    /// A frame already in hand -- a preview of one being edited, or a test's.
    #[must_use]
    pub fn from_style(id: impl Into<OsString>, style: DecorationStyle) -> Self {
        Self {
            id: id.into(),
            style,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The theme's name, as `theme.decorations` holds it.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// Whether this is the built-in theme.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.id == OsStr::new(BUILT_IN)
    }

    /// The theme's frame -- or the built-in one when it could not be used.
    #[must_use]
    pub fn style(&self) -> DecorationStyle {
        self.style
    }

    /// What in the theme's file was ignored; see [`ThemeFile::warnings`].
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Why the chosen theme's frame is not in use, as a sentence for the
    /// user; `None` when it is.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

/// Find and read the installed theme `id` for its frame, with what in its
/// file was ignored.
fn read_for_decorations(
    dirs: &ThemeDirs,
    id: &OsStr,
) -> Result<(DecorationStyle, Vec<String>), ThemeError> {
    if !is_valid_id(id) {
        return Err(ThemeError::InvalidName);
    }
    let (dir, _) = dirs.find(id).ok_or(ThemeError::NotInstalled)?;
    let ThemeFile {
        decorations,
        warnings,
        ..
    } = read_theme_file(&dir.join(FILE_NAME))?;
    let style = decorations.ok_or(ThemeError::NoDecorations)?;
    Ok((style, warnings))
}

#[cfg(test)]
#[path = "decorations_tests.rs"]
mod tests;
