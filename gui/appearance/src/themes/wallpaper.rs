//! The wallpaper axis: the pictures a theme recommends for the desktop, one
//! for dark mode and one for light.
//!
//! `roadmap-detailed.md` (*Tier 3 -- Wallpaper Integration*) asks that a
//! theme can bundle or recommend wallpapers. A theme bundles pictures in a
//! [`WALLPAPERS_DIR`] folder beside its `theme.yaml`, and recommends two of
//! them -- or one, for both modes -- in a [`WALLPAPERS_SECTION`] section:
//!
//! ```yaml
//! wallpapers:
//!   dark: wallpapers/aurora-night.jpg
//!   light: wallpapers/aurora-day.jpg
//! ```
//!
//! Chosen as the other axes are -- `theme.wallpaper: <name>` in
//! `appearance.yaml` -- the desktop shows the recommended picture for the
//! mode it is drawn in, so a theme's day and night pictures follow the
//! automatic light and dark mode as its colours do. The built-in theme
//! recommends none, and a user who chose no wallpaper theme keeps the picture
//! they chose themselves. A picture a theme bundles and does not recommend is
//! still offered by a theme list ([`super::ThemeInfo::wallpapers`]), to be
//! chosen as a picture like any other.
//!
//! A recommendation names a file inside the theme's folder, as a screenshot
//! does: a theme cannot reach outside its folder.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use yamldoc::Document;

use super::{BUILT_IN, ThemeDirs, ThemeError, ThemeFile, Warnings, confined, is_valid_id, quoted};

/// The section in which a theme recommends its wallpapers. Named as the
/// axis is in `meta.supports`.
pub const WALLPAPERS_SECTION: &str = "wallpapers";

/// The folder inside a theme's folder holding the pictures it bundles.
pub const WALLPAPERS_DIR: &str = "wallpapers";

/// The two keys of the section.
const MODES: [&str; 2] = ["dark", "light"];

/// The wallpapers a theme's file recommends, as it names them: paths inside
/// the theme's folder, not yet looked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WallpaperNames {
    /// The picture for dark mode.
    pub dark: Option<String>,
    /// The picture for light mode.
    pub light: Option<String>,
}

impl WallpaperNames {
    /// Whether it names any picture.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dark.is_none() && self.light.is_none()
    }
}

/// Read the [`WALLPAPERS_SECTION`] of `doc`: `None` when there is none, or
/// one that names no picture. What it holds that is not a mode is listed in
/// `warnings`.
pub(super) fn read(doc: &Document, warnings: &mut Warnings) -> Option<WallpaperNames> {
    if !doc.contains(&[WALLPAPERS_SECTION]) {
        return None;
    }
    for key in doc.keys(&[WALLPAPERS_SECTION]) {
        if !MODES.contains(&key.as_str()) {
            warnings.push(format!(
                "`{WALLPAPERS_SECTION}.{}` is ignored: a theme recommends a wallpaper for `dark` and for `light`",
                quoted(&key)
            ));
        }
    }
    let named = |mode: &str| {
        doc.get_str(&[WALLPAPERS_SECTION, mode])
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let names = WallpaperNames {
        dark: named("dark"),
        light: named("light"),
    };
    (!names.is_empty()).then_some(names)
}

/// The wallpaper theme in use: which theme's pictures were chosen, and what
/// reading them gave -- one value, as each axis's is, so the name and the
/// pictures cannot disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallpaperTheme {
    id: OsString,
    dark: Option<PathBuf>,
    light: Option<PathBuf>,
    warnings: Vec<String>,
    problem: Option<String>,
}

impl Default for WallpaperTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl WallpaperTheme {
    /// The built-in theme, which recommends no wallpaper: the user's own
    /// picture is shown.
    #[must_use]
    pub fn built_in() -> Self {
        Self {
            id: OsString::from(BUILT_IN),
            dark: None,
            light: None,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The wallpapers of the theme named `id`, from the standard
    /// directories.
    ///
    /// Never fails. A theme that cannot be used -- not installed, unreadable,
    /// recommending no picture that is there -- keeps its name, so saving the
    /// settings does not forget the choice, recommends nothing, and says why
    /// in [`problem`](Self::problem).
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
        match read_for_wallpapers(dirs, id) {
            Ok(Recommended {
                dark,
                light,
                warnings,
            }) => Self {
                id: id.to_owned(),
                dark,
                light,
                warnings,
                problem: None,
            },
            Err(err) => Self {
                id: id.to_owned(),
                dark: None,
                light: None,
                warnings: Vec::new(),
                problem: Some(format!(
                    "\"{}\" {err}, so your own wallpaper is shown.",
                    pathcodec::display_os(id)
                )),
            },
        }
    }

    /// Pictures already in hand -- a preview, or a test's.
    #[must_use]
    pub fn from_pictures(
        id: impl Into<OsString>,
        dark: Option<PathBuf>,
        light: Option<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            dark,
            light,
            warnings: Vec::new(),
            problem: None,
        }
    }

    /// The theme's name, as `theme.wallpaper` holds it.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// Whether this is the built-in theme.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.id == OsStr::new(BUILT_IN)
    }

    /// The picture for the desktop drawn in light mode when `light`, else in
    /// dark: the theme's for that mode, or its other one when it recommends
    /// only one -- a picture is not made for one mode as colours are. `None`
    /// when the theme recommends none.
    #[must_use]
    pub fn picture(&self, light: bool) -> Option<&Path> {
        let (wanted, other) = if light {
            (&self.light, &self.dark)
        } else {
            (&self.dark, &self.light)
        };
        wanted.as_deref().or(other.as_deref())
    }

    /// What in the theme's file was ignored, or recommended and not there.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Why the chosen theme's wallpapers are not in use, as a sentence for
    /// the user; `None` when they are.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

/// What an installed theme's file recommends, found: the picture for each
/// mode that is there, and what was dropped and why.
struct Recommended {
    dark: Option<PathBuf>,
    light: Option<PathBuf>,
    warnings: Vec<String>,
}

/// Find and read the installed theme `id` for its wallpapers: each
/// recommendation resolved inside the theme's folder and kept only when it is
/// a file there, with what was dropped and why.
fn read_for_wallpapers(dirs: &ThemeDirs, id: &OsStr) -> Result<Recommended, ThemeError> {
    if !is_valid_id(id) {
        return Err(ThemeError::InvalidName);
    }
    let (dir, _) = dirs.find(id).ok_or(ThemeError::NotInstalled)?;
    let ThemeFile {
        wallpapers,
        mut warnings,
        ..
    } = super::read_theme_file(&dir.join(super::FILE_NAME))?;
    let names = wallpapers.ok_or(ThemeError::NoWallpapers)?;
    let mut resolve = |name: Option<String>| {
        let name = name?;
        match confined(&dir, &name) {
            Some(path) if path.is_file() => Some(path),
            Some(_) => {
                warnings.push(format!(
                    "wallpaper `{}` is ignored: there is no such picture in the theme's folder",
                    quoted(&name)
                ));
                None
            }
            None => {
                warnings.push(format!(
                    "wallpaper `{}` is ignored: it must be a path inside the theme's folder",
                    quoted(&name)
                ));
                None
            }
        }
    };
    let dark = resolve(names.dark);
    let light = resolve(names.light);
    if dark.is_none() && light.is_none() {
        return Err(ThemeError::NoWallpapers);
    }
    Ok(Recommended {
        dark,
        light,
        warnings,
    })
}

/// What the wallpapers of the theme `id` depend on besides its file, whose
/// place and bytes the dependency fingerprint holds already: which of the
/// pictures it recommends are there, one byte for each mode. A
/// recommendation names a file that can arrive after the theme's own -- a
/// theme being copied in, or written -- and without this the desktop would
/// go on showing the user's own picture until the file next changed. Empty
/// for the built-in theme and for a name that cannot be a theme, as their
/// files' parts are.
pub(super) fn fingerprint(id: &OsStr) -> Vec<u8> {
    if id == OsStr::new(BUILT_IN) || !is_valid_id(id) {
        return Vec::new();
    }
    let theme = WallpaperTheme::load(id);
    [&theme.dark, &theme.light]
        .into_iter()
        .map(|picture| if picture.is_some() { b'1' } else { b'0' })
        .collect()
}

/// The pictures the theme in `dir` bundles: the files of its
/// [`WALLPAPERS_DIR`], by name. What they hold is not read here -- a list
/// shows them, and decoding is the drawing's.
#[must_use]
pub fn bundled(dir: &Path) -> Vec<PathBuf> {
    let Ok(listing) = std::fs::read_dir(dir.join(WALLPAPERS_DIR)) else {
        return Vec::new();
    };
    let mut pictures: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    pictures.sort();
    pictures
}

#[cfg(test)]
#[path = "wallpaper_tests.rs"]
mod tests;
