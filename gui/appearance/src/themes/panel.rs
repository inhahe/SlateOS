//! A theme's taskbar-panel axis: its `taskbar-panel` section, read, and the
//! panel chosen from it ([`PanelTheme`]).
//!
//! The section sets how the taskbar is finished and spaced
//! ([`crate::panel::PanelStyle`]): how much of the Aero reference's glass it
//! wears, and the gaps between its tiles. The user picks which theme's to use
//! as `theme.taskbar_panel` in `appearance.yaml`, independently of every other
//! axis. Its colours are the colours axis's, and whether the bar is
//! see-through is the user's own setting. `design-decisions.md` §1460.
//!
//! ```yaml
//! taskbar-panel:
//!   gloss: 1                 # 0 (a flat bar) to 1 (the reference's glass)
//!   spacing:
//!     tiles: 1               # 0 to 24 pixels between two tiles
//!     after-start: 6         # 0 to 48 pixels from the start button
//!     between-sections: 19   # 3 to 64 pixels, the divider in the middle
//! ```
//!
//! As with every section, a setting left out keeps the built-in theme's, and
//! a value that cannot be understood costs that value and is listed for the
//! theme's author; the rest of the section is used. A value outside its range
//! is taken as the nearer end, with a note.

use super::values::{at, read_pixels, read_share};
use super::{
    BUILT_IN, FILE_NAME, PANEL_SECTION as SECTION, ThemeDirs, ThemeError, ThemeFile, Warnings,
    is_valid_id, quoted, read_theme_file,
};
use crate::panel::PanelStyle;
use std::ffi::{OsStr, OsString};
use yamldoc::Document;

/// The parts a section may hold, as a warning offers them.
const PARTS: &str = "gloss and spacing";

// ============================================================================
// Reading the section
// ============================================================================

/// The panel `doc`'s section sets, over the built-in one; `None` when the
/// section is absent or sets nothing usable.
pub(super) fn read(doc: &Document, warnings: &mut Warnings) -> Option<PanelStyle> {
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
    let mut style = PanelStyle::AERO;
    let mut any = false;
    for key in keys {
        let path = [SECTION, key.as_str()];
        let set = match key.as_str() {
            "gloss" => read_share(doc, &path, "the gloss", warnings)
                .map(|gloss| style.gloss = gloss)
                .is_some(),
            "spacing" => read_spacing(doc, &mut style, warnings),
            other => {
                warnings.push(format!(
                    "`{SECTION}.{}` is ignored: the taskbar panel has no part called `{}` \
                     (it has {PARTS})",
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

/// The `spacing` part: the gaps between the tiles, after the start button
/// and between the sections.
fn read_spacing(doc: &Document, style: &mut PanelStyle, warnings: &mut Warnings) -> bool {
    let keys = doc.keys(&[SECTION, "spacing"]);
    if keys.is_empty() {
        if let Some(value) = doc
            .get_str(&[SECTION, "spacing"])
            .filter(|v| !v.trim().is_empty())
        {
            warnings.push(format!(
                "`{SECTION}.spacing` is ignored: it holds gaps (tiles, after-start and \
                 between-sections), not a value like `{}`",
                quoted(value.trim())
            ));
        }
        return false;
    }
    let mut any = false;
    for key in keys {
        let path = [SECTION, "spacing", key.as_str()];
        any |= match key.as_str() {
            "tiles" => read_pixels(
                doc,
                &path,
                0,
                PanelStyle::MAX_TILE_GAP,
                "the gap between two tiles",
                warnings,
            )
            .map(|px| style.tile_gap = px)
            .is_some(),
            "after-start" => read_pixels(
                doc,
                &path,
                0,
                PanelStyle::MAX_START_GAP,
                "the gap after the start button",
                warnings,
            )
            .map(|px| style.start_gap = px)
            .is_some(),
            "between-sections" => read_pixels(
                doc,
                &path,
                PanelStyle::MIN_SECTION_GAP,
                PanelStyle::MAX_SECTION_GAP,
                "the gap between the sections",
                warnings,
            )
            .map(|px| style.section_gap = px)
            .is_some(),
            other => {
                warnings.push(format!(
                    "`{}` is ignored: the spacing has no gap called `{}` \
                     (it has tiles, after-start and between-sections)",
                    at(&path),
                    quoted(other)
                ));
                false
            }
        };
    }
    any
}

// ============================================================================
// The panel in use
// ============================================================================

/// The taskbar-panel theme in use: which theme's panel was chosen, and what
/// reading it gave. [`super::DecorationTheme`]'s counterpart for this axis,
/// and one value for the same reason: the name and the panel cannot
/// disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelTheme {
    id: OsString,
    style: PanelStyle,
    warnings: Vec<String>,
    problem: Option<String>,
}

impl Default for PanelTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl PanelTheme {
    /// The built-in theme's panel, [`PanelStyle::AERO`].
    #[must_use]
    pub fn built_in() -> Self {
        Self {
            id: OsString::from(BUILT_IN),
            style: PanelStyle::AERO,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The panel of the theme named `id`, read from the standard directories.
    ///
    /// Never fails. A theme that cannot be used -- not installed, unreadable,
    /// with no `taskbar-panel` section -- keeps its name, so saving the
    /// settings does not quietly forget the choice, and the built-in panel is
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
        match read_for_panel(dirs, id) {
            Ok((style, warnings)) => Self {
                id: id.to_owned(),
                style,
                warnings,
                problem: None,
            },
            Err(err) => Self {
                id: id.to_owned(),
                style: PanelStyle::AERO,
                warnings: Vec::new(),
                problem: Some(format!(
                    "\"{}\" {err}, so the built-in taskbar panel is used.",
                    pathcodec::display_os(id)
                )),
            },
        }
    }

    /// A panel already in hand -- a preview of one being edited, or a test's.
    #[must_use]
    pub fn from_style(id: impl Into<OsString>, style: PanelStyle) -> Self {
        Self {
            id: id.into(),
            style,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The theme's name, as `theme.taskbar_panel` holds it.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// Whether this is the built-in theme.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.id == OsStr::new(BUILT_IN)
    }

    /// The theme's panel -- or the built-in one when it could not be used.
    #[must_use]
    pub fn style(&self) -> PanelStyle {
        self.style
    }

    /// What in the theme's file was ignored; see [`ThemeFile::warnings`].
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Why the chosen theme's panel is not in use, as a sentence for the
    /// user; `None` when it is.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

/// Find and read the installed theme `id` for its panel, with what in its
/// file was ignored.
fn read_for_panel(dirs: &ThemeDirs, id: &OsStr) -> Result<(PanelStyle, Vec<String>), ThemeError> {
    if !is_valid_id(id) {
        return Err(ThemeError::InvalidName);
    }
    let (dir, _) = dirs.find(id).ok_or(ThemeError::NotInstalled)?;
    let ThemeFile {
        panel, warnings, ..
    } = read_theme_file(&dir.join(FILE_NAME))?;
    let style = panel.ok_or(ThemeError::NoPanel)?;
    Ok((style, warnings))
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;
