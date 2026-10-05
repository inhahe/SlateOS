//! The fonts axis: the typefaces a theme recommends for the desktop's text
//! and for code.
//!
//! `roadmap-detailed.md` (*Tier 2 -- Font Preferences*) asks that a theme
//! recommend fonts rather than bundle them -- a typeface's licence seldom
//! lets a theme carry it -- and that Settings offer to install what a theme
//! recommends. A theme names families in a [`FONTS_SECTION`] section: for
//! each role a list, tried in order, or a single name.
//!
//! ```yaml
//! fonts:
//!   ui: [Inter, Noto Sans]
//!   mono: Fira Code
//! ```
//!
//! Chosen as the other axes are -- `theme.fonts: <name>` in
//! `appearance.yaml` -- the first family of a role's list this machine has
//! takes the place of the user's own for that role, and where it has none of
//! them the user's own is drawn ([`FontTheme::families_in_use`]). The
//! built-in theme recommends none: choosing it is choosing your own fonts.
//!
//! A theme sets no sizes. How large text is drawn is the reader's, as the
//! scale is: a theme that made text smaller would make it unreadable to
//! someone, and "what themes do not control" in `roadmap-detailed.md` keeps
//! layout out of them.

use std::ffi::{OsStr, OsString};

use yamldoc::Document;

use super::{BUILT_IN, ThemeDirs, ThemeError, ThemeFile, Warnings, is_valid_id, list, quoted};

/// The section in which a theme recommends its fonts. Named as the axis is
/// in `meta.supports`.
pub const FONTS_SECTION: &str = "fonts";

/// The section's keys: the interface's text, and fixed-pitch text -- code
/// and terminals.
const ROLES: [&str; 2] = ["ui", "mono"];

/// The fonts a theme's file recommends, as it names them: for each role the
/// families in the order they are tried, not yet looked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontNames {
    /// The families for the interface's text.
    pub ui: Vec<String>,
    /// The families for fixed-pitch text.
    pub mono: Vec<String>,
}

impl FontNames {
    /// Whether it names any family.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ui.is_empty() && self.mono.is_empty()
    }
}

/// Read the [`FONTS_SECTION`] of `doc`: `None` when there is none, or one
/// that names no family. What it holds that is not a role, and a name that
/// cannot be a family's, is listed in `warnings`.
pub(super) fn read(doc: &Document, warnings: &mut Warnings) -> Option<FontNames> {
    if !doc.contains(&[FONTS_SECTION]) {
        return None;
    }
    for key in doc.keys(&[FONTS_SECTION]) {
        if !ROLES.contains(&key.as_str()) {
            warnings.push(format!(
                "`{FONTS_SECTION}.{}` is ignored: a theme recommends fonts for `ui` and for `mono`",
                quoted(&key)
            ));
        }
    }
    let mut families = |role: &str| -> Vec<String> {
        let mut kept: Vec<String> = Vec::new();
        for name in list(doc, &[FONTS_SECTION, role]) {
            if name.chars().any(char::is_control) {
                warnings.push(format!(
                    "font `{}` is ignored: a family's name has no control characters",
                    quoted(&name)
                ));
            } else if !kept
                .iter()
                .any(|seen| seen.to_lowercase() == name.to_lowercase())
            {
                // Not named already: a family named twice -- in any case, as
                // the font index matches names -- is tried once.
                kept.push(name);
            }
        }
        kept
    };
    let names = FontNames {
        ui: families("ui"),
        mono: families("mono"),
    };
    (!names.is_empty()).then_some(names)
}

/// The font theme in use: which theme's recommendations were chosen, and
/// what reading them gave -- one value, as each axis's is, so the name and
/// the families cannot disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontTheme {
    id: OsString,
    ui: Vec<String>,
    mono: Vec<String>,
    warnings: Vec<String>,
    problem: Option<String>,
}

impl Default for FontTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl FontTheme {
    /// The built-in theme, which recommends no font: the user's own are
    /// drawn.
    #[must_use]
    pub fn built_in() -> Self {
        Self {
            id: OsString::from(BUILT_IN),
            ui: Vec::new(),
            mono: Vec::new(),
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The fonts of the theme named `id`, from the standard directories.
    ///
    /// Never fails. A theme that cannot be used -- not installed,
    /// unreadable, recommending no font -- keeps its name, so saving the
    /// settings does not forget the choice, recommends nothing, and says why
    /// in [`problem`](Self::problem). Whether the families it recommends are
    /// installed is not asked here: that is a question about this machine,
    /// answered where they are drawn ([`families_in_use`](Self::families_in_use)).
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
        match read_for_fonts(dirs, id) {
            Ok((names, warnings)) => Self {
                id: id.to_owned(),
                ui: names.ui,
                mono: names.mono,
                warnings,
                problem: None,
            },
            Err(err) => Self {
                id: id.to_owned(),
                ui: Vec::new(),
                mono: Vec::new(),
                warnings: Vec::new(),
                problem: Some(format!(
                    "\"{}\" {err}, so your own fonts are used.",
                    pathcodec::display_os(id)
                )),
            },
        }
    }

    /// Families already in hand -- a preview, or a test's.
    #[must_use]
    pub fn from_families(id: impl Into<OsString>, ui: Vec<String>, mono: Vec<String>) -> Self {
        Self {
            id: id.into(),
            ui,
            mono,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The theme's name, as `theme.fonts` holds it.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// Whether this is the built-in theme.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.id == OsStr::new(BUILT_IN)
    }

    /// The families it recommends for the interface's text, in the order
    /// they are tried.
    #[must_use]
    pub fn ui(&self) -> &[String] {
        &self.ui
    }

    /// The families it recommends for fixed-pitch text, in the order they
    /// are tried.
    #[must_use]
    pub fn mono(&self) -> &[String] {
        &self.mono
    }

    /// The families to draw in: the user's `own`, with each role's first
    /// recommended family that `installed` says this machine has in its
    /// place. A role whose recommendations are none of them installed keeps
    /// the user's own family -- not the toolkit's default: the user chose
    /// that one.
    #[must_use]
    pub fn families_in_use(
        &self,
        own: &crate::FontSettings,
        installed: impl Fn(&str) -> bool,
    ) -> crate::FontSettings {
        let first = |families: &[String]| {
            families
                .iter()
                .find(|family| installed(family.as_str()))
                .cloned()
        };
        let mut fonts = own.clone();
        if let Some(family) = first(&self.ui) {
            fonts.ui_font = family;
        }
        if let Some(family) = first(&self.mono) {
            fonts.mono_font = family;
        }
        fonts
    }

    /// What in the theme's file was ignored.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Why the chosen theme's fonts are not in use, as a sentence for the
    /// user; `None` when they are.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

/// Find and read the installed theme `id` for its fonts.
fn read_for_fonts(dirs: &ThemeDirs, id: &OsStr) -> Result<(FontNames, Vec<String>), ThemeError> {
    if !is_valid_id(id) {
        return Err(ThemeError::InvalidName);
    }
    let (dir, _) = dirs.find(id).ok_or(ThemeError::NotInstalled)?;
    let ThemeFile {
        fonts, warnings, ..
    } = super::read_theme_file(&dir.join(super::FILE_NAME))?;
    let names = fonts.ok_or(ThemeError::NoFonts)?;
    Ok((names, warnings))
}

#[cfg(test)]
#[path = "fonts_tests.rs"]
mod tests;
