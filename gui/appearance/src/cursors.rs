//! Cursor themes: the pictures the pointer is drawn as, found by name in the
//! theme the user chose.
//!
//! # Where a cursor comes from
//!
//! A cursor theme is a folder with a `cursors` directory of XCursor files,
//! one per cursor name -- the layout every desktop on Linux uses, so a theme
//! drawn for one of them (Adwaita, Breeze, Bibata, ...) is installed here as
//! it is. A theme is looked for under the SlateOS theme roots first, beside a
//! `theme.yaml` as a theme's icons are ([`crate::themes::ThemeDirs`]), and
//! then where other desktops install cursor themes: `$XDG_DATA_HOME/icons`,
//! `~/.icons`, and each of `$XDG_DATA_DIRS`' `icons` -- libXcursor's order.
//! The first root that has the file wins, so a user's copy of a theme is the
//! one drawn.
//!
//! Cursors are asked for by their CSS names (`default`, `text`, `pointer`,
//! `ns-resize`, ...), which is what current themes name their files. A theme
//! that lacks one is asked for its older X11 names ([`aliases`]: `pointer`
//! was `hand2`), then each theme its `index.theme` inherits from, in order --
//! a theme's own picture under any name before another theme's, so its look
//! stays together.
//!
//! With no theme chosen, or none that draws a cursor, there is no picture
//! here: the compositor draws its own (`gui/compositor/src/cursor.rs`), which
//! is the built-in theme's pointer. The built-in theme's folder has no
//! `cursors` directory; give it one and those files are read first, as an
//! edited copy of its icons is.
//!
//! # Sizes, frames and trust
//!
//! See [`xcursor`]: the pictures of the nominal size nearest the size asked
//! for, every frame of an animated one, each file checked before it is
//! believed. A file larger than [`MAX_CURSOR_BYTES`], or one that is not a
//! readable XCursor file, is passed over for the next place to look.

pub mod xcursor;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use desktopentry::DesktopEntry;
use desktopentry::scan::DataDirs;

use crate::themes::{self, ThemeDirs};

pub use xcursor::{CursorFrame, CursorImages};

/// The directory inside a theme's folder that holds its cursors.
pub const CURSORS_DIR: &str = "cursors";

/// The file that names a cursor theme and the themes it inherits from.
pub const INDEX_FILE: &str = "index.theme";

/// The largest cursor file read. A common theme's busy pointer -- sixty
/// frames at five sizes -- is 4 MiB; a file past this is not read to find out
/// what it is.
pub const MAX_CURSOR_BYTES: u64 = 32 * 1024 * 1024;

/// The largest `index.theme` read: a few lines in practice.
const MAX_INDEX_BYTES: u64 = 64 * 1024;

/// The most themes one lookup visits, the chosen one and every theme it
/// inherits from, however they inherit. A real chain is two or three long;
/// this bounds a theme that inherits from itself by another name.
const MAX_THEMES_VISITED: usize = 16;

/// Each CSS cursor name with the older names themes still ship it under:
/// X11's cursor-font names (`left_ptr`, `xterm`, `fleur`), and the names
/// Qt and GTK settled on before CSS's were common (`size_ver`,
/// `pointing_hand`). In the order they are tried.
const ALIASES: &[(&str, &[&str])] = &[
    ("default", &["left_ptr", "arrow", "top_left_arrow"]),
    ("text", &["xterm", "ibeam"]),
    ("pointer", &["hand2", "pointing_hand", "hand"]),
    ("help", &["question_arrow", "whats_this", "left_ptr_help"]),
    ("progress", &["left_ptr_watch", "half-busy"]),
    ("wait", &["watch"]),
    ("crosshair", &["cross", "tcross"]),
    ("move", &["fleur", "size_all"]),
    ("not-allowed", &["crossed_circle", "forbidden", "circle"]),
    ("no-drop", &["dnd-no-drop"]),
    ("grab", &["openhand", "hand1"]),
    ("grabbing", &["closedhand", "dnd-none"]),
    ("copy", &["dnd-copy"]),
    ("alias", &["dnd-link", "link"]),
    ("cell", &["plus"]),
    ("all-scroll", &["fleur"]),
    (
        "ns-resize",
        &["sb_v_double_arrow", "size_ver", "v_double_arrow"],
    ),
    (
        "ew-resize",
        &["sb_h_double_arrow", "size_hor", "h_double_arrow"],
    ),
    ("nesw-resize", &["fd_double_arrow", "size_bdiag"]),
    ("nwse-resize", &["bd_double_arrow", "size_fdiag"]),
    ("col-resize", &["split_h", "sb_h_double_arrow"]),
    ("row-resize", &["split_v", "sb_v_double_arrow"]),
    ("n-resize", &["top_side"]),
    ("s-resize", &["bottom_side"]),
    ("e-resize", &["right_side"]),
    ("w-resize", &["left_side"]),
    ("ne-resize", &["top_right_corner"]),
    ("nw-resize", &["top_left_corner"]),
    ("se-resize", &["bottom_right_corner"]),
    ("sw-resize", &["bottom_left_corner"]),
];

/// The older names the cursor `name` may be shipped under, in the order they
/// are tried; none for a name with no older spelling, or one not a CSS name.
#[must_use]
pub fn aliases(name: &str) -> &'static [&'static str] {
    ALIASES
        .iter()
        .find(|(css, _)| *css == name)
        .map_or(&[], |(_, older)| older)
}

/// Whether `name` can name a cursor file: ASCII letters, digits, `-` and
/// `_`, up to 64 of them -- every CSS and X11 name, and the hashes some
/// themes name a cursor by, and never a path.
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A cursor theme, by the name of its folder.
///
/// Named as a colour theme or an icon theme is, by a folder name that need
/// not be text (`design-decisions.md` §426). Nothing is read until a cursor
/// is asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorTheme {
    id: OsString,
    /// The SlateOS theme roots, or `None` for the standard ones -- read when a
    /// cursor is looked up, so the setting is the name alone.
    dirs: Option<ThemeDirs>,
    /// The roots other desktops' cursor themes are under, or `None` for the
    /// environment's ([`icon_dirs`]).
    icon_dirs: Option<Vec<PathBuf>>,
}

impl Default for CursorTheme {
    fn default() -> Self {
        Self::built_in()
    }
}

impl CursorTheme {
    /// The built-in theme: the compositor's own pointer, unless the built-in
    /// theme's folder has been given cursors.
    #[must_use]
    pub fn built_in() -> Self {
        Self::load(OsStr::new(themes::BUILT_IN))
    }

    /// The theme `id`, looked for in the standard places.
    #[must_use]
    pub fn load(id: &OsStr) -> Self {
        Self {
            id: id.to_os_string(),
            dirs: None,
            icon_dirs: None,
        }
    }

    /// The theme `id`, looked for under the SlateOS theme roots `dirs` and
    /// then the cursor-theme roots `icon_dirs`, in that order.
    #[must_use]
    pub fn named(id: &OsStr, dirs: ThemeDirs, icon_dirs: Vec<PathBuf>) -> Self {
        Self {
            id: id.to_os_string(),
            dirs: Some(dirs),
            icon_dirs: Some(icon_dirs),
        }
    }

    /// The theme's name -- its folder's.
    #[must_use]
    pub fn id(&self) -> &OsStr {
        &self.id
    }

    /// Whether this is the built-in theme.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.id == themes::BUILT_IN
    }

    /// The pictures for the cursor `name` -- a CSS cursor name -- at the
    /// nominal size nearest `size` pixels. `None` when neither this theme nor
    /// one it inherits from draws it under any of its names, or `name` is not
    /// a name: then the compositor draws its own.
    #[must_use]
    pub fn cursor(&self, name: &str, size: u32) -> Option<CursorImages> {
        if !is_valid_name(name) || size == 0 {
            return None;
        }
        let roots = self.roots();
        let mut visited = Vec::new();
        find(&roots, &self.id, name, size, &mut visited)
    }

    /// Where this theme is looked for: the SlateOS roots, then the others.
    fn roots(&self) -> Vec<PathBuf> {
        let dirs = self.dirs.clone().unwrap_or_else(ThemeDirs::standard);
        let mut roots: Vec<PathBuf> = dirs
            .roots()
            .into_iter()
            .map(|(root, _)| root.to_path_buf())
            .collect();
        match &self.icon_dirs {
            Some(icon_dirs) => roots.extend(icon_dirs.iter().cloned()),
            None => roots.extend(icon_dirs()),
        }
        roots
    }
}

/// The cursor `name` at `size` in the theme `id` or a theme it inherits
/// from, every theme visited recorded in `visited`.
fn find(
    roots: &[PathBuf],
    id: &OsStr,
    name: &str,
    size: u32,
    visited: &mut Vec<OsString>,
) -> Option<CursorImages> {
    if visited.len() >= MAX_THEMES_VISITED
        || visited.iter().any(|seen| seen == id)
        || !themes::is_valid_id(id)
    {
        return None;
    }
    visited.push(id.to_os_string());
    for candidate in std::iter::once(name).chain(aliases(name).iter().copied()) {
        for root in roots {
            let file = root.join(id).join(CURSORS_DIR).join(candidate);
            if let Some(images) = read_cursor(&file, size) {
                return Some(images);
            }
        }
    }
    // The themes it inherits from, as its first `index.theme` says.
    let index = roots.iter().find_map(|root| read_index(&root.join(id)))?;
    index
        .inherits
        .iter()
        .find_map(|parent| find(roots, parent, name, size, visited))
}

/// The cursor file at `path`, at `size`: `None` for one that is missing,
/// too large, or not a readable XCursor file.
fn read_cursor(path: &Path, size: u32) -> Option<CursorImages> {
    xcursor::read(&read_capped(path, MAX_CURSOR_BYTES)?, size)
}

/// The bytes of the file at `path`, if it is no larger than `cap`.
///
/// Read through `take` rather than after checking the length, so a file
/// growing between the two cannot be read past the cap.
fn read_capped(path: &Path, cap: u64) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    // `ok()`: every failure means the same thing here -- this file is no
    // cursor, so the next place is looked in -- and there is no one to tell.
    file.take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    (u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= cap).then_some(bytes)
}

/// What a theme's `index.theme` says about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Index {
    /// The name it gives itself, if any.
    name: Option<String>,
    /// The themes it inherits from, in order.
    inherits: Vec<OsString>,
}

/// The `index.theme` in the theme folder `dir`, or `None` for one that is
/// missing, too large or not a key file.
fn read_index(dir: &Path) -> Option<Index> {
    let bytes = read_capped(&dir.join(INDEX_FILE), MAX_INDEX_BYTES)?;
    let entry = DesktopEntry::parse(&bytes).ok()?;
    const GROUP: &str = "Icon Theme";
    // The Icon Theme Specification separates the list with commas; some
    // themes write semicolons, as a desktop entry's lists are.
    let inherits = entry
        .raw(GROUP, "Inherits")
        .map(|list| {
            list.split([',', ';'])
                .map(str::trim)
                .filter(|parent| !parent.is_empty())
                .map(OsString::from)
                .filter(|parent| themes::is_valid_id(parent))
                .collect()
        })
        .unwrap_or_default();
    let name = entry
        .string(GROUP, "Name")
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    Some(Index { name, inherits })
}

/// Where other desktops' cursor themes are installed, as the environment
/// says: `$XDG_DATA_HOME/icons` (or `~/.local/share/icons`), `~/.icons`, then
/// each `$XDG_DATA_DIRS/icons` -- libXcursor's order, the user's before the
/// system's.
#[must_use]
pub fn icon_dirs() -> Vec<PathBuf> {
    icon_dirs_from(|name| std::env::var_os(name))
}

/// [`icon_dirs`], with the environment as `get`.
fn icon_dirs_from(get: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let all = DataDirs::from_env(&get);
    // The same reading with no user named: what is left is the system's, so
    // the difference is the user's own data directory, if any.
    let system = DataDirs::from_env(|name| {
        if name == "XDG_DATA_DIRS" {
            get(name)
        } else {
            None
        }
    });
    let users = all.dirs().len().saturating_sub(system.dirs().len());
    let (user, rest) = all.dirs().split_at(users.min(all.dirs().len()));
    let mut dirs: Vec<PathBuf> = user.iter().map(|dir| dir.join("icons")).collect();
    if let Some(home) = get("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
    {
        dirs.push(home.join(".icons"));
    }
    dirs.extend(rest.iter().map(|dir| dir.join("icons")));
    dirs
}

/// A cursor theme that is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorThemeInfo {
    /// Its folder's name: what [`CursorTheme::load`] takes.
    pub id: OsString,
    /// The name it gives itself in its `index.theme`, else its folder's.
    pub name: String,
    /// Whether this is the built-in theme -- the compositor's own pointer.
    pub built_in: bool,
}

/// Every cursor theme installed, for a picker: the built-in one first, then
/// every folder with a `cursors` directory under any root, by name. A theme
/// installed in two roots is listed once, as the first root's copy -- the
/// one [`CursorTheme::cursor`] would read.
#[must_use]
pub fn available() -> Vec<CursorThemeInfo> {
    let theme = CursorTheme::built_in();
    available_in(&theme.roots())
}

/// [`available`], under the roots `roots`, in order.
#[must_use]
pub fn available_in(roots: &[PathBuf]) -> Vec<CursorThemeInfo> {
    let mut found: BTreeMap<OsString, CursorThemeInfo> = BTreeMap::new();
    for root in roots {
        // A root that does not exist has no themes in it: the ordinary state
        // of a user's data directory, not an error.
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        // An entry the listing could not describe could not be opened as a
        // theme either; skipping it loses a row nobody could have chosen.
        for entry in entries.flatten() {
            let id = entry.file_name();
            if found.contains_key(&id)
                || !themes::is_valid_id(&id)
                || id == themes::BUILT_IN
                || !entry.path().join(CURSORS_DIR).is_dir()
            {
                continue;
            }
            // A folder named in bytes that are not text is shown escaped, as
            // a colour theme's is, rather than with characters made up.
            let name = read_index(&entry.path())
                .and_then(|index| index.name)
                .unwrap_or_else(|| pathcodec::display_os(&id));
            found.insert(
                id.clone(),
                CursorThemeInfo {
                    id,
                    name,
                    built_in: false,
                },
            );
        }
    }
    let mut list: Vec<CursorThemeInfo> = found.into_values().collect();
    list.sort_by_key(|info| info.name.to_lowercase());
    list.insert(
        0,
        CursorThemeInfo {
            id: OsString::from(themes::BUILT_IN),
            name: themes::BUILT_IN_NAME.to_owned(),
            built_in: true,
        },
    );
    list
}

// `pub(crate)` for its XCursor builder, which the theme checker's tests
// build their cursors with too.
#[cfg(test)]
#[path = "cursors_tests.rs"]
pub(crate) mod tests;
